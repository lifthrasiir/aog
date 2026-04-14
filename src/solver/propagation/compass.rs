use super::super::{EdgeForcer, Solver};
use crate::types::*;
use std::collections::VecDeque;

impl Solver {
    pub(crate) fn propagate_compass(&mut self) -> Result<bool, ()> {
        let mut ef = EdgeForcer::new();

        for cl_idx in 0..self.puzzle.cell_clues.len() {
            let CellClue::Compass { cell, compass } = &self.puzzle.cell_clues[cl_idx] else {
                continue;
            };
            let cell = *cell;
            if !self.grid.cell_exists[cell] {
                continue;
            }
            let (r, c) = self.grid.cell_pos(cell);

            for &(dr, dc, val) in &[
                (-1isize, 0, compass.n),
                (0, 1, compass.e),
                (1, 0, compass.s),
                (0, -1, compass.w),
            ] {
                let Some(v) = val else { continue };
                if v != 0 {
                    continue;
                }

                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0
                    || nr >= self.grid.rows as isize
                    || nc < 0
                    || nc >= self.grid.cols as isize
                {
                    continue;
                }

                let nid = self.grid.cell_id(nr as usize, nc as usize);
                if !self.grid.cell_exists[nid] {
                    continue;
                }

                let Some(edge) = self.grid.edge_between(cell, nid) else {
                    continue;
                };

                if self.edges[edge] == EdgeState::Unknown {
                    ef.force_cut(edge);
                } else if self.edges[edge] != EdgeState::Cut {
                    return Err(());
                }
            }
        }

        let progress = ef.apply(self)?;
        Ok(progress)
    }

    /// Bridge/articulation-point based path forcing + single-gateway-edge forcing.
    ///
    /// For each growing component with unsatisfied compass directions:
    /// 1. Build the reachable subgraph via non-Cut edges.
    /// 2. Run iterative Tarjan bridge detection.
    /// 3. Force Uncut on any bridge whose removal would disconnect CI cells from
    ///    cells needed for an unsatisfied compass direction.
    /// 4. Force Uncut on a single Unknown gateway edge into an unsatisfied direction.
    ///
    /// Called only when `!self.in_probing` to avoid performance overhead.
    pub(crate) fn force_compass_via_bridges_and_gateways(
        &mut self,
        compass_per_comp: &[Vec<(CellId, CompassData)>],
        compass_cut_ef: &mut EdgeForcer,
        compass_uncut_ef: &mut EdgeForcer,
    ) -> Result<(), ()> {
        for &ci in &self.prop.growing_list {
            if compass_per_comp[ci].is_empty() {
                continue;
            }

            // Collect unsatisfied directions: (dir_idx, target_count, compass_row, compass_col)
            // dir_idx: 0=N, 1=S, 2=E, 3=W
            let mut unsatisfied: Vec<(usize, usize, isize, isize)> = Vec::new();
            for &(cell, ref compass) in &compass_per_comp[ci] {
                let (cr, cc) = self.grid.cell_pos(cell);
                let (cri, cci) = (cr as isize, cc as isize);
                let mut counts = [0usize; 4];
                for &c in &self.comp_cells[ci] {
                    let (pr, pc) = self.grid.cell_pos(c);
                    let dr = pr as isize - cri;
                    let dc = pc as isize - cci;
                    if dr < 0 {
                        counts[0] += 1;
                    }
                    if dr > 0 {
                        counts[1] += 1;
                    }
                    if dc > 0 {
                        counts[2] += 1;
                    }
                    if dc < 0 {
                        counts[3] += 1;
                    }
                }
                for &(val, idx) in &[
                    (compass.n, 0usize),
                    (compass.s, 1),
                    (compass.e, 2),
                    (compass.w, 3),
                ] {
                    let Some(v) = val else { continue };
                    if counts[idx] < v {
                        unsatisfied.push((idx, v, cri, cci));
                    }
                }
            }

            if unsatisfied.is_empty() {
                continue;
            }

            // Build reachable subgraph from CI via non-Cut edges (BFS)
            let nc = self.grid.num_cells();
            let mut local_id = vec![usize::MAX; nc];
            let mut local_cells: Vec<CellId> = Vec::new();
            let mut queue: VecDeque<CellId> = VecDeque::new();
            for &c in &self.comp_cells[ci] {
                if local_id[c] == usize::MAX {
                    local_id[c] = local_cells.len();
                    local_cells.push(c);
                    queue.push_back(c);
                }
            }
            while let Some(cur) = queue.pop_front() {
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if !self.grid.cell_exists[other] {
                        continue;
                    }
                    if local_id[other] == usize::MAX {
                        local_id[other] = local_cells.len();
                        local_cells.push(other);
                        queue.push_back(other);
                    }
                }
            }

            let n_local = local_cells.len();
            if n_local <= 1 {
                continue;
            }

            // Direction-reachability contradiction check.
            // If total reachable cells in an unsatisfied direction < target → contradiction.
            for &(dir_idx, v, cri, cci) in &unsatisfied {
                let reachable_dir = local_cells
                    .iter()
                    .filter(|&&c| {
                        let (pr, pc) = self.grid.cell_pos(c);
                        match dir_idx {
                            0 => (pr as isize) < cri,
                            1 => (pr as isize) > cri,
                            2 => (pc as isize) > cci,
                            3 => (pc as isize) < cci,
                            _ => false,
                        }
                    })
                    .count();
                if reachable_dir < v {
                    return Err(());
                }
            }

            // Build undirected adjacency list for the reachable subgraph
            let mut adj: Vec<Vec<(usize, EdgeId)>> = vec![Vec::new(); n_local];
            for (li, &c) in local_cells.iter().enumerate() {
                for eid in self.grid.cell_edges(c).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == c { c2 } else { c1 };
                    let lj = local_id[other];
                    if lj == usize::MAX {
                        continue;
                    }
                    adj[li].push((lj, eid));
                }
            }

            // Iterative Tarjan bridge detection
            let bridges = Self::find_bridges_in_subgraph(&adj, n_local);

            // For each Unknown bridge, check if it separates CI cells from
            // cells needed for an unsatisfied compass direction
            for bridge_eid in bridges {
                if self.edges[bridge_eid] != EdgeState::Unknown {
                    continue;
                }

                // BFS from CI cells in subgraph without the bridge
                let mut ci_side = vec![false; n_local];
                let mut bfs: VecDeque<usize> = VecDeque::new();
                for (i, &c) in local_cells.iter().enumerate() {
                    if self.curr_comp_id[c] == ci {
                        ci_side[i] = true;
                        bfs.push_back(i);
                    }
                }
                while let Some(u) = bfs.pop_front() {
                    for &(v, eid) in &adj[u] {
                        if eid == bridge_eid {
                            continue;
                        }
                        if !ci_side[v] {
                            ci_side[v] = true;
                            bfs.push_back(v);
                        }
                    }
                }

                // Force Uncut if CI side can't satisfy an unsatisfied direction
                // alone but the other side has cells in that direction
                let mut force_uncut = false;
                'dir_check: for &(dir_idx, v, cri, cci) in &unsatisfied {
                    let mut ci_side_count = 0usize;
                    let mut other_side_count = 0usize;
                    for (i, &cell) in local_cells.iter().enumerate() {
                        // Only count ci's own cells and unassigned cells —
                        // cells in other components may have incompatible constraints.
                        let cell_comp = self.curr_comp_id[cell];
                        if cell_comp != ci && cell_comp != usize::MAX {
                            continue;
                        }
                        let (pr, pc) = self.grid.cell_pos(cell);
                        let in_dir = match dir_idx {
                            0 => (pr as isize) < cri,
                            1 => (pr as isize) > cri,
                            2 => (pc as isize) > cci,
                            3 => (pc as isize) < cci,
                            _ => false,
                        };
                        if in_dir {
                            if ci_side[i] {
                                ci_side_count += 1;
                            } else {
                                other_side_count += 1;
                            }
                        }
                    }
                    if ci_side_count < v && other_side_count > 0 {
                        force_uncut = true;
                        break 'dir_check;
                    }
                }

                if force_uncut {
                    compass_uncut_ef.force_uncut(bridge_eid);
                }
            }

            // Single-gateway-edge forcing.
            // Skip if there are pending forced cuts: those cuts have not yet been applied
            // to self.edges, so the reachable subgraph is stale and the backward BFS may
            // traverse soon-to-be-Cut edges, leading to spurious forced Uncuts.
            if !compass_cut_ef.is_empty() {
                continue;
            }

            // Build a fresh CI membership set using current Uncut edges.
            // curr_comp_id may be stale (cells joined via Uncut edges set in this
            // propagation round but before build_components was re-run).
            let mut is_fresh_ci = vec![false; n_local];
            {
                let mut fc_bfs: VecDeque<usize> = VecDeque::new();
                for li in 0..n_local {
                    if self.curr_comp_id[local_cells[li]] == ci {
                        is_fresh_ci[li] = true;
                        fc_bfs.push_back(li);
                    }
                }
                while let Some(u) = fc_bfs.pop_front() {
                    for &(vj, eid) in &adj[u] {
                        if is_fresh_ci[vj] {
                            continue;
                        }
                        if self.edges[eid] == EdgeState::Uncut {
                            is_fresh_ci[vj] = true;
                            fc_bfs.push_back(vj);
                        }
                    }
                }
            }

            // For each unsatisfied direction, backward BFS from non-CI dir-cells through
            // the local subgraph. Any Unknown edge from CI to a reachable cell is a
            // "gateway edge". If exactly 1 exists, force Uncut.
            for &(dir_idx, _v, cri, cci) in &unsatisfied {
                let mut visited_local = vec![false; n_local];
                let mut bfs: VecDeque<usize> = VecDeque::new();

                for li in 0..n_local {
                    if is_fresh_ci[li] {
                        continue;
                    }
                    let c = local_cells[li];
                    // Only consider unassigned cells as direction targets to avoid
                    // false "only 1 gateway" conclusions via incompatible components.
                    if self.curr_comp_id[c] != usize::MAX {
                        continue;
                    }
                    let (pr, pc) = self.grid.cell_pos(c);
                    let in_dir = match dir_idx {
                        0 => (pr as isize) < cri,
                        1 => (pr as isize) > cri,
                        2 => (pc as isize) > cci,
                        3 => (pc as isize) < cci,
                        _ => false,
                    };
                    if in_dir {
                        visited_local[li] = true;
                        bfs.push_back(li);
                    }
                }

                if bfs.is_empty() {
                    continue;
                }

                // BFS through non-CI unassigned cells
                while let Some(u) = bfs.pop_front() {
                    for &(vj, _eid) in &adj[u] {
                        if visited_local[vj] {
                            continue;
                        }
                        if is_fresh_ci[vj] {
                            continue;
                        }
                        if self.curr_comp_id[local_cells[vj]] != usize::MAX {
                            continue;
                        }
                        visited_local[vj] = true;
                        bfs.push_back(vj);
                    }
                }

                // Collect Unknown edges from CI to visited non-CI cells
                let mut gateway_edges: Vec<EdgeId> = Vec::new();
                for li in 0..n_local {
                    if !is_fresh_ci[li] {
                        continue;
                    }
                    for &(vj, eid) in &adj[li] {
                        if !visited_local[vj] {
                            continue;
                        }
                        if self.edges[eid] != EdgeState::Unknown {
                            continue;
                        }
                        gateway_edges.push(eid);
                    }
                }

                if gateway_edges.is_empty() {
                    // Do NOT return Err() here: pending forced cuts may already block the
                    // paths the backward BFS traversed. Leave to reachability/bridge analysis.
                    continue;
                }

                if gateway_edges.len() == 1 {
                    compass_uncut_ef.force_uncut(gateway_edges[0]);
                }
            }
        }
        Ok(())
    }

    /// Iterative Tarjan bridge detection on a local subgraph.
    /// `adj[u]` is a list of `(neighbor_local_id, EdgeId)`.
    /// Returns the list of bridge EdgeIds.
    fn find_bridges_in_subgraph(adj: &[Vec<(usize, EdgeId)>], n_local: usize) -> Vec<EdgeId> {
        let mut disc = vec![usize::MAX; n_local];
        let mut low = vec![0usize; n_local];
        let mut parent_edge: Vec<Option<EdgeId>> = vec![None; n_local];
        let mut timer = 0usize;
        let mut bridges: Vec<EdgeId> = Vec::new();
        let mut dfs_stack: Vec<(usize, usize)> = Vec::new();

        for root in 0..n_local {
            if disc[root] != usize::MAX {
                continue;
            }
            disc[root] = timer;
            low[root] = timer;
            timer += 1;
            dfs_stack.push((root, 0));

            while !dfs_stack.is_empty() {
                let (u, adj_idx) = *dfs_stack.last().unwrap();
                if adj_idx < adj[u].len() {
                    let (v, eid) = adj[u][adj_idx];
                    dfs_stack.last_mut().unwrap().1 += 1;
                    if disc[v] == usize::MAX {
                        parent_edge[v] = Some(eid);
                        disc[v] = timer;
                        low[v] = timer;
                        timer += 1;
                        dfs_stack.push((v, 0));
                    } else if Some(eid) != parent_edge[u] {
                        low[u] = low[u].min(disc[v]);
                    }
                } else {
                    dfs_stack.pop();
                    if let Some(&(p, _)) = dfs_stack.last() {
                        low[p] = low[p].min(low[u]);
                        if let Some(eid) = parent_edge[u] {
                            if low[u] > disc[p] {
                                bridges.push(eid);
                            }
                        }
                    }
                }
            }
        }
        bridges
    }
}
