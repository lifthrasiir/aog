use super::Solver;
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
        let mut same_area_forced_uncuts: Vec<EdgeId> = Vec::new();

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
                            same_area_forced_uncuts.push(e);
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
        for &e in &same_area_forced_uncuts {
            if self.edges[e] == EdgeState::Unknown && !self.set_edge(e, EdgeState::Uncut) {
                return Err(());
            }
        }

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
            for ci in 0..num_comp {
                if self.can_grow_buf[ci] {
                    continue;
                }
                let missing = self.rose_bits_all & !comp_rose[ci];
                if missing != 0 {
                    return Err(());
                }
            }

            // 3) Growth edge forcing: collect forced Cut/Uncut edges
            let mut rose_cut_set: HashSet<EdgeId> = HashSet::new();

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
                    rose_cut_set.insert(e);
                }
            }

            for &e in &rose_cut_set {
                if self.edges[e] == EdgeState::Unknown && !self.set_edge(e, EdgeState::Cut) {
                    return Err(());
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
                if !self.can_grow_buf[ci1] {
                    sealed_neighbor_sizes[ci2].insert(self.curr_comp_sz[ci1]);
                } else if let Some(t) = self.curr_target_area[ci1] {
                    // Growing with target: final size will be t
                    sealed_neighbor_sizes[ci2].insert(t);
                }
                if !self.can_grow_buf[ci2] {
                    sealed_neighbor_sizes[ci1].insert(self.curr_comp_sz[ci2]);
                } else if let Some(t) = self.curr_target_area[ci2] {
                    sealed_neighbor_sizes[ci1].insert(t);
                }
            }

            // Step 2: For each Unknown growth edge, check if merging
            // the two adjacent components would create a size that conflicts with
            // any sealed neighbor of either component. If so, force Cut.
            let mut merge_conflict_cuts: Vec<EdgeId> = Vec::new();
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
                    merge_conflict_cuts.push(e);
                }
            }
            for &e in &merge_conflict_cuts {
                if self.edges[e] == EdgeState::Unknown {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    progress = true;
                }
            }

            // Step 3: Forbidden size checks.
            // (a) If a growing component's target area is forbidden → contradiction.
            // (b) If a growing component's current size is forbidden and it has
            //     exactly 1 growth edge, force that edge Uncut.
            let mut forced_uncuts: Vec<EdgeId> = Vec::new();
            for ci in 0..num_comp {
                let forbidden = &sealed_neighbor_sizes[ci];
                if forbidden.is_empty() {
                    continue;
                }

                if !self.can_grow_buf[ci] {
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
                            forced_uncuts.push(last_unk.unwrap());
                        }
                    }
                }
            }
            for &e in &forced_uncuts {
                if self.edges[e] == EdgeState::Unknown {
                    if !self.set_edge(e, EdgeState::Uncut) {
                        return Err(());
                    }
                    progress = true;
                }
            }

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
                    }
                }
            }
        }
        count
    }

    fn propagate_compass_in_components(&mut self, num_comp: usize) -> Result<bool, ()> {
        // Compass bounds: prune and propagate based on compass clues
        let mut progress = false;
        let mut compass_forced_cuts: Vec<EdgeId> = Vec::new();
        let mut compass_forced_uncuts: Vec<EdgeId> = Vec::new();

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
                            compass_forced_cuts.push(e);
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
                    if self.can_grow_buf[ci] {
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
                                compass_forced_uncuts.push(dir_last_edge.unwrap());
                            }
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
            }
        }

        for &e in &compass_forced_cuts {
            if self.edges[e] == EdgeState::Unknown {
                if !self.set_edge(e, EdgeState::Cut) {
                    return Err(());
                }
                progress = true;
            }
        }
        for &e in &compass_forced_uncuts {
            if self.edges[e] == EdgeState::Unknown {
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
            }
        }
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
            let mut non_boxy_forced_cuts: Vec<EdgeId> = Vec::new();

            for ci in 0..num_comp {
                let cell_count = self.curr_comp_sz[ci];
                if cell_count == 0 {
                    continue;
                }
                let bbox_w = max_r[ci] - min_r[ci] + 1;
                let bbox_h = max_c[ci] - min_c[ci] + 1;
                let bbox_size = bbox_w * bbox_h;
                let is_rect = cell_count == bbox_size;

                if !self.can_grow_buf[ci] {
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
                                                non_boxy_forced_cuts.push(e);
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
            for &e in &non_boxy_forced_cuts {
                if self.edges[e] == EdgeState::Unknown {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    progress = true;
                }
            }
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
            let smaller_done = !self.can_grow_buf[smaller_ci];
            let larger_done = !self.can_grow_buf[larger_ci];

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
        let mut diff_forced_cuts: Vec<EdgeId> = Vec::new();
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
            let sealed1 = !self.can_grow_buf[ci1];
            let sealed2 = !self.can_grow_buf[ci2];
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
                            diff_forced_cuts.push(ge);
                        }
                    }
                }
            }
        }
        for &ge in &diff_forced_cuts {
            if self.edges[ge] == EdgeState::Unknown {
                if !self.set_edge(ge, EdgeState::Cut) {
                    return Err(());
                }
                progress = true;
            }
        }
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
                if self.curr_comp_sz[ci] < t && !self.can_grow_buf[ci] {
                    return Err(());
                }
                if self.curr_comp_sz[ci] == t && self.can_grow_buf[ci] {
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
                if self.curr_comp_sz[ci] < min_a && !self.can_grow_buf[ci] {
                    return Err(());
                }
                if self.curr_comp_sz[ci] == max_a && self.can_grow_buf[ci] {
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
                if self.can_grow_buf[ci1] || self.can_grow_buf[ci2] {
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

        // Pre-cut edge straddle check: if both cells of a pre-cut edge
        // are in the same component (connected via an alternative path
        // around the Cut edge), that piece would contain a pre-cut edge → invalid.
        for e in 0..self.grid.num_edges() {
            if !self.is_pre_cut[e] {
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

        let progress = self.propagate_area_constraints(num_comp)?;
        self.propagate_shape_constraints(num_comp)?;
        Ok(progress)
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

        let n = compass.n.or_else(|| if r == 0 { Some(0) } else { None });
        let s = compass.s.or_else(|| {
            if r == self.grid.rows - 1 {
                Some(0)
            } else {
                None
            }
        });
        let e = compass.e.or_else(|| {
            if c == self.grid.cols - 1 {
                Some(0)
            } else {
                None
            }
        });
        let w = compass.w.or_else(|| if c == 0 { Some(0) } else { None });

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
