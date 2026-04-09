use super::super::Solver;
use crate::types::*;

impl Solver {
    /// Geometric interaction between Gemini and Delta clues at a vertex.
    ///
    /// If a Gemini edge and a Delta edge (both same orientation) meet at a vertex,
    /// the two orthogonal edges at that vertex cannot BOTH be Uncut, because that
    /// would merge the pieces on both sides, requiring Shape(L) == Shape(R)
    /// (Gemini) and Shape(L) != Shape(R) (Delta) simultaneously.
    ///
    /// If Bricky rule is on, they also cannot BOTH be Cut (as clues are already Cut).
    pub(crate) fn propagate_delta_gemini_interaction(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        let mut edge_kinds = vec![None; self.grid.num_edges()];
        for clue in &self.puzzle.edge_clues {
            edge_kinds[clue.edge] = Some(clue.kind);
        }

        for r in 0..=self.grid.rows {
            for c in 0..=self.grid.cols {
                let ve = self.grid.vertex_edges(r, c);
                let (h_west, h_east) = ve.horiz;
                let (v_north, v_south) = ve.vert;

                // Case 1: Gemini/Delta on collinear h_edge pair (h_west, h_east).
                // They form a straight horizontal line through vertex (r, c).
                // Transverse edges are (v_north, v_south).
                if let (Some(e1), Some(e2)) = (h_west, h_east) {
                    if matches!(
                        (edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta))
                            | (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini))
                    ) {
                        if let (Some(t1), Some(t2)) = (v_north, v_south) {
                            progress |= self.propagate_transverse_pair(t1, t2)?;
                        }
                    }
                }

                // Case 2: Gemini/Delta on collinear v_edge pair (v_north, v_south).
                // They form a straight vertical line through vertex (r, c).
                // Transverse edges are (h_west, h_east).
                if let (Some(e1), Some(e2)) = (v_north, v_south) {
                    if matches!(
                        (edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta))
                            | (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini))
                    ) {
                        if let (Some(t1), Some(t2)) = (h_west, h_east) {
                            progress |= self.propagate_transverse_pair(t1, t2)?;
                        }
                    }
                }
            }
        }

        Ok(progress)
    }

    fn propagate_transverse_pair(&mut self, e1: EdgeId, e2: EdgeId) -> Result<bool, ()> {
        let mut progress = false;
        let s1 = self.edges[e1];
        let s2 = self.edges[e2];

        // 1. Cannot both be Uncut
        if s1 == EdgeState::Uncut && s2 == EdgeState::Uncut {
            return Err(());
        }
        if s1 == EdgeState::Uncut && s2 == EdgeState::Unknown && self.set_edge(e2, EdgeState::Cut) {
            progress = true;
        }
        if s2 == EdgeState::Uncut && s1 == EdgeState::Unknown && self.set_edge(e1, EdgeState::Cut) {
            progress = true;
        }

        // 2. If Bricky, cannot both be Cut
        if self.puzzle.rules.bricky {
            if s1 == EdgeState::Cut && s2 == EdgeState::Cut {
                return Err(());
            }
            if s1 == EdgeState::Cut
                && s2 == EdgeState::Unknown
                && self.set_edge(e2, EdgeState::Uncut)
            {
                progress = true;
            }
            if s2 == EdgeState::Cut
                && s1 == EdgeState::Unknown
                && self.set_edge(e1, EdgeState::Uncut)
            {
                progress = true;
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
    fn propagate_delta_gemini_h_pair_forces_cut() {
        // 3x3 grid. Interior vertex (1,1) has all 4 edges.
        // h_west=H(0,0) and h_east=H(0,1) form a collinear h_edge pair at vertex(1,1).
        // With Gemini on h_west and Delta on h_east, transverse (v_north, v_south)
        // cannot both be Uncut. If v_north=Uncut → v_south must be Cut.
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
        let h_west = s.grid.h_edge(0, 0); // west h_edge at vertex(1,1)
        let h_east = s.grid.h_edge(0, 1); // east h_edge at vertex(1,1)
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_west,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_east,
            kind: EdgeClueKind::Delta,
        });
        s.edges[h_west] = EdgeState::Cut;
        s.edges[h_east] = EdgeState::Cut;

        let v_north = s.grid.v_edge(0, 0); // north v_edge at vertex(1,1)
        let v_south = s.grid.v_edge(1, 0); // south v_edge at vertex(1,1)
        let _ = s.set_edge(v_north, EdgeState::Uncut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[v_south], EdgeState::Cut);
    }

    #[test]
    fn propagate_delta_gemini_h_pair_bricky_forces_uncut() {
        // Same setup but with bricky rule: if v_north=Cut → v_south must be Uncut.
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
        let h_west = s.grid.h_edge(0, 0);
        let h_east = s.grid.h_edge(0, 1);
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_west,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_east,
            kind: EdgeClueKind::Delta,
        });
        s.edges[h_west] = EdgeState::Cut;
        s.edges[h_east] = EdgeState::Cut;

        let v_north = s.grid.v_edge(0, 0);
        let v_south = s.grid.v_edge(1, 0);
        let _ = s.set_edge(v_north, EdgeState::Cut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[v_south], EdgeState::Uncut);
    }

    #[test]
    fn propagate_delta_gemini_v_pair_forces_cut() {
        // 3x3 grid. v_north=V(0,0) and v_south=V(1,0) form a collinear v_edge pair
        // at vertex(1,1). Gemini on v_north, Delta on v_south → transverse
        // (h_west, h_east) cannot both be Uncut. If h_west=Uncut → h_east must be Cut.
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
        let v_north = s.grid.v_edge(0, 0); // north v_edge at vertex(1,1)
        let v_south = s.grid.v_edge(1, 0); // south v_edge at vertex(1,1)
        s.puzzle.edge_clues.push(EdgeClue {
            edge: v_north,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: v_south,
            kind: EdgeClueKind::Delta,
        });
        s.edges[v_north] = EdgeState::Cut;
        s.edges[v_south] = EdgeState::Cut;

        let h_west = s.grid.h_edge(0, 0); // west h_edge at vertex(1,1)
        let h_east = s.grid.h_edge(0, 1); // east h_edge at vertex(1,1)
        let _ = s.set_edge(h_west, EdgeState::Uncut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[h_east], EdgeState::Cut);
    }
}
