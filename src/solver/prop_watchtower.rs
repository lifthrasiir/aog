use super::Solver;
use crate::types::*;
use std::collections::HashSet;

impl Solver {
    /// Propagate watchtower (vertex) clues.
    ///
    /// For a vertex surrounded by N existing cells with E internal edges:
    ///   - N=4, E=4 (2×2 block, one cycle): pieces = max(1, k) where k = cut edges
    ///   - N=2..3 (tree): pieces = 1 + k
    ///   - N=1: always 1 piece (no edges to propagate)
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
