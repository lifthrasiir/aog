use super::Solver;
use crate::polyomino::{self, canonical, Rotation};
use crate::types::*;
use std::collections::{BTreeSet, HashSet};

impl Solver {
    pub(crate) fn compute_area_bounds(&mut self) {
        let r = &self.puzzle.rules;
        let total = self.grid.total_existing_cells();
        let mut lo = 1usize;
        let mut hi = total;
        if let Some(m) = r.minimum {
            lo = lo.max(m);
        }
        if let Some(m) = r.maximum {
            hi = hi.min(m);
        }
        for s in &r.shape_bank {
            let sz = s.cells.len();
            lo = lo.max(sz);
            hi = hi.min(sz);
        }
        self.eff_min_area = lo;
        self.eff_max_area = hi;
    }

    /// When no shape bank is given, auto-populate based on constraints
    /// for much faster piece-based search.
    pub(crate) fn auto_populate_shape_bank(&mut self) {
        if !self.puzzle.rules.shape_bank.is_empty() {
            return;
        }
        if self.puzzle.rules.boxy {
            self.populate_boxy();
        } else if self.puzzle.rules.non_boxy {
            self.populate_non_boxy();
        } else {
            self.populate_general();
        }
        self.auto_populated_bank = !self.puzzle.rules.shape_bank.is_empty();
    }

    /// General: only for tight (single-size) puzzles with max <= 5.
    fn populate_general(&mut self) {
        if self.eff_max_area > 5 || self.eff_max_area < 1 {
            return;
        }
        let target = self.eff_max_area;
        let names = [
            "o", "oo", "ooo", "8o", "I", "O", "T", "S", "Z", "L", "J", "F", "P", "N", "U", "V",
            "W", "X", "Y", "II", "LL", "TT", "ZZ",
        ];
        let mut seen: HashSet<Vec<(i32, i32)>> = HashSet::new();
        for &name in &names {
            if let Some(shape) = polyomino::get_named_shape(name) {
                if shape.cells.len() == target {
                    let canon = canonical(&shape);
                    if seen.insert(canon.cells.clone()) {
                        self.puzzle.rules.shape_bank.push(shape);
                    }
                }
            }
        }
    }

    /// Boxy: only rectangles. Extremely few shapes even for large max.
    fn populate_boxy(&mut self) {
        let max_size = self.eff_max_area.min(self.total_cells);
        if max_size < self.eff_min_area || self.eff_min_area < 1 {
            return;
        }
        let mut seen: HashSet<Vec<(i32, i32)>> = HashSet::new();
        for size in self.eff_min_area..=max_size {
            for w in 1..=size {
                if size % w != 0 {
                    continue;
                }
                let h = size / w;
                if w > h {
                    continue; // avoid rotation duplicates (only w <= h)
                }
                let cells: Vec<(i32, i32)> = (0..h as i32)
                    .flat_map(|r| (0..w as i32).map(move |c| (r, c)))
                    .collect();
                let shape = polyomino::make_shape(&cells);
                let canon = canonical(&shape);
                if seen.insert(canon.cells.clone()) {
                    self.puzzle.rules.shape_bank.push(shape);
                }
            }
        }
        eprintln!(
            "boxy shape bank: {} shapes (sizes {}-{})",
            self.puzzle.rules.shape_bank.len(),
            self.eff_min_area,
            max_size
        );
    }

    /// Non-boxy: free polyominoes with rectangular ones filtered out.
    fn populate_non_boxy(&mut self) {
        let max_size = self.eff_max_area.min(8);
        if max_size < self.eff_min_area || self.eff_min_area < 1 {
            return;
        }
        let mut seen: HashSet<Vec<(i32, i32)>> = HashSet::new();
        for size in self.eff_min_area..=max_size {
            let shapes = polyomino::enumerate_free_polyominoes(size);
            for shape in shapes {
                if polyomino::is_rectangular_shape(&shape) {
                    continue;
                }
                let canon = canonical(&shape);
                if seen.insert(canon.cells.clone()) {
                    self.puzzle.rules.shape_bank.push(shape);
                }
            }
        }
        eprintln!(
            "non-boxy shape bank: {} shapes (sizes {}-{})",
            self.puzzle.rules.shape_bank.len(),
            self.eff_min_area,
            max_size
        );
    }

    pub(crate) fn prepare_shape_transforms(&mut self) {
        for shape in &self.puzzle.rules.shape_bank {
            let mut seen: BTreeSet<Vec<(isize, isize)>> = BTreeSet::new();
            let mut transforms = Vec::new();

            for rot in Rotation::all() {
                for flip in [false, true] {
                    let t: Vec<(isize, isize)> = shape
                        .cells
                        .iter()
                        .map(|&(r, c)| {
                            let (nr, nc) = rot.transform(r, c);
                            if flip {
                                (nr as isize, -nc as isize)
                            } else {
                                (nr as isize, nc as isize)
                            }
                        })
                        .collect();
                    let minr = t.iter().map(|&(r, _)| r).min().unwrap();
                    let minc = t.iter().map(|&(_, c)| c).min().unwrap();
                    let mut normalized: Vec<_> =
                        t.iter().map(|&(r, c)| (r - minr, c - minc)).collect();
                    normalized.sort();
                    if seen.insert(normalized.clone()) {
                        transforms.push(normalized);
                    }
                }
            }
            self.shape_transforms.push(transforms);
        }
    }

    pub(crate) fn generate_all_polyominoes(
        &self,
        start: CellId,
        size: usize,
        clue_at: &[Option<usize>],
        results: &mut Vec<Vec<CellId>>,
    ) {
        let mut current = vec![start];
        let mut candidates = BTreeSet::new();
        for eid in self.grid.cell_edges(start).into_iter().flatten() {
            let (c1, c2) = self.grid.edge_cells(eid);
            let neighbor = if c1 == start { c2 } else { c1 };
            if self.grid.cell_exists[neighbor] && clue_at[neighbor].is_none() {
                candidates.insert(neighbor);
            }
        }
        self.poly_rec(&mut current, &mut candidates, size, clue_at, results);
    }

    fn poly_rec(
        &self,
        current: &mut Vec<CellId>,
        candidates: &mut BTreeSet<CellId>,
        size: usize,
        clue_at: &[Option<usize>],
        results: &mut Vec<Vec<CellId>>,
    ) {
        if current.len() == size {
            let mut res = current.clone();
            res.sort();
            results.push(res);
            return;
        }
        if candidates.is_empty() {
            return;
        }

        let mut my_candidates = candidates.clone();
        while let Some(&next) = my_candidates.iter().next() {
            my_candidates.remove(&next);
            candidates.remove(&next);

            let mut added = Vec::new();
            for eid in self.grid.cell_edges(next).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let neighbor = if c1 == next { c2 } else { c1 };
                if self.grid.cell_exists[neighbor]
                    && clue_at[neighbor].is_none()
                    && !current.contains(&neighbor)
                    && !my_candidates.contains(&neighbor)
                {
                    if candidates.insert(neighbor) {
                        added.push(neighbor);
                    }
                }
            }

            current.push(next);
            self.poly_rec(current, candidates, size, clue_at, results);
            current.pop();

            for a in added {
                candidates.remove(&a);
            }
        }
    }

    pub(crate) fn generate_clue_placements(&self) -> Vec<(usize, Piece)> {
        let mut placements = Vec::new();
        let n = self.grid.num_cells();
        let mut clue_at = vec![None; n];
        for (i, clue) in self.puzzle.cell_clues.iter().enumerate() {
            clue_at[clue.cell()] = Some(i);
        }

        for (clue_idx, clue) in self.puzzle.cell_clues.iter().enumerate() {
            let start_cell = clue.cell();
            match clue {
                CellClue::Area { value, .. } => {
                    let mut results = Vec::new();
                    self.generate_all_polyominoes(start_cell, *value, &clue_at, &mut results);
                    for cells in results {
                        let sc: Vec<(i32, i32)> = cells
                            .iter()
                            .map(|&c| {
                                let (r, col) = self.grid.cell_pos(c);
                                (r as i32, col as i32)
                            })
                            .collect();
                        let p = Piece {
                            cells,
                            area: *value,
                            canonical: canonical(&polyomino::make_shape(&sc)),
                        };
                        placements.push((clue_idx, p));
                    }
                }
                CellClue::Polyomino { shape, .. } => {
                    let (cr, cc) = self.grid.cell_pos(start_cell);
                    let mut transforms = Vec::new();
                    let mut seen = HashSet::new();
                    for rot in Rotation::all() {
                        for flip in [false, true] {
                            let mut t: Vec<(isize, isize)> = shape
                                .cells
                                .iter()
                                .map(|&(r, c)| {
                                    let (nr, nc) = rot.transform(r, c);
                                    if flip {
                                        (nr as isize, -nc as isize)
                                    } else {
                                        (nr as isize, nc as isize)
                                    }
                                })
                                .collect();
                            let minr = t.iter().map(|&(r, _)| r).min().unwrap();
                            let minc = t.iter().map(|&(_, c)| c).min().unwrap();
                            for p in &mut t {
                                p.0 -= minr;
                                p.1 -= minc;
                            }
                            t.sort();
                            if seen.insert(t.clone()) {
                                transforms.push(t);
                            }
                        }
                    }
                    for transform in transforms {
                        for &(tdr, tdc) in &transform {
                            let sr = cr as isize - tdr;
                            let sc = cc as isize - tdc;
                            let mut cells = Vec::new();
                            let mut ok = true;
                            for &(dr, dc) in &transform {
                                let nr = sr + dr;
                                let nc = sc + dc;
                                if nr < 0
                                    || nr >= self.grid.rows as isize
                                    || nc < 0
                                    || nc >= self.grid.cols as isize
                                {
                                    ok = false;
                                    break;
                                }
                                let cid = self.grid.cell_id(nr as usize, nc as usize);
                                if !self.grid.cell_exists[cid] {
                                    ok = false;
                                    break;
                                }
                                if let Some(other_idx) = clue_at[cid] {
                                    if other_idx != clue_idx {
                                        ok = false;
                                        break;
                                    }
                                }
                                cells.push(cid);
                            }
                            if ok {
                                cells.sort();
                                placements.push((
                                    clue_idx,
                                    Piece {
                                        cells,
                                        area: shape.cells.len(),
                                        canonical: canonical(shape),
                                    },
                                ));
                            }
                        }
                    }
                }
                _ => {} // Other clue types don't define whole pieces themselves
            }
        }
        placements
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;
    use crate::types::{GlobalRules, Puzzle};

    #[test]
    fn compute_area_bounds_with_minimum() {
        let input = "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
";
        let s = make_solver(input);
        let puzzle = Puzzle {
            rules: GlobalRules {
                minimum: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };
        let grid = s.get_grid().clone();
        let mut solver = Solver::new(puzzle, grid);
        solver.compute_area_bounds();
        assert_eq!(solver.eff_min_area, 3);
    }

    #[test]
    fn auto_populate_shape_bank_populates() {
        let input = "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
";
        let s = make_solver(input);
        let puzzle = Puzzle {
            rules: GlobalRules {
                minimum: Some(2),
                maximum: Some(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let grid = s.get_grid().clone();
        let mut solver = Solver::new(puzzle, grid);
        solver.compute_area_bounds();
        assert_eq!(solver.eff_max_area, 2);
        assert!(solver.puzzle.rules.shape_bank.is_empty());

        solver.auto_populate_shape_bank();
        assert!(!solver.puzzle.rules.shape_bank.is_empty());
        // All shapes should be dominoes (area 2)
        for shape in &solver.puzzle.rules.shape_bank {
            assert_eq!(shape.cells.len(), 2);
        }
    }

    #[test]
    fn auto_populate_non_boxy_filters_rectangles() {
        let input = "\
+---+---+---+---+
| _ . _ . _ . _ |
+ . + . + . + . +
| _ . _ . _ . _ |
+---+---+---+---+
";
        let s = make_solver(input);
        let puzzle = Puzzle {
            rules: GlobalRules {
                minimum: Some(3),
                maximum: Some(4),
                non_boxy: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let grid = s.get_grid().clone();
        let mut solver = Solver::new(puzzle, grid);
        solver.compute_area_bounds();
        solver.auto_populate_shape_bank();
        assert!(!solver.puzzle.rules.shape_bank.is_empty());
        // No shape should be rectangular
        for shape in &solver.puzzle.rules.shape_bank {
            assert!(
                !polyomino::is_rectangular_shape(shape),
                "non-boxy bank should not contain rectangular shapes, got area={}",
                shape.cells.len()
            );
        }
        // Should have shapes of sizes 3 and 4
        let sizes: std::collections::HashSet<usize> = solver
            .puzzle
            .rules
            .shape_bank
            .iter()
            .map(|s| s.cells.len())
            .collect();
        assert!(sizes.contains(&3));
        assert!(sizes.contains(&4));
        assert!(!sizes.contains(&2)); // domino is rectangular
    }

    #[test]
    fn auto_populate_boxy_only_rectangles() {
        let input = "\
+---+---+---+---+---+
| _ . _ . _ . _ . _ |
+ . + . + . + . + . +
| _ . _ . _ . _ . _ |
+ . + . + . + . + . +
| _ . _ . _ . _ . _ |
+---+---+---+---+---+
";
        let s = make_solver(input);
        let puzzle = Puzzle {
            rules: GlobalRules {
                minimum: Some(1),
                maximum: Some(10),
                boxy: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let total = s.grid.total_existing_cells();
        let grid = s.get_grid().clone();
        let mut solver = Solver::new(puzzle, grid);
        solver.total_cells = total;
        solver.compute_area_bounds();
        solver.auto_populate_shape_bank();
        assert!(!solver.puzzle.rules.shape_bank.is_empty());
        // All shapes should be rectangular
        for shape in &solver.puzzle.rules.shape_bank {
            assert!(
                polyomino::is_rectangular_shape(shape),
                "boxy bank should only contain rectangular shapes, got area={}",
                shape.cells.len()
            );
        }
        // Should have shapes of various sizes 1-10
        let sizes: std::collections::HashSet<usize> = solver
            .puzzle
            .rules
            .shape_bank
            .iter()
            .map(|s| s.cells.len())
            .collect();
        assert!(sizes.contains(&1));
        assert!(sizes.contains(&2));
        assert!(sizes.contains(&4));
        assert!(sizes.contains(&6));
        assert!(sizes.contains(&10));
        // Boxy shapes are few: area n has ceil(d(n)/2) rectangles
        assert!(solver.puzzle.rules.shape_bank.len() <= 17); // sum of ceil(d(n)/2) for n=1..10
    }
}
