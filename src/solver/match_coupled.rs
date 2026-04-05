use super::Solver;
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

impl Solver {
    /// Build adjacency list from edges that are NOT Cut (Unknown or Uncut),
    /// only between existing cells.
    fn build_adjacency(&self) -> Vec<Vec<CellId>> {
        let n = self.grid.num_cells();
        let mut adj = vec![Vec::new(); n];
        for e in 0..self.grid.num_edges() {
            if self.edges[e] == EdgeState::Cut {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            adj[c1].push(c2);
            adj[c2].push(c1);
        }
        adj
    }

    fn record_bipartite_solution(&mut self, piece1: &[CellId], _piece2: &[CellId]) {
        let snap = self.changed.len();

        // Dedup: skip if this piece1 partition was already recorded
        let mut key: Vec<CellId> = piece1.to_vec();
        key.sort();
        if !self.seen_partitions.insert(key) {
            self.restore(snap);
            return;
        }

        let piece1_set: HashSet<CellId> = piece1.iter().copied().collect();

        for e in 0..self.grid.num_edges() {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let same_piece = piece1_set.contains(&c1) == piece1_set.contains(&c2);
            let new_state = if same_piece {
                EdgeState::Uncut
            } else {
                EdgeState::Cut
            };
            self.edges[e] = new_state;
            self.changed.push((e, EdgeState::Unknown));
        }

        let pieces = self.compute_pieces();
        if self.validate(&pieces) {
            self.save_solution(pieces);
        }

        self.restore(snap);
    }

    /// Apply rotation (0=R0,1=R90CW,2=R180,3=R270CW) then optional horizontal flip.
    pub(crate) fn apply_sigma(rot: usize, flip: bool, r: i32, c: i32) -> (i32, i32) {
        let (nr, nc) = match rot {
            0 => (r, c),
            1 => (c, -r),
            2 => (-r, -c),
            3 => (-c, r),
            _ => unreachable!(),
        };
        if flip {
            (nr, -nc)
        } else {
            (nr, nc)
        }
    }

    pub(crate) fn solve_match_2piece_coupled(&mut self) {
        let rose_cells: Vec<CellId> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| {
                if let CellClue::Rose { cell, .. } = cl {
                    Some(*cell)
                } else {
                    None
                }
            })
            .collect();

        if rose_cells.len() != 2 {
            return;
        }
        if !self.total_cells.is_multiple_of(2) {
            return;
        }

        let anchor1 = rose_cells[0];
        let anchor2 = rose_cells[1];
        let target = self.total_cells / 2;

        let (a1r, a1c) = self.grid.cell_pos(anchor1);
        let (a1r, a1c) = (a1r as i32, a1c as i32);

        let n = self.grid.num_cells();
        let existing_cells: Vec<CellId> = (0..n).filter(|&c| self.grid.cell_exists[c]).collect();

        let mut pos_to_cell: HashMap<(i32, i32), CellId> = HashMap::new();
        for &cell in &existing_cells {
            let (r, c) = self.grid.cell_pos(cell);
            pos_to_cell.insert((r as i32, c as i32), cell);
        }

        let adj = self.build_adjacency();
        let mut total_nodes: u64 = 0;

        for rot in 0..4usize {
            for flip in [false, true] {
                let (sa_r, sa_c) = Self::apply_sigma(rot, flip, a1r, a1c);

                for &dst in &existing_cells {
                    if dst == anchor1 {
                        continue;
                    }
                    let (dr, dc) = {
                        let (dr_, dc_) = self.grid.cell_pos(dst);
                        (dr_ as i32 - sa_r, dc_ as i32 - sa_c)
                    };

                    // Build forward (T) and backward (T⁻¹) maps
                    let mut fwd: Vec<Option<CellId>> = vec![None; n];
                    let mut bwd: Vec<Option<CellId>> = vec![None; n];
                    let mut ok = true;
                    for &cell in &existing_cells {
                        let (r, c) = self.grid.cell_pos(cell);
                        let (nr, nc) = Self::apply_sigma(rot, flip, r as i32, c as i32);
                        if let Some(&mapped) = pos_to_cell.get(&(nr + dr, nc + dc)) {
                            if mapped == cell {
                                ok = false;
                                break;
                            }
                            fwd[cell] = Some(mapped);
                            bwd[mapped] = Some(cell);
                        }
                    }
                    if !ok {
                        continue;
                    }

                    // Classify cells
                    let mut forced_p1 = 0usize;
                    let mut forced_p2 = 0usize;
                    for &cell in &existing_cells {
                        let in_img = bwd[cell].is_some();
                        let in_dom = fwd[cell].is_some();
                        if !in_img && !in_dom {
                            ok = false;
                            break;
                        }
                        if !in_img {
                            forced_p1 += 1;
                        }
                        if !in_dom {
                            forced_p2 += 1;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    if forced_p1 > target || forced_p2 > target {
                        continue;
                    }

                    // A1 must have T(A1) defined
                    if fwd[anchor1].is_none() {
                        continue;
                    }
                    // T⁻¹(A2) must exist
                    let a2_pre = match bwd[anchor2] {
                        Some(c) => c,
                        None => continue,
                    };
                    if fwd[a2_pre].is_none() {
                        continue;
                    }

                    // Build initial piece1 and piece2
                    let mut in_p1 = vec![false; n];
                    let mut in_p2 = vec![false; n];

                    // forced_p1 cells into piece1
                    for &cell in &existing_cells {
                        if bwd[cell].is_none() {
                            in_p1[cell] = true;
                        }
                    }
                    // forced_p2 cells into piece2
                    for &cell in &existing_cells {
                        if fwd[cell].is_none() {
                            in_p2[cell] = true;
                        }
                    }
                    // A1 and T⁻¹(A2) into piece1
                    in_p1[anchor1] = true;
                    in_p1[a2_pre] = true;
                    // preimages of forced_p2 into piece1
                    for &cell in &existing_cells {
                        if fwd[cell].is_none() {
                            // cell ∈ forced_p2, its preimage must be in piece1
                            if let Some(pre) = bwd[cell] {
                                in_p1[pre] = true;
                            }
                        }
                    }

                    let init_size = in_p1.iter().filter(|&&x| x).count();
                    if init_size > target {
                        continue;
                    }
                    // Add T-images of init piece1 to piece2
                    for &cell in &existing_cells {
                        if in_p1[cell] {
                            if let Some(tc) = fwd[cell] {
                                in_p2[tc] = true;
                            }
                        }
                    }

                    // Don't check piece1 connectivity upfront — DFS may bridge

                    let remaining = target - init_size;
                    if remaining == 0 {
                        // piece1 already complete — verify connectivity + coverage
                        let all_covered = existing_cells.iter().all(|&c| in_p1[c] || in_p2[c]);
                        if all_covered && Self::is_connected_set(&adj, &in_p1, &existing_cells) {
                            let p1: Vec<CellId> = existing_cells
                                .iter()
                                .filter(|&&c| in_p1[c])
                                .copied()
                                .collect();
                            let p2: Vec<CellId> = existing_cells
                                .iter()
                                .filter(|&&c| in_p2[c])
                                .copied()
                                .collect();
                            self.record_bipartite_solution(&p1, &p2);
                            if self.solution_count >= 2 {
                                return;
                            }
                        }
                        continue;
                    }

                    eprintln!("coupled: rot={} flip={} rem={}", rot, flip, remaining,);

                    self.node_count = 0;
                    self.coupled_dfs_v2(
                        &adj,
                        &fwd,
                        &existing_cells,
                        &mut in_p1,
                        &mut in_p2,
                        init_size,
                        target,
                    );
                    total_nodes += self.node_count;
                    if self.solution_count > 0 {
                        eprintln!(
                            "coupled: found {} solution(s) in {} nodes",
                            self.solution_count, self.node_count
                        );
                    }

                    if self.solution_count >= 2 {
                        return;
                    }
                }
            }
        }
        eprintln!("coupled: explored {} DFS nodes total", total_nodes);
    }

    pub(crate) fn is_connected_set(
        adj: &[Vec<CellId>],
        in_set: &[bool],
        existing_cells: &[CellId],
    ) -> bool {
        let start = match existing_cells.iter().find(|&&c| in_set[c]) {
            Some(&s) => s,
            None => return true,
        };
        let n = in_set.len();
        let mut vis = vec![false; n];
        let mut q = VecDeque::new();
        q.push_back(start);
        vis[start] = true;
        let mut cnt = 0usize;
        while let Some(c) = q.pop_front() {
            if !in_set[c] {
                continue;
            }
            cnt += 1;
            for &nb in &adj[c] {
                if !vis[nb] && in_set[nb] {
                    vis[nb] = true;
                    q.push_back(nb);
                }
            }
        }
        cnt == existing_cells.iter().filter(|&&c| in_set[c]).count()
    }

    #[allow(clippy::too_many_arguments)]
    fn coupled_dfs_v2(
        &mut self,
        adj: &[Vec<CellId>],
        fwd: &[Option<CellId>],
        existing_cells: &[CellId],
        in_p1: &mut [bool],
        in_p2: &mut [bool],
        size: usize,
        target: usize,
    ) {
        self.node_count += 1;
        if self.solution_count >= 2 {
            return;
        }

        if size == target {
            // piece1 ∪ piece2 = all cells guaranteed by sizes + disjointness
            // Check piece1 connectivity before recording
            if !Self::is_connected_set(adj, in_p1, existing_cells) {
                return;
            }
            let p1: Vec<CellId> = existing_cells
                .iter()
                .filter(|&&c| in_p1[c])
                .copied()
                .collect();
            let p2: Vec<CellId> = existing_cells
                .iter()
                .filter(|&&c| in_p2[c])
                .copied()
                .collect();
            self.record_bipartite_solution(&p1, &p2);
            return;
        }

        // Reachability pruning: count reachable unassigned cells from piece1 frontier
        {
            let remaining = target - size;
            let mut vis = vec![false; in_p1.len()];
            let mut q = VecDeque::new();
            for &c in existing_cells {
                if in_p1[c] {
                    vis[c] = true;
                    for &nb in &adj[c] {
                        if !vis[nb] && !in_p1[nb] && !in_p2[nb] && fwd[nb].is_some() {
                            vis[nb] = true;
                            q.push_back(nb);
                        }
                    }
                }
            }
            let mut reachable = 0usize;
            while let Some(c) = q.pop_front() {
                reachable += 1;
                for &nb in &adj[c] {
                    if !vis[nb] && !in_p1[nb] && !in_p2[nb] {
                        vis[nb] = true;
                        if fwd[nb].is_some() {
                            q.push_back(nb);
                        }
                    }
                }
            }
            if reachable < remaining {
                return;
            }
        }

        for &c in existing_cells {
            if self.solution_count >= 2 {
                return;
            }
            if in_p1[c] || in_p2[c] {
                continue;
            }
            // c must be frontier (adjacent to piece1)
            if !adj[c].iter().any(|&nb| in_p1[nb]) {
                continue;
            }
            let tc = match fwd[c] {
                Some(tc) if !in_p1[tc] && !in_p2[tc] => tc,
                _ => continue,
            };

            in_p1[c] = true;
            in_p2[tc] = true;
            self.coupled_dfs_v2(adj, fwd, existing_cells, in_p1, in_p2, size + 1, target);
            in_p1[c] = false;
            in_p2[tc] = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sigma_identity() {
        assert_eq!(Solver::apply_sigma(0, false, 3, 5), (3, 5));
    }

    #[test]
    fn apply_sigma_rotations() {
        let (r, c) = (2, 3);
        let mut p = (r, c);
        for _ in 0..4 {
            p = Solver::apply_sigma(1, false, p.0, p.1);
        }
        assert_eq!(p, (r, c), "4x R90 should return to original point");
    }

    #[test]
    fn apply_sigma_flip() {
        let result = Solver::apply_sigma(0, true, 3, 5);
        assert_eq!(result, (3, -5));
    }

    #[test]
    fn apply_sigma_all_symmetries() {
        // Verify R0 = identity
        assert_eq!(Solver::apply_sigma(0, false, 1, 0), (1, 0));
        // Verify R180 = (-r, -c)
        assert_eq!(Solver::apply_sigma(2, false, 3, 4), (-3, -4));
        // Verify R90 + flip of (1,0) → R90=(0,-1), flip=(0,1)
        assert_eq!(Solver::apply_sigma(1, true, 1, 0), (0, 1));
    }

    #[test]
    fn is_connected_set_connected() {
        // Simple graph: 0-1-2-3 all connected
        let adj = vec![
            vec![1],    // 0
            vec![0, 2], // 1
            vec![1, 3], // 2
            vec![2],    // 3
        ];
        let in_set = vec![true, true, true, true];
        let existing = vec![0, 1, 2, 3];
        assert!(Solver::is_connected_set(&adj, &in_set, &existing));
    }

    #[test]
    fn is_connected_set_disconnected() {
        // Graph: 0-1, 2-3 (two separate components)
        let adj = vec![
            vec![1], // 0
            vec![0], // 1
            vec![3], // 2
            vec![2], // 3
        ];
        let in_set = vec![true, true, true, true];
        let existing = vec![0, 1, 2, 3];
        assert!(!Solver::is_connected_set(&adj, &in_set, &existing));
    }

    #[test]
    fn is_connected_set_empty() {
        let adj: Vec<Vec<usize>> = vec![vec![], vec![]];
        let in_set = vec![false, false];
        let existing = vec![0, 1];
        // Empty set is trivially connected
        assert!(Solver::is_connected_set(&adj, &in_set, &existing));
    }
}
