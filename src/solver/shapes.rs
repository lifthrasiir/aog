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

    /// When no shape bank is given but eff_max_area is small enough, auto-populate
    /// with all free polyominoes up to that size for much faster piece-based search.
    pub(crate) fn auto_populate_shape_bank(&mut self) {
        if !self.puzzle.rules.shape_bank.is_empty() {
            return;
        }
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
                    let is_new = seen.insert(canon.cells.clone());
                    if is_new {
                        self.puzzle.rules.shape_bank.push(shape);
                    }
                }
            }
        }
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
}
