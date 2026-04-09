use super::Solver;
use crate::types::*;
use crate::uf::ParityUF;

impl Solver {
    /// BFS from component ci through Uncut+Unknown edges, collect reachable rose types.
    /// If `exclude_rose_mask` is nonzero, cells containing those rose symbols are
    /// treated as blocked (not entered during BFS). This gives tighter reachability
    /// estimates because same-type cells must be in different pieces.
    /// If `exclude_e` is Some, that edge is also treated as blocked.
    fn bfs_reachable_rose_types(
        &mut self,
        ci: usize,
        exclude_rose_mask: u8,
        exclude_e: Option<EdgeId>,
    ) -> u8 {
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        let mut types: u8 = 0;
        let sym = &self.cell_rose_sym;

        self.q_buf.clear();
        for &c in &self.comp_cells[ci] {
            self.rose_visited[c] = true;
            self.q_buf.push(c);
            if sym[c] != u8::MAX {
                types |= 1 << sym[c];
            }
        }

        while let Some(cur) = self.q_buf.pop() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                if let Some(ex) = exclude_e {
                    if eid == ex {
                        continue;
                    }
                }
                if self.edges[eid] == EdgeState::Cut {
                    continue;
                }
                let (a, b) = self.grid.edge_cells(eid);
                let other = if a == cur { b } else { a };
                if !self.grid.cell_exists[other] || self.rose_visited[other] {
                    continue;
                }
                if exclude_rose_mask != 0
                    && sym[other] != u8::MAX
                    && (exclude_rose_mask & (1 << sym[other])) != 0
                {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                if sym[other] != u8::MAX {
                    types |= 1 << sym[other];
                }
            }
        }

        types
    }

    /// Rose window advanced propagation.
    pub(crate) fn propagate_rose_separation(&mut self) -> Result<bool, ()> {
        if self.rose_bits_all == 0 {
            return Ok(false);
        }
        if self.curr_comp_id.is_empty() {
            return Ok(false);
        }

        // Quick check: if no components have rose symbols, skip
        let num_comp = self.curr_comp_sz.len();
        let mut any_rose_comp = false;
        for ci in self.growing(num_comp).collect::<Vec<_>>() {
            for &c in &self.comp_cells[ci] {
                if self.cell_rose_sym[c] != u8::MAX {
                    any_rose_comp = true;
                    break;
                }
            }
            if any_rose_comp {
                break;
            }
        }
        if !any_rose_comp {
            return Ok(false);
        }

        // Precompute comp_rose bitmask for each growing component
        let mut comp_rose_arr: Vec<u8> = vec![0; num_comp];
        for ci in self.growing(num_comp).collect::<Vec<_>>() {
            for &c in &self.comp_cells[ci] {
                let sym = self.cell_rose_sym[c];
                if sym != u8::MAX {
                    comp_rose_arr[ci] |= 1 << sym;
                }
            }
        }

        // --- Phase 1: Cross-type chokepoint Uncut forcing ---
        // Only check components where cutting a growth edge might disconnect a required type.
        // Skip components with many unknown growth edges (cutting one rarely disconnects).
        for ci in self.growing(num_comp).collect::<Vec<_>>() {

            let comp_rose = comp_rose_arr[ci];
            let missing = self.rose_bits_all & !comp_rose;
            if missing == 0 {
                continue;
            }

            // Count unknown growth edges; only check chokepoints (1-2 edges)
            let mut unknown_edges: Vec<EdgeId> = Vec::new();
            for &e in &self.growth_edges[ci] {
                if self.edges[e] == EdgeState::Unknown {
                    unknown_edges.push(e);
                }
            }
            if unknown_edges.len() > 2 {
                continue;
            }

            for e in unknown_edges {
                let reachable_without =
                    self.bfs_reachable_rose_types(ci, comp_rose, Some(e));
                if (reachable_without & missing) != missing {
                    if !self.set_edge(e, EdgeState::Uncut) {
                        return Err(());
                    }
                    return Ok(true);
                }
            }
        }

        // --- Phase 2: Two-level restricted reachability + single-growth-edge forcing ---
        for ci in self.growing(num_comp).collect::<Vec<_>>() {

            let comp_rose = comp_rose_arr[ci];
            let missing = self.rose_bits_all & !comp_rose;
            if missing == 0 {
                continue;
            }

            // Level 1: basic restricted reachability
            // Only run BFS for components missing exactly 1 type (most likely to fail,
            // and single BFS is cheap). Skip 2+ missing types (well-connected, unlikely to fail).
            if missing.count_ones() == 1 {
                let reachable = self.bfs_reachable_rose_types(ci, comp_rose, None);
                if (reachable & missing) != missing {
                    return Err(());
                }
            }

            // Single Unknown growth edge → force Uncut
            let mut unknown_growth: Option<EdgeId> = None;
            let mut unknown_count = 0usize;
            for &e in &self.growth_edges[ci] {
                if self.edges[e] == EdgeState::Unknown {
                    unknown_count += 1;
                    unknown_growth = Some(e);
                }
            }

            if unknown_count == 1 {
                let e = unknown_growth.unwrap();
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    // --- Phase 3: All-types complete component rose blocking ---
    pub(crate) fn propagate_rose_phase3(&mut self) -> Result<bool, ()> {
        if self.curr_comp_id.is_empty() {
            return Ok(false);
        }
        let num_comp = self.curr_comp_sz.len();
        for ci in self.growing(num_comp).collect::<Vec<_>>() {

            let mut comp_rose: u8 = 0;
            for &c in &self.comp_cells[ci] {
                let sym = self.cell_rose_sym[c];
                if sym != u8::MAX {
                    comp_rose |= 1 << sym;
                }
            }

            // Only applies if component has ALL rose types
            if comp_rose != self.rose_bits_all {
                continue;
            }

            // Check each growth edge: if neighbor cell has a rose symbol, force Cut
            let growth: Vec<EdgeId> = self.growth_edges[ci]
                .iter()
                .copied()
                .filter(|&e| self.edges[e] == EdgeState::Unknown)
                .collect();

            for e in growth {
                let (c1, c2) = self.grid.edge_cells(e);
                let other = if self.curr_comp_id[c1] == ci { c2 } else { c1 };
                if !self.grid.cell_exists[other] {
                    continue;
                }
                // Check if neighbor has a rose symbol
                if self.cell_rose_sym[other] != u8::MAX {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Parity propagation using a Union-Find with parity.
    ///
    /// Seeds the UF with:
    ///   - Rose cells of the same type → parity=1 (must be in different pieces)
    ///   - Uncut edges → parity=0 (cells are in the same piece)
    ///   - manual_sames → parity=0, manual_diffs → parity=1
    ///
    /// Cut edges are NOT used as seeds: a cut edge between two cells does not
    /// guarantee they are in different pieces (they may be connected via other paths).
    ///
    /// Detects contradictions (parity conflict) and forces Cut on unknown edges
    /// where both endpoints are already determined to be in different pieces.
    pub(crate) fn propagate_parity(&mut self) -> Result<bool, ()> {
        if self.rose_bits_all == 0
            && self.manual_diffs.is_empty()
            && self.manual_sames.is_empty()
        {
            return Ok(false);
        }

        let n = self.grid.num_cells();
        let ne = self.grid.num_edges();
        let two_piece = self.rose_exact_piece_count == Some(2);

        let mut uf = ParityUF::new(n);

        // Seed: rose cells of same type → different pieces (parity=1)
        // Only valid when there are exactly 2 cells per type (bipartite UF
        // cannot represent "all different" for 3+ cells without false same-piece
        // implications between the non-primary pairs).
        if let Some(pl) = &self.pair_layer {
            for type_cells in pl.rose_by_type() {
                if type_cells.len() == 2 {
                    if uf.union(type_cells[0], type_cells[1], 1).is_err() {
                        return Err(());
                    }
                }
            }
        }

        // manual_sames → parity=0: always valid (asserting same piece definitively)
        for &(c1, c2) in &self.manual_sames {
            if uf.union(c1, c2, 0).is_err() {
                return Err(());
            }
        }
        // manual_diffs → parity=1: only safe for 2-piece puzzles (same bipartite limitation)
        if two_piece {
            for &(c1, c2) in &self.manual_diffs {
                if uf.union(c1, c2, 1).is_err() {
                    return Err(());
                }
            }
        }

        // Seed: Uncut edges → parity=0 (always valid for any number of pieces)
        // Seed: Cut edges → parity=1 (only valid for 2-piece puzzles; in 3+ piece puzzles
        // A-B cut + B-C cut incorrectly implies A-C same via XOR transitivity)
        for e in 0..ne {
            let rel = match self.edges[e] {
                EdgeState::Uncut => 0u8,
                EdgeState::Cut => {
                    if two_piece { 1u8 } else { continue }
                }
                EdgeState::Unknown => continue,
            };
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            if uf.union(c1, c2, rel).is_err() {
                return Err(());
            }
        }

        // Force: unknown edges where both endpoints have parity=1 → Cut
        for e in 0..ne {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let (r1, p1) = uf.find(c1);
            let (r2, p2) = uf.find(c2);
            if r1 == r2 && (p1 ^ p2) == 1 {
                // Must be in different pieces → this edge must be Cut
                if !self.set_edge(e, EdgeState::Cut) {
                    return Err(());
                }
                return Ok(true);
            }
        }

        Ok(false)
    }
}
