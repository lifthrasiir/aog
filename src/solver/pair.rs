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
        // Manual DIFFs from branching
        for &(d1, d2) in &self.manual_diffs {
            if (d1 == c1 && d2 == c2) || (d1 == c2 && d2 == c1) {
                return true;
            }
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
