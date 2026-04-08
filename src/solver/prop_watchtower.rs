use super::Solver;
use crate::types::*;
use crate::uf::{uf_find, uf_union};
use std::collections::HashSet;

impl Solver {
    /// Propagate watchtower (vertex) clues.
    ///
    /// For a vertex surrounded by N existing cells with E internal edges:
    ///   - N=4, E=4 (2×2 block, one cycle): pieces = max(1, k) where k = cut edges
    ///   - N=2..3 (tree): pieces = 1 + k
    ///   - N=1: always 1 piece (no edges to propagate)
    ///
    /// value=v constrains the required number of cut edges accordingly.
    pub(crate) fn propagate_watchtower(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        // Adjacent cell pairs in the 2×2 layout: (TL,TR), (TL,BL), (TR,BR), (BL,BR)
        let cell_pair_indices: [(usize, usize); 4] = [(0, 1), (0, 2), (1, 3), (2, 3)];

        // === Component-ID-based pass ===
        // Use curr_comp_id for more precise distinct piece counting.
        // Sealed components are definitely separate pieces; growing components
        // might merge, giving us a [min_distinct, max_distinct] range.
        // Only run when curr_comp_id has been populated (by propagate_area_bounds).
        if !self.curr_comp_id.is_empty() {
            let comp_id_results: Vec<(bool, Vec<EdgeId>)> = self
                .puzzle
                .vertex_clues
                .iter()
                .map(|clue| {
                    let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                    let cell_opts = self.grid.vertex_cells(vi, vj);
                    let value = clue.value;

                    let cells: Vec<CellId> = cell_opts
                        .iter()
                        .copied()
                        .flatten()
                        .filter(|&cid| self.grid.cell_exists[cid])
                        .collect();
                    let n = cells.len();
                    if n == 0 || value > n || (n == 1 && value > 1) {
                        return (false, vec![]); // will be caught by edge-based pass
                    }

                    let comp_set: HashSet<usize> =
                        cells.iter().map(|&c| self.curr_comp_id[c]).collect();
                    let num_sealed = comp_set
                        .iter()
                        .filter(|&&ci| !self.can_grow_buf[ci])
                        .count();
                    let num_growing = comp_set.len() - num_sealed;

                    let min_distinct = num_sealed + if num_growing > 0 { 1 } else { 0 };
                    let max_distinct = comp_set.len();

                    let is_err = value < min_distinct || value > max_distinct;

                    let mut forced_cuts = Vec::new();
                    if max_distinct == value && comp_set.len() > 1 {
                        for &(a_idx, b_idx) in &cell_pair_indices {
                            if let (Some(a), Some(b)) = (cell_opts[a_idx], cell_opts[b_idx]) {
                                if !self.grid.cell_exists[a] || !self.grid.cell_exists[b] {
                                    continue;
                                }
                                if self.curr_comp_id[a] != self.curr_comp_id[b] {
                                    if let Some(eid) = self.grid.edge_between(a, b) {
                                        if self.edges[eid] == EdgeState::Unknown {
                                            forced_cuts.push(eid);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    (is_err, forced_cuts)
                })
                .collect();

            for (is_err, _) in &comp_id_results {
                if *is_err {
                    return Err(());
                }
            }
            for (_, forced_cuts) in &comp_id_results {
                for &eid in forced_cuts {
                    let p = self.set_edge(eid, EdgeState::Cut);
                    if !p {
                        return Err(());
                    }
                    progress = true;
                }
            }
        } // end if !curr_comp_id.is_empty()

        // === Edge-based pass (original logic) ===

        // Collect constraints upfront to avoid borrow conflicts
        let constraints: Vec<(usize, Vec<EdgeId>, bool)> = self
            .puzzle
            .vertex_clues
            .iter()
            .filter_map(|clue| {
                let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                let cell_opts = self.grid.vertex_cells(vi, vj);
                let value = clue.value;

                // Count existing cells
                let n = cell_opts
                    .iter()
                    .copied()
                    .flatten()
                    .filter(|&cid| self.grid.cell_exists[cid])
                    .count();

                if n == 0 || (n == 1 && value == 1) {
                    return None; // nothing to propagate
                }
                if value > n {
                    // More pieces required than cells exist → impossible
                    return Some((value, vec![], false));
                }
                if n == 1 {
                    // value > 1 with only 1 cell → impossible
                    return Some((value, vec![], false));
                }

                // Collect internal edges between adjacent existing cells
                let mut edge_ids = Vec::new();
                for &(a_idx, b_idx) in &cell_pair_indices {
                    if let (Some(a_cid), Some(b_cid)) = (cell_opts[a_idx], cell_opts[b_idx]) {
                        if self.grid.cell_exists[a_cid] && self.grid.cell_exists[b_cid] {
                            if let Some(eid) = self.grid.edge_between(a_cid, b_cid) {
                                edge_ids.push(eid);
                            }
                        }
                    }
                }

                let is_cycle = n == 4 && edge_ids.len() == 4;
                Some((value, edge_ids, is_cycle))
            })
            .collect();

        for (value, edge_ids, is_cycle) in constraints {
            // Empty edges with value > 1 signals an impossibility (caught above)
            if edge_ids.is_empty() && value > 1 {
                return Err(());
            }
            if edge_ids.is_empty() {
                continue;
            }

            let mut n_cut = 0usize;
            let mut unk = Vec::new();
            for &eid in &edge_ids {
                match self.edges[eid] {
                    EdgeState::Cut => n_cut += 1,
                    EdgeState::Unknown => unk.push(eid),
                    EdgeState::Uncut => {}
                }
            }

            if is_cycle {
                // 4 cells, 4 edges, one cycle: pieces = max(1, k)
                if value == 1 {
                    if n_cut >= 2 {
                        return Err(());
                    }
                    if n_cut == 1 && !unk.is_empty() {
                        for eid in unk {
                            if !self.set_edge(eid, EdgeState::Uncut) {
                                return Err(());
                            }
                            progress = true;
                        }
                    }
                    // n_cut == 0: 0 or 1 cuts both give 1 piece → no forcing
                } else {
                    // value >= 2: need exactly value cuts
                    if n_cut > value {
                        return Err(());
                    }
                    if n_cut == value && !unk.is_empty() {
                        for eid in unk {
                            if !self.set_edge(eid, EdgeState::Uncut) {
                                return Err(());
                            }
                            progress = true;
                        }
                    } else if n_cut + unk.len() < value {
                        return Err(());
                    } else if n_cut + unk.len() == value && !unk.is_empty() {
                        for eid in unk {
                            if !self.set_edge(eid, EdgeState::Cut) {
                                return Err(());
                            }
                            progress = true;
                        }
                    }
                }
            } else {
                // Tree (2 or 3 cells): pieces = 1 + k, need k = value - 1
                let needed_k = value.saturating_sub(1);
                if n_cut > needed_k {
                    return Err(());
                }
                if n_cut == needed_k && !unk.is_empty() {
                    for eid in unk {
                        if !self.set_edge(eid, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                } else if n_cut + unk.len() < needed_k {
                    return Err(());
                } else if n_cut + unk.len() == needed_k && !unk.is_empty() {
                    for eid in unk {
                        if !self.set_edge(eid, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            }
        }
        Ok(progress)
    }

    /// Edge-level parity propagation for watchtower vertices.
    ///
    /// For each watchtower vertex with a deterministic cut count parity,
    /// creates pairwise XOR constraints between unknown edges and propagates
    /// them globally through a Union-Find. For example, @ (value=2) on a
    /// 4-cell cycle requires exactly 2 cuts, so e1⊕e2⊕e3⊕e4=0 (mod 2).
    /// When 2 edges are known and 2 are unknown, the unknowns are unioned
    /// with their XOR relationship. Known values then cascade through the UF.
    pub(crate) fn propagate_vertex_edge_parity(&mut self) -> Result<bool, ()> {
        if self.puzzle.vertex_clues.is_empty() {
            return Ok(false);
        }

        let ne = self.grid.num_edges();
        let pair_idx: [(usize, usize); 4] = [(0, 1), (0, 2), (1, 3), (2, 3)];

        // Collect vertex constraints: (edge_ids, required_parity)
        // required_parity = k mod 2 where k is the required cut count
        let constraints: Vec<(Vec<EdgeId>, u8)> = self
            .puzzle
            .vertex_clues
            .iter()
            .filter_map(|clue| {
                let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                let cell_opts = self.grid.vertex_cells(vi, vj);
                let n = cell_opts
                    .iter()
                    .copied()
                    .flatten()
                    .filter(|&cid| self.grid.cell_exists[cid])
                    .count();
                if n < 2 {
                    return None;
                }
                let is_cycle = n == 4;
                let required_k = if is_cycle {
                    if clue.value <= 1 {
                        return None; // k ∈ {0,1}, parity not fixed
                    }
                    clue.value
                } else {
                    clue.value.saturating_sub(1)
                };
                let mut edge_ids: Vec<EdgeId> = Vec::new();
                for &(a, b) in &pair_idx {
                    if let (Some(ca), Some(cb)) = (cell_opts[a], cell_opts[b]) {
                        if self.grid.cell_exists[ca] && self.grid.cell_exists[cb] {
                            if let Some(eid) = self.grid.edge_between(ca, cb) {
                                edge_ids.push(eid);
                            }
                        }
                    }
                }
                if edge_ids.len() < 2 {
                    return None;
                }
                Some((edge_ids, (required_k & 1) as u8))
            })
            .collect();

        if constraints.is_empty() {
            return Ok(false);
        }

        // Build edge-level parity UF
        let mut parent: Vec<usize> = (0..ne).collect();
        let mut rank: Vec<u8> = vec![0; ne];
        let mut par: Vec<u8> = vec![0; ne];

        // ev: 0=Uncut, 1=Cut, 2=Unknown
        let mut ev: Vec<u8> = self
            .edges
            .iter()
            .map(|&e| match e {
                EdgeState::Cut => 1,
                EdgeState::Uncut => 0,
                EdgeState::Unknown => 2,
            })
            .collect();

        let mut forced: Vec<(EdgeId, EdgeState)> = Vec::new();

        // Phase 1: Build UF from pairwise constraints (0, 1, or 2 unknowns)
        for (edge_ids, parity) in &constraints {
            let mut kx = 0u8;
            let mut unks: Vec<EdgeId> = Vec::new();
            for &e in edge_ids {
                if ev[e] <= 1 {
                    kx ^= ev[e];
                } else {
                    unks.push(e);
                }
            }
            match unks.len() {
                0 => {
                    if kx != *parity {
                        return Err(());
                    }
                }
                1 => {
                    let v = kx ^ parity;
                    ev[unks[0]] = v;
                    forced.push((
                        unks[0],
                        if v == 1 {
                            EdgeState::Cut
                        } else {
                            EdgeState::Uncut
                        },
                    ));
                }
                2 => {
                    uf_union(&mut parent, &mut rank, &mut par, unks[0], unks[1], kx ^ parity)?;
                }
                _ => {}
            }
        }

        // Phase 2: Use UF to resolve 3+ unknown cases
        // If two unknowns at a vertex are already in the same UF component
        // (from other vertices), their XOR is known, reducing the constraint.
        for (edge_ids, parity) in &constraints {
            let mut kx = 0u8;
            let mut unks: Vec<EdgeId> = Vec::new();
            for &e in edge_ids {
                if ev[e] <= 1 {
                    kx ^= ev[e];
                } else {
                    unks.push(e);
                }
            }
            if unks.len() < 3 {
                continue;
            }
            let target = kx ^ parity;
            'outer: for i in 0..unks.len() {
                for j in (i + 1)..unks.len() {
                    let (r1, p1) = uf_find(&parent, &par, unks[i]);
                    let (r2, p2) = uf_find(&parent, &par, unks[j]);
                    if r1 == r2 {
                        let xij = p1 ^ p2;
                        let rem: Vec<EdgeId> = unks
                            .iter()
                            .enumerate()
                            .filter(|(idx, _)| *idx != i && *idx != j)
                            .map(|(_, &e)| e)
                            .collect();
                        if rem.len() == 1 {
                            let v = target ^ xij;
                            ev[rem[0]] = v;
                            forced.push((
                                rem[0],
                                if v == 1 {
                                    EdgeState::Cut
                                } else {
                                    EdgeState::Uncut
                                },
                            ));
                        } else if rem.len() == 2 {
                            uf_union(
                                &mut parent,
                                &mut rank,
                                &mut par,
                                rem[0],
                                rem[1],
                                target ^ xij,
                            )?;
                        }
                        break 'outer;
                    }
                }
            }
        }

        // Phase 3: Propagate known values through UF
        let mut rv: Vec<Option<u8>> = vec![None; ne];
        for e in 0..ne {
            if ev[e] > 1 {
                continue;
            }
            let (root, p) = uf_find(&parent, &par, e);
            let r = p ^ ev[e];
            if let Some(ex) = rv[root] {
                if ex != r {
                    return Err(());
                }
            } else {
                rv[root] = Some(r);
            }
        }
        for e in 0..ne {
            if ev[e] <= 1 {
                continue;
            }
            let (root, p) = uf_find(&parent, &par, e);
            if let Some(r) = rv[root] {
                let v = p ^ r;
                ev[e] = v;
                forced.push((
                    e,
                    if v == 1 {
                        EdgeState::Cut
                    } else {
                        EdgeState::Uncut
                    },
                ));
            }
        }

        if forced.is_empty() {
            return Ok(false);
        }

        let mut progress = false;
        for (e, state) in &forced {
            if self.edges[*e] == EdgeState::Unknown {
                if !self.set_edge(*e, *state) {
                    return Err(());
                }
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

    /// Helper: create a solver and add a watchtower vertex clue at grid point (vi, vj).
    fn make_watchtower_solver(input: &str, vi: usize, vj: usize, value: usize) -> Solver {
        let mut s = make_solver(input);
        s.puzzle.vertex_clues.push(VertexClue {
            vertex: s.grid.vertex(vi, vj),
            value,
        });
        s
    }

    #[test]
    fn watchtower_boundary_2cells_value1_forces_uncut() {
        // 1×2 grid. Vertex (0,1) is a top boundary vertex with cells (0,0) and (0,1).
        // value=1 → 1 piece → the edge between them must be Uncut.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            1,
        );
        let v_edge = s.grid.v_edge(0, 0);
        assert_eq!(s.edges[v_edge], EdgeState::Unknown);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(result.unwrap(), "should have made progress");
        assert_eq!(s.edges[v_edge], EdgeState::Uncut);
    }

    #[test]
    fn watchtower_boundary_2cells_value2_forces_cut() {
        // 1×2 grid. Vertex (0,1) top boundary. value=2 → edge must be Cut.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            2,
        );
        let v_edge = s.grid.v_edge(0, 0);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[v_edge], EdgeState::Cut);
    }

    #[test]
    fn watchtower_boundary_2cells_value2_already_cut_ok() {
        // 1×2 grid. Edge already Cut, value=2 → no contradiction.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            2,
        );
        let v_edge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(v_edge, EdgeState::Cut);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(!result.unwrap(), "no progress needed");
    }

    #[test]
    fn watchtower_boundary_2cells_value1_already_uncut_ok() {
        // 1×2 grid. Edge already Uncut, value=1 → no contradiction.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            1,
        );
        let v_edge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(v_edge, EdgeState::Uncut);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
    }

    #[test]
    fn watchtower_boundary_2cells_value1_already_cut_err() {
        // 1×2 grid. Edge already Cut, value=1 → contradiction (2 pieces).
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            1,
        );
        let v_edge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(v_edge, EdgeState::Cut);

        let result = s.propagate_watchtower();
        assert!(
            result.is_err(),
            "Cut edge with value=1 should be contradiction"
        );
    }

    #[test]
    fn watchtower_boundary_2cells_value3_err() {
        // 1×2 grid. Only 2 cells but value=3 → impossible.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
            0,
            1,
            3,
        );

        let result = s.propagate_watchtower();
        assert!(result.is_err(), "value > n_cells should be contradiction");
    }

    #[test]
    fn watchtower_interior_4cells_value1_one_cut_forces_rest_uncut() {
        // 2×2 grid. Vertex (1,1) interior. value=1.
        // Set one internal edge to Cut → all others must be Uncut.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
            1,
            1,
            1,
        );
        // Cut the top horizontal edge (TL-TR)
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(result.unwrap());
        // The remaining 3 internal edges should be Uncut
        assert_eq!(s.edges[s.grid.h_edge(0, 0)], EdgeState::Uncut);
        assert_eq!(s.edges[s.grid.h_edge(0, 1)], EdgeState::Uncut);
        assert_eq!(s.edges[s.grid.v_edge(1, 0)], EdgeState::Uncut);
    }

    #[test]
    fn watchtower_interior_4cells_value2_two_cuts_forces_rest_uncut() {
        // 2×2 grid. value=2. Set two cuts → rest Uncut.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
            1,
            1,
            2,
        );
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[s.grid.h_edge(0, 1)], EdgeState::Uncut);
        assert_eq!(s.edges[s.grid.v_edge(1, 0)], EdgeState::Uncut);
    }

    #[test]
    fn watchtower_interior_4cells_value1_three_cuts_err() {
        // 2×2 grid. value=1. Set three cuts → contradiction.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
            1,
            1,
            1,
        );
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);

        let result = s.propagate_watchtower();
        assert!(result.is_err());
    }

    #[test]
    fn watchtower_interior_4cells_value3_need_all_unknowns_cut() {
        // 2×2 grid. value=3. One cut + one uncut + two unknowns → both unknowns must be Cut.
        let mut s = make_watchtower_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
            1,
            1,
            3,
        );
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert_eq!(s.edges[s.grid.h_edge(0, 1)], EdgeState::Cut);
        assert_eq!(s.edges[s.grid.v_edge(1, 0)], EdgeState::Cut);
    }

    #[test]
    fn watchtower_corner_1cell_value1_no_propagation() {
        // 1×1 grid. Corner vertex (0,0) has 1 cell. value=1 → no action.
        let mut s = make_watchtower_solver(
            "\
+---+
| _ |
+---+
",
            0,
            0,
            1,
        );

        let result = s.propagate_watchtower();
        assert!(result.is_ok());
        assert!(!result.unwrap(), "no progress for 1-cell vertex");
    }

    #[test]
    fn watchtower_corner_1cell_value2_err() {
        // 1×1 grid. value=2 with only 1 cell → impossible.
        let mut s = make_watchtower_solver(
            "\
+---+
| _ |
+---+
",
            0,
            0,
            2,
        );

        let result = s.propagate_watchtower();
        assert!(result.is_err());
    }
}
