use super::{EdgeForcer, Solver};
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

impl Solver {
    /// Compute an upper bound on the maximum possible final size for a component.
    /// If the component has a target area, returns that exactly.
    /// Otherwise, BFS through all non-Cut edges to find the total reachable cell
    /// count (i.e. the size if all Unknown edges were Uncut), capped at local max area.
    /// This is always a true upper bound: the component can never grow beyond this.
    fn growth_potential(&mut self, ci: usize) -> usize {
        if let Some(target) = self.curr_target_area[ci] {
            return target;
        }
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        self.q_buf.clear();
        let mut reachable = 0usize;
        for &c in &self.comp_cells[ci] {
            self.rose_visited[c] = true;
            self.q_buf.push(c);
            reachable += 1;
        }
        while let Some(cur) = self.q_buf.pop() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                if self.edges[eid] == EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == cur { c2 } else { c1 };
                if !self.grid.cell_exists[other] || self.rose_visited[other] {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                reachable += 1;
            }
        }
        reachable.min(self.curr_max_area[ci])
    }

    /// Flood fill, assign component IDs, compute target areas,
    /// growth edges, same-area merges, rose window propagation.
    /// Returns the number of components.
    fn build_components(&mut self) -> Result<usize, ()> {
        let n = self.grid.num_cells();
        self.comp_buf.fill(usize::MAX);

        for c in 0..n {
            if !self.grid.cell_exists[c] || self.comp_buf[c] != usize::MAX {
                continue;
            }
            self.flood_fill_decided(c);
        }

        let mut num_comp = 0usize;
        // reuse comp_buf values as IDs by mapping them to 0..num_comp
        // but we can just use a small array for mapping if we want to be super fast,
        // or just use the fact that comp_buf[c] is the representative cell.
        // Let's use a temporary mapping array to keep IDs contiguous.
        self.comp_buf2.clear();
        self.comp_buf2.resize(n, usize::MAX);
        let id_map = &mut self.comp_buf2[..n];
        for c in 0..n {
            if !self.grid.cell_exists[c] {
                continue;
            }
            let rep = self.comp_buf[c];
            if id_map[rep] == usize::MAX {
                id_map[rep] = num_comp;
                num_comp += 1;
            }
        }

        self.curr_comp_id.resize(n, usize::MAX);
        for c in 0..n {
            if self.grid.cell_exists[c] {
                self.curr_comp_id[c] = id_map[self.comp_buf[c]];
            }
        }

        self.curr_comp_sz.clear();
        self.curr_comp_sz.resize(num_comp, 0);
        // Reuse comp_cells allocations
        self.comp_cells.truncate(num_comp);
        for v in &mut self.comp_cells {
            v.clear();
        }
        while self.comp_cells.len() < num_comp {
            self.comp_cells.push(Vec::new());
        }
        let mut comp_clues = vec![Vec::new(); num_comp]; // This one is still a bit heavy, but clues are few
        let mut comp_rose: Vec<u8> = vec![0u8; num_comp];
        for c in 0..n {
            if self.grid.cell_exists[c] {
                let ci = self.curr_comp_id[c];
                self.curr_comp_sz[ci] += 1;
                self.comp_cells[ci].push(c);
                for &clue_idx in &self.cell_clues_indexed[c] {
                    let cl = &self.puzzle.cell_clues[clue_idx];
                    if let CellClue::Rose { symbol, .. } = cl {
                        let bit = 1u8 << symbol;
                        if comp_rose[ci] & bit != 0 {
                            return Err(()); // duplicate symbol in same component
                        }
                        comp_rose[ci] |= bit;
                    }
                    comp_clues[ci].push(cl);
                }
            }
        }

        self.curr_target_area.clear();
        self.curr_target_area.resize(num_comp, None);
        self.curr_min_area.clear();
        self.curr_min_area
            .resize(num_comp, self.eff_min_area.max(1));
        self.curr_max_area.clear();
        self.curr_max_area.resize(num_comp, self.eff_max_area);

        for ci in 0..num_comp {
            let mut areas = Vec::new();
            let mut local_min = self.curr_min_area[ci];
            let mut local_max = self.curr_max_area[ci];

            for clue in &comp_clues[ci] {
                if let CellClue::Area { value, .. } = clue {
                    areas.push(*value);
                } else if let CellClue::Polyomino { shape, .. } = clue {
                    areas.push(shape.cells.len());
                } else if let CellClue::Compass { cell, compass } = clue {
                    let (cmin, cmax, cexact) = self.get_compass_area_bounds(*cell, compass);
                    if let Some(exact) = cexact {
                        areas.push(exact);
                    }
                    local_min = local_min.max(cmin);
                    if let Some(maxv) = cmax {
                        local_max = local_max.min(maxv);
                    }
                }
            }

            if self.puzzle.rules.solitude && areas.len() > 1 {
                return Err(());
            }

            if !areas.is_empty() {
                let a0 = areas[0];
                if areas.iter().any(|&a| a != a0) {
                    return Err(());
                }
                if a0 < local_min || a0 > local_max {
                    return Err(());
                }
                self.curr_target_area[ci] = Some(a0);
                self.curr_min_area[ci] = a0;
                self.curr_max_area[ci] = a0;
                if self.curr_comp_sz[ci] > a0 {
                    return Err(());
                }
            } else {
                if local_min > local_max || self.curr_comp_sz[ci] > local_max {
                    return Err(());
                }
                self.curr_min_area[ci] = local_min;
                self.curr_max_area[ci] = local_max;
                // If local_min == local_max, we found an exact target!
                if local_min == local_max {
                    self.curr_target_area[ci] = Some(local_min);
                } else if self.curr_comp_sz[ci] > self.eff_max_area {
                    return Err(());
                }
            }
        }

        // Check Unknown edges to outside
        self.can_grow_buf.clear();
        self.can_grow_buf.resize(num_comp, false);
        // Reuse growth_edges allocations
        self.growth_edges.truncate(num_comp);
        for v in &mut self.growth_edges {
            v.clear();
        }
        while self.growth_edges.len() < num_comp {
            self.growth_edges.push(Vec::new());
        }
        let mut same_area_ef = EdgeForcer::new();

        for e in 0..self.grid.num_edges() {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let ci1 = self.curr_comp_id[c1];
            let ci2 = self.curr_comp_id[c2];
            if ci1 != ci2 {
                let cannot_merge = if self.puzzle.rules.solitude {
                    self.curr_target_area[ci1].is_some() && self.curr_target_area[ci2].is_some()
                } else if let (Some(a1), Some(a2)) =
                    (self.curr_target_area[ci1], self.curr_target_area[ci2])
                {
                    a1 != a2
                } else {
                    false
                };

                if cannot_merge {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    continue;
                }

                // Same-area merge: when distinct area sum = total cells,
                // all same-target components must eventually be in one piece.
                if self.same_area_groups {
                    if let (Some(a1), Some(a2)) =
                        (self.curr_target_area[ci1], self.curr_target_area[ci2])
                    {
                        if a1 == a2 {
                            same_area_ef.force_uncut(e);
                            self.can_grow_buf[ci1] = true;
                            self.can_grow_buf[ci2] = true;
                            continue;
                        }
                    }
                }

                self.can_grow_buf[ci1] = true;
                self.can_grow_buf[ci2] = true;
                self.growth_edges[ci1].push(e);
                self.growth_edges[ci2].push(e);

                let limit1 = self.curr_max_area[ci1];
                let limit2 = self.curr_max_area[ci2];

                if (self.curr_comp_sz[ci1] >= limit1 || self.curr_comp_sz[ci2] >= limit2)
                    && !self.set_edge(e, EdgeState::Cut)
                {
                    return Err(());
                }
            }
        }

        // Apply same-area forced Uncuts (collected above to avoid stale component state)
        same_area_ef.apply(self)?;

        // === Rose window propagation ===
        if self.rose_bits_all != 0 {
            // 1) Duplicate rose symbols within a component → contradiction.
            // Two same-type rose cells connected by Uncut edges are in the same piece,
            // violating the "exactly one of each type per piece" rule.
            for ci in 0..num_comp {
                let mut rose_counts: [u8; 8] = [0; 8];
                for &c in &self.comp_cells[ci] {
                    for &clue_idx in &self.cell_clues_indexed[c] {
                        if let CellClue::Rose { symbol, .. } = &self.puzzle.cell_clues[clue_idx] {
                            rose_counts[*symbol as usize] += 1;
                        }
                    }
                }
                for sym in 0..8u8 {
                    if self.rose_bits_all & (1 << sym) != 0 && rose_counts[sym as usize] > 1 {
                        return Err(());
                    }
                }
            }

            // 2) Sealed component missing a rose symbol → contradiction
            for ci in self.sealed(num_comp).collect::<Vec<_>>() {
                let missing = self.rose_bits_all & !comp_rose[ci];
                if missing != 0 {
                    return Err(());
                }
            }

            // 3) Growth edge forcing: collect forced Cut/Uncut edges
            let mut rose_cut_ef = EdgeForcer::new();

            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Unknown {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                if ci1 == ci2 {
                    continue;
                }

                // If merging would introduce a duplicate symbol → force Cut.
                // Check both: endpoint symbols and existing component symbols.
                let would_dup = (comp_rose[ci1] & comp_rose[ci2]) != 0;
                if would_dup {
                    rose_cut_ef.force_cut(e);
                }
            }

            rose_cut_ef.apply(self)?;
        }

        // === Rose exact piece count cap ===
        // If we know there are exactly K pieces, and K components are already sealed,
        // all remaining inter-component edges must be Cut (no more pieces allowed).
        if let Some(k) = self.rose_exact_piece_count {
            let sealed_count = self.sealed(num_comp).count();
            if sealed_count > k {
                return Err(());
            }
            if sealed_count == k {
                for e in 0..self.grid.num_edges() {
                    if self.edges[e] != EdgeState::Unknown {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(e);
                    if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                        continue;
                    }
                    let ci1 = self.curr_comp_id[c1];
                    let ci2 = self.curr_comp_id[c2];
                    if ci1 != ci2 {
                        if !self.set_edge(e, EdgeState::Cut) {
                            return Err(());
                        }
                    }
                }
            }
        }
        Ok(num_comp)
    }

    fn propagate_size_separation(&mut self, num_comp: usize) -> Result<bool, ()> {
        let mut progress = false;
        // === Size separation: early propagation ===
        if self.puzzle.rules.size_separation {
            // Step 1: Build sealed_neighbor_sizes — for each component, the sizes
            // of its adjacent sealed components (connected by Cut edges).
            // Also include target areas of adjacent growing components that have
            // a fixed target — their final size is known even before they seal.
            let mut sealed_neighbor_sizes: Vec<HashSet<usize>> = vec![HashSet::new(); num_comp];
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                if ci1 == ci2 {
                    continue;
                }
                // Sealed component: its current size is its final size
                if self.is_sealed(ci1) {
                    sealed_neighbor_sizes[ci2].insert(self.curr_comp_sz[ci1]);
                } else if let Some(t) = self.curr_target_area[ci1] {
                    // Growing with target: final size will be t
                    sealed_neighbor_sizes[ci2].insert(t);
                }
                if self.is_sealed(ci2) {
                    sealed_neighbor_sizes[ci1].insert(self.curr_comp_sz[ci2]);
                } else if let Some(t) = self.curr_target_area[ci2] {
                    sealed_neighbor_sizes[ci1].insert(t);
                }
            }

            // Step 2: For each Unknown growth edge, check if merging
            // the two adjacent components would create a size that conflicts with
            // any sealed neighbor of either component. If so, force Cut.
            let mut merge_conflict_ef = EdgeForcer::new();
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Unknown {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                if ci1 == ci2 {
                    continue;
                }
                let merged_sz = self.curr_comp_sz[ci1] + self.curr_comp_sz[ci2];
                if sealed_neighbor_sizes[ci1].contains(&merged_sz)
                    || sealed_neighbor_sizes[ci2].contains(&merged_sz)
                {
                    merge_conflict_ef.force_cut(e);
                }
            }
            progress = merge_conflict_ef.apply(self)? || progress;

            // Step 3: Forbidden size checks.
            // (a) If a growing component's target area is forbidden → contradiction.
            // (b) If a growing component's current size is forbidden and it has
            //     exactly 1 growth edge, force that edge Uncut.
            let mut forbidden_uncut_ef = EdgeForcer::new();
            for ci in 0..num_comp {
                let forbidden = &sealed_neighbor_sizes[ci];
                if forbidden.is_empty() {
                    continue;
                }

                if self.is_sealed(ci) {
                    // Sealed component at forbidden size — already caught by the
                    // sealed-vs-sealed check below, but let's be explicit:
                    if forbidden.contains(&self.curr_comp_sz[ci]) {
                        return Err(());
                    }
                } else {
                    // Target area is forbidden
                    if let Some(t) = self.curr_target_area[ci] {
                        if forbidden.contains(&t) {
                            return Err(());
                        }
                    }

                    // Current size is forbidden → must grow.
                    // If exactly 1 growth edge remains, force it Uncut.
                    if forbidden.contains(&self.curr_comp_sz[ci]) {
                        let mut unk_count = 0usize;
                        let mut last_unk = None;
                        for &e in &self.growth_edges[ci] {
                            if self.edges[e] == EdgeState::Unknown {
                                unk_count += 1;
                                last_unk = Some(e);
                            }
                        }
                        if unk_count == 0 {
                            return Err(());
                        }
                        if unk_count == 1 {
                            forbidden_uncut_ef.force_uncut(last_unk.unwrap());
                        }
                    }
                }
            }
            progress = forbidden_uncut_ef.apply(self)? || progress;

            // Cache for edge selection heuristic (Proposal C)
            self.cached_sealed_neighbor_sizes = Some(sealed_neighbor_sizes);
        } else {
            self.cached_sealed_neighbor_sizes = None;
        }
        Ok(progress)
    }

    /// Check if two compass cells are incompatible (cannot be in the same piece).
    /// Returns true if they cannot coexist in the same component.
    fn compass_cells_incompatible(
        &self,
        ca: CellId,
        pa: &CompassData,
        cb: CellId,
        pb: &CompassData,
    ) -> bool {
        let (ra, cola) = self.grid.cell_pos(ca);
        let (rb, colb) = self.grid.cell_pos(cb);

        // Zero-value direction conflicts
        if pa.n == Some(0) && rb < ra {
            return true;
        }
        if pb.n == Some(0) && ra < rb {
            return true;
        }
        if pa.s == Some(0) && rb > ra {
            return true;
        }
        if pb.s == Some(0) && ra > rb {
            return true;
        }
        if pa.e == Some(0) && colb > cola {
            return true;
        }
        if pb.e == Some(0) && cola > colb {
            return true;
        }
        if pa.w == Some(0) && colb < cola {
            return true;
        }
        if pb.w == Some(0) && cola < colb {
            return true;
        }

        // Value ordering: North
        if rb < ra {
            if let (Some(vb), Some(va)) = (pb.n, pa.n) {
                if vb >= va {
                    return true;
                }
            }
        } else if ra < rb {
            if let (Some(va), Some(vb)) = (pa.n, pb.n) {
                if va >= vb {
                    return true;
                }
            }
        } else if let (Some(va), Some(vb)) = (pa.n, pb.n) {
            if va != vb {
                return true;
            }
        }
        // Value ordering: South
        if rb > ra {
            if let (Some(vb), Some(va)) = (pb.s, pa.s) {
                if vb >= va {
                    return true;
                }
            }
        } else if ra > rb {
            if let (Some(va), Some(vb)) = (pa.s, pb.s) {
                if va >= vb {
                    return true;
                }
            }
        } else if let (Some(va), Some(vb)) = (pa.s, pb.s) {
            if va != vb {
                return true;
            }
        }
        // Value ordering: East
        if colb > cola {
            if let (Some(vb), Some(va)) = (pb.e, pa.e) {
                if vb >= va {
                    return true;
                }
            }
        } else if cola > colb {
            if let (Some(va), Some(vb)) = (pa.e, pb.e) {
                if va >= vb {
                    return true;
                }
            }
        } else if let (Some(va), Some(vb)) = (pa.e, pb.e) {
            if va != vb {
                return true;
            }
        }
        // Value ordering: West
        if colb < cola {
            if let (Some(vb), Some(va)) = (pb.w, pa.w) {
                if vb >= va {
                    return true;
                }
            }
        } else if cola < colb {
            if let (Some(va), Some(vb)) = (pa.w, pb.w) {
                if va >= vb {
                    return true;
                }
            }
        } else if let (Some(va), Some(vb)) = (pa.w, pb.w) {
            if va != vb {
                return true;
            }
        }

        false
    }

    /// Pre-search compass incompatibility check.
    /// For every pair of compass cells that are NOT yet in the same component,
    /// check if they are incompatible. If so:
    /// - If adjacent: force the edge between them to Cut
    /// - If non-adjacent: add to manual_diffs
    /// Returns the number of incompatibilities found.
    pub(crate) fn init_compass_incompatibility(&mut self) -> usize {
        // Collect all compass cells
        let compass_cells: Vec<(CellId, CompassData)> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| {
                if let CellClue::Compass { cell, compass } = cl {
                    if self.grid.cell_exists[*cell] {
                        return Some((*cell, compass.clone()));
                    }
                }
                None
            })
            .collect();

        let mut count = 0usize;
        for i in 0..compass_cells.len() {
            for j in (i + 1)..compass_cells.len() {
                let (ca, pa) = &compass_cells[i];
                let (cb, pb) = &compass_cells[j];

                // Skip if already in the same component
                if self.curr_comp_id[*ca] == self.curr_comp_id[*cb] {
                    continue;
                }

                if self.compass_cells_incompatible(*ca, pa, *cb, pb) {
                    count += 1;
                    if let Some(eid) = self.grid.edge_between(*ca, *cb) {
                        // Adjacent: force Cut
                        if self.edges[eid] == EdgeState::Unknown {
                            self.set_edge(eid, EdgeState::Cut);
                        }
                    } else {
                        // Non-adjacent: add to manual_diffs
                        self.manual_diffs.push((*ca, *cb));
                        self.manual_diff_set.insert((*ca, *cb));
                    }
                }
            }
        }
        count
    }

    fn propagate_compass_in_components(&mut self, num_comp: usize) -> Result<bool, ()> {
        self.debug_current_prop = "compass_in_comp";
        // Compass bounds: prune and propagate based on compass clues
        let mut progress = false;
        let mut compass_cut_ef = EdgeForcer::new();
        let mut compass_uncut_ef = EdgeForcer::new();

        for clue in &self.puzzle.cell_clues {
            let CellClue::Compass { cell, compass } = clue else {
                continue;
            };
            if !self.grid.cell_exists[*cell] {
                continue;
            }
            let ci = self.curr_comp_id[*cell];
            if ci == usize::MAX {
                continue;
            }
            let (cr, cc) = self.grid.cell_pos(*cell);
            let (cr_i, cc_i) = (cr as isize, cc as isize);

            let mut counts = [0usize; 4]; // N, S, E, W
            for &c in &self.comp_cells[ci] {
                let (pr, pc) = self.grid.cell_pos(c);
                let dr = pr as isize - cr_i;
                let dc = pc as isize - cc_i;
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
                (compass.n, 0),
                (compass.s, 1),
                (compass.e, 2),
                (compass.w, 3),
            ] {
                let Some(v) = val else { continue };

                if counts[idx] > v {
                    return Err(());
                }

                if counts[idx] == v {
                    // At limit: cut growth edges in this direction
                    for &e in &self.growth_edges[ci] {
                        let (c1, c2) = self.grid.edge_cells(e);
                        let other = if self.curr_comp_id[c1] == ci { c2 } else { c1 };
                        let (pr, pc) = self.grid.cell_pos(other);
                        let matches = match idx {
                            0 => (pr as isize) < cr_i,
                            1 => (pr as isize) > cr_i,
                            2 => (pc as isize) > cc_i,
                            3 => (pc as isize) < cc_i,
                            _ => false,
                        };
                        if matches {
                            compass_cut_ef.force_cut(e);
                        }
                    }
                }

                if counts[idx] < v {
                    // Below limit: if only 1 growth edge in this direction,
                    // and ALL other directions are blocked (at compass limit
                    // or have no growth edges), force Uncut.
                    // Growing in any other direction could create new growth
                    // edges in this direction via multi-hop paths, so we must
                    // ensure no alternative paths exist.
                    if self.is_growing(ci) {
                        let mut dir_growth_count = 0usize;
                        let mut dir_last_edge: Option<EdgeId> = None;
                        for &e in &self.growth_edges[ci] {
                            let (c1, c2) = self.grid.edge_cells(e);
                            let other = if self.curr_comp_id[c1] == ci { c2 } else { c1 };
                            let (pr, pc) = self.grid.cell_pos(other);
                            let matches = match idx {
                                0 => (pr as isize) < cr_i,
                                1 => (pr as isize) > cr_i,
                                2 => (pc as isize) > cc_i,
                                3 => (pc as isize) < cc_i,
                                _ => false,
                            };
                            if matches {
                                dir_growth_count += 1;
                                dir_last_edge = Some(e);
                            }
                        }
                        if dir_growth_count == 1 {
                            // Check ALL non-target directions
                            let compass_vals: [Option<usize>; 4] =
                                [compass.n, compass.s, compass.e, compass.w];
                            let mut all_others_blocked = true;
                            for pidx in 0..4 {
                                if pidx == idx {
                                    continue;
                                }
                                if let Some(pv) = compass_vals[pidx] {
                                    if counts[pidx] < pv {
                                        all_others_blocked = false;
                                        break;
                                    }
                                } else {
                                    let has_growth = self.growth_edges[ci].iter().any(|&e| {
                                        let (c1, c2) = self.grid.edge_cells(e);
                                        let other =
                                            if self.curr_comp_id[c1] == ci { c2 } else { c1 };
                                        let (pr, pc) = self.grid.cell_pos(other);
                                        match pidx {
                                            0 => (pr as isize) < cr_i,
                                            1 => (pr as isize) > cr_i,
                                            2 => (pc as isize) > cc_i,
                                            3 => (pc as isize) < cc_i,
                                            _ => false,
                                        }
                                    });
                                    if has_growth {
                                        all_others_blocked = false;
                                        break;
                                    }
                                }
                            }
                            if all_others_blocked {
                                compass_uncut_ef.force_uncut(dir_last_edge.unwrap());
                            }
                        }
                    }
                }

                // Sealed component with unsatisfied compass constraint → contradiction.
                // If the component can't grow but needs more cells in some direction,
                // the compass requirement can never be met.
                if self.is_sealed(ci) {
                    for &(val, idx) in &[
                        (compass.n, 0),
                        (compass.s, 1),
                        (compass.e, 2),
                        (compass.w, 3),
                    ] {
                        let Some(v) = val else { continue };
                        if counts[idx] < v {
                            return Err(());
                        }
                    }
                }
            }
        }

        // Pair-wise compass consistency within same component
        {
            let mut compass_per_comp: Vec<Vec<(CellId, CompassData)>> = vec![Vec::new(); num_comp];
            for cl in &self.puzzle.cell_clues {
                if let CellClue::Compass { cell, compass } = cl {
                    if self.grid.cell_exists[*cell] {
                        let ci = self.curr_comp_id[*cell];
                        if ci != usize::MAX {
                            compass_per_comp[ci].push((*cell, compass.clone()));
                        }
                    }
                }
            }

            for ccomp in &compass_per_comp {
                for i in 0..ccomp.len() {
                    for j in (i + 1)..ccomp.len() {
                        let (ca, pa) = &ccomp[i];
                        let (cb, pb) = &ccomp[j];
                        if self.compass_cells_incompatible(*ca, pa, *cb, pb) {
                            return Err(());
                        }
                    }
                }
            }

            // Compass bounding box propagation:
            // For each component with compass clues, compute bounding box from
            // compass values and cut growth edges leading outside it.
            for ci in 0..num_comp {
                if compass_per_comp[ci].is_empty() {
                    continue;
                }

                let mut bbox_min_r = 0isize;
                let mut bbox_max_r = self.grid.rows as isize - 1;
                let mut bbox_min_c = 0isize;
                let mut bbox_max_c = self.grid.cols as isize - 1;

                for &(cell, ref compass) in &compass_per_comp[ci] {
                    let (r, c) = self.grid.cell_pos(cell);
                    let (ri, ci_col) = (r as isize, c as isize);

                    // N=v: piece has v cells north of row r; connected path needs
                    // at least v northward steps, so min_row >= r - v.
                    if let Some(v) = compass.n {
                        bbox_min_r = bbox_min_r.max(ri - v as isize);
                    }
                    if let Some(v) = compass.s {
                        bbox_max_r = bbox_max_r.min(ri + v as isize);
                    }
                    if let Some(v) = compass.e {
                        bbox_max_c = bbox_max_c.min(ci_col + v as isize);
                    }
                    if let Some(v) = compass.w {
                        bbox_min_c = bbox_min_c.max(ci_col - v as isize);
                    }
                }

                if bbox_min_r > bbox_max_r || bbox_min_c > bbox_max_c {
                    return Err(());
                }

                // Cut growth edges whose target cell is outside the bounding box
                let to_cut: Vec<EdgeId> = self.growth_edges[ci]
                    .iter()
                    .filter(|&&e| {
                        if self.edges[e] != EdgeState::Unknown {
                            return false;
                        }
                        let (c1, c2) = self.grid.edge_cells(e);
                        let other = if self.curr_comp_id[c1] == ci { c2 } else { c1 };
                        let (pr, pc) = self.grid.cell_pos(other);
                        let (pri, pci) = (pr as isize, pc as isize);
                        pri < bbox_min_r || pri > bbox_max_r || pci < bbox_min_c || pci > bbox_max_c
                    })
                    .copied()
                    .collect();

                for e in to_cut {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    progress = true;
                }

                // NOTE: Compass reachability check was attempted but caused false
                // positives during intermediate propagation states. Probing handles
                // this kind of global constraint checking more reliably.
            }

            // Bridge/articulation-point based path forcing and single-gateway-edge forcing.
            // Skip during probing to avoid performance overhead on every probing propagation.
            if !self.in_probing {
                self.force_compass_via_bridges_and_gateways(
                    num_comp,
                    &compass_per_comp,
                    &mut compass_cut_ef,
                    &mut compass_uncut_ef,
                )?;
            }
        }

        progress = compass_cut_ef.apply(self)? || progress;
        progress = compass_uncut_ef.apply(self)? || progress;
        Ok(progress)
    }

    fn propagate_boxy_nonboxy(&mut self, num_comp: usize) -> Result<bool, ()> {
        let mut progress = false;
        if self.puzzle.rules.non_boxy || self.puzzle.rules.boxy {
            // Single O(N) pass to compute bounding boxes for all components
            let (mut min_r, mut max_r, mut min_c, mut max_c) = (
                vec![self.grid.rows; num_comp],
                vec![0; num_comp],
                vec![self.grid.cols; num_comp],
                vec![0; num_comp],
            );
            for ci in 0..num_comp {
                for &c in &self.comp_cells[ci] {
                    let (r, col) = self.grid.cell_pos(c);
                    min_r[ci] = min_r[ci].min(r);
                    max_r[ci] = max_r[ci].max(r);
                    min_c[ci] = min_c[ci].min(col);
                    max_c[ci] = max_c[ci].max(col);
                }
            }

            // Collect edges to force Cut for non-boxy (to avoid rectangle formation)
            let mut non_boxy_ef = EdgeForcer::new();

            for ci in 0..num_comp {
                let cell_count = self.curr_comp_sz[ci];
                if cell_count == 0 {
                    continue;
                }
                let bbox_w = max_r[ci] - min_r[ci] + 1;
                let bbox_h = max_c[ci] - min_c[ci] + 1;
                let bbox_size = bbox_w * bbox_h;
                let is_rect = cell_count == bbox_size;

                if self.is_sealed(ci) {
                    // Sealed component: final check
                    if self.puzzle.rules.non_boxy && is_rect {
                        return Err(());
                    }
                    if self.puzzle.rules.boxy && !is_rect {
                        return Err(());
                    }
                } else {
                    // Growing component
                    let max_possible = self.curr_max_area[ci];

                    if self.puzzle.rules.boxy && cell_count < bbox_size && bbox_size > max_possible
                    {
                        // Has holes in bbox but can't grow large enough to fill them
                        return Err(());
                    }

                    if self.puzzle.rules.non_boxy && cell_count < bbox_size {
                        let holes = bbox_size - cell_count;
                        if holes == 1 && max_possible >= bbox_size {
                            // 1 hole left and component can grow to fill it →
                            // must Cut all edges to the hole to prevent rectangle
                            for &c in &self.comp_cells[ci] {
                                let (r, col) = self.grid.cell_pos(c);
                                // Check all 4 neighbors
                                for (dr, dc) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                                    let nr = r as isize + dr;
                                    let nc = col as isize + dc;
                                    if nr < 0
                                        || nr >= self.grid.rows as isize
                                        || nc < 0
                                        || nc >= self.grid.cols as isize
                                    {
                                        continue;
                                    }
                                    let nid = self.grid.cell_id(nr as usize, nc as usize);
                                    if !self.grid.cell_exists[nid] || self.curr_comp_id[nid] == ci {
                                        continue;
                                    }
                                    // Check if this cell is the hole (inside bbox)
                                    let (hr, hc) = self.grid.cell_pos(nid);
                                    if hr >= min_r[ci]
                                        && hr <= max_r[ci]
                                        && hc >= min_c[ci]
                                        && hc <= max_c[ci]
                                    {
                                        if let Some(e) = self.grid.edge_between(c, nid) {
                                            if self.edges[e] == EdgeState::Unknown {
                                                non_boxy_ef.force_cut(e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Apply non-boxy forced cuts
            progress = non_boxy_ef.apply(self)? || progress;
        }
        Ok(progress)
    }

    fn propagate_inequality_clues(&mut self, _num_comp: usize) -> Result<bool, ()> {
        // Collect inequality clues (edge, smaller_first) to avoid borrow conflict
        // with growth_potential's mutable borrow of self.
        let ineq_clues: Vec<(EdgeId, bool)> = self
            .puzzle
            .edge_clues
            .iter()
            .filter_map(|cl| {
                if let EdgeClueKind::Inequality { smaller_first } = cl.kind {
                    Some((cl.edge, smaller_first))
                } else {
                    None
                }
            })
            .collect();

        for (e, smaller_first) in ineq_clues {
            if self.edges[e] != EdgeState::Cut {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let ci1 = self.curr_comp_id[c1];
            let ci2 = self.curr_comp_id[c2];
            if ci1 == ci2 {
                continue;
            }
            let (smaller_ci, larger_ci) = if smaller_first {
                (ci1, ci2)
            } else {
                (ci2, ci1)
            };
            let smaller_done = self.is_sealed(smaller_ci);
            let larger_done = self.is_sealed(larger_ci);

            if smaller_done && larger_done {
                if self.curr_comp_sz[smaller_ci] >= self.curr_comp_sz[larger_ci] {
                    return Err(());
                }
            } else if larger_done && self.curr_comp_sz[larger_ci] <= self.curr_comp_sz[smaller_ci] {
                return Err(());
            } else if smaller_done {
                // Component-wise max: use actual growth potential instead of global eff_max_area
                if self.curr_comp_sz[smaller_ci] >= self.growth_potential(larger_ci) {
                    return Err(());
                }
            } else if larger_done {
                // Larger sealed, smaller still growing: smaller's target must stay below larger
                if let Some(t) = self.curr_target_area[smaller_ci] {
                    if t >= self.curr_comp_sz[larger_ci] {
                        return Err(());
                    }
                }
            } else {
                // Both sides growing: use growth potentials for bounds checking
                let max_larger = self.growth_potential(larger_ci);

                // If smaller's current size already >= larger's maximum possible → impossible
                if self.curr_comp_sz[smaller_ci] >= max_larger {
                    return Err(());
                }
                // If smaller has a target that would make it >= larger's max → impossible
                if let Some(t) = self.curr_target_area[smaller_ci] {
                    if t >= max_larger {
                        return Err(());
                    }
                }
                // If larger has a target that would make it <= smaller's current → impossible
                if let Some(t) = self.curr_target_area[larger_ci] {
                    if t <= self.curr_comp_sz[smaller_ci] {
                        return Err(());
                    }
                }
            }
        }
        Ok(false)
    }

    fn propagate_diff_clues(&mut self, _num_comp: usize) -> Result<bool, ()> {
        // Diff clues: when one side is sealed, propagate target area to the other side
        let mut progress = false;
        let mut diff_ef = EdgeForcer::new();
        for &(e, value) in &self.diff_clues {
            if self.edges[e] != EdgeState::Cut {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let ci1 = self.curr_comp_id[c1];
            let ci2 = self.curr_comp_id[c2];
            if ci1 == ci2 {
                continue;
            }
            let sealed1 = self.is_sealed(ci1);
            let sealed2 = self.is_sealed(ci2);
            if sealed1 && sealed2 {
                if self.curr_comp_sz[ci1].abs_diff(self.curr_comp_sz[ci2]) != value {
                    return Err(());
                }
                continue;
            }
            let (sealed_ci, other_ci) = if sealed1 {
                (ci1, ci2)
            } else if sealed2 {
                (ci2, ci1)
            } else {
                continue;
            };
            let sealed_sz = self.curr_comp_sz[sealed_ci];
            let min_area = self.curr_min_area[other_ci];
            let max_area = self.curr_max_area[other_ci];
            let mut candidates: Vec<usize> = Vec::new();
            candidates.push(sealed_sz + value);
            if sealed_sz > value {
                candidates.push(sealed_sz - value);
            }
            candidates.retain(|&a| a >= min_area && a <= max_area);
            if candidates.is_empty() {
                return Err(());
            }
            if let Some(existing) = self.curr_target_area[other_ci] {
                if !candidates.contains(&existing) {
                    return Err(());
                }
                continue;
            }
            if candidates.len() == 1 {
                let new_target = candidates[0];
                if self.curr_comp_sz[other_ci] > new_target {
                    return Err(());
                }
                self.curr_target_area[other_ci] = Some(new_target);
                // Only seal growth edges if the component is already at the target.
                // Do NOT set progress = true here — curr_target_area is recomputed
                // each call, so this is not a persistent state change.
                if self.curr_comp_sz[other_ci] == new_target {
                    for &ge in &self.growth_edges[other_ci] {
                        if self.edges[ge] == EdgeState::Unknown {
                            diff_ef.force_cut(ge);
                        }
                    }
                }
            }
        }
        progress = diff_ef.apply(self)? || progress;
        Ok(progress)
    }

    /// Size separation, compass bounds, boxy/non-boxy,
    /// inequality and diff edge clue propagation.
    fn propagate_area_constraints(&mut self, num_comp: usize) -> Result<bool, ()> {
        let mut progress = false;

        progress |= self.propagate_size_separation(num_comp)?;

        // Cache growth edge counts for edge selection heuristic
        {
            let mut gec = vec![0usize; num_comp];
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Unknown {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                if ci1 == ci2 {
                    continue;
                }
                gec[ci1] += 1;
                gec[ci2] += 1;
            }
            self.cached_growth_edge_count = gec;
        }

        for ci in 0..num_comp {
            let target = self.curr_target_area[ci];
            let min_a = self.curr_min_area[ci];
            let max_a = self.curr_max_area[ci];

            if let Some(t) = target {
                if self.curr_comp_sz[ci] < t && self.is_sealed(ci) {
                    return Err(());
                }
                // Growth potential check: if the component can grow but
                // its maximum possible size (flood fill through non-Cut
                // edges, capped at max_area) is less than the target,
                // the target can never be reached → contradiction.
                // Only check when nearly sealed (few growth options) to
                // limit overhead. Skip during probing.
                if !self.in_probing && self.is_growing(ci) && self.curr_comp_sz[ci] < t {
                    let unk_growth = self.growth_edges[ci]
                        .iter()
                        .filter(|&&e| self.edges[e] == EdgeState::Unknown)
                        .count();
                    if unk_growth <= 4 {
                        let potential = self.growth_potential(ci);
                        if potential < t {
                            return Err(());
                        }
                    }
                }
                if self.curr_comp_sz[ci] == t && self.is_growing(ci) {
                    let to_cut: Vec<EdgeId> = self.growth_edges[ci]
                        .iter()
                        .filter(|&&e| self.edges[e] == EdgeState::Unknown)
                        .copied()
                        .collect();
                    for e in to_cut {
                        if !self.set_edge(e, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            } else {
                // Not a fixed target, but check bounds
                if self.curr_comp_sz[ci] < min_a && self.is_sealed(ci) {
                    return Err(());
                }
                if self.curr_comp_sz[ci] == max_a && self.is_growing(ci) {
                    let to_cut: Vec<EdgeId> = self.growth_edges[ci]
                        .iter()
                        .filter(|&&e| self.edges[e] == EdgeState::Unknown)
                        .copied()
                        .collect();
                    for e in to_cut {
                        if !self.set_edge(e, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            }
        }

        progress |= self.propagate_compass_in_components(num_comp)?;
        progress |= self.propagate_compass_placement_enumeration(num_comp)?;
        progress |= self.propagate_boxy_nonboxy(num_comp)?;
        progress |= self.propagate_inequality_clues(num_comp)?;
        progress |= self.propagate_diff_clues(num_comp)?;

        if self.puzzle.rules.size_separation {
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                if ci1 == ci2 {
                    continue;
                }
                if self.is_growing(ci1) || self.is_growing(ci2) {
                    continue;
                }
                if self.curr_comp_sz[ci1] == self.curr_comp_sz[ci2] {
                    return Err(());
                }
            }
        }

        Ok(progress)
    }

    pub(crate) fn propagate_area_bounds(&mut self) -> Result<bool, ()> {
        let num_comp = self.build_components()?;

        // Cut edge straddle check: if both cells of any Cut edge are in
        // the same component (connected via Uncut edges around the Cut),
        // that Cut is inside a piece → invalid.
        for e in 0..self.grid.num_edges() {
            if self.edges[e] != EdgeState::Cut {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            if self.curr_comp_id[c1] == self.curr_comp_id[c2] {
                return Err(());
            }
        }

        // Manual DIFF constraint check: if two cells declared as DIFF
        // (from branching or compass incompatibility) have ended up in
        // the same component, that's a contradiction.
        for &(d1, d2) in &self.manual_diffs {
            if self.curr_comp_id[d1] == self.curr_comp_id[d2] {
                return Err(());
            }
        }

        // Manual SAME constraint check: if two cells declared as SAME
        // are in different components, verify they can still be connected
        // via Uncut+Unknown edges. If fully isolated, it's a contradiction.
        for i in 0..self.manual_sames.len() {
            let (s1, s2) = self.manual_sames[i];
            let ci1 = self.curr_comp_id[s1];
            let ci2 = self.curr_comp_id[s2];
            if ci1 == ci2 {
                continue; // Already in same component, constraint satisfied
            }
            if !self.can_connect_comps(s1, s2) {
                return Err(());
            }
        }

        let progress = self.propagate_area_constraints(num_comp)?;
        self.propagate_shape_constraints(num_comp)?;
        self.check_complement_feasibility(num_comp)?;
        Ok(progress)
    }

    /// Complement feasibility check: after removing sealed component cells,
    /// verify that each connected region of remaining (non-sealed) cells
    /// can form at least one valid piece.
    ///
    /// A region of non-sealed cells (connected via non-Cut edges) must have
    /// size >= the maximum per-component min_area within it. If a sealed
    /// component splits the grid such that the remaining cells form a pocket
    /// too small for the most demanding component in it, that's a contradiction.
    ///
    /// Also checks compass-aware isolation: for each compass cell, computes
    /// the maximum reach in each specified direction. Cells outside all compass
    /// reaches that form a connected group with size < max_component_min_area
    /// in their region are contradictions (trapped by compass constraints).
    fn check_complement_feasibility(&mut self, num_comp: usize) -> Result<(), ()> {
        if self.in_probing {
            return Ok(());
        }

        // Quick check: need at least one sealed component
        let has_sealed = self.sealed(num_comp).next().is_some();
        if !has_sealed {
            return Ok(());
        }

        let n = self.grid.num_cells();

        // Phase 1: Region-based check.
        // Find connected regions of non-sealed cells via non-Cut edges.
        // For each region, verify it can accommodate the most demanding component.
        // Reuse comp_buf as visited (usize::MAX = unvisited).
        self.comp_buf[..n].fill(usize::MAX);

        for c in 0..n {
            if !self.grid.cell_exists[c] || self.comp_buf[c] != usize::MAX {
                continue;
            }
            let ci = self.curr_comp_id[c];
            if !self.is_growing(ci) {
                continue;
            }

            // BFS through non-Cut edges, skipping sealed cells
            let mut region_size = 0usize;
            let mut region_max_min = 0usize;
            self.q_buf.clear();
            self.q_buf.push(c);
            self.comp_buf[c] = 0;
            while let Some(cur) = self.q_buf.pop() {
                region_size += 1;
                let cur_ci = self.curr_comp_id[cur];
                if cur_ci < num_comp && cur_ci < self.curr_min_area.len() {
                    region_max_min = region_max_min.max(self.curr_min_area[cur_ci]);
                }
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if !self.grid.cell_exists[other] || self.comp_buf[other] != usize::MAX {
                        continue;
                    }
                    let oci = self.curr_comp_id[other];
                    if !self.is_growing(oci) {
                        continue;
                    }
                    self.comp_buf[other] = 0;
                    self.q_buf.push(other);
                }
            }

            // Region must be large enough for the most demanding component
            if region_size < region_max_min {
                return Err(());
            }

            // If eff_max_area is small, verify piece count feasibility
            if self.eff_max_area != usize::MAX && self.eff_max_area > 0 {
                let min_pieces = (region_size + self.eff_max_area - 1) / self.eff_max_area;
                let max_pieces = region_size / self.eff_min_area.max(1);
                if max_pieces < min_pieces {
                    return Err(());
                }
            }
        }

        // Phase 2: Compass-aware isolation check.
        // For each compass cell, compute max reach in each specified direction.
        // Cells outside ALL compass reaches that form a small connected group
        // (via non-Cut edges, skipping sealed cells) are contradictions.
        if self.has_compass_clue {
            self.check_compass_isolation(num_comp)?;
        }

        Ok(())
    }

    /// Check if cells outside all compass bounding boxes form groups that
    /// are too small to be valid pieces. A compass cell with N=v means the
    /// piece can have at most v cells north of it. Any cell beyond row
    /// (compass_row - v) is definitely NOT reachable by that compass piece
    /// (any connected path would require > v north cells).
    fn check_compass_isolation(&mut self, num_comp: usize) -> Result<(), ()> {
        let n = self.grid.num_cells();

        // Collect compass reach bounds: for each compass cell, compute
        // the max row/col it can reach in each specified direction.
        // A cell at (r, c) is "compass-covered" if it's within the reach
        // of at least one compass cell's component.
        let mut compass_covered = vec![false; n];

        for clue in &self.puzzle.cell_clues {
            let CellClue::Compass { cell, compass } = clue else {
                continue;
            };
            if !self.grid.cell_exists[*cell] {
                continue;
            }

            let (cr, cc) = self.grid.cell_pos(*cell);
            let cri = cr as isize;
            let cci = cc as isize;

            // Mark cells within compass reach as covered.
            // Only consider non-sealed cells (sealed cells are already assigned).
            for c2 in 0..n {
                if !self.grid.cell_exists[c2] || compass_covered[c2] {
                    continue;
                }
                let ci2 = self.curr_comp_id[c2];
                if self.is_sealed(ci2) {
                    continue; // skip sealed cells
                }

                let (r2, c2_col) = self.grid.cell_pos(c2);
                let r2i = r2 as isize;
                let c2i = c2_col as isize;

                // Check if cell is within compass reach in ALL specified directions
                let mut within_reach = true;
                if let Some(v) = compass.n {
                    if r2i < cri - v as isize {
                        within_reach = false;
                    }
                }
                if within_reach {
                    if let Some(v) = compass.s {
                        if r2i > cri + v as isize {
                            within_reach = false;
                        }
                    }
                }
                if within_reach {
                    if let Some(v) = compass.e {
                        if c2i > cci + v as isize {
                            within_reach = false;
                        }
                    }
                }
                if within_reach {
                    if let Some(v) = compass.w {
                        if c2i < cci - v as isize {
                            within_reach = false;
                        }
                    }
                }

                if within_reach {
                    compass_covered[c2] = true;
                }
            }
        }

        // Find connected groups of non-covered, non-sealed cells
        // Reuse comp_buf as visited (already filled from Phase 1, re-fill)
        self.comp_buf[..n].fill(usize::MAX);

        // Compute max min_area among compass-covered components for threshold
        let mut max_compass_min = 0usize;
        for ci in 0..num_comp {
            if self.is_growing(ci) && self.curr_min_area.len() > ci {
                max_compass_min = max_compass_min.max(self.curr_min_area[ci]);
            }
        }
        if max_compass_min <= 1 {
            return Ok(()); // Nothing useful to check
        }

        for c in 0..n {
            if !self.grid.cell_exists[c] || compass_covered[c] || self.comp_buf[c] != usize::MAX {
                continue;
            }
            let ci = self.curr_comp_id[c];
            if !self.is_growing(ci) {
                continue;
            }

            // BFS through non-Cut edges, skipping covered and sealed cells
            let mut group_size = 0usize;
            self.q_buf.clear();
            self.q_buf.push(c);
            self.comp_buf[c] = 0;
            while let Some(cur) = self.q_buf.pop() {
                group_size += 1;
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Cut {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if !self.grid.cell_exists[other]
                        || compass_covered[other]
                        || self.comp_buf[other] != usize::MAX
                    {
                        continue;
                    }
                    let oci = self.curr_comp_id[other];
                    if !self.is_growing(oci) {
                        continue;
                    }
                    self.comp_buf[other] = 0;
                    self.q_buf.push(other);
                }
            }

            if group_size < max_compass_min {
                return Err(());
            }
        }

        Ok(())
    }

    /// Check if two cells can be connected via Uncut+Unknown edges (BFS).
    /// Used to verify manual_sames constraints: if no path exists, the SAME
    /// constraint can never be satisfied.
    fn can_connect_comps(&mut self, c1: CellId, c2: CellId) -> bool {
        let n = self.grid.num_cells();
        self.rose_visited[..n].fill(false);
        self.rose_visited[c1] = true;
        self.q_buf.clear();
        self.q_buf.push(c1);
        while let Some(cur) = self.q_buf.pop() {
            if cur == c2 {
                return true;
            }
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                if self.edges[eid] == EdgeState::Cut {
                    continue;
                }
                let (a, b) = self.grid.edge_cells(eid);
                let other = if a == cur { b } else { a };
                if !self.grid.cell_exists[other] || self.rose_visited[other] {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
            }
        }
        false
    }

    pub(crate) fn flood_fill_decided(&mut self, start: CellId) {
        self.comp_buf[start] = start;
        self.q_buf.clear();
        self.q_buf.push(start);
        while let Some(cur) = self.q_buf.pop() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == cur { c2 } else { c1 };
                if !self.grid.cell_exists[other] || self.comp_buf[other] != usize::MAX {
                    continue;
                }
                if self.edges[eid] == EdgeState::Uncut {
                    self.comp_buf[other] = start;
                    self.q_buf.push(other);
                }
            }
        }
    }

    fn get_compass_area_bounds(
        &self,
        cell: CellId,
        compass: &CompassData,
    ) -> (usize, Option<usize>, Option<usize>) {
        let (r, c) = self.grid.cell_pos(cell);

        // Infer 0 for directions where no cells exist in the grid.
        // Check actual cell existence, not just row/col boundaries,
        // to handle non-rectangular grids (diamonds, irregular shapes).
        let mut has_north = false;
        let mut has_south = false;
        let mut has_east = false;
        let mut has_west = false;
        for dr in 1..self.grid.rows {
            let nr = r as isize - dr as isize;
            if nr < 0 {
                break;
            }
            let nid = self.grid.cell_id(nr as usize, c);
            if self.grid.cell_exists[nid] {
                has_north = true;
                break;
            }
        }
        for dr in 1..self.grid.rows {
            let sr = r + dr;
            if sr >= self.grid.rows {
                break;
            }
            let sid = self.grid.cell_id(sr, c);
            if self.grid.cell_exists[sid] {
                has_south = true;
                break;
            }
        }
        for dc in 1..self.grid.cols {
            let ec = c + dc;
            if ec >= self.grid.cols {
                break;
            }
            let eid = self.grid.cell_id(r, ec);
            if self.grid.cell_exists[eid] {
                has_east = true;
                break;
            }
        }
        for dc in 1..self.grid.cols {
            let wc = c as isize - dc as isize;
            if wc < 0 {
                break;
            }
            let wid = self.grid.cell_id(r, wc as usize);
            if self.grid.cell_exists[wid] {
                has_west = true;
                break;
            }
        }

        let n = compass
            .n
            .or_else(|| if !has_north { Some(0) } else { None });
        let s = compass
            .s
            .or_else(|| if !has_south { Some(0) } else { None });
        let e = compass.e.or_else(|| if !has_east { Some(0) } else { None });
        let w = compass.w.or_else(|| if !has_west { Some(0) } else { None });

        let nv = n.unwrap_or(0);
        let sv = s.unwrap_or(0);
        let ev = e.unwrap_or(0);
        let wv = w.unwrap_or(0);

        let min_area = 1 + (nv + sv).max(ev + wv);

        let mut exact_area = None;
        if e == Some(0) && w == Some(0) {
            exact_area = Some(1 + nv + sv);
        } else if n == Some(0) && s == Some(0) {
            exact_area = Some(1 + ev + wv);
        }

        let mut max_area = None;
        if n.is_some() && s.is_some() && e.is_some() && w.is_some() {
            max_area = Some(1 + nv + sv + ev + wv);
        }

        (min_area, max_area, exact_area)
    }

    /// Tight compass placement enumeration for small-area components.
    /// For each growing compass component with max_area ≤ 8, finds all reachable
    /// components within the compass bounding box, then enumerates valid connected
    /// merges. Forces growth edges to Uncut/Cut based on whether neighboring
    /// components appear in all/no valid placements.
    fn propagate_compass_placement_enumeration(&mut self, _num_comp: usize) -> Result<bool, ()> {
        self.debug_current_prop = "compass_place_enum";
        const MAX_AREA_THRESHOLD: usize = 8;
        const MAX_REACHABLE_COMPS: usize = 16;
        const MAX_PLACEMENTS: usize = 500;

        // Enumeration is expensive (O(cells+edges) per compass clue); skip during probing.
        if self.in_probing {
            return Ok(false);
        }

        let mut forced_cuts: Vec<(EdgeId, CellId)> = Vec::new(); // (edge, compass_cell)
        let mut forced_uncuts: Vec<(EdgeId, CellId)> = Vec::new();

        let compass_entries: Vec<(CellId, CompassData)> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| {
                if let CellClue::Compass { cell, compass } = cl {
                    if self.grid.cell_exists[*cell] {
                        Some((*cell, compass.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        'outer: for (cell, compass) in &compass_entries {
            let ci = self.curr_comp_id[*cell];
            if ci == usize::MAX {
                continue;
            }
            if self.is_sealed(ci) {
                continue;
            }
            let max_a = self.curr_max_area[ci];
            let min_a = self.curr_min_area[ci];
            if max_a > MAX_AREA_THRESHOLD {
                continue;
            }

            let (cr, cc) = self.grid.cell_pos(*cell);
            let (cri, cci) = (cr as isize, cc as isize);

            // Compass limits [N, S, E, W]
            let limits = [compass.n, compass.s, compass.e, compass.w];

            // Bounding box from compass constraints (tightest possible)
            let bbox_min_r = limits[0].map_or(0isize, |v| cri - v as isize).max(0);
            let bbox_max_r = limits[1]
                .map_or(self.grid.rows as isize - 1, |v| cri + v as isize)
                .min(self.grid.rows as isize - 1);
            let bbox_min_c = limits[3].map_or(0isize, |v| cci - v as isize).max(0);
            let bbox_max_c = limits[2]
                .map_or(self.grid.cols as isize - 1, |v| cci + v as isize)
                .min(self.grid.cols as isize - 1);

            // Build fresh local components using CURRENT edge states (not stale curr_comp_id).
            // This avoids false contradictions when earlier propagation in this same round
            // has set edges Uncut but build_components hasn't been re-run yet.
            //
            // Step 1: Find all cells reachable from compass cell via non-Cut edges within bbox.
            let n = self.grid.num_cells();
            let mut cell_in_reachable = vec![false; n];
            let mut reachable_cells: Vec<CellId> = Vec::new();
            {
                cell_in_reachable[*cell] = true;
                reachable_cells.push(*cell);
                let mut bfs_q: VecDeque<CellId> = VecDeque::new();
                bfs_q.push_back(*cell);
                while let Some(cur) = bfs_q.pop_front() {
                    for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                        if self.edges[eid] == EdgeState::Cut {
                            continue;
                        }
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let other = if c1 == cur { c2 } else { c1 };
                        if !self.grid.cell_exists[other] || cell_in_reachable[other] {
                            continue;
                        }
                        let (pr, pc) = self.grid.cell_pos(other);
                        if (pr as isize) < bbox_min_r
                            || (pr as isize) > bbox_max_r
                            || (pc as isize) < bbox_min_c
                            || (pc as isize) > bbox_max_c
                        {
                            continue;
                        }
                        cell_in_reachable[other] = true;
                        reachable_cells.push(other);
                        bfs_q.push_back(other);
                    }
                }
            }

            // Step 2: Group reachable cells into fresh local components
            // using CURRENT Uncut edges (not stale curr_comp_id).
            // IMPORTANT: Follow Uncut edges GLOBALLY (not restricted to bbox).
            // If a cell X within bbox is Uncut-connected to a cell Y outside bbox,
            // they form the SAME local component. This prevents false forced-uncuts:
            // e.g., if X is already committed to another piece (via Y), including X
            // in the current compass component would drag in all of Y's cells too.
            // comp_dir_counts correctly counts ALL cells (including outside-bbox ones),
            // so the DFS prunes such components when they cause count violations.
            //
            // The compass cell always ends up in local component 0.
            let mut local_comp_of = vec![usize::MAX; n]; // cell -> local comp id
            let mut local_comps: Vec<Vec<CellId>> = Vec::new(); // local comp -> cells

            for &start in &reachable_cells {
                if local_comp_of[start] != usize::MAX {
                    continue;
                }
                if local_comps.len() >= MAX_REACHABLE_COMPS {
                    continue 'outer;
                }
                let lc = local_comps.len();
                let mut lcomp_cells = vec![start];
                local_comp_of[start] = lc;
                let mut q: VecDeque<CellId> = VecDeque::new();
                q.push_back(start);
                while let Some(cur) = q.pop_front() {
                    for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                        if self.edges[eid] != EdgeState::Uncut {
                            continue; // Only follow Uncut edges for same-component grouping
                        }
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let other = if c1 == cur { c2 } else { c1 };
                        // GLOBAL flood-fill: follow Uncut edges beyond bbox boundary.
                        // Do NOT restrict to cell_in_reachable here.
                        if !self.grid.cell_exists[other] || local_comp_of[other] != usize::MAX {
                            continue;
                        }
                        local_comp_of[other] = lc;
                        lcomp_cells.push(other);
                        q.push_back(other);
                    }
                }
                local_comps.push(lcomp_cells);
            }

            // Ensure compass cell is in local component 0 (swap if needed)
            let compass_lc = local_comp_of[*cell];
            if compass_lc != 0 {
                local_comps.swap(0, compass_lc);
                for &c in &local_comps[0] {
                    local_comp_of[c] = 0;
                }
                for &c in &local_comps[compass_lc] {
                    local_comp_of[c] = compass_lc;
                }
            }

            let num_rc = local_comps.len();
            if num_rc > MAX_REACHABLE_COMPS {
                continue 'outer;
            }

            // Check if local comp 0 can still grow (has Unknown edges to other local comps)
            let can_grow = local_comps[0].iter().any(|&c| {
                self.grid.cell_edges(c).into_iter().flatten().any(|eid| {
                    if self.edges[eid] != EdgeState::Unknown {
                        return false;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == c { c2 } else { c1 };
                    cell_in_reachable[other]
                        && local_comp_of[other] != 0
                        && local_comp_of[other] != usize::MAX
                })
            });
            // Also check: are there Unknown edges from local comp 0 to OUTSIDE bbox?
            let has_outside_growth = local_comps[0].iter().any(|&c| {
                self.grid.cell_edges(c).into_iter().flatten().any(|eid| {
                    if self.edges[eid] != EdgeState::Unknown {
                        return false;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == c { c2 } else { c1 };
                    self.grid.cell_exists[other] && !cell_in_reachable[other]
                })
            });
            if !can_grow && !has_outside_growth {
                continue; // sealed within this propagation, skip enumeration (handled elsewhere)
            }

            // Compute per-component: directional counts and cell sizes
            let mut comp_dir_counts = vec![[0usize; 4]; num_rc];
            let mut comp_sizes = vec![0usize; num_rc];
            for (lc, lcomp) in local_comps.iter().enumerate() {
                for &c in lcomp {
                    let (pr, pc) = self.grid.cell_pos(c);
                    let dr = pr as isize - cri;
                    let dc = pc as isize - cci;
                    if dr < 0 {
                        comp_dir_counts[lc][0] += 1; // N
                    }
                    if dr > 0 {
                        comp_dir_counts[lc][1] += 1; // S
                    }
                    if dc > 0 {
                        comp_dir_counts[lc][2] += 1; // E
                    }
                    if dc < 0 {
                        comp_dir_counts[lc][3] += 1; // W
                    }
                    comp_sizes[lc] += 1;
                }
            }

            // Check local comp 0's base counts don't already exceed limits
            for d in 0..4 {
                if let Some(v) = limits[d] {
                    if comp_dir_counts[0][d] > v {
                        eprintln!("Compass placement base_count_err: compass={:?} dir={} count={} limit={}",
                            self.grid.cell_pos(*cell), d, comp_dir_counts[0][d], v);
                        return Err(());
                    }
                }
            }

            // Build component adjacency bitmask via Unknown edges.
            // Must iterate ALL cells in every local comp, including outside-bbox cells
            // that arrived via global flood-fill. Otherwise an outside-bbox cell in
            // local_comps[lc] that has an Unknown edge to another local comp would not
            // be recorded in adj_mask, causing the DFS to miss valid placements, which
            // in turn inflates in_all (false forced-uncuts) or empties valid_placements
            // (false contradictions).
            let mut adj_mask = vec![0u32; num_rc];
            for lc in 0..num_rc {
                for ci in 0..local_comps[lc].len() {
                    let c = local_comps[lc][ci];
                    for eid in self.grid.cell_edges(c).into_iter().flatten() {
                        if self.edges[eid] != EdgeState::Unknown {
                            continue;
                        }
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let other = if c1 == c { c2 } else { c1 };
                        if !self.grid.cell_exists[other] {
                            continue;
                        }
                        let l2 = local_comp_of[other];
                        if l2 == usize::MAX || l2 == lc {
                            continue;
                        }
                        adj_mask[lc] |= 1u32 << l2;
                    }
                }
            }

            // Enumerate valid connected merges via DFS
            // Local comp 0 (compass cell's component) is the mandatory start
            let initial_frontier = adj_mask[0];
            let base_counts = comp_dir_counts[0];
            let base_size = comp_sizes[0];

            let mut valid_placements: Vec<u32> = Vec::new();
            let overflow = Self::compass_placement_dfs(
                1u32, // current_mask: bit 0 = local comp 0
                initial_frontier,
                0u32, // excluded_mask: none
                base_counts,
                base_size,
                &comp_dir_counts,
                &comp_sizes,
                &adj_mask,
                &limits,
                min_a,
                max_a,
                MAX_PLACEMENTS,
                &mut valid_placements,
            );

            if overflow {
                continue 'outer; // too many placements, can't usefully force anything
            }
            if valid_placements.is_empty() {
                // No valid bbox-internal placement found. This is a genuine contradiction
                // if the DFS covered all possibilities (adj_mask now includes outside-bbox
                // cell adjacencies). Return Err to signal the contradiction.
                return Err(());
            }

            // Compute intersection (in_all) and union (in_any) over valid placements
            let mut in_all: u32 = u32::MAX;
            let mut in_any: u32 = 0;
            for &m in &valid_placements {
                in_all &= m;
                in_any |= m;
            }
            in_all &= !1u32; // local comp 0 is always merged, clear from forced-uncut

            // Find growth edges from local comp 0 and apply forced cuts/uncuts
            // Growth edges: Unknown edges from local comp 0's cells to other cells
            for &c in &local_comps[0] {
                for eid in self.grid.cell_edges(c).into_iter().flatten() {
                    if self.edges[eid] != EdgeState::Unknown {
                        continue;
                    }
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == c { c2 } else { c1 };
                    if !self.grid.cell_exists[other] {
                        continue;
                    }

                    if !cell_in_reachable[other] {
                        // Outside bbox → force Cut
                        forced_cuts.push((eid, *cell));
                        continue;
                    }

                    let lj = local_comp_of[other];
                    if lj == usize::MAX || lj == 0 {
                        continue; // shouldn't happen, but skip
                    }

                    let bit = 1u32 << lj;
                    if in_all & bit != 0 {
                        eprintln!("Compass placement forced_uncut: compass={:?} comp0_cell={:?} other={:?} lj={} comp_dir={:?} valid={}",
                            self.grid.cell_pos(*cell), self.grid.cell_pos(c), self.grid.cell_pos(other), lj,
                            comp_dir_counts[lj], valid_placements.len());
                        forced_uncuts.push((eid, *cell));
                    } else if in_any & bit == 0 {
                        forced_cuts.push((eid, *cell)); // can never merge
                    }
                }
            }
        }

        // Apply all forced edges
        let mut progress = false;
        for &(e, compass_c) in &forced_cuts {
            if self.edges[e] == EdgeState::Unknown {
                let (ea, eb) = self.grid.edge_cells(e);
                let pa = self.grid.cell_pos(ea);
                let pb = self.grid.cell_pos(eb);
                eprintln!(
                    "Compass placement forced_cut: compass={:?} cells={:?}-{:?}",
                    self.grid.cell_pos(compass_c),
                    pa,
                    pb
                );
                if !self.set_edge(e, EdgeState::Cut) {
                    return Err(());
                }
                progress = true;
            }
        }
        for &(e, compass_c) in &forced_uncuts {
            if self.edges[e] == EdgeState::Unknown {
                let (ea, eb) = self.grid.edge_cells(e);
                let pa = self.grid.cell_pos(ea);
                let pb = self.grid.cell_pos(eb);
                eprintln!(
                    "Compass placement forced_uncut_apply: compass={:?} cells={:?}-{:?}",
                    self.grid.cell_pos(compass_c),
                    pa,
                    pb
                );
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
            }
        }

        Ok(progress)
    }

    /// DFS for compass placement enumeration: enumerate all valid connected component merges.
    /// Uses include/exclude branching on frontier components (smallest index first).
    /// Returns true if the result set overflowed (too many placements).
    fn compass_placement_dfs(
        current_mask: u32,
        frontier_mask: u32,
        excluded_mask: u32,
        counts: [usize; 4],
        size: usize,
        comp_dir_counts: &[[usize; 4]],
        comp_sizes: &[usize],
        adj_mask: &[u32],
        limits: &[Option<usize>; 4],
        min_a: usize,
        max_a: usize,
        max_placements: usize,
        results: &mut Vec<u32>,
    ) -> bool {
        // Record if current merged set is a valid placement
        if size >= min_a {
            let satisfied = (0..4).all(|d| limits[d].map_or(true, |v| counts[d] == v));
            if satisfied {
                results.push(current_mask);
                if results.len() >= max_placements {
                    return true; // overflow
                }
            }
        }

        if size >= max_a || frontier_mask == 0 {
            return false;
        }

        // Pick the smallest-index frontier component
        let v = frontier_mask.trailing_zeros() as usize;
        let v_bit = 1u32 << v;
        let rest = frontier_mask & !v_bit;

        // Branch 1: include component v
        let new_counts = [
            counts[0] + comp_dir_counts[v][0],
            counts[1] + comp_dir_counts[v][1],
            counts[2] + comp_dir_counts[v][2],
            counts[3] + comp_dir_counts[v][3],
        ];
        let new_size = size + comp_sizes[v];

        let exceeds = (0..4).any(|d| limits[d].map_or(false, |lim| new_counts[d] > lim));
        if !exceeds && new_size <= max_a {
            let new_current = current_mask | v_bit;
            // Expand frontier: add adj of v that are not yet merged or excluded
            let new_frontier = rest | (adj_mask[v] & !new_current & !excluded_mask);
            if Self::compass_placement_dfs(
                new_current,
                new_frontier,
                excluded_mask,
                new_counts,
                new_size,
                comp_dir_counts,
                comp_sizes,
                adj_mask,
                limits,
                min_a,
                max_a,
                max_placements,
                results,
            ) {
                return true;
            }
        }

        // Branch 2: exclude component v (don't merge in this branch)
        Self::compass_placement_dfs(
            current_mask,
            rest,
            excluded_mask | v_bit,
            counts,
            size,
            comp_dir_counts,
            comp_sizes,
            adj_mask,
            limits,
            min_a,
            max_a,
            max_placements,
            results,
        )
    }

    /// Returns Err if any group's anchors are disconnected.
    pub(crate) fn propagate_same_area_reachability(&mut self) -> Result<bool, ()> {
        if !self.same_area_groups {
            return Ok(false);
        }

        // Group area clue cells by value
        let mut area_anchors: HashMap<usize, Vec<CellId>> = HashMap::new();
        for clue in &self.puzzle.cell_clues {
            if let CellClue::Area { cell, value } = clue {
                area_anchors.entry(*value).or_default().push(*cell);
            }
        }

        let n = self.grid.num_cells();

        for (&target, anchors) in &area_anchors {
            if anchors.len() <= 1 {
                continue;
            }

            // BFS from all cells in target-area components, traversing through
            // non-Cut edges, only entering no-target or same-target components.
            let mut visited = vec![false; n];
            let mut queue = VecDeque::new();

            for c in 0..n {
                if !self.grid.cell_exists[c] {
                    continue;
                }
                let ci = self.curr_comp_id[c];
                if ci == usize::MAX {
                    continue;
                }
                if self.curr_target_area[ci] == Some(target) {
                    visited[c] = true;
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
                    if !self.grid.cell_exists[other] || visited[other] {
                        continue;
                    }
                    let oci = self.curr_comp_id[other];
                    if oci == usize::MAX {
                        continue;
                    }
                    match self.curr_target_area[oci] {
                        Some(t) if t == target => {
                            visited[other] = true;
                            queue.push_back(other);
                        }
                        None => {
                            // No-target cell: can potentially join this group
                            visited[other] = true;
                            queue.push_back(other);
                        }
                        _ => {} // Different target: blocked
                    }
                }
            }

            // Check all anchors for this area value are reachable
            for &anchor in anchors {
                if !visited[anchor] {
                    return Err(());
                }
            }

            // If reachable set is smaller than target, force cuts on edges from
            // reachable set boundary to different-target components to prevent
            // the reachable set from shrinking further.
            // (Currently just checking anchor reachability; size budget check
            // would be imprecise due to shared no-target cells.)
        }
        Ok(false)
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
    fn flood_fill_decided_basic() {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        // Make top-left and top-center connected via Uncut
        let v_edge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(v_edge, EdgeState::Uncut);

        s.comp_buf.fill(usize::MAX);
        s.flood_fill_decided(s.grid.cell_id(0, 0));

        // Cell (0,0) and (0,1) should have same component id
        assert_eq!(
            s.comp_buf[s.grid.cell_id(0, 0)],
            s.comp_buf[s.grid.cell_id(0, 1)]
        );
        // Cell (1,0) should be in a different component
        assert_ne!(
            s.comp_buf[s.grid.cell_id(0, 0)],
            s.comp_buf[s.grid.cell_id(1, 0)]
        );
    }
}
