use super::Solver;
use crate::polyomino::{self, canonical, Rotation};
use crate::types::*;
use std::collections::{BTreeSet, HashMap, HashSet};

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
            if self.is_pre_cut[eid] {
                continue;
            }
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
                if self.is_pre_cut[eid] {
                    continue;
                }
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

    /// Solve when same_area_groups is true: each distinct area value maps to
    /// exactly one piece. Uses recursive placement generation with lazy evaluation.
    pub(crate) fn solve_grouped_areas(&mut self) {
        // Group area clues by value
        let mut area_groups: Vec<(usize, Vec<CellId>)> = Vec::new();
        {
            let mut map: std::collections::HashMap<usize, Vec<CellId>> = std::collections::HashMap::new();
            for clue in &self.puzzle.cell_clues {
                if let CellClue::Area { cell, value } = clue {
                    map.entry(*value).or_default().push(*cell);
                }
            }
            for (value, cells) in map {
                area_groups.push((value, cells));
            }
        }
        area_groups.sort_by_key(|(v, _)| *v);

        // Collect all anchor cells as forbidden for placement generation
        let all_anchors: HashSet<CellId> = area_groups.iter().flat_map(|(_, a)| a.iter().copied()).collect();

        // Generate placements per group (ordered by constraint level = fewest first)
        let n_groups = area_groups.len();
        let mut group_placements: Vec<Vec<Vec<CellId>>> = Vec::with_capacity(n_groups);
        for &(area, ref anchors) in &area_groups {
            let forbidden = all_anchors.iter().filter(|c| !anchors.contains(c)).copied().collect();
            let placements = self.generate_grouped_placements(anchors, area, &forbidden);
            eprintln!("area {}: {} placements", area, placements.len());
            group_placements.push(placements);
        }

        // Sort groups by placement count (most constrained first)
        let mut order: Vec<usize> = (0..n_groups).collect();
        order.sort_by_key(|&i| group_placements[i].len());

        // Recursive backtracking: try each placement for the most constrained group,
        // then the next, etc.
        let mut used = vec![false; self.grid.num_cells()];
        let mut solution = Vec::with_capacity(n_groups);

        self.grouped_backtrack(
            &order,
            &group_placements,
            &area_groups,
            &mut used,
            &mut solution,
        );
    }

    fn grouped_backtrack(
        &mut self,
        order: &[usize],
        all_placements: &[Vec<Vec<CellId>>],
        area_groups: &[(usize, Vec<CellId>)],
        used: &mut Vec<bool>,
        solution: &mut Vec<(usize, Vec<CellId>)>, // (group_index, cells)
    ) {
        if self.solution_count >= 2 {
            return;
        }

        let depth = solution.len();
        if depth == order.len() {
            // All groups placed. Set edges then verify solution.
            self.report_progress();
            let n = self.grid.num_cells();
            let mut cell_to_piece = vec![usize::MAX; n];
            for (pi, (_, cells)) in solution.iter().enumerate() {
                for &c in cells {
                    cell_to_piece[c] = pi;
                }
            }
            // Set edges from cell partition (validate requires no Unknown edges)
            for (_, ref cells) in solution.iter() {
                for &cid in cells {
                    for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let other = if c1 == cid { c2 } else { c1 };
                        if !self.grid.cell_exists[other]
                            || cell_to_piece[other] != cell_to_piece[cid]
                        {
                            self.edges[eid] = EdgeState::Cut;
                        } else {
                            self.edges[eid] = EdgeState::Uncut;
                        }
                    }
                }
            }
            let pieces = self.compute_pieces_from_groups(solution, area_groups);
            if self.validate(&pieces) {
                self.save_solution(pieces);
            }
            return;
        }

        let gi = order[depth];
        let placements = &all_placements[gi];
        self.node_count += 1;

        for cells in placements {
            // Check cell availability
            if cells.iter().any(|&c| used[c]) {
                continue;
            }

            // Mark cells as used
            for &c in cells {
                used[c] = true;
            }
            solution.push((gi, cells.clone()));

            // Check adjacency constraints against previously placed groups
            let adj_ok = self.check_grouped_adjacency(solution, area_groups);
            if adj_ok {
                self.grouped_backtrack(order, all_placements, area_groups, used, solution);
            }

            // Unmark
            solution.pop();
            for &c in cells {
                used[c] = false;
            }

            if self.solution_count >= 2 {
                return;
            }
        }
    }

    /// Check size_separation and mingle_shape constraints between the most
    /// recently placed group and all previously placed adjacent groups.
    fn check_grouped_adjacency(
        &self,
        solution: &[(usize, Vec<CellId>)],
        area_groups: &[(usize, Vec<CellId>)],
    ) -> bool {
        if !self.puzzle.rules.size_separation && !self.puzzle.rules.mingle_shape {
            return true;
        }

        let last_idx = solution.len() - 1;
        let (_, ref last_cells) = solution[last_idx];
        let last_area = area_groups[solution[last_idx].0].0;
        let last_set: HashSet<CellId> = last_cells.iter().copied().collect();

        // Compute canonical shape of the last group for mingle_shape check
        let last_canonical = if self.puzzle.rules.mingle_shape {
            let sc: Vec<(i32, i32)> = last_cells
                .iter()
                .map(|&c| {
                    let (r, col) = self.grid.cell_pos(c);
                    (r as i32, col as i32)
                })
                .collect();
            Some(canonical(&polyomino::make_shape(&sc)))
        } else {
            None
        };

        // Build cell_to_piece for all placed groups
        let mut cell_to_piece: HashMap<CellId, usize> = HashMap::new();
        for (pi, (_, ref cells)) in solution.iter().enumerate() {
            for &c in cells {
                cell_to_piece.insert(c, pi);
            }
        }

        // For each cell in the last placed group, check grid neighbors
        for &cid in last_cells {
            for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == cid { c2 } else { c1 };
                if !self.grid.cell_exists[other] || last_set.contains(&other) {
                    continue;
                }
                let Some(&other_pi) = cell_to_piece.get(&other) else {
                    continue;
                };
                if other_pi == last_idx {
                    continue;
                }
                let other_area = area_groups[solution[other_pi].0].0;

                if self.puzzle.rules.size_separation && last_area == other_area {
                    return false;
                }

                if let Some(ref last_shape) = last_canonical {
                    let (_, ref other_cells): &(usize, Vec<CellId>) = &solution[other_pi];
                    let osc: Vec<(i32, i32)> = other_cells
                        .iter()
                        .map(|&c| {
                            let (r, col) = self.grid.cell_pos(c);
                            (r as i32, col as i32)
                        })
                        .collect();
                    let other_shape = canonical(&polyomino::make_shape(&osc));
                    if last_shape != &other_shape {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn compute_pieces_from_groups(
        &self,
        solution: &[(usize, Vec<CellId>)],
        area_groups: &[(usize, Vec<CellId>)],
    ) -> Vec<Piece> {
        let mut pieces = Vec::new();
        for &(gi, ref cells) in solution {
            let area = area_groups[gi].0;
            let sc: Vec<(i32, i32)> = cells
                .iter()
                .map(|&c| {
                    let (r, col) = self.grid.cell_pos(c);
                    (r as i32, col as i32)
                })
                .collect();
            pieces.push(Piece {
                cells: cells.clone(),
                area,
                canonical: canonical(&polyomino::make_shape(&sc)),
            });
        }
        pieces
    }

    /// Generate all connected subsets of `target_size` cells that include all `anchors`
    /// and exclude `forbidden` cells.
    fn generate_grouped_placements(
        &self,
        anchors: &[CellId],
        target_size: usize,
        forbidden: &HashSet<CellId>,
    ) -> Vec<Vec<CellId>> {
        let mut results = Vec::new();
        if anchors.is_empty() || anchors.len() > target_size {
            return results;
        }

        // Try each anchor as the BFS starting point
        for (start_i, &start) in anchors.iter().enumerate() {
            let others: Vec<CellId> = anchors
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != start_i)
                .map(|(_, &a)| a)
                .collect();

            let mut current = vec![start];
            let mut in_set: HashSet<CellId> = HashSet::from([start]);
            let mut frontier: BTreeSet<CellId> = BTreeSet::new();

            for eid in self.grid.cell_edges(start).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == start { c2 } else { c1 };
                if self.grid.cell_exists[other]
                    && !in_set.contains(&other)
                    && !forbidden.contains(&other)
                {
                    frontier.insert(other);
                }
            }

            self.grouped_grow(
                &mut current,
                &mut in_set,
                &mut frontier,
                target_size,
                forbidden,
                &others,
                &mut results,
            );
        }

        // Deduplicate
        results.sort();
        results.dedup();
        results
    }

    fn grouped_grow(
        &self,
        current: &mut Vec<CellId>,
        in_set: &mut HashSet<CellId>,
        frontier: &mut BTreeSet<CellId>,
        target_size: usize,
        forbidden: &HashSet<CellId>,
        remaining_anchors: &[CellId],
        results: &mut Vec<Vec<CellId>>,
    ) {
        let left = target_size - current.len();
        if left == 0 {
            if remaining_anchors.iter().all(|a| in_set.contains(a)) {
                let mut sorted = current.clone();
                sorted.sort();
                results.push(sorted);
            }
            return;
        }

        if frontier.is_empty() {
            return;
        }

        // Pruning: must be able to reach all remaining anchors
        let unreached = remaining_anchors.iter().filter(|a| !in_set.contains(a)).count();
        if unreached > 0 && left < unreached {
            return;
        }
        // Note: we do NOT prune on `left > frontier.len()` because the frontier
        // can grow as cells are added (each new cell may expose new neighbors).

        let mut my_frontier = frontier.clone();
        while let Some(&next) = my_frontier.iter().next() {
            my_frontier.remove(&next);
            frontier.remove(&next);

            let mut added = Vec::new();
            in_set.insert(next);
            for eid in self.grid.cell_edges(next).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == next { c2 } else { c1 };
                if self.grid.cell_exists[other]
                    && !in_set.contains(&other)
                    && !forbidden.contains(&other)
                    && !my_frontier.contains(&other)
                {
                    if frontier.insert(other) {
                        added.push(other);
                    }
                }
            }

            current.push(next);
            self.grouped_grow(
                current, in_set, frontier, target_size, forbidden, remaining_anchors, results,
            );
            current.pop();

            in_set.remove(&next);
            for a in added {
                frontier.remove(&a);
            }
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
