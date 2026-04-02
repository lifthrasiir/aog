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

            if score > best_score {
                best_score = score;
                best_e = Some(e);
                if score >= 200 {
                    break;
                } // Heuristic: found an edge between two needy components
            }
        }

        best_e
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

        for &val in &[EdgeState::Cut, EdgeState::Uncut] {
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
