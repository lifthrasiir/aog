use super::Solver;
use crate::types::*;

impl Solver {
    /// Check if all currently-decided edges are consistent with the known solution.
    /// Used for debug tracing of false contradictions.
    fn on_solution_path(&self) -> bool {
        if self.debug_known_solution.is_empty() || self.in_probing {
            return false;
        }
        self.edges.iter().enumerate().all(|(i, &curr)| {
            if curr == EdgeState::Unknown {
                return true;
            }
            if i >= self.debug_known_solution.len() {
                return true;
            }
            let k = self.debug_known_solution[i];
            k == EdgeState::Unknown || curr == k
        })
    }

    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            macro_rules! run_prop {
                ($name:literal, $cond:expr, $call:expr) => {
                    if $cond {
                        self.debug_current_prop = $name;
                        let r = $call;
                        if r.is_err() && self.on_solution_path() {
                            eprintln!(
                                "FALSE_ERR: prop={} depth={} unknown={}",
                                $name, self.search_depth, self.curr_unknown
                            );
                        }
                        progress |= r?;
                    }
                };
            }

            run_prop!(
                "bricky_loopy",
                self.puzzle.rules.bricky || self.puzzle.rules.loopy,
                self.propagate_bricky_loopy()
            );
            run_prop!(
                "delta_gemini",
                !self.puzzle.edge_clues.is_empty(),
                self.propagate_delta_gemini_interaction()
            );
            run_prop!("area_bounds", true, self.propagate_area_bounds());
            run_prop!("rose_sep", true, self.propagate_rose_separation());
            run_prop!("rose_phase3", true, self.propagate_rose_phase3());
            run_prop!(
                "same_area_reach",
                self.same_area_groups,
                self.propagate_same_area_reachability()
            );
            run_prop!(
                "palisade",
                self.has_palisade_clue,
                self.propagate_palisade_constraints()
            );
            run_prop!(
                "compass_basic",
                self.has_compass_clue,
                self.propagate_compass()
            );
            run_prop!("watchtower", true, self.propagate_watchtower());

            if !progress {
                // Failed literal detection (probing): probe each unknown edge
                // to see if one value causes contradiction. in_probing guard
                // prevents recursion when called from within a probe.
                if !self.in_probing && self.curr_unknown > 0 && self.curr_unknown <= 256 {
                    let saved = self.in_probing;
                    self.in_probing = true;
                    self.debug_current_prop = "probe";
                    progress |= self.probe_one_round()?;
                    // Pair probing: for small unknown counts, probe pairs of
                    // edges sharing a vertex. Catches contradictions requiring
                    // two simultaneous decisions.
                    if !progress && self.curr_unknown <= 10 {
                        progress |= self.probe_pair_round()?;
                    }
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
    /// Returns early on first force to let the outer loop cascade.
    fn probe_one_round(&mut self) -> Result<bool, ()> {
        let num_edges = self.grid.num_edges();

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }

            // Probe Cut
            let snap = self.snapshot();
            let cut_ok = self.set_edge(e, EdgeState::Cut) && self.propagate().is_ok();
            self.restore(snap);

            if !cut_ok {
                // Cut contradicts -> force Uncut
                if self.edges[e] == EdgeState::Unknown && self.set_edge(e, EdgeState::Uncut) {
                    return Ok(true);
                }
                continue;
            }

            if self.edges[e] != EdgeState::Unknown {
                continue; // forced by a previous probe's cascade
            }

            // Probe Uncut
            let snap = self.snapshot();
            let uncut_ok = self.set_edge(e, EdgeState::Uncut) && self.propagate().is_ok();
            self.restore(snap);

            if !uncut_ok {
                // Uncut contradicts -> force Cut
                if self.edges[e] == EdgeState::Unknown && self.set_edge(e, EdgeState::Cut) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Probe pairs of edges sharing a vertex. For each pair (e1, e2),
    /// try all 4 combinations. If 3 contradict, force the 4th.
    /// Only probes pairs where both edges are Unknown and share a vertex.
    fn probe_pair_round(&mut self) -> Result<bool, ()> {
        // Collect unknown edges
        let unknowns: Vec<EdgeId> = (0..self.grid.num_edges())
            .filter(|&e| self.edges[e] == EdgeState::Unknown)
            .collect();

        if unknowns.len() < 2 || unknowns.len() > 16 {
            return Ok(false);
        }

        // Build vertex-to-edge mapping for pairing
        let mut vert_edges: Vec<Vec<EdgeId>> = Vec::new();
        for e in &unknowns {
            let (is_h, r, c) = self.grid.decode_edge(*e);
            let v1 = self.grid.vertex(r, c);
            let v2 = if is_h {
                self.grid.vertex(r + 1, c)
            } else {
                self.grid.vertex(r, c + 1)
            };
            while vert_edges.len() <= v1 {
                vert_edges.push(Vec::new());
            }
            while vert_edges.len() <= v2 {
                vert_edges.push(Vec::new());
            }
            vert_edges[v1].push(*e);
            vert_edges[v2].push(*e);
        }

        // Probe pairs of edges sharing a vertex
        let vals = [EdgeState::Cut, EdgeState::Uncut];
        for v_edges in &vert_edges {
            if v_edges.len() < 2 {
                continue;
            }
            for i in 0..v_edges.len() {
                let e1 = v_edges[i];
                if self.edges[e1] != EdgeState::Unknown {
                    continue;
                }
                for j in (i + 1)..v_edges.len() {
                    let e2 = v_edges[j];
                    if self.edges[e2] != EdgeState::Unknown {
                        continue;
                    }

                    let mut ok_count = 0usize;
                    let mut last_ok = (EdgeState::Cut, EdgeState::Cut);

                    for &v1 in &vals {
                        for &v2 in &vals {
                            let snap = self.snapshot();
                            let ok = self.set_edge(e1, v1)
                                && self.set_edge(e2, v2)
                                && self.propagate().is_ok();
                            self.restore(snap);

                            if ok {
                                ok_count += 1;
                                last_ok = (v1, v2);
                            }
                        }
                    }

                    if ok_count == 1 {
                        // Only one combination works — force it
                        let (v1, v2) = last_ok;
                        if self.edges[e1] == EdgeState::Unknown {
                            let _ = self.set_edge(e1, v1);
                        }
                        if self.edges[e2] == EdgeState::Unknown {
                            let _ = self.set_edge(e2, v2);
                        }
                        return Ok(true);
                    }
                    if ok_count == 0 {
                        // All combinations contradict — current state is invalid
                        return Err(());
                    }

                    if self.edges[e1] != EdgeState::Unknown {
                        break; // e1 was forced by a previous pair probe
                    }
                }
            }
        }

        Ok(false)
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
                    if matches!(
                        (edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta))
                            | (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini))
                    ) {
                        if let (Some(t1), Some(t2)) = (v_left, v_right) {
                            progress |= self.propagate_transverse_pair(t1, t2)?;
                        }
                    }
                }

                // Case 2: Gemini/Delta on horizontal stack (v_left, v_right)
                // Note: v_edge is a VERTICAL line, but v_left/v_right form a HORIZONTAL stack.
                if let (Some(e1), Some(e2)) = (v_left, v_right) {
                    if matches!(
                        (edge_kinds[e1], edge_kinds[e2]),
                        (Some(EdgeClueKind::Gemini), Some(EdgeClueKind::Delta))
                            | (Some(EdgeClueKind::Delta), Some(EdgeClueKind::Gemini))
                    ) {
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
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_up,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_down,
            kind: EdgeClueKind::Delta,
        });
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
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_up,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: h_down,
            kind: EdgeClueKind::Delta,
        });
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
        s.puzzle.edge_clues.push(EdgeClue {
            edge: v_left,
            kind: EdgeClueKind::Gemini,
        });
        s.puzzle.edge_clues.push(EdgeClue {
            edge: v_right,
            kind: EdgeClueKind::Delta,
        });
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
