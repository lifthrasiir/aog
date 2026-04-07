use super::Solver;
use crate::types::*;

/// Cell-pair data for rose window puzzles.
/// Stores rose cells grouped by type for pair-based branching.
pub(crate) struct CellPairLayer {
    /// Rose cells grouped by type index. rose_by_type[0]=A cells, [1]=B, etc.
    rose_by_type: Vec<Vec<CellId>>,
}

impl CellPairLayer {
    /// Build from solver state. Static rose info is computed once.
    pub(crate) fn new(nc: usize, rose_bits_all: u8, cell_rose_sym: &[u8]) -> Self {
        let num_types = {
            let mut max_sym = 0u8;
            for c in 0..nc {
                if cell_rose_sym[c] != u8::MAX && cell_rose_sym[c] > max_sym {
                    max_sym = cell_rose_sym[c];
                }
            }
            if rose_bits_all == 0 {
                0
            } else {
                (max_sym as usize) + 1
            }
        };

        let mut rose_by_type: Vec<Vec<CellId>> = vec![Vec::new(); num_types];

        for c in 0..nc {
            if cell_rose_sym[c] != u8::MAX {
                let sym = cell_rose_sym[c];
                rose_by_type[sym as usize].push(c);
            }
        }

        Self { rose_by_type }
    }

    /// Accessor for rose_by_type.
    #[inline]
    pub(crate) fn rose_by_type(&self) -> &[Vec<CellId>] {
        &self.rose_by_type
    }
}

impl Solver {
    // --- Inline DIFF check (no rebuild needed) ---

    /// Check if two cells must be in different pieces.
    /// Uses only Solver's existing state: cell_rose_sym, manual_diffs, edges.
    #[inline]
    fn is_diff_inline(&self, c1: CellId, c2: CellId) -> bool {
        // Same-type rose cells are always DIFF
        let s1 = self.cell_rose_sym[c1];
        let s2 = self.cell_rose_sym[c2];
        if s1 != u8::MAX && s1 == s2 {
            return true;
        }
        // Manual DIFFs from branching (use precomputed set for O(1) lookup)
        if self.manual_diff_set.contains(&(c1, c2))
            || self.manual_diff_set.contains(&(c2, c1))
        {
            return true;
        }
        // Direct Cut edge between them
        for eid in self.grid.cell_edges(c1).into_iter().flatten() {
            let (a, b) = self.grid.edge_cells(eid);
            let other = if a == c1 { b } else { a };
            if other == c2 && self.edges[eid] == EdgeState::Cut {
                return true;
            }
        }
        false
    }

    // --- Pair-based branching methods ---

    /// BFS from c1 through Uncut+Unknown edges to find c2.
    /// Fills self.bfs_prev with path reconstruction data.
    pub(crate) fn bfs_path(&mut self, c1: CellId, c2: CellId) -> bool {
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        self.bfs_prev.resize(n, None);
        self.rose_visited[c1] = true;
        self.q_buf.clear();
        self.q_buf.push(c1);

        while let Some(cur) = self.q_buf.pop() {
            if cur == c2 {
                return true;
            }
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                if self.edges[eid] == EdgeState::Cut {
                    continue;
                }
                let (a, b) = self.grid.edge_cells(eid);
                let other = if a == cur { b } else { a };
                if !self.grid.cell_exists[other] || self.rose_visited[other] {
                    continue;
                }
                self.rose_visited[other] = true;
                self.bfs_prev[other] = Some((cur, eid));
                self.q_buf.push(other);
            }
        }
        false
    }

    /// Branch: force c1 and c2 into the same piece.
    /// BFS from c1 to c2 through Uncut+Unknown, set all Unknown edges on path to Uncut.
    pub(crate) fn branch_pair_same(&mut self, c1: CellId, c2: CellId) -> Result<(), ()> {
        if self.curr_comp_id[c1] == self.curr_comp_id[c2] {
            return Ok(());
        }
        if !self.bfs_path(c1, c2) {
            return Err(());
        }
        let mut cur = c2;
        while cur != c1 {
            if let Some((prev, eid)) = self.bfs_prev[cur] {
                if self.edges[eid] == EdgeState::Unknown {
                    if !self.set_edge(eid, EdgeState::Uncut) {
                        return Err(());
                    }
                }
                cur = prev;
            } else {
                return Err(());
            }
        }
        Ok(())
    }

    // --- Compass membership branching ---

    /// Select up to K independent compass membership pairs for flat branching.
    /// Returns pairs sorted by score (highest first).
    pub(crate) fn select_compass_branches_flat(&mut self, max_pairs: usize) -> Vec<(CellId, CellId)> {
        if !self.has_compass_clue || self.curr_comp_id.is_empty() {
            return Vec::new();
        }

        let n = self.grid.num_cells();
        let num_comp = self.curr_comp_sz.len();

        // Collect compass cells with non-empty compass data
        let compass_cells: Vec<(CellId, CompassData)> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| {
                if let CellClue::Compass { cell, compass } = cl {
                    if self.grid.cell_exists[*cell] {
                        if compass.n.is_none()
                            && compass.s.is_none()
                            && compass.e.is_none()
                            && compass.w.is_none()
                        {
                            return None;
                        }
                        let ci = self.curr_comp_id[*cell];
                        if ci < num_comp && self.can_grow_buf[ci] {
                            return Some((*cell, compass.clone()));
                        }
                    }
                }
                None
            })
            .collect();

        if compass_cells.is_empty() {
            return Vec::new();
        }

        // Collect all candidate pairs with scores
        let mut candidates: Vec<(i32, CellId, CellId)> = Vec::new();

        for &(compass_cell, ref compass) in &compass_cells {
            let ci_compass = self.curr_comp_id[compass_cell];
            let (cr, cc) = self.grid.cell_pos(compass_cell);
            let (cr_i, cc_i) = (cr as isize, cc as isize);

            // Precompute direction counts for compass cell's own component
            let mut self_dir_counts = [0usize; 4];
            for &c in &self.comp_cells[ci_compass] {
                let (pr, pc) = self.grid.cell_pos(c);
                if (pr as isize) < cr_i {
                    self_dir_counts[0] += 1;
                }
                if (pr as isize) > cr_i {
                    self_dir_counts[1] += 1;
                }
                if (pc as isize) > cc_i {
                    self_dir_counts[2] += 1;
                }
                if (pc as isize) < cc_i {
                    self_dir_counts[3] += 1;
                }
            }

            // BFS from compass cell through Unknown+Uncut edges
            self.rose_visited[..n].fill(false);
            self.bfs_prev.resize(n, None);
            self.rose_visited[compass_cell] = true;
            self.q_buf.clear();
            self.q_buf.push(compass_cell);

            let mut seen_comps = vec![false; num_comp];
            seen_comps[ci_compass] = true;

            while let Some(cur) = self.q_buf.pop() {
                let cur_ci = self.curr_comp_id[cur];

                if cur_ci != ci_compass && cur_ci < num_comp && !seen_comps[cur_ci] {
                    seen_comps[cur_ci] = true;

                    if self.can_grow_buf[cur_ci] {
                        let target_cell = self.comp_cells[cur_ci][0];
                        if self.is_diff_inline(compass_cell, target_cell) {
                            continue;
                        }

                        let mut dist = 0usize;
                        let mut tmp = cur;
                        while tmp != compass_cell {
                            if let Some((p, _)) = self.bfs_prev[tmp] {
                                dist += 1;
                                tmp = p;
                            } else {
                                dist = usize::MAX;
                                break;
                            }
                        }

                        if dist > 6 {
                            continue;
                        }

                        let mut tightness = 0i32;
                        let mut would_contradict = false;

                        let dirs: [(Option<usize>, usize); 4] = [
                            (compass.n, 0),
                            (compass.s, 1),
                            (compass.e, 2),
                            (compass.w, 3),
                        ];

                        let mut target_dir_counts = [0usize; 4];
                        for &c in &self.comp_cells[cur_ci] {
                            let (pr, pc) = self.grid.cell_pos(c);
                            if (pr as isize) < cr_i {
                                target_dir_counts[0] += 1;
                            }
                            if (pr as isize) > cr_i {
                                target_dir_counts[1] += 1;
                            }
                            if (pc as isize) > cc_i {
                                target_dir_counts[2] += 1;
                            }
                            if (pc as isize) < cc_i {
                                target_dir_counts[3] += 1;
                            }
                        }

                        for &(val, idx) in &dirs {
                            let Some(v) = val else { continue };
                            let combined = self_dir_counts[idx] + target_dir_counts[idx];
                            if combined > v {
                                would_contradict = true;
                                break;
                            } else if combined == v {
                                tightness += 50;
                            } else if combined == v - 1 {
                                tightness += 30;
                            }
                        }

                        if would_contradict {
                            continue;
                        }

                        let mut score: i32 = 100 + tightness;

                        // Bonus: target component contributes cells to a
                        // compass-constrained below-limit direction. This means
                        // the SAME branch creates meaningful pruning power.
                        let mut constrained_contribution = false;
                        for &(val, idx) in &dirs {
                            let Some(v) = val else { continue };
                            let combined = self_dir_counts[idx] + target_dir_counts[idx];
                            if combined < v && target_dir_counts[idx] > 0 {
                                constrained_contribution = true;
                                score += 40;
                            }
                        }
                        // Penalty: target doesn't help any constrained direction
                        if !constrained_contribution {
                            score -= 30;
                        }

                        if dist >= 2 && dist <= 4 {
                            score += 30;
                        } else if dist == 1 {
                            score += 15;
                        }

                        let comp_has_compass = self.comp_cells[cur_ci].iter().any(|&c| {
                            self.cell_clues_indexed[c].iter().any(|&idx| {
                                matches!(
                                    &self.puzzle.cell_clues[idx],
                                    CellClue::Compass { .. }
                                )
                            })
                        });
                        if comp_has_compass {
                            score += 60;
                        }

                        if self.curr_target_area[cur_ci].is_some() {
                            score += 30;
                        }

                        if self.curr_comp_sz[cur_ci] >= 3 {
                            score += 10;
                        }

                        if score >= 140 {
                            candidates.push((score, compass_cell, cur));
                        }
                    }
                }

                // Expand BFS
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if !self.grid.cell_exists[other] || self.rose_visited[other] {
                        continue;
                    }
                    self.rose_visited[other] = true;
                    self.bfs_prev[other] = Some((cur, eid));
                    self.q_buf.push(other);
                }
            }
        }

        // Sort by score descending, select independent pairs
        candidates.sort_by(|a, b| b.0.cmp(&a.0));

        let mut selected: Vec<(CellId, CellId)> = Vec::new();
        let mut used_compass: Vec<bool> = vec![false; n];
        let mut used_comp: Vec<bool> = vec![false; num_comp];

        for (_score, compass_cell, target_cell) in &candidates {
            if selected.len() >= max_pairs {
                break;
            }
            if used_compass[*compass_cell] {
                continue;
            }
            let target_ci = self.curr_comp_id[*target_cell];
            if used_comp[target_ci] {
                continue;
            }
            let compass_ci = self.curr_comp_id[*compass_cell];
            if used_comp[compass_ci] {
                continue;
            }

            used_compass[*compass_cell] = true;
            used_comp[target_ci] = true;
            used_comp[compass_ci] = true;
            selected.push((*compass_cell, *target_cell));
        }

        selected
    }

    /// Flat compass branching: make all compass membership decisions first,
    /// then fall back to edge branching.
    pub(crate) fn branch_compass_flat(&mut self, pairs: Vec<(CellId, CellId)>) {
        self.branch_compass_flat_inner(&pairs, 0);
    }

    fn branch_compass_flat_inner(&mut self, pairs: &[(CellId, CellId)], idx: usize) {
        if self.solution_count >= 2 {
            return;
        }

        if idx >= pairs.len() {
            self.backtrack_edges();
            return;
        }

        let (compass_cell, target_cell) = pairs[idx];

        // SAME branch
        {
            let snap = self.snapshot();
            if self.branch_pair_same(compass_cell, target_cell).is_ok() {
                if self.propagate().is_ok() {
                    self.branch_compass_flat_inner(pairs, idx + 1);
                }
            }
            self.restore(snap);
        }

        if self.solution_count >= 2 {
            return;
        }

        // DIFF branch
        {
            let snap = self.snapshot();
            self.manual_diffs.push((compass_cell, target_cell));
            self.manual_diff_set.insert((compass_cell, target_cell));
            if self.propagate().is_ok() {
                self.branch_compass_flat_inner(pairs, idx + 1);
            }
            self.restore(snap);
        }
    }

    // --- Rose pair branching ---

    /// Branch on a rose cell pair (c1, c2): try SAME, then try DIFF.
    pub(crate) fn branch_on_pair(&mut self, c1: CellId, c2: CellId) {
        let in_same_comp = self.curr_comp_id[c1] == self.curr_comp_id[c2];

        if !in_same_comp {
            // --- Branch 1: SAME (force them into the same piece) ---
            let snap = self.snapshot();
            if self.branch_pair_same(c1, c2).is_ok() {
                if self.propagate().is_ok() {
                    self.backtrack_edges();
                }
            }
            self.restore(snap);
        }

        if self.solution_count >= 2 {
            return;
        }

        // --- Branch 2: DIFF (force them into different pieces) ---
        let snap = self.snapshot();
        self.manual_diffs.push((c1, c2));
        self.manual_diff_set.insert((c1, c2));
        if self.propagate().is_ok() {
            self.backtrack_edges();
        }
        self.restore(snap);
    }

    /// Select the best rose cell pair to branch on, if any pair scores
    /// higher than the given edge_score threshold.
    /// Uses a single BFS per type-0 rose cell to find cross-type pairs cheaply.
    pub(crate) fn select_rose_pair(&mut self, edge_score: i32) -> Option<(CellId, CellId)> {
        let pl = self.pair_layer.as_ref()?;
        if self.curr_comp_id.is_empty() {
            return None;
        }

        let rose_by_type: &[Vec<CellId>] = pl.rose_by_type();
        if rose_by_type.len() < 2 {
            return None;
        }
        let num_types = rose_by_type.len();
        let sym = &self.cell_rose_sym;
        let n = self.grid.num_cells();

        // Precompute rose type count per component for scoring
        let num_comp = self.curr_comp_sz.len();
        let mut comp_rose_count: Vec<u8> = vec![0; num_comp];
        for ci in 0..num_comp {
            if ci >= self.comp_cells.len() {
                break;
            }
            let mut mask: u8 = 0;
            for &c in &self.comp_cells[ci] {
                if sym[c] != u8::MAX {
                    mask |= 1 << sym[c];
                }
            }
            comp_rose_count[ci] = mask.count_ones() as u8;
        }

        let mut best_pair: Option<(CellId, CellId)> = None;
        let mut best_pair_score: i32 = edge_score;

        // Only BFS from type-0 cells (each pair (T0, Tx) is covered once)
        for &c1 in &rose_by_type[0] {
            let ci1 = self.curr_comp_id[c1];

            // BFS from c1 through Uncut+Unknown, excluding type-0 cells
            self.rose_visited[..n].fill(false);
            self.bfs_prev.resize(n, None);
            self.rose_visited[c1] = true;
            self.q_buf.clear();
            self.q_buf.push(c1);

            while let Some(cur) = self.q_buf.pop() {
                let cur_sym = sym[cur];

                // Skip type-0 cells (must be DIFF by rose rule)
                if cur_sym != u8::MAX && cur_sym == 0 && cur != c1 {
                    continue;
                }

                // Check if non-type-0 rose cell in different component
                if cur_sym != u8::MAX {
                    let ci2 = self.curr_comp_id[cur];
                    if ci1 != ci2 && !self.is_diff_inline(c1, cur) {
                        let mut dist = 0usize;
                        let mut tmp = cur;
                        while tmp != c1 {
                            if let Some((p, _)) = self.bfs_prev[tmp] {
                                dist += 1;
                                tmp = p;
                            } else {
                                break;
                            }
                        }

                        let mut score: i32 = 150;
                        if dist <= 2 {
                            score += 50;
                        }
                        if comp_rose_count[ci1] >= (num_types as u8) - 1 {
                            score += 40;
                        }
                        if comp_rose_count[ci2] >= (num_types as u8) - 1 {
                            score += 40;
                        }

                        if score > best_pair_score {
                            best_pair_score = score;
                            best_pair = Some((c1, cur));
                        }

                        if best_pair_score >= 280 {
                            return best_pair;
                        }
                    }
                }

                // Expand BFS
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (a, b) = self.grid.edge_cells(eid);
                    let other = if a == cur { b } else { a };
                    if !self.grid.cell_exists[other] || self.rose_visited[other] {
                        continue;
                    }
                    self.rose_visited[other] = true;
                    self.bfs_prev[other] = Some((cur, eid));
                    self.q_buf.push(other);
                }
            }
        }

        best_pair
    }
}
