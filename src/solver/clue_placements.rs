use super::Solver;
use crate::polyomino::{self, canonical, Rotation};
use crate::types::*;
use std::collections::{BTreeSet, HashMap, HashSet};

impl Solver {
    const MAX_COMPASS_PLACEMENTS: usize = 2000;

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
                    && candidates.insert(neighbor)
                {
                    added.push(neighbor);
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

    /// Generate all valid connected polyominoes containing `start` that satisfy
    /// compass direction constraints. Unlike `generate_all_polyominoes`, this
    /// does not require a fixed target size — it accepts any size where all
    /// specified compass values are exactly matched.
    pub(crate) fn generate_compass_polyominoes(
        &self,
        start: CellId,
        compass: &CompassData,
        clue_at: &[Option<usize>],
    ) -> Vec<Vec<CellId>> {
        let (cr, cc) = self.grid.cell_pos(start);
        let (cr_i, cc_i) = (cr as isize, cc as isize);
        let mut results = Vec::new();

        let mut current = vec![start];
        let mut counts = [0usize; 4]; // N=0, S=1, E=2, W=3

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

        self.compass_rec(
            &mut current,
            &mut counts,
            &mut candidates,
            cr_i,
            cc_i,
            compass,
            clue_at,
            &mut results,
        );
        results
    }

    fn compass_rec(
        &self,
        current: &mut Vec<CellId>,
        counts: &mut [usize; 4],
        candidates: &mut BTreeSet<CellId>,
        cr_i: isize,
        cc_i: isize,
        compass: &CompassData,
        clue_at: &[Option<usize>],
        results: &mut Vec<Vec<CellId>>,
    ) {
        // Check if any specified direction exceeds its compass value
        if compass.n.map_or(false, |v| counts[0] > v)
            || compass.s.map_or(false, |v| counts[1] > v)
            || compass.e.map_or(false, |v| counts[2] > v)
            || compass.w.map_or(false, |v| counts[3] > v)
        {
            return;
        }

        // Check if all specified compass values are exactly satisfied
        let all_satisfied = compass.n.map_or(true, |v| counts[0] == v)
            && compass.s.map_or(true, |v| counts[1] == v)
            && compass.e.map_or(true, |v| counts[2] == v)
            && compass.w.map_or(true, |v| counts[3] == v);

        if all_satisfied {
            let mut res = current.clone();
            res.sort();
            results.push(res);
            // Don't return — can continue growing in unspecified directions
        }

        if candidates.is_empty() {
            return;
        }

        // Size cap: don't grow beyond eff_max_area or total_cells
        let max_sz = self.eff_max_area.min(self.total_cells);
        if current.len() >= max_sz {
            return;
        }

        // Placement count cap
        if results.len() >= Self::MAX_COMPASS_PLACEMENTS {
            return;
        }

        let mut my_candidates = candidates.clone();
        while let Some(&next) = my_candidates.iter().next() {
            my_candidates.remove(&next);
            candidates.remove(&next);

            let (pr, pc) = self.grid.cell_pos(next);
            let dr = pr as isize - cr_i;
            let dc = pc as isize - cc_i;

            let dir_idx = if dr < 0 {
                0
            } else if dr > 0 {
                1
            } else if dc > 0 {
                2
            } else {
                3
            };

            // Check if this direction is already at its compass limit
            let at_limit = match dir_idx {
                0 => compass.n.map_or(false, |v| counts[0] >= v),
                1 => compass.s.map_or(false, |v| counts[1] >= v),
                2 => compass.e.map_or(false, |v| counts[2] >= v),
                3 => compass.w.map_or(false, |v| counts[3] >= v),
                _ => false,
            };

            // Find newly discovered neighbors (only reachable through next)
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
                    && candidates.insert(neighbor)
                {
                    added.push(neighbor);
                }
            }

            if at_limit {
                // Can't add more cells in this direction.
                // Remove newly discovered neighbors (only reachable through next).
                for a in added {
                    candidates.remove(&a);
                }
                continue;
            }

            // Size cap check
            let max_sz = self.eff_max_area.min(self.total_cells);
            if current.len() + 1 > max_sz {
                for a in added {
                    candidates.remove(&a);
                }
                continue;
            }

            // Add this cell and recurse
            counts[dir_idx] += 1;
            current.push(next);
            self.compass_rec(
                current, counts, candidates, cr_i, cc_i, compass, clue_at, results,
            );
            current.pop();
            counts[dir_idx] -= 1;

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
                CellClue::Compass { compass, .. } => {
                    // Only include compass clues that are constraining enough
                    // for placement enumeration. Loosely constrained clues
                    // (1-2 specified directions without forcing a line shape)
                    // generate too many placements and are better handled
                    // by the edge-based search with propagation.
                    let spec_count = [compass.n, compass.s, compass.e, compass.w]
                        .iter()
                        .filter(|x| x.is_some())
                        .count();
                    let forces_strip = (compass.e == Some(0) && compass.w == Some(0))
                        || (compass.n == Some(0) && compass.s == Some(0));
                    if spec_count < 3 && !forces_strip {
                        continue;
                    }
                    let mut results =
                        self.generate_compass_polyominoes(start_cell, compass, &clue_at);
                    if results.len() > Self::MAX_COMPASS_PLACEMENTS {
                        tracing::info!(
                            cell = ?self.grid.cell_pos(start_cell),
                            placements = results.len(),
                            "compass clue: too many placements, skipping"
                        );
                        results.clear();
                    }
                    for cells in results {
                        let area = cells.len();
                        let sc: Vec<(i32, i32)> = cells
                            .iter()
                            .map(|&c| {
                                let (r, col) = self.grid.cell_pos(c);
                                (r as i32, col as i32)
                            })
                            .collect();
                        let p = Piece {
                            cells,
                            area,
                            canonical: canonical(&polyomino::make_shape(&sc)),
                        };
                        placements.push((clue_idx, p));
                    }
                }
                _ => {}
            }
        }
        placements
    }

    pub(crate) fn solve_grouped_areas(&mut self) {
        let mut area_groups: Vec<(usize, Vec<CellId>)> = Vec::new();
        {
            let mut map: std::collections::HashMap<usize, Vec<CellId>> =
                std::collections::HashMap::new();
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

        let all_anchors: HashSet<CellId> = area_groups
            .iter()
            .flat_map(|(_, a)| a.iter().copied())
            .collect();

        let n_groups = area_groups.len();
        let mut group_placements: Vec<Vec<Vec<CellId>>> = Vec::with_capacity(n_groups);
        for &(area, ref anchors) in &area_groups {
            let forbidden = all_anchors
                .iter()
                .filter(|c| !anchors.contains(c))
                .copied()
                .collect();
            let placements = self.generate_grouped_placements(anchors, area, &forbidden);
            tracing::info!(area, placements = placements.len(), "grouped area placements");
            group_placements.push(placements);
        }

        let mut order: Vec<usize> = (0..n_groups).collect();
        order.sort_by_key(|&i| group_placements[i].len());

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
        solution: &mut Vec<(usize, Vec<CellId>)>,
    ) {
        if self.solution_count >= 2 {
            return;
        }

        let depth = solution.len();
        if depth == order.len() {
            self.report_progress();
            let n = self.grid.num_cells();
            let mut cell_to_piece = vec![usize::MAX; n];
            for (pi, (_, cells)) in solution.iter().enumerate() {
                for &c in cells {
                    cell_to_piece[c] = pi;
                }
            }
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
            if cells.iter().any(|&c| used[c]) {
                continue;
            }

            for &c in cells {
                used[c] = true;
            }
            solution.push((gi, cells.clone()));

            let adj_ok = self.check_grouped_adjacency(solution, area_groups);
            if adj_ok {
                self.grouped_backtrack(order, all_placements, area_groups, used, solution);
            }

            solution.pop();
            for &c in cells {
                used[c] = false;
            }

            if self.solution_count >= 2 {
                return;
            }
        }
    }

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

        let mut cell_to_piece: HashMap<CellId, usize> = HashMap::new();
        for (pi, (_, ref cells)) in solution.iter().enumerate() {
            for &c in cells {
                cell_to_piece.insert(c, pi);
            }
        }

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

        results.sort();
        results.dedup();
        results
    }

    #[allow(clippy::too_many_arguments)]
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

        let unreached = remaining_anchors
            .iter()
            .filter(|a| !in_set.contains(a))
            .count();
        if unreached > 0 && left < unreached {
            return;
        }

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
                    && frontier.insert(other)
                {
                    added.push(other);
                }
            }

            current.push(next);
            self.grouped_grow(
                current,
                in_set,
                frontier,
                target_size,
                forbidden,
                remaining_anchors,
                results,
            );
            current.pop();

            in_set.remove(&next);
            for a in added {
                frontier.remove(&a);
            }
        }
    }
}
