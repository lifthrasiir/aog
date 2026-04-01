use super::Solver;
use crate::types::*;

impl Solver {
    fn select_edge(&self) -> Option<EdgeId> {
        if self.curr_comp_id.is_empty() {
            return (0..self.grid.num_edges()).find(|&e| self.edges[e] == EdgeState::Unknown);
        }

        let best = (0..self.grid.num_edges())
            .filter(|&e| self.edges[e] == EdgeState::Unknown)
            .filter(|&e| {
                let (c1, c2) = self.grid.edge_cells(e);
                self.grid.cell_exists[c1] && self.grid.cell_exists[c2]
            })
            .map(|e| {
                let (c1, c2) = self.grid.edge_cells(e);
                let mut score = 0i32;

                for &cid in &[c1, c2] {
                    let ci = self.curr_comp_id[cid];
                    let current_sz = self.curr_comp_sz[ci];

                    if let Some(target) = self.curr_target_area[ci] {
                        if current_sz < target {
                            score += 100;
                        } else {
                            score += 1;
                        }
                    } else {
                        score += 10;
                    }
                }

                (e, score)
            })
            .max_by(|(_, s1), (_, s2)| s1.cmp(s2));

        best.map(|(e, _)| e)
    }

    pub(crate) fn backtrack_edges(&mut self) {
        if self.solution_count >= 2 {
            return;
        }

        self.node_count += 1;
        self.report_progress();

        let all_decided = (0..self.grid.num_edges()).all(|e| self.edges[e] != EdgeState::Unknown);
        if all_decided {
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
