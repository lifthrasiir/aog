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

        // Precompute clue-constrained components: components adjacent to any
        // clue edge (inequality, diff, gemini, delta). These components have
        // shape or size constraints from edge clues.
        let has_clue_edges = !self.clue_cut_edges.is_empty();
        let mut clue_constrained_comp: Vec<bool> = if has_clue_edges {
            vec![false; self.curr_comp_sz.len()]
        } else {
            Vec::new()
        };
        if has_clue_edges {
            for &ce in &self.clue_cut_edges {
                if self.edges[ce] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(ce);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                clue_constrained_comp[ci1] = true;
                clue_constrained_comp[ci2] = true;
            }
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

            let sealed1 = ci1 < self.can_grow_buf.len() && !self.can_grow_buf[ci1];
            let sealed2 = ci2 < self.can_grow_buf.len() && !self.can_grow_buf[ci2];

            // General bonuses (apply regardless of size_separation)

            // Bonus: cutting would seal a component that has a target area
            if ci1 < self.cached_growth_edge_count.len()
                && self.cached_growth_edge_count[ci1] == 1
                && self.curr_target_area[ci1].is_some()
            {
                score += 75;
            }
            if ci2 < self.cached_growth_edge_count.len()
                && self.cached_growth_edge_count[ci2] == 1
                && self.curr_target_area[ci2].is_some()
            {
                score += 75;
            }

            // Bonus: edge between sealed and target component
            if sealed1 ^ sealed2 {
                let (other_ci, _) = if sealed1 { (ci2, ci1) } else { (ci1, ci2) };
                if self.curr_target_area[other_ci].is_some() {
                    score += 50;
                }
            }

            // Bonus: edge adjacent to a clue-constrained component
            // (component touching a gemini/delta/inequality/diff edge)
            if has_clue_edges
                && (clue_constrained_comp[ci1] || clue_constrained_comp[ci2]) {
                    score += 30;
                }

            // Bonus: edge incident to a watchtower vertex
            if !self.watchtower_vertices.is_empty() {
                let (is_h, r, c) = self.grid.decode_edge(e);
                let v1 = self.grid.vertex(r, c);
                let v2 = if is_h {
                    self.grid.vertex(r + 1, c)
                } else {
                    self.grid.vertex(r, c + 1)
                };
                if self.watchtower_vertices.contains(&v1) || self.watchtower_vertices.contains(&v2)
                {
                    score += 25;
                }
            }

            // Size separation heuristic bonuses
            if self.puzzle.rules.size_separation {
                // Bonus: Uncut would create merge-size conflict with sealed neighbor
                if let Some(ref sns) = self.cached_sealed_neighbor_sizes {
                    if ci1 < sns.len() && ci2 < sns.len() {
                        let merged_sz = sz1 + sz2;
                        if sns[ci1].contains(&merged_sz) || sns[ci2].contains(&merged_sz) {
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

                // Additional sealing bonus: cutting would seal any component
                // (even without target, sealing helps size_separation propagation)
                if ci1 < self.cached_growth_edge_count.len()
                    && self.cached_growth_edge_count[ci1] == 1
                {
                    score += 30;
                }
                if ci2 < self.cached_growth_edge_count.len()
                    && self.cached_growth_edge_count[ci2] == 1
                {
                    score += 30;
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
        if self.puzzle.rules.size_separation {
            if let Some(ref sns) = self.cached_sealed_neighbor_sizes {
                if ci1 < sns.len() && ci2 < sns.len() {
                    let merged_sz = sz1 + sz2;
                    if sns[ci1].contains(&merged_sz) || sns[ci2].contains(&merged_sz) {
                        return true;
                    }
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
            && self.puzzle.rules.size_separation
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
                self.save_solution(pieces);
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
