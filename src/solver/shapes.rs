use super::Solver;
use crate::polyomino::{self, canonical, Rotation};
use crate::types::Shape;
use std::collections::{BTreeSet, HashSet};

/// Precomputed shape bank data for piece-based search.
/// Written by shapes.rs/match_solver.rs, read by pieces.rs.
pub(crate) struct ShapeSearchState {
    /// All distinct orientations for each shape in the bank.
    pub(crate) transforms: Vec<Vec<Vec<(isize, isize)>>>,
    /// Canonical forms of shapes in the bank (for dedup/comparison).
    pub(crate) canonicals: Vec<Shape>,
}

impl ShapeSearchState {
    pub(crate) fn new() -> Self {
        Self {
            transforms: Vec::new(),
            canonicals: Vec::new(),
        }
    }
}

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
        let mut bank_min = usize::MAX;
        let mut bank_max = 0usize;
        for s in &r.shape_bank {
            let sz = s.cells.len();
            bank_min = bank_min.min(sz);
            bank_max = bank_max.max(sz);
        }
        if bank_max > 0 {
            lo = lo.max(bank_min);
            hi = hi.min(bank_max);
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
        // Cap eff_max_area by the largest connected component via non-pre-cut edges.
        // Any piece larger than this cannot exist without crossing a pre-cut edge.
        let max_comp = self.max_non_precut_component_size();
        if self.eff_max_area > max_comp {
            tracing::info!(
                from = self.eff_max_area,
                to = max_comp,
                "eff_max_area capped (max non-pre-cut component)"
            );
            self.eff_max_area = max_comp;
        }
        if self.puzzle.rules.boxy {
            self.populate_boxy();
        } else if self.puzzle.rules.non_boxy {
            self.populate_non_boxy();
        } else {
            self.populate_general();
        }
    }

    /// General: free polyominoes for all sizes in [eff_min_area, eff_max_area], capped at 5.
    fn populate_general(&mut self) {
        if self.eff_max_area > 5 || self.eff_max_area < 1 {
            return;
        }
        let mut seen: HashSet<Vec<(i32, i32)>> = HashSet::new();
        for size in self.eff_min_area..=self.eff_max_area {
            let shapes = polyomino::enumerate_free_polyominoes(size);
            for shape in shapes {
                let canon = canonical(&shape);
                if seen.insert(canon.cells.clone()) {
                    self.puzzle.rules.shape_bank.push(shape);
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
                // Skip shapes that cannot be placed without crossing a pre-cut edge.
                // Check both orientations since dedup only keeps w <= h.
                if !self.has_valid_boxy_placement(w, h)
                    && (w == h || !self.has_valid_boxy_placement(h, w))
                {
                    continue;
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
        tracing::info!(
            shapes = self.puzzle.rules.shape_bank.len(),
            min_area = self.eff_min_area,
            max_area = max_size,
            "boxy shape bank populated"
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
        tracing::info!(
            shapes = self.puzzle.rules.shape_bank.len(),
            min_area = self.eff_min_area,
            max_area = max_size,
            "non-boxy shape bank populated"
        );
    }

    /// Largest connected component reachable via non-pre-cut edges.
    fn max_non_precut_component_size(&self) -> usize {
        let n = self.grid.num_cells();
        let mut visited = vec![false; n];
        let mut max_size = 0usize;
        for start in 0..n {
            if !self.grid.cell_exists[start] || visited[start] {
                continue;
            }
            let mut size = 0usize;
            visited[start] = true;
            let mut stack = vec![start];
            while let Some(cur) = stack.pop() {
                size += 1;
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.is_pre_cut[eid] {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if self.grid.cell_exists[other] && !visited[other] {
                        visited[other] = true;
                        stack.push(other);
                    }
                }
            }
            max_size = max_size.max(size);
        }
        max_size
    }

    /// Check if a w*h rectangle can be placed somewhere on the grid
    /// such that all cells exist and no internal edge is pre-cut.
    fn has_valid_boxy_placement(&self, w: usize, h: usize) -> bool {
        if w > self.grid.cols || h > self.grid.rows {
            return false;
        }
        for sr in 0..=(self.grid.rows - h) {
            for sc in 0..=(self.grid.cols - w) {
                if self.is_rect_precut_free(sr, sc, w, h) {
                    return true;
                }
            }
        }
        false
    }

    fn is_rect_precut_free(&self, sr: usize, sc: usize, w: usize, h: usize) -> bool {
        // Check all cells exist
        for r in sr..sr + h {
            for c in sc..sc + w {
                if !self.grid.cell_exists[self.grid.cell_id(r, c)] {
                    return false;
                }
            }
        }
        // Internal horizontal edges (between rows within the rectangle)
        for r in sr..sr + h.saturating_sub(1) {
            for c in sc..sc + w {
                if self.is_pre_cut[self.grid.h_edge(r, c)] {
                    return false;
                }
            }
        }
        // Internal vertical edges (between columns within the rectangle)
        for r in sr..sr + h {
            for c in sc..sc + w.saturating_sub(1) {
                if self.is_pre_cut[self.grid.v_edge(r, c)] {
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn prepare_transforms(&mut self) {
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
            self.shape_search.transforms.push(transforms);
        }
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
