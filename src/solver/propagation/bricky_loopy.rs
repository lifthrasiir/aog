use super::super::Solver;
use crate::types::*;

impl Solver {
    pub(crate) fn propagate_bricky_loopy(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        let is_loopy = self.puzzle.rules.loopy;
        for i in 0..=self.grid.rows {
            for j in 0..=self.grid.cols {
                let mut cut_count = 0usize;
                let mut unk_edges = Vec::new();
                for eid in self.grid.vertex_edges(i, j).into_iter().flatten() {
                    match self.edges[eid] {
                        EdgeState::Cut => cut_count += 1,
                        EdgeState::Unknown => unk_edges.push(eid),
                        _ => {}
                    }
                }

                if is_loopy {
                    // Loopy: exactly 3 cuts is forbidden (T-junctions).
                    // 0, 1, 2, 4 are all allowed.
                    if cut_count == 3 && unk_edges.is_empty() {
                        return Err(()); // T-junction confirmed (no unknowns left)
                    }
                    // cut_count=3, n_unk=1: unknown must become Cut to make a
                    // cross (4 cuts = OK); if it stays, T-junction → force Cut.
                    if cut_count == 3 && unk_edges.len() == 1 {
                        if !self.set_edge(unk_edges[0], EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    // cut_count=2, n_unk=1: if unknown becomes Cut, 3 cuts → error
                    // So force unknown to Uncut.
                    if cut_count == 2 && unk_edges.len() == 1 {
                        if !self.set_edge(unk_edges[0], EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    // Dead-end detection: cut_count=1 means this vertex is an
                    // endpoint of a cut path. If all unknown edges lead to
                    // vertices that are already saturated (cut >= required),
                    // this endpoint can never reach the required degree.
                    if cut_count == 1 && unk_edges.len() == 2 {
                        let mut blocked = 0usize;
                        for &eid in &unk_edges {
                            let (is_h_e, r_e, c_e) = self.grid.decode_edge(eid);
                            let (ovi, ovj) = if is_h_e {
                                (r_e + 1, c_e)
                            } else {
                                (r_e, c_e + 1)
                            };
                            let other_cut: usize = self
                                .grid
                                .vertex_edges(ovi, ovj)
                                .into_iter()
                                .flatten()
                                .filter(|e2| self.edges[*e2] == EdgeState::Cut)
                                .count();
                            // Check if the other vertex has a watchtower
                            // constraint that limits cuts to exactly 2
                            let other_clue = self.puzzle.vertex_clues.iter().find(|cl| {
                                let (vi, vj) = self.grid.vertex_pos(cl.vertex);
                                vi == ovi && vj == ovj
                            });
                            if let Some(cl) = other_clue {
                                if other_cut >= cl.value {
                                    blocked += 1;
                                }
                            }
                        }
                        if blocked == unk_edges.len() {
                            return Err(());
                        }
                    }
                } else if self.puzzle.rules.bricky {
                    // Bricky: at most 3 cuts (no cross junctions)
                    if cut_count > 3 {
                        return Err(());
                    }
                    if cut_count + unk_edges.len() > 3 {
                        let must_uncut = cut_count + unk_edges.len() - 3;
                        for &eid in &unk_edges[..must_uncut] {
                            if !self.set_edge(eid, EdgeState::Uncut) {
                                return Err(());
                            }
                            progress = true;
                        }
                    }
                }
            }
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

    #[test]
    fn propagate_bricky_rejects_4_cut() {
        // Need a 3x3 grid so vertex (1,1) has 4 edges
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.bricky = true;

        // Set all 4 edges around vertex (1,1) to Cut
        for eid in s.grid.vertex_edges(1, 1).into_iter().flatten() {
            let _ = s.set_edge(eid, EdgeState::Cut);
        }

        let result = s.propagate_bricky_loopy();
        assert!(
            result.is_err(),
            "bricky: 4 cut edges at vertex should be contradiction"
        );
    }

    #[test]
    fn propagate_loopy_allows_4_cut() {
        // Loopy forbids exactly 3 cuts but allows 0, 1, 2, and 4.
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.loopy = true;

        // Set all 4 edges around vertex (1,1) to Cut
        for eid in s.grid.vertex_edges(1, 1).into_iter().flatten() {
            let _ = s.set_edge(eid, EdgeState::Cut);
        }

        let result = s.propagate_bricky_loopy();
        assert!(
            result.is_ok(),
            "loopy: 4 cut edges at vertex should be allowed"
        );
        assert!(!result.unwrap(), "no progress expected (already decided)");
    }

    #[test]
    fn propagate_loopy_forces_cut_when_3_known_and_1_unknown() {
        // Loopy: if 3 edges are Cut and 1 is Unknown, must force the unknown to Cut
        // (making a cross = 4 cuts, which is allowed). Returning Err would be wrong.
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.loopy = true;

        // Set 3 of 4 edges around vertex (1,1) to Cut, leave the 4th Unknown
        let edges: Vec<_> = s.grid.vertex_edges(1, 1).into_iter().flatten().collect();
        let _ = s.set_edge(edges[0], EdgeState::Cut);
        let _ = s.set_edge(edges[1], EdgeState::Cut);
        let _ = s.set_edge(edges[2], EdgeState::Cut);
        // edges[3] remains Unknown

        let result = s.propagate_bricky_loopy();
        assert!(
            result.is_ok(),
            "loopy: 3 Cut + 1 Unknown should force Cut, not error"
        );
        assert!(
            result.unwrap(),
            "loopy: should have made progress (forced the 4th edge to Cut)"
        );
        assert_eq!(
            s.edges[edges[3]],
            EdgeState::Cut,
            "loopy: 4th edge should be forced to Cut to make cross junction"
        );
    }

    #[test]
    fn propagate_loopy_rejects_3_cut_confirmed() {
        // Loopy forbids exactly 3 cuts with no unknowns (T-junction confirmed).
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.loopy = true;

        // Set 3 of 4 edges around vertex (1,1) to Cut, set 4th to Uncut
        let edges: Vec<_> = s.grid.vertex_edges(1, 1).into_iter().flatten().collect();
        let _ = s.set_edge(edges[0], EdgeState::Cut);
        let _ = s.set_edge(edges[1], EdgeState::Cut);
        let _ = s.set_edge(edges[2], EdgeState::Cut);
        let _ = s.set_edge(edges[3], EdgeState::Uncut);

        let result = s.propagate_bricky_loopy();
        assert!(
            result.is_err(),
            "loopy: exactly 3 Cut + 1 Uncut (T-junction confirmed) should be contradiction"
        );
    }
}
