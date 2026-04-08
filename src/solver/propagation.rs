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
        let is_loopy = self.puzzle.rules.loopy;
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

    /// Iterative vertex-level watchtower config probing.
    /// For each interior vertex with a watchtower clue, enumerate all valid
    /// configurations. If only one survives propagation, force it.
    /// Repeat until no progress. Returns total edges forced.
    pub(crate) fn probe_watchtower_vertex_configs(&mut self) -> usize {
        if self.in_probing {
            return 0;
        }
        let cell_pair_indices: [(usize, usize); 4] = [(0, 1), (0, 2), (1, 3), (2, 3)];
        let is_loopy = self.puzzle.rules.loopy;
        let mut total_forced = 0usize;
        let saved = self.in_probing;
        self.in_probing = true;

        loop {
            let snap_iteration = self.snapshot();

            // Collect vertex info upfront to avoid borrow conflicts
            // possible_ks: for each vertex, the set of valid cut counts.
            // For non-loopy cycles: single value (needed cuts = value).
            // For loopy cycles: may have multiple values since k=4 can produce
            // 2-4 distinct pieces depending on external connections.
            let vertex_info: Vec<(usize, usize, Vec<usize>, Vec<EdgeId>)> = self
                .puzzle
                .vertex_clues
                .iter()
                .filter_map(|clue| {
                    let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                    let value = clue.value;
                    let cell_opts = self.grid.vertex_cells(vi, vj);

                    let n = cell_opts
                        .iter()
                        .copied()
                        .flatten()
                        .filter(|&cid| self.grid.cell_exists[cid])
                        .count();
                    if n == 0 || n == 1 {
                        return None;
                    }
                    let is_cycle = n == 4;
                    let possible_ks: Vec<usize> = if is_cycle {
                        if is_loopy {
                            // Loopy forbids k=3.
                            // Cycle piece counts: k=2→exactly 2 pieces, k=4→2-4 pieces.
                            // k=4's piece count depends on external connections, so
                            // propagation alone can't verify it matches `value`.
                            // Only use k values that guarantee the correct piece count.
                            match value {
                                2 => vec![2], // k=2 always gives exactly 2
                                _ => vec![],  // k=3 forbidden, k=4 uncertain
                            }
                        } else {
                            vec![value]
                        }
                    } else {
                        // Tree: pieces = 1 + k, so k = value - 1
                        vec![value.saturating_sub(1)]
                    };
                    if possible_ks.is_empty() {
                        return None;
                    }
                    // Skip if all possible k values are 0 (nothing to enumerate)
                    if possible_ks.iter().all(|&k| k == 0) {
                        return None;
                    }

                    let mut edge_ids: Vec<EdgeId> = Vec::new();
                    for &(a_idx, b_idx) in &cell_pair_indices {
                        if let (Some(a), Some(b)) = (cell_opts[a_idx], cell_opts[b_idx]) {
                            if self.grid.cell_exists[a] && self.grid.cell_exists[b] {
                                if let Some(eid) = self.grid.edge_between(a, b) {
                                    edge_ids.push(eid);
                                }
                            }
                        }
                    }
                    if edge_ids.len() < 2 {
                        return None;
                    }

                    Some((vi, vj, possible_ks, edge_ids))
                })
                .collect();

            if vertex_info.is_empty() {
                break;
            }

            let mut made_progress = false;

            for (_vi, _vj, possible_ks, edge_ids) in &vertex_info {
                let states: Vec<EdgeState> = edge_ids.iter().map(|&e| self.edges[e]).collect();
                let n_cut = states.iter().filter(|&&s| s == EdgeState::Cut).count();
                let n_unk = states.iter().filter(|&&s| s == EdgeState::Unknown).count();

                // Check if any k value is achievable
                let any_achievable = possible_ks
                    .iter()
                    .any(|&k| k >= n_cut && k.saturating_sub(n_cut) <= n_unk);
                if !any_achievable {
                    break; // contradiction
                }

                // Skip if all k values are already satisfied (no enumeration needed)
                let all_satisfied = possible_ks.iter().all(|&k| n_cut == k);
                if all_satisfied {
                    continue;
                }

                // Only enumerate if practical
                if n_unk > 4 || n_unk == 0 {
                    continue;
                }

                let unk_indices: Vec<usize> = states
                    .iter()
                    .enumerate()
                    .filter(|(_, &s)| s == EdgeState::Unknown)
                    .map(|(i, _)| i)
                    .collect();

                let nm = unk_indices.len();
                let mut edge_cut_count: Vec<usize> = vec![0; nm];
                let mut total_surviving = 0usize;

                for &k in possible_ks {
                    let remaining = k.saturating_sub(n_cut);
                    if remaining > n_unk {
                        continue; // this k not achievable
                    }

                    if remaining == 0 {
                        // Current state already satisfies this k value
                        total_surviving += 1;
                        // No unknowns to set, so no edge_cut_count updates
                    } else {
                        for mask in 0u32..(1u32 << nm) {
                            if mask.count_ones() as usize != remaining {
                                continue;
                            }
                            let snap = self.snapshot();
                            let mut ok = true;
                            for (bit, &idx) in unk_indices.iter().enumerate() {
                                let val = if (mask >> bit) & 1 == 1 {
                                    EdgeState::Cut
                                } else {
                                    EdgeState::Uncut
                                };
                                if !self.set_edge(edge_ids[idx], val) {
                                    ok = false;
                                    break;
                                }
                            }
                            if ok {
                                ok = self.propagate().is_ok();
                            }
                            self.restore(snap);
                            if ok {
                                total_surviving += 1;
                                for (bit, _) in unk_indices.iter().enumerate() {
                                    if (mask >> bit) & 1 == 1 {
                                        edge_cut_count[bit] += 1;
                                    }
                                }
                            }
                        }
                    }
                }

                if total_surviving == 0 {
                    // Contradiction: undo all forcing from this iteration
                    self.restore(snap_iteration);
                    total_forced = 0;
                    made_progress = false;
                    break;
                }

                // Save state before forcing edges at this vertex.
                // If propagation fails after forcing, we undo.
                let snap_before_force = self.snapshot();
                let mut forced_here = 0usize;

                for (bit, &idx) in unk_indices.iter().enumerate() {
                    if self.edges[edge_ids[idx]] != EdgeState::Unknown {
                        continue;
                    }
                    if edge_cut_count[bit] == total_surviving {
                        // Cut in ALL surviving configs → force Cut
                        let _ = self.set_edge(edge_ids[idx], EdgeState::Cut);
                        total_forced += 1;
                        forced_here += 1;
                    } else if edge_cut_count[bit] == 0 {
                        // Uncut in ALL surviving configs → force Uncut
                        let _ = self.set_edge(edge_ids[idx], EdgeState::Uncut);
                        total_forced += 1;
                        forced_here += 1;
                    }
                }

                if forced_here > 0 {
                    if self.propagate().is_err() {
                        // This vertex's forced edges caused a contradiction.
                        // Undo and skip this vertex.
                        self.restore(snap_before_force);
                        total_forced -= forced_here;
                    } else {
                        made_progress = true;
                    }
                }
            }

            if !made_progress {
                break;
            }
        }

        self.in_probing = saved;
        total_forced
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
