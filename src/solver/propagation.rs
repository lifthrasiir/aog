use super::Solver;
use crate::types::*;

impl Solver {
    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            if self.puzzle.rules.bricky || self.puzzle.rules.loopy {
                progress |= self.propagate_bricky_loopy()?;
            }
            progress |= self.propagate_delta_gemini_interaction()?;
            progress |= self.propagate_area_bounds()?;
            progress |= self.propagate_rose_separation()?;
            progress |= self.propagate_same_area_reachability()?;
            progress |= self.propagate_palisade_constraints()?;
            progress |= self.propagate_compass()?;
            progress |= self.propagate_watchtower()?;

            if !progress {
                // Failed literal detection (probing): probe each unknown edge
                // to see if one value causes contradiction. Uses recursion guard
                // to prevent infinite loop when called from within a probe.
                if !self.in_probing
                    && self.rose_bits_all != 0
                    && self.curr_unknown > 0
                    && self.curr_unknown <= 256
                {
                    let saved = self.in_probing;
                    self.in_probing = true;
                    progress |= self.probe_one_round()?;
                    self.in_probing = saved;
                }

                if !progress {
                    return Ok(true);
                }
            }
        }
    }

    /// Single round of failed literal detection: for each unknown edge,
    /// temporarily assign Cut and Uncut, run propagation, and if one causes
    /// contradiction, force the opposite value.
    fn probe_one_round(&mut self) -> Result<bool, ()> {
        let mut forced = 0usize;
        let num_edges = self.grid.num_edges();

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }

            // Probe Cut
            let snap = self.changed.len();
            let cut_ok = self.set_edge(e, EdgeState::Cut)
                && self.propagate().is_ok();
            self.restore(snap);

            if !cut_ok {
                // Cut contradicts → force Uncut
                if self.edges[e] == EdgeState::Unknown {
                    if self.set_edge(e, EdgeState::Uncut) {
                        forced += 1;
                        self.propagate()?;
                    }
                }
                continue;
            }

            if self.edges[e] != EdgeState::Unknown {
                continue; // forced by a previous probe's cascade
            }

            // Probe Uncut
            let snap = self.changed.len();
            let uncut_ok = self.set_edge(e, EdgeState::Uncut)
                && self.propagate().is_ok();
            self.restore(snap);

            if !uncut_ok {
                // Uncut contradicts → force Cut
                if self.edges[e] == EdgeState::Unknown {
                    if self.set_edge(e, EdgeState::Cut) {
                        forced += 1;
                        self.propagate()?;
                    }
                }
            }
        }

        Ok(forced > 0)
    }

    pub(crate) fn propagate_bricky_loopy(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        for i in 1..self.grid.rows {
            for j in 1..self.grid.cols {
                let mut cut_count = 0usize;
                let mut unk_edges = Vec::new();
                for eid in self.grid.vertex_edges(i, j).into_iter().flatten() {
                    match self.edges[eid] {
                        EdgeState::Cut => cut_count += 1,
                        EdgeState::Unknown => unk_edges.push(eid),
                        _ => {}
                    }
                }
                let max_cut = if self.puzzle.rules.loopy { 2 } else { 3 };

                if cut_count > max_cut {
                    return Err(());
                }
                if cut_count + unk_edges.len() > max_cut {
                    let must_uncut = cut_count + unk_edges.len() - max_cut;
                    for &eid in &unk_edges[..must_uncut] {
                        if !self.set_edge(eid, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            }
        }
        Ok(progress)
    }

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
                let [h_up, h_down, v_left, v_right] = self.grid.vertex_edges(r, c);

                // Case 1: Gemini/Delta on vertical stack (h_up, h_down)
                // Note: h_edge is a HORIZONTAL line, but h_up/h_down form a VERTICAL stack.
                if let (Some(e1), Some(e2)) = (h_up, h_down) {
                    if matches!((edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta)) |
                        (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini)))
                    {
                        if let (Some(t1), Some(t2)) = (v_left, v_right) {
                            progress |= self.propagate_transverse_pair(t1, t2)?;
                        }
                    }
                }

                // Case 2: Gemini/Delta on horizontal stack (v_left, v_right)
                // Note: v_edge is a VERTICAL line, but v_left/v_right form a HORIZONTAL stack.
                if let (Some(e1), Some(e2)) = (v_left, v_right) {
                    if matches!((edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta)) |
                        (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini)))
                    {
                        if let (Some(t1), Some(t2)) = (h_up, h_down) {
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
        if s1 == EdgeState::Uncut && s2 == EdgeState::Unknown {
            if self.set_edge(e2, EdgeState::Cut) {
                progress = true;
            }
        }
        if s2 == EdgeState::Uncut && s1 == EdgeState::Unknown {
            if self.set_edge(e1, EdgeState::Cut) {
                progress = true;
            }
        }

        // 2. If Bricky, cannot both be Cut
        if self.puzzle.rules.bricky {
            if s1 == EdgeState::Cut && s2 == EdgeState::Cut {
                return Err(());
            }
            if s1 == EdgeState::Cut && s2 == EdgeState::Unknown {
                if self.set_edge(e2, EdgeState::Uncut) {
                    progress = true;
                }
            }
            if s2 == EdgeState::Cut && s1 == EdgeState::Unknown {
                if self.set_edge(e1, EdgeState::Uncut) {
                    progress = true;
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
    fn propagate_delta_gemini_v_stack_forces_cut() {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . + . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        // Vertex (1, 1): h_up=H(0, 1), h_down=H(1, 1), v_left=V(1, 0), v_right=V(1, 1)
        let h_up = s.grid.h_edge(0, 1);
        let h_down = s.grid.h_edge(1, 1);
        s.puzzle.edge_clues.push(EdgeClue { edge: h_up, kind: EdgeClueKind::Gemini });
        s.puzzle.edge_clues.push(EdgeClue { edge: h_down, kind: EdgeClueKind::Delta });
        s.edges[h_up] = EdgeState::Cut;
        s.edges[h_down] = EdgeState::Cut;

        let v_left = s.grid.v_edge(1, 0);
        let v_right = s.grid.v_edge(1, 1);
        let _ = s.set_edge(v_left, EdgeState::Uncut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[v_right], EdgeState::Cut);
    }

    #[test]
    fn propagate_delta_gemini_v_stack_bricky_forces_uncut() {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . + . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.bricky = true;
        let h_up = s.grid.h_edge(0, 1);
        let h_down = s.grid.h_edge(1, 1);
        s.puzzle.edge_clues.push(EdgeClue { edge: h_up, kind: EdgeClueKind::Gemini });
        s.puzzle.edge_clues.push(EdgeClue { edge: h_down, kind: EdgeClueKind::Delta });
        s.edges[h_up] = EdgeState::Cut;
        s.edges[h_down] = EdgeState::Cut;

        let v_left = s.grid.v_edge(1, 0);
        let v_right = s.grid.v_edge(1, 1);
        let _ = s.set_edge(v_left, EdgeState::Cut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[v_right], EdgeState::Uncut);
    }

    #[test]
    fn propagate_delta_gemini_h_stack_forces_cut() {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . + . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        // Vertex (1, 1): v_left=V(1, 0), v_right=V(1, 1)
        let v_left = s.grid.v_edge(1, 0);
        let v_right = s.grid.v_edge(1, 1);
        s.puzzle.edge_clues.push(EdgeClue { edge: v_left, kind: EdgeClueKind::Gemini });
        s.puzzle.edge_clues.push(EdgeClue { edge: v_right, kind: EdgeClueKind::Delta });
        s.edges[v_left] = EdgeState::Cut;
        s.edges[v_right] = EdgeState::Cut;

        let h_up = s.grid.h_edge(0, 1);
        let h_down = s.grid.h_edge(1, 1);
        let _ = s.set_edge(h_up, EdgeState::Uncut);

        let result = s.propagate_delta_gemini_interaction();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[h_down], EdgeState::Cut);
    }
}
