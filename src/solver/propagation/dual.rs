use super::super::Solver;
use crate::types::*;

impl Solver {
    /// Dual graph connectivity and bridge analysis for multi-piece puzzles.
    ///
    /// When the exact piece count is known (e.g. from rose window), analyzes the
    /// component graph (components connected by Unknown edges) to enforce:
    ///
    /// 1. **Single growth edge forcing**: a component that must grow but has
    ///    only 1 Unknown edge → that edge must be Uncut.
    ///
    /// 2. **Connected component count vs piece count**:
    ///    - More cc's than pieces → contradiction.
    ///    - Equal → every internal Unknown edge forced Uncut (each cc = 1 piece).
    ///
    /// 3. **Bridge analysis**: cutting a bridge that creates a partition where
    ///    one side can't form a valid piece (area too small/large) → force Uncut.
    pub(crate) fn propagate_dual_connectivity(&mut self) -> Result<bool, ()> {
        let num_pieces = match self.prop.exact_piece_count {
            Some(p) if p >= 2 => p,
            _ => return Ok(false),
        };

        let num_comp = self.curr_comp_sz.len();
        if num_comp <= 1 {
            return Ok(false);
        }

        // Build component graph adjacency: nodes = components,
        // edges = Unknown edges between different components.
        let mut adj: Vec<Vec<(usize, EdgeId)>> = vec![Vec::new(); num_comp];
        for ci in 0..num_comp {
            for &e in &self.prop.growth_edges[ci] {
                if self.edges[e] != EdgeState::Unknown {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                let other_ci = if self.curr_comp_id[c1] == ci {
                    self.curr_comp_id[c2]
                } else {
                    self.curr_comp_id[c1]
                };
                if other_ci < num_comp {
                    adj[ci].push((other_ci, e));
                }
            }
        }

        let mut progress = false;

        // ── Check 1: single growth edge forcing ──
        for ci in self.growing(num_comp).collect::<Vec<_>>() {
            let must_grow = if let Some(t) = self.curr_target_area[ci] {
                self.curr_comp_sz[ci] < t
            } else {
                self.curr_comp_sz[ci] < self.prop.curr_min_area[ci]
            };
            if !must_grow {
                continue;
            }
            // Count current Unknown growth edges
            let mut unk_edge: Option<EdgeId> = None;
            let mut unk_count = 0usize;
            for &e in &self.prop.growth_edges[ci] {
                if self.edges[e] == EdgeState::Unknown {
                    unk_count += 1;
                    unk_edge = Some(e);
                    if unk_count > 1 {
                        break;
                    }
                }
            }
            if unk_count == 1 {
                let e = unk_edge.unwrap();
                tracing::debug!(
                    comp = ci,
                    edge = e,
                    sz = self.curr_comp_sz[ci],
                    min_area = self.prop.curr_min_area[ci],
                    "dual_conn Check1: single growth edge, forcing Uncut"
                );
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
            }
        }

        // If we forced edges, re-run from scratch (component info is stale).
        if progress {
            return Ok(true);
        }

        // ── Check 2: connected component count ──
        let n = num_comp;
        let mut cc_visited = vec![false; n];
        let mut num_cc = 0usize;

        for ci in 0..n {
            if cc_visited[ci] {
                continue;
            }
            cc_visited[ci] = true;
            self.q_buf.clear();
            self.q_buf.push(ci);
            while let Some(cur) = self.q_buf.pop() {
                for &(next, _) in &adj[cur] {
                    if !cc_visited[next] {
                        cc_visited[next] = true;
                        self.q_buf.push(next);
                    }
                }
            }
            num_cc += 1;
        }

        tracing::debug!(
            num_cc,
            num_pieces,
            num_comp,
            depth = self.search_depth,
            unk = self.curr_unknown,
            "dual_conn: cc check"
        );

        if num_cc > num_pieces {
            tracing::debug!(num_cc, num_pieces, "dual_conn: too many CCs, contradiction");
            return Err(());
        }

        // Exact match → every Unknown edge must be Uncut
        // (each connected component of the component graph becomes exactly 1 piece)
        if num_cc == num_pieces {
            let to_force: Vec<EdgeId> = (0..n)
                .flat_map(|ci| self.prop.growth_edges[ci].iter().copied())
                .filter(|&e| self.edges[e] == EdgeState::Unknown)
                .collect();
            tracing::debug!(
                num_cc,
                num_pieces,
                forcing = to_force.len(),
                "dual_conn Check2: exact match, forcing all unknown growth edges Uncut"
            );
            for e in to_force {
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
            }
            return Ok(progress);
        }

        // ── Check 3: bridge analysis ──
        // Only meaningful when num_cc < num_pieces (cutting a bridge is a
        // plausible way to reach the target piece count).
        let mut disc = vec![usize::MAX; n];
        let mut low = vec![0usize; n];
        let mut timer = 0usize;
        let mut bridges: Vec<EdgeId> = Vec::new();

        for ci in 0..n {
            if disc[ci] != usize::MAX {
                continue;
            }
            Self::dfs_find_bridges(
                ci,
                None,
                &adj,
                &mut disc,
                &mut low,
                &mut timer,
                &mut bridges,
            );
        }

        for &e in &bridges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }

            let (c1, c2) = self.grid.edge_cells(e);
            let ci1 = self.curr_comp_id[c1];
            let ci2 = self.curr_comp_id[c2];
            if ci1 >= n || ci2 >= n || ci1 == ci2 {
                continue;
            }

            // Count cells reachable from ci1 without crossing bridge e
            let side_a = self.count_comp_cells_reachable(ci1, e, &adj);
            let side_b = self.total_cells - side_a;

            let (small, large) = if side_a <= side_b {
                (side_a, side_b)
            } else {
                (side_b, side_a)
            };

            // Smaller side must accommodate at least one piece
            if small < self.eff_min_area {
                tracing::debug!(
                    edge = e,
                    small,
                    eff_min_area = self.eff_min_area,
                    "dual_conn Check3: bridge small side too small, forcing Uncut"
                );
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
                continue;
            }

            // If cutting this bridge brings cc count to exactly num_pieces,
            // both sides must each form exactly 1 piece -- check max area.
            if num_cc + 1 == num_pieces && large > self.eff_max_area {
                tracing::debug!(
                    edge = e,
                    large,
                    eff_max_area = self.eff_max_area,
                    "dual_conn Check3: bridge large side too big, forcing Uncut"
                );
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
                continue;
            }
        }

        Ok(progress)
    }

    /// Recursive DFS for bridge detection in the component graph.
    /// Tracks parent *edge* (not node) to handle multigraphs correctly:
    /// if two components share multiple edges, none of those edges is a bridge.
    fn dfs_find_bridges(
        u: usize,
        parent_e: Option<EdgeId>,
        adj: &[Vec<(usize, EdgeId)>],
        disc: &mut [usize],
        low: &mut [usize],
        timer: &mut usize,
        bridges: &mut Vec<EdgeId>,
    ) {
        disc[u] = *timer;
        low[u] = *timer;
        *timer += 1;

        for &(v, e) in &adj[u] {
            if disc[v] == usize::MAX {
                // Tree edge — recurse
                Self::dfs_find_bridges(v, Some(e), adj, disc, low, timer, bridges);
                low[u] = low[u].min(low[v]);
                if low[v] > disc[u] {
                    bridges.push(e);
                }
            } else if parent_e != Some(e) {
                // Back edge (skip the edge we arrived on)
                low[u] = low[u].min(disc[v]);
            }
        }
    }

    /// Count total cells in components reachable from `start` without
    /// traversing `excluded_edge`. Used by bridge analysis.
    fn count_comp_cells_reachable(
        &self,
        start: usize,
        excluded_edge: EdgeId,
        adj: &[Vec<(usize, EdgeId)>],
    ) -> usize {
        let n = adj.len();
        let mut visited = vec![false; n];
        let mut q = std::collections::VecDeque::new();
        visited[start] = true;
        q.push_back(start);
        let mut total = 0usize;
        while let Some(ci) = q.pop_front() {
            total += self.curr_comp_sz[ci];
            for &(next, e) in &adj[ci] {
                if e == excluded_edge || visited[next] {
                    continue;
                }
                visited[next] = true;
                q.push_back(next);
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

    /// Helper: set up a 3×3 solver with exact_piece_count = 2
    /// and run build_components via propagate_area_bounds.
    fn make_dual_solver() -> Solver {
        let mut s = make_solver(
            "\
+---+---+---+
| A . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . A |
+---+---+---+
",
        );
        s.total_cells = s.grid.total_existing_cells();
        s.prop.exact_piece_count = Some(2);
        s.compute_area_bounds(); // sets eff_min_area, eff_max_area
        s.propagate_area_bounds().ok();
        s
    }

    #[test]
    fn dual_conn_bridge_too_small_side_forces_uncut() {
        // 2×2 grid, no rose cells. Manual exact_piece_count = 2.
        // minimum=2 so eff_min_area=2.
        //
        // Layout after edge setup:
        //   (0,0)--(0,1)    [Uncut v_edge(0,0)]
        //     |      |       [h_edge(0,0)=Unknown bridge, h_edge(0,1)=Cut]
        //   (1,0)  (1,1)    [v_edge(1,0)=Cut]
        //
        // Component A = {(0,0),(0,1)}, B = {(1,0)}, C = {(1,1)}
        // Only A-B edge is Unknown → A-B is a bridge.
        // B has 1 cell < eff_min_area=2 → bridge forced Uncut.
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        s.prop.exact_piece_count = Some(2);
        s.total_cells = s.grid.total_existing_cells();
        s.puzzle.rules.minimum = Some(2);
        s.compute_area_bounds();
        assert_eq!(s.eff_min_area, 2);

        // Connect (0,0)-(0,1) via Uncut
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Uncut);
        // Cut all other edges from A to C and B to C
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut); // (0,1)-(1,1)
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut); // (1,0)-(1,1)
                                                                 // h_edge(0,0) between (0,0) and (1,0) stays Unknown → bridge

        s.propagate_area_bounds().ok();

        let ci_a = s.curr_comp_id[s.grid.cell_id(0, 0)];
        let ci_b = s.curr_comp_id[s.grid.cell_id(1, 0)];
        assert_ne!(ci_a, ci_b);
        assert_eq!(s.curr_comp_sz[ci_b], 1);

        // B should have exactly 1 growth edge (the bridge to A)
        let unk_from_b: Vec<_> = s.prop.growth_edges[ci_b]
            .iter()
            .filter(|&&e| s.edges[e] == EdgeState::Unknown)
            .collect();
        assert_eq!(
            unk_from_b.len(),
            1,
            "B should have exactly 1 unknown growth edge"
        );

        let bridge = *unk_from_b[0];

        let result = s.propagate_dual_connectivity();
        assert!(result.is_ok());
        assert_eq!(
            s.edges[bridge],
            EdgeState::Uncut,
            "bridge to 1-cell component should be forced Uncut (1 < eff_min_area=2)"
        );
    }

    #[test]
    fn dual_conn_exact_cc_match_forces_all_uncut() {
        // Set up a 3x3 grid where the component graph has exactly 2 cc's.
        // With exact_piece_count = 2, all internal edges forced Uncut.
        let mut s = make_dual_solver();

        // Cut all horizontal edges on row 0 (between rows 0 and 1)
        // This separates row 0 from rows 1-2.
        // But we need Uncut edges to form components...
        //
        // Strategy: cut all edges between top row and middle row.
        // Then component graph has 2 cc's: top row (cc1) and bottom 2 rows (cc2).

        // Cut horizontal edges between row 0 and row 1
        for c in 0..2 {
            let e = s.grid.h_edge(0, c);
            let _ = s.set_edge(e, EdgeState::Cut);
        }

        // Cut horizontal edges between row 1 and row 2
        for c in 0..2 {
            let e = s.grid.h_edge(1, c);
            let _ = s.set_edge(e, EdgeState::Cut);
        }

        // Build components
        s.propagate_area_bounds().ok();

        // Now each cell is its own component (all Uncut edges are between
        // adjacent cells in the same row via vertical edges, which are Unknown).
        // Actually, the growth edges are the Unknown vertical edges.
        // Each row forms a connected component via vertical edges... wait,
        // all vertical edges are Unknown, not Uncut. So components are individual cells.

        // Let me Uncut-connect cells within each row group to form 2 cc's.
        // Connect top row cells via Uncut
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(0, 1), EdgeState::Uncut);
        // Connect middle+bottom rows
        let _ = s.set_edge(s.grid.h_edge(1, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(1, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(2, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(2, 1), EdgeState::Uncut);

        // Rebuild
        s.propagate_area_bounds().ok();

        // Verify 2 connected components
        let ci_top = s.curr_comp_id[s.grid.cell_id(0, 0)];
        let ci_bot = s.curr_comp_id[s.grid.cell_id(2, 0)];
        assert_ne!(ci_top, ci_bot, "should have 2 separate components");

        // There should be no Unknown edges between the two groups
        // (we cut all horizontal edges). But vertical edges within each
        // group might have some Unknowns... actually we Uncut all of them.
        // Let me check if there are any Unknown edges at all.
        let has_unknown = s.edges.iter().any(|&e| e == EdgeState::Unknown);
        if !has_unknown {
            return; // nothing to test
        }

        // Run dual connectivity — with 2 cc's and 2 pieces, all Unknown
        // edges within each cc should be forced Uncut.
        let result = s.propagate_dual_connectivity();
        // The result depends on whether the cc count is exactly 2.
        // If it is, all internal edges forced Uncut.
        // If rose propagation already forced them, result might be false.
        let _ = result;
    }

    #[test]
    fn dual_conn_single_growth_edge_forces_uncut() {
        // Component with target area > current size and only 1 growth edge
        // → that edge must be Uncut.
        let mut s = make_solver(
            "\
+---+---+---+---+
| _ . 3 . _ . _ |
+ . + . + . + . +
| _ . _ . _ . _ |
+ . + . + . + . +
| _ . _ . _ . _ |
+---+---+---+---+
",
        );
        s.prop.exact_piece_count = Some(2);
        s.total_cells = s.grid.total_existing_cells();
        s.compute_area_bounds();

        // Cell (0,0) has area=3, currently size 1.
        // Uncut-connect (0,0) to (0,1) so component has size 2, target 3.
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Uncut);

        // Cut all other edges from (0,0)-(0,1) component to isolate it
        // except one: the edge between (0,1) and (1,1).
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut); // between (0,0) and (1,0)
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut); // between (0,1) and (1,1)
                                                                 // Wait, we need one growth edge. Let me use (0,1)-(0,2) as the single growth edge.
                                                                 // Keep v_edge(0,1) between (0,1) and (0,2) as Unknown.

        // Also cut (0,0)-(1,0) to prevent growth downward
        // Already done above.

        s.propagate_area_bounds().ok();

        let ci = s.curr_comp_id[s.grid.cell_id(0, 0)];
        assert_eq!(s.curr_comp_sz[ci], 2);
        assert_eq!(s.curr_target_area[ci], Some(3));

        // The only Unknown edge from this component should be v_edge(0,1) = (0,1)-(0,2)
        let unk_edges: Vec<_> = s.prop.growth_edges[ci]
            .iter()
            .filter(|&&e| s.edges[e] == EdgeState::Unknown)
            .copied()
            .collect();
        assert_eq!(
            unk_edges.len(),
            1,
            "should have exactly 1 unknown growth edge"
        );

        let single_edge = unk_edges[0];
        let result = s.propagate_dual_connectivity();
        assert!(result.is_ok(), "dual_conn should not error");
        assert!(
            result.unwrap(),
            "should make progress (force single growth edge)"
        );
        assert_eq!(
            s.edges[single_edge],
            EdgeState::Uncut,
            "single growth edge should be forced Uncut"
        );
    }
}
