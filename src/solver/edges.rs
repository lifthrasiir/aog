use super::Solver;
use crate::types::*;

impl Solver {
    fn select_edge(&self) -> Option<EdgeId> {
        let num_edges = self.grid.num_edges();
        if self.curr_comp_id.is_empty() {
            for e in 0..num_edges {
                if self.edges[e] == EdgeState::Unknown {
                    return Some(e);
                }
            }
            return None;
        }

        let mut best_e = None;
        let mut best_score = i32::MIN;

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }

            let mut score = 0i32;
            // Cell 1
            let ci1 = self.curr_comp_id[c1];
            let sz1 = self.curr_comp_sz[ci1];
            if let Some(target) = self.curr_target_area[ci1] {
                score += if sz1 < target { 100 } else { 1 };
            } else {
                score += 10;
            }
            // Cell 2
            let ci2 = self.curr_comp_id[c2];
            let sz2 = self.curr_comp_sz[ci2];
            if let Some(target) = self.curr_target_area[ci2] {
                score += if sz2 < target { 100 } else { 1 };
            } else {
                score += 10;
            }

            // Size separation heuristic bonuses (Proposal C)
            if self.puzzle.rules.size_separation {
                let sealed1 =
                    ci1 < self.can_grow_buf.len() && !self.can_grow_buf[ci1];
                let sealed2 =
                    ci2 < self.can_grow_buf.len() && !self.can_grow_buf[ci2];

                // Bonus: Uncut would create merge-size conflict with sealed neighbor
                if let Some(ref sns) = self.cached_sealed_neighbor_sizes {
                    if ci1 < sns.len() && ci2 < sns.len() {
                        let merged_sz = sz1 + sz2;
                        if sns[ci1].contains(&merged_sz)
                            || sns[ci2].contains(&merged_sz)
                        {
                            score += 200; // immediate contradiction if Uncut
                        }
                    }
                }

                // Bonus: edge between same-size no-target components
                // (size separation requires them to differ)
                if sz1 == sz2
                    && self.curr_target_area[ci1].is_none()
                    && self.curr_target_area[ci2].is_none()
                {
                    score += 80;
                }

                // Bonus: edge adjacent to a sealed component
                if sealed1 ^ sealed2 {
                    score += 50;
                }

                // Bonus: cutting this edge would seal a component
                if ci1 < self.cached_growth_edge_count.len()
                    && self.cached_growth_edge_count[ci1] == 1
                {
                    score += 75;
                }
                if ci2 < self.cached_growth_edge_count.len()
                    && self.cached_growth_edge_count[ci2] == 1
                {
                    score += 75;
                }
            }

            if score > best_score {
                best_score = score;
                best_e = Some(e);
                if score >= 200 {
                    break;
                }
            }
        }

        best_e
    }

    /// For the selected edge, determine whether to try Cut or Uncut first.
    /// Returns true if Cut should be tried first (default), false if Uncut first.
    fn prefer_cut_first(&self, e: EdgeId) -> bool {
        if !self.puzzle.rules.size_separation {
            return true;
        }
        if self.curr_comp_id.is_empty() {
            return true;
        }
        let (c1, c2) = self.grid.edge_cells(e);
        if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
            return true;
        }
        let ci1 = self.curr_comp_id[c1];
        let ci2 = self.curr_comp_id[c2];
        let sz1 = self.curr_comp_sz[ci1];
        let sz2 = self.curr_comp_sz[ci2];

        // If Uncut would create a merge-size conflict, definitely try Cut first
        if let Some(ref sns) = self.cached_sealed_neighbor_sizes {
            if ci1 < sns.len() && ci2 < sns.len() {
                let merged_sz = sz1 + sz2;
                if sns[ci1].contains(&merged_sz) || sns[ci2].contains(&merged_sz) {
                    return true;
                }
            }
        }

        // If both components are same-size no-target, Uncut first:
        // merging them creates a larger component that's more likely to be
        // uniquely sized, while cutting leaves two same-size neighbors
        // that will need further constraint.
        if sz1 == sz2
            && self.curr_target_area[ci1].is_none()
            && self.curr_target_area[ci2].is_none()
        {
            return false;
        }

        // If one side is needy (below target), prefer Uncut to help it grow
        if let Some(target) = self.curr_target_area[ci1] {
            if sz1 < target {
                return false;
            }
        }
        if let Some(target) = self.curr_target_area[ci2] {
            if sz2 < target {
                return false;
            }
        }

        true
    }

    pub(crate) fn backtrack_edges(&mut self) {
        if self.solution_count >= 2 {
            return;
        }

        self.node_count += 1;
        self.report_progress();

        if self.curr_unknown == 0 {
            let pieces = self.compute_pieces();
            if self.validate(&pieces) {
                // Deduplicate: skip if same edge assignment as previous solution
                if self.solution_count > 0 && self.edges == self.best_edges {
                    return;
                }
                self.solution_count += 1;
                self.best_pieces = pieces;
                self.best_edges = self.edges.clone();
                self.report_solution(self.solution_count);
            }
            return;
        }

        let e = match self.select_edge() {
            Some(e) => e,
            None => return,
        };

        let cut_first = self.prefer_cut_first(e);
        let order: &[EdgeState; 2] = if cut_first {
            &[EdgeState::Cut, EdgeState::Uncut]
        } else {
            &[EdgeState::Uncut, EdgeState::Cut]
        };

        for &val in order {
            let snap = self.changed.len();
            if !self.set_edge(e, val) {
                continue;
            }
            if self.propagate().is_ok() {
                self.backtrack_edges();
            }
            self.restore(snap);
        }
    }
}
