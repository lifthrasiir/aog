use super::Solver;
use crate::types::*;

impl Solver {
    /// BFS from a single cell through Uncut+Unknown edges, excluding cells whose
    /// rose symbol is in `exclude_rose_mask`. Returns bitmask of reachable rose types.
    fn bfs_reachable_from_cell(&mut self, start: CellId, exclude_rose_mask: u8) -> u8 {
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        let mut types: u8 = 0;
        let sym = &self.cell_rose_sym;

        self.rose_visited[start] = true;
        self.q_buf.clear();
        self.q_buf.push(start);
        if sym[start] != u8::MAX {
            types |= 1 << sym[start];
        }

        while let Some(cur) = self.q_buf.pop() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
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

    /// BFS from component ci through Uncut+Unknown edges, excluding cells whose
    /// rose symbol is in `exclude_rose_mask`. Returns (reachable cells, reachable rose types).
    fn bfs_restricted_cells(&mut self, ci: usize, exclude_rose_mask: u8) -> (Vec<CellId>, u8) {
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        let mut cells = Vec::new();
        let mut types: u8 = 0;
        let sym = &self.cell_rose_sym;

        self.q_buf.clear();
        for &c in &self.comp_cells[ci] {
            self.rose_visited[c] = true;
            self.q_buf.push(c);
            cells.push(c);
            if sym[c] != u8::MAX {
                types |= 1 << sym[c];
            }
        }

        while let Some(cur) = self.q_buf.pop() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
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
                cells.push(other);
                if sym[other] != u8::MAX {
                    types |= 1 << sym[other];
                }
            }
        }

        (cells, types)
    }

    /// BFS from component ci through Uncut+Unknown edges, collect reachable rose types.
    /// If `exclude_rose_mask` is nonzero, cells containing those rose symbols are
    /// treated as blocked (not entered during BFS). This gives tighter reachability
    /// estimates because same-type cells must be in different pieces.
    fn bfs_reachable_rose_types(&mut self, ci: usize, exclude_rose_mask: u8) -> u8 {
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
                if self.edges[eid] == EdgeState::Cut {
                    continue;
                }
                let (a, b) = self.grid.edge_cells(eid);
                let other = if a == cur { b } else { a };
                if !self.grid.cell_exists[other] || self.rose_visited[other] {
                    continue;
                }
                // Skip cells whose rose symbol is in the exclusion mask
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

    /// BFS from component ci through Uncut+Unknown edges, excluding a specific
    /// edge `exclude_e` AND cells whose rose symbol is in `exclude_rose_mask`.
    /// Returns the bitmask of reachable rose types.
    fn bfs_reachable_excluding_edge(
        &mut self,
        ci: usize,
        exclude_rose_mask: u8,
        exclude_e: EdgeId,
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
                if eid == exclude_e || self.edges[eid] == EdgeState::Cut {
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
        for ci in 0..num_comp {
            if !self.can_grow_buf[ci] {
                continue;
            }
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

        // --- Phase 1: Cross-type chokepoint Uncut forcing ---
        for ci in 0..num_comp {
            if !self.can_grow_buf[ci] {
                continue;
            }

            let mut comp_rose: u8 = 0;
            for &c in &self.comp_cells[ci] {
                let sym = self.cell_rose_sym[c];
                if sym != u8::MAX {
                    comp_rose |= 1 << sym;
                }
            }

            let missing = self.rose_bits_all & !comp_rose;
            if missing == 0 {
                continue;
            }

            // For each Unknown growth edge, check if removing it
            // makes any required rose type unreachable.
            let unknown_edges: Vec<EdgeId> = self.growth_edges[ci]
                .iter()
                .copied()
                .filter(|&e| self.edges[e] == EdgeState::Unknown)
                .collect();

            for e in unknown_edges {
                // BFS from component, excluding e and same-type cells
                let reachable_without = self.bfs_reachable_excluding_edge(ci, comp_rose, e);
                if (reachable_without & missing) != missing {
                    // Cutting e would make some required type unreachable → force Uncut
                    if !self.set_edge(e, EdgeState::Uncut) {
                        return Err(());
                    }
                    return Ok(true);
                }
            }
        }

        // --- Phase 2: Two-level restricted reachability + single-growth-edge forcing ---
        for ci in 0..num_comp {
            if !self.can_grow_buf[ci] {
                continue;
            }

            let mut comp_rose: u8 = 0;
            for &c in &self.comp_cells[ci] {
                let sym = self.cell_rose_sym[c];
                if sym != u8::MAX {
                    comp_rose |= 1 << sym;
                }
            }

            let missing = self.rose_bits_all & !comp_rose;
            if missing == 0 {
                continue;
            }

            // Level 1: basic restricted reachability (same as before)
            let reachable = self.bfs_reachable_rose_types(ci, comp_rose);
            if (reachable & missing) != missing {
                return Err(());
            }

            // Level 2 (only for components close to completion: 1-2 missing types):
            // for each missing type S, check that at least one reachable S-cell
            // can reach ALL other missing types (with both comp_rose and S excluded).
            let mut missing_types: Vec<u8> = Vec::new();
            let mut m = missing;
            while m != 0 {
                missing_types.push(m.trailing_zeros() as u8);
                m &= m - 1;
            }

            // Only run two-level check for components close to completion (1-2 missing types)
            if missing_types.len() <= 2 {
                // Pick the missing type with fewest reachable cells to minimize BFS calls
                let n = self.grid.num_cells();
                let best_type = missing_types
                    .iter()
                    .copied()
                    .min_by_key(|&sym| {
                        self.cell_rose_sym[..n]
                            .iter()
                            .filter(|&&s| s == sym)
                            .count()
                    })
                    .unwrap();

                let others_mask = missing & !(1 << best_type); // other missing types
                if others_mask != 0 {
                    // BFS from component ci with comp_rose excluded, collecting reachable cells
                    // and reachable rose types
                    let (reachable_cells, _) = self.bfs_restricted_cells(ci, comp_rose);

                    // Find reachable cells of best_type
                    let mut found_valid = false;
                    for &c in &reachable_cells {
                        if self.cell_rose_sym[c] != best_type {
                            continue;
                        }
                        // BFS from this S-cell, excluding comp_rose AND best_type
                        let deeper_mask = comp_rose | (1 << best_type);
                        let deeper_reachable = self.bfs_reachable_from_cell(c, deeper_mask);
                        if (deeper_reachable & others_mask) == others_mask {
                            found_valid = true;
                            break;
                        }
                    }

                    if !found_valid {
                        return Err(());
                    }
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

        for ci in 0..num_comp {
            if !self.can_grow_buf[ci] {
                continue;
            }

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
}
