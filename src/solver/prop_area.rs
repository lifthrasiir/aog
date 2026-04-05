use super::Solver;
use crate::polyomino::{self, canonical};
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

impl Solver {
    /// Compute the maximum possible final size for a component.
    /// If the component has a target area, returns that.
    /// Otherwise, estimates by summing unique adjacent component sizes
    /// through remaining Unknown growth edges, capped at eff_max_area.
    fn growth_potential(&self, ci: usize) -> usize {
        self.curr_target_area[ci].unwrap_or_else(|| {
            let mut adj_sz: HashSet<usize> = HashSet::new();
            for &ge in &self.growth_edges[ci] {
                if self.edges[ge] != EdgeState::Unknown {
                    continue;
                }
                let (gc1, gc2) = self.grid.edge_cells(ge);
                let other_ci = if self.curr_comp_id[gc1] == ci {
                    self.curr_comp_id[gc2]
                } else {
                    self.curr_comp_id[gc1]
                };
                adj_sz.insert(self.curr_comp_sz[other_ci]);
            }
            (self.curr_comp_sz[ci] + adj_sz.iter().sum::<usize>()).min(self.eff_max_area)
        })
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
        let mut id_map = vec![usize::MAX; n];
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
        self.comp_cells = vec![Vec::new(); num_comp];
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
        for ci in 0..num_comp {
            let mut areas = Vec::new();
            for clue in &comp_clues[ci] {
                if let CellClue::Area { value, .. } = clue {
                    areas.push(*value);
                } else if let CellClue::Polyomino { shape, .. } = clue {
                    areas.push(shape.cells.len());
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
                self.curr_target_area[ci] = Some(a0);
                if self.curr_comp_sz[ci] > a0 {
                    return Err(());
                }
            } else if self.curr_comp_sz[ci] > self.eff_max_area {
                return Err(());
            }
        }

        // Check Unknown edges to outside
        self.can_grow_buf.clear();
        self.can_grow_buf.resize(num_comp, false);
        self.growth_edges = vec![Vec::new(); num_comp];
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
                } else {
                    if let (Some(a1), Some(a2)) =
                        (self.curr_target_area[ci1], self.curr_target_area[ci2])
                    {
                        a1 != a2
                    } else {
                        false
                    }
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

                let limit1 = self.curr_target_area[ci1].unwrap_or(self.eff_max_area);
                let limit2 = self.curr_target_area[ci2].unwrap_or(self.eff_max_area);

                if self.curr_comp_sz[ci1] >= limit1 || self.curr_comp_sz[ci2] >= limit2 {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                }
            }
        }

        // Apply same-area forced Uncuts (collected above to avoid stale component state)
        for &e in &same_area_forced_uncuts {
            if self.edges[e] == EdgeState::Unknown {
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
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
                    if self.rose_bits_all & (1 << sym) != 0
                        && rose_counts[sym as usize] > 1
                    {
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
                if self.edges[e] == EdgeState::Unknown {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                }
            }
        }
        Ok(num_comp)
    }

    /// Size separation, compass bounds, boxy/non-boxy,
    /// inequality and diff edge clue propagation.
    fn propagate_area_constraints(&mut self, num_comp: usize) -> Result<bool, ()> {
        let mut progress = false;
        // === Size separation: early propagation (Proposals A + B + D) ===
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

            // Step 2 (Proposal A): For each Unknown growth edge, check if merging
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

            // Step 3 (Proposal B): Forbidden size checks.
            // (a) If a growing component's target area is forbidden → contradiction.
            // (b) If a growing component's current size is forbidden and it has
            //     exactly 1 growth edge, force that edge Uncut (Proposal D).
            let mut forced_uncuts: Vec<EdgeId> = Vec::new();
            for ci in 0..num_comp {
                if self.can_grow_buf[ci] {
                    continue;
                }
                let forbidden = &sealed_neighbor_sizes[ci];
                if forbidden.is_empty() {
                    continue;
                }

                // (a) Target area is forbidden
                if let Some(t) = self.curr_target_area[ci] {
                    if forbidden.contains(&t) {
                        return Err(());
                    }
                }

                // Sealed component at forbidden size — already caught by the
                // sealed-vs-sealed check below, but let's be explicit:
                if forbidden.contains(&self.curr_comp_sz[ci]) {
                    return Err(());
                }
            }
            // (b) Growing components: check if current size is forbidden
            for ci in 0..num_comp {
                if !self.can_grow_buf[ci] {
                    continue; // sealed, already handled above
                }
                let forbidden = &sealed_neighbor_sizes[ci];
                if forbidden.is_empty() {
                    continue;
                }
                let cur_sz = self.curr_comp_sz[ci];

                // Target area is forbidden
                if let Some(t) = self.curr_target_area[ci] {
                    if forbidden.contains(&t) {
                        return Err(());
                    }
                }

                // Current size is forbidden → must grow.
                // If exactly 1 growth edge remains, force it Uncut.
                if forbidden.contains(&cur_sz) {
                    let mut unk_count = 0usize;
                    let mut last_unk = None;
                    for &e in &self.growth_edges[ci] {
                        if self.edges[e] == EdgeState::Unknown {
                            unk_count += 1;
                            last_unk = Some(e);
                        }
                    }
                    if unk_count == 0 {
                        return Err(()); // sealed at forbidden size
                    }
                    if unk_count == 1 {
                        forced_uncuts.push(last_unk.unwrap());
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
            if let Some(target) = self.curr_target_area[ci] {
                if self.curr_comp_sz[ci] < target && !self.can_grow_buf[ci] {
                    return Err(());
                }
                if self.curr_comp_sz[ci] == target && self.can_grow_buf[ci] {
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
            } else if self.eff_min_area > 1
                && self.curr_comp_sz[ci] < self.eff_min_area
                && !self.can_grow_buf[ci]
            {
                return Err(());
            }
        }

        // Compass bounds: prune and propagate based on compass clues
        let mut compass_forced_cuts: Vec<EdgeId> = Vec::new();

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

                if !self.can_grow_buf[ci] && counts[idx] < v {
                    return Err(());
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
                        let (ra, cola) = self.grid.cell_pos(*ca);
                        let (rb, colb) = self.grid.cell_pos(*cb);

                        if pa.n == Some(0) && rb < ra {
                            return Err(());
                        }
                        if pb.n == Some(0) && ra < rb {
                            return Err(());
                        }
                        if pa.s == Some(0) && rb > ra {
                            return Err(());
                        }
                        if pb.s == Some(0) && ra > rb {
                            return Err(());
                        }
                        if pa.e == Some(0) && colb > cola {
                            return Err(());
                        }
                        if pb.e == Some(0) && cola > colb {
                            return Err(());
                        }
                        if pa.w == Some(0) && colb < cola {
                            return Err(());
                        }
                        if pb.w == Some(0) && cola < colb {
                            return Err(());
                        }

                        if rb < ra {
                            if let (Some(vb), Some(va)) = (pb.n, pa.n) {
                                if vb >= va {
                                    return Err(());
                                }
                            }
                        } else if ra < rb {
                            if let (Some(va), Some(vb)) = (pa.n, pb.n) {
                                if va >= vb {
                                    return Err(());
                                }
                            }
                        } else if let (Some(va), Some(vb)) = (pa.n, pb.n) {
                            if va != vb {
                                return Err(());
                            }
                        }
                        if rb > ra {
                            if let (Some(vb), Some(va)) = (pb.s, pa.s) {
                                if vb >= va {
                                    return Err(());
                                }
                            }
                        } else if ra > rb {
                            if let (Some(va), Some(vb)) = (pa.s, pb.s) {
                                if va >= vb {
                                    return Err(());
                                }
                            }
                        } else if let (Some(va), Some(vb)) = (pa.s, pb.s) {
                            if va != vb {
                                return Err(());
                            }
                        }
                        if colb > cola {
                            if let (Some(vb), Some(va)) = (pb.e, pa.e) {
                                if vb >= va {
                                    return Err(());
                                }
                            }
                        } else if cola > colb {
                            if let (Some(va), Some(vb)) = (pa.e, pb.e) {
                                if va >= vb {
                                    return Err(());
                                }
                            }
                        } else if let (Some(va), Some(vb)) = (pa.e, pb.e) {
                            if va != vb {
                                return Err(());
                            }
                        }
                        if colb < cola {
                            if let (Some(vb), Some(va)) = (pb.w, pa.w) {
                                if vb >= va {
                                    return Err(());
                                }
                            }
                        } else if cola < colb {
                            if let (Some(va), Some(vb)) = (pa.w, pb.w) {
                                if va >= vb {
                                    return Err(());
                                }
                            }
                        } else if let (Some(va), Some(vb)) = (pa.w, pb.w) {
                            if va != vb {
                                return Err(());
                            }
                        }
                    }
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

        if self.puzzle.rules.non_boxy || self.puzzle.rules.boxy {
            // Single O(N) pass to compute bounding boxes for all components
            let (mut min_r, mut max_r, mut min_c, mut max_c) =
                (vec![self.grid.rows; num_comp], vec![0; num_comp], vec![self.grid.cols; num_comp], vec![0; num_comp]);
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
                    let max_possible = self.curr_target_area[ci].unwrap_or(self.eff_max_area);

                    if self.puzzle.rules.boxy && cell_count < bbox_size && bbox_size > max_possible {
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
                                    if nr < 0 || nr >= self.grid.rows as isize
                                        || nc < 0 || nc >= self.grid.cols as isize {
                                        continue;
                                    }
                                    let nid = self.grid.cell_id(nr as usize, nc as usize);
                                    if !self.grid.cell_exists[nid] {
                                        continue;
                                    }
                                    if self.curr_comp_id[nid] == ci {
                                        continue; // same component
                                    }
                                    // Check if this cell is the hole (inside bbox)
                                    let (hr, hc) = self.grid.cell_pos(nid);
                                    if hr >= min_r[ci] && hr <= max_r[ci]
                                        && hc >= min_c[ci] && hc <= max_c[ci] {
                                        let Some(e) = self.grid.edge_between(c, nid) else {
                                            continue;
                                        };
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

            // Apply non-boxy forced cuts
            for &e in &non_boxy_forced_cuts {
                if self.edges[e] == EdgeState::Unknown {
                    let p = self.set_edge(e, EdgeState::Cut);
                    if !p {
                        return Err(());
                    }
                    progress = true;
                }
            }
        }

        for clue in &self.puzzle.edge_clues {
            let EdgeClueKind::Inequality { smaller_first } = clue.kind else {
                continue;
            };
            let e = clue.edge;
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
                continue;
            }
            if larger_done && self.curr_comp_sz[larger_ci] <= self.curr_comp_sz[smaller_ci] {
                return Err(());
            }
            if smaller_done {
                // Component-wise max: use actual growth potential instead of global eff_max_area
                let max_larger = self.growth_potential(larger_ci);
                if self.curr_comp_sz[smaller_ci] >= max_larger {
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

        // Diff clues: when one side is sealed, propagate target area to the other side
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
            let min_area = self.eff_min_area.max(1);
            let max_area = self.eff_max_area;
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

        // Size separation: adjacent finished components must have different sizes
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

    /// Mingle, gemini, delta, mismatch shape-based constraints.
    fn propagate_shape_constraints(&mut self, num_comp: usize) -> Result<(), ()> {
        // Compute canonical shapes for sealed components (shared by mingle_shape, gemini & mismatch)
        let has_mingle = self.puzzle.rules.mingle_shape;
        let has_gemini = self
            .puzzle
            .edge_clues
            .iter()
            .any(|cl| matches!(cl.kind, EdgeClueKind::Gemini));
        let has_mismatch = self.puzzle.rules.mismatch;
        let has_delta = self
            .puzzle
            .edge_clues
            .iter()
            .any(|cl| matches!(cl.kind, EdgeClueKind::Delta));

        if has_mingle || has_gemini || has_mismatch || has_delta {
            let mut comp_shape: Vec<Option<Shape>> = vec![None; num_comp];
            for ci in 0..num_comp {
                if self.can_grow_buf[ci] {
                    continue;
                }
                let at_limit = match self.curr_target_area[ci] {
                    Some(t) => self.curr_comp_sz[ci] == t,
                    None => true,
                };
                if !at_limit {
                    continue;
                }
                let cells: Vec<(i32, i32)> = self.comp_cells[ci]
                    .iter()
                    .map(|&c| {
                        let (r, col) = self.grid.cell_pos(c);
                        (r as i32, col as i32)
                    })
                    .collect();
                comp_shape[ci] = Some(canonical(&polyomino::make_shape(&cells)));
            }

            // Mingle shape: adjacent pieces must have the same canonical shape.
            // When both sides are sealed, verify shapes match.
            // When one side is sealed, propagate size constraint to the other side.
            if has_mingle {
                let mut mingle_required_size: Vec<Option<usize>> = vec![None; num_comp];

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

                    // Both sides have known shapes: verify they match
                    match (&comp_shape[ci1], &comp_shape[ci2]) {
                        (Some(s1), Some(s2)) if s1 != s2 => return Err(()),
                        _ => {}
                    }

                    // One side sealed: propagate size constraint to the other
                    let sealed1 = !self.can_grow_buf[ci1];
                    let sealed2 = !self.can_grow_buf[ci2];
                    if sealed1 && sealed2 {
                        continue;
                    }
                    if !sealed1 && !sealed2 {
                        continue;
                    }

                    let (sealed_ci, other_ci) =
                        if sealed1 { (ci1, ci2) } else { (ci2, ci1) };
                    let sealed_sz = self.curr_comp_sz[sealed_ci];

                    // Check for conflicting mingle size requirements
                    if let Some(prev) = mingle_required_size[other_ci] {
                        if prev != sealed_sz {
                            return Err(());
                        }
                    }
                    mingle_required_size[other_ci] = Some(sealed_sz);

                    // If other side has a target area, it must match
                    if let Some(target) = self.curr_target_area[other_ci] {
                        if target != sealed_sz {
                            return Err(());
                        }
                        continue;
                    }

                    // No target on other side: check size compatibility.
                    // Only return Err if the component CANNOT reach sealed_sz.
                    let other_sz = self.curr_comp_sz[other_ci];
                    if other_sz > sealed_sz {
                        return Err(());
                    }
                }
            }

            // Gemini edge clues: adjacent pieces must have the same canonical shape.
            // When one side is sealed, propagate size constraints to the other side.
            // When both sides are sealed, verify canonical shapes match.
            if has_gemini {
                // Track required sizes from gemini constraints to detect conflicts
                // when a component is adjacent to multiple gemini edges.
                let mut gemini_required_size: Vec<Option<usize>> = vec![None; num_comp];

                for clue in &self.puzzle.edge_clues {
                    if !matches!(clue.kind, EdgeClueKind::Gemini) {
                        continue;
                    }
                    let e = clue.edge;
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

                    // Both sealed: check canonical shapes match
                    if sealed1 && sealed2 {
                        match (&comp_shape[ci1], &comp_shape[ci2]) {
                            (Some(s1), Some(s2)) if s1 != s2 => return Err(()),
                            _ => {}
                        }
                        continue;
                    }

                    // One side sealed: propagate size constraint to the other
                    let (sealed_ci, other_ci) =
                        if sealed1 { (ci1, ci2) } else { (ci2, ci1) };
                    let sealed_sz = self.curr_comp_sz[sealed_ci];

                    // Check for conflicting gemini size requirements
                    if let Some(prev) = gemini_required_size[other_ci] {
                        if prev != sealed_sz {
                            return Err(());
                        }
                    }
                    gemini_required_size[other_ci] = Some(sealed_sz);

                    // If other side has a target area from clues, it must match
                    if let Some(target) = self.curr_target_area[other_ci] {
                        if target != sealed_sz {
                            return Err(());
                        }
                        continue;
                    }

                    // No target on other side: check size compatibility.
                    // Only return Err if the component CANNOT reach sealed_sz.
                    // A component that has already exceeded sealed_sz can only
                    // grow further, so it's a genuine contradiction.
                    let other_sz = self.curr_comp_sz[other_ci];
                    if other_sz > sealed_sz {
                        return Err(());
                    }
                    // other_sz <= sealed_sz: no further action. We intentionally
                    // do NOT force Cut on growth edges here — during initial
                    // propagation, components may be sealed at sub-target sizes
                    // (e.g., monominoes from pre-cut edges), and forcing adjacent
                    // components to stay small would incorrectly cascade and
                    // prevent valid growth toward shape bank requirements.
                }
            }

            // Delta edge clues: adjacent pieces must have different canonical shapes.
            // When both sides are sealed, verify shapes differ.
            if has_delta {
                for clue in &self.puzzle.edge_clues {
                    if !matches!(clue.kind, EdgeClueKind::Delta) {
                        continue;
                    }
                    let e = clue.edge;
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

                    match (&comp_shape[ci1], &comp_shape[ci2]) {
                        (Some(s1), Some(s2)) if s1 == s2 => return Err(()),
                        _ => {}
                    }
                }
            }

            // Mismatch: all pieces must have distinct canonical shapes.
            // 1) Two sealed components sharing the same shape → contradiction.
            // 2) Growing component whose target area has no available shape left → contradiction.
            if has_mismatch {
                // Build set of canonical shapes used by sealed components
                let mut taken_shapes: HashSet<Shape> = HashSet::new();
                for ci in 0..num_comp {
                    if let Some(shape) = &comp_shape[ci] {
                        if !taken_shapes.insert(shape.clone()) {
                            return Err(()); // duplicate shape among sealed components
                        }
                    }
                }

                // Growing components: check if at least one shape of their target size is available
                for ci in 0..num_comp {
                    if !self.can_grow_buf[ci] {
                        continue; // already sealed, handled above
                    }
                    let Some(target) = self.curr_target_area[ci] else {
                        continue; // no fixed target area, skip
                    };

                    let mut any_available = false;

                    if !self.puzzle.rules.shape_bank.is_empty() {
                        // Shape bank: check canonical shapes of matching size in the bank
                        for bs in &self.puzzle.rules.shape_bank {
                            if bs.cells.len() != target {
                                continue;
                            }
                            let bc = canonical(bs);
                            if !taken_shapes.contains(&bc) {
                                any_available = true;
                                break;
                            }
                        }
                    } else {
                        // No shape bank: for small sizes, enumerate free polyominoes
                        if target <= 4 {
                            let all_shapes =
                                polyomino::enumerate_free_polyominoes(target);
                            any_available =
                                all_shapes.iter().any(|s| !taken_shapes.contains(s));
                        } else {
                            // Too many shapes to enumerate; skip this check
                            any_available = true;
                        }
                    }

                    if !any_available {
                        return Err(());
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn propagate_area_bounds(&mut self) -> Result<bool, ()> {
        let num_comp = self.build_components()?;
        let progress = self.propagate_area_constraints(num_comp)?;
        self.propagate_shape_constraints(num_comp)?;
        Ok(progress)
    }

    pub(crate) fn propagate_compass(&mut self) -> Result<bool, ()> {
        // Collect compass clues upfront to avoid borrow conflicts with set_edge
        let entries: Vec<(CellId, CompassData)> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| match cl {
                CellClue::Compass { cell, compass } => {
                    if self.grid.cell_exists[*cell] {
                        Some((*cell, compass.clone()))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        let mut progress = false;

        for (cell, compass) in &entries {
            let (r, c) = self.grid.cell_pos(*cell);

            for &(dr, dc, val) in &[
                (-1isize, 0, compass.n),
                (0, 1, compass.e),
                (1, 0, compass.s),
                (0, -1, compass.w),
            ] {
                let Some(v) = val else { continue };

                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0
                    || nr >= self.grid.rows as isize
                    || nc < 0
                    || nc >= self.grid.cols as isize
                {
                    continue; // v > 0 is fine: detour possible via other cells
                }

                let nid = self.grid.cell_id(nr as usize, nc as usize);
                if !self.grid.cell_exists[nid] {
                    continue; // v > 0 is fine: detour possible via other cells
                }

                let Some(edge) = self.grid.edge_between(*cell, nid) else {
                    continue;
                };

                if v == 0 {
                    // No cells in this direction: direct edge must be Cut
                    if self.edges[edge] == EdgeState::Unknown {
                        if !self.set_edge(edge, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    } else if self.edges[edge] != EdgeState::Cut {
                        return Err(());
                    }
                }
                // v > 0: cells can join via detour, so no edge constraint
            }
        }

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
        if sym[start] != 0xff {
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
                    && sym[other] != 0xff
                    && (exclude_rose_mask & (1 << sym[other])) != 0
                {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                if sym[other] != 0xff {
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
            if sym[c] != 0xff {
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
                    && sym[other] != 0xff
                    && (exclude_rose_mask & (1 << sym[other])) != 0
                {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                cells.push(other);
                if sym[other] != 0xff {
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
            if sym[c] != 0xff {
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
                    && sym[other] != 0xff
                    && (exclude_rose_mask & (1 << sym[other])) != 0
                {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                if sym[other] != 0xff {
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
            if sym[c] != 0xff {
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
                    && sym[other] != 0xff
                    && (exclude_rose_mask & (1 << sym[other])) != 0
                {
                    continue;
                }
                self.rose_visited[other] = true;
                self.q_buf.push(other);
                if sym[other] != 0xff {
                    types |= 1 << sym[other];
                }
            }
        }

        types
    }

    /// Rose window advanced propagation.
    ///
    /// Phase 1: Cross-type chokepoint Uncut forcing.
    ///   For each growing component missing rose symbols, BFS (excluding
    ///   same-type cells) finds reachable cells of each missing type.
    ///   If removing a growth edge makes a required type unreachable,
    ///   that edge must be Uncut.
    ///
    ///   Proof: The solution graph is a subgraph of G\{e}. If no S-cell
    ///   is reachable in G\{e} (avoiding same-type cells), then no S-cell
    ///   is reachable in the solution either (even fewer edges). Since the
    ///   component needs type S, e must be Uncut.
    ///
    /// Phase 2: Restricted reachability + single-growth-edge Uncut forcing.
    ///   For growing components missing rose symbols, check reachability while
    ///   excluding cells with already-owned symbols (tighter than unrestricted BFS).
    ///   If only one Unknown growth edge remains, force it Uncut.
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
                if self.cell_rose_sym[c] != 0xff {
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
                if sym != 0xff {
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
                if sym != 0xff {
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
            //
            // Proof: In the final solution, the piece containing this component must
            // include exactly one cell of each missing type. If component grows to
            // include S-cell X, then from X it must also reach all other missing types.
            // The solution subgraph is a subset of the current graph minus Cut edges,
            // so if no reachable S-cell can reach all other types now, none can in
            // the solution either → contradiction.
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
                        self.cell_rose_sym[..n].iter().filter(|&&s| s == sym).count()
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

        // --- Phase 3: All-types complete component rose blocking ---
        // If a component already has all 4 rose types, any growth edge leading
        // to a cell with ANY rose symbol must be Cut (would create duplicate type).
        //
        // Proof: Rose window rule requires exactly one of each type per piece.
        // Adding a cell with rose symbol S to a component that already has type S
        // would create two cells with type S → contradiction.
        for ci in 0..num_comp {
            if !self.can_grow_buf[ci] {
                continue;
            }

            let mut comp_rose: u8 = 0;
            for &c in &self.comp_cells[ci] {
                let sym = self.cell_rose_sym[c];
                if sym != 0xff {
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
                if self.cell_rose_sym[other] != 0xff {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;
    use crate::types::{CellClue, EdgeClue, EdgeClueKind};


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

    // === Gemini propagation tests ===
    // Uses plain grids (no 'g') and manually adds gemini clues for full control.

    /// Helper: create a 2x2 solver with gemini clue on v_edge(0,0) between (0,0) and (0,1).
    fn make_gemini_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let ge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(ge, EdgeState::Cut);
        s.puzzle.edge_clues.push(EdgeClue {
            edge: ge,
            kind: EdgeClueKind::Gemini,
        });
        s
    }

    /// Helper: create a 2x3 solver with gemini clues on specified v_edge columns.
    fn make_gemini_solver_2x3(cols: &[usize]) -> Solver {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        for &c in cols {
            let ge = s.grid.v_edge(0, c);
            let _ = s.set_edge(ge, EdgeState::Cut);
            s.puzzle.edge_clues.push(EdgeClue {
                edge: ge,
                kind: EdgeClueKind::Gemini,
            });
        }
        s
    }

    #[test]
    fn gemini_both_sealed_same_shape_ok() {
        // 2x2: gemini on v_edge(0,0) between (0,0) and (0,1).
        // Both sides are monominoes (sealed, same shape) → OK.
        let mut s = make_gemini_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn gemini_both_sealed_different_shape_err() {
        // 2x2: gemini on v_edge(0,0) between (0,0) and (0,1).
        // Left: domino (0,0)+(1,0), Right: monomino (0,1). Different shapes → Err.
        let mut s = make_gemini_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "gemini: domino vs monomino should be contradiction"
        );
    }

    #[test]
    fn gemini_one_sealed_size_exceeds_other_err() {
        // 2x2: gemini on v_edge(0,0). Left: sealed monomino. Right: growing domino (size 2).
        // sealed_sz=1, other_sz=2 > 1 → Err.
        let mut s = make_gemini_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "gemini: sealed monomino (1) vs growing domino (2) should be contradiction"
        );
    }

    #[test]
    fn gemini_one_sealed_size_conflicts_target_err() {
        // 2x2: gemini on v_edge(0,0). Left: sealed monomino (size 1).
        // Right-top has area=3 clue → target 3 ≠ 1 → Err.
        let mut s = make_gemini_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        let right_top = s.grid.cell_id(0, 1);
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: right_top,
            value: 3,
        });
        let nc = s.grid.num_cells();
        s.cell_clues_indexed = vec![vec![]; nc];
        for (i, clue) in s.puzzle.cell_clues.iter().enumerate() {
            s.cell_clues_indexed[clue.cell()].push(i);
        }

        assert!(
            s.propagate_area_bounds().is_err(),
            "gemini: sealed size 1 vs target area 3 should be contradiction"
        );
    }

    #[test]
    fn gemini_sealed_same_size_no_force_cut() {
        // 2x3: gemini on v_edge(0,0) between left-top and mid-top.
        // Left-top sealed at 1. Mid-top at size 1 with growth edges.
        // We intentionally do NOT force Cut (to avoid cascading issues
        // when sealed components are at sub-target sizes).
        let mut s = make_gemini_solver_2x3(&[0]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 2), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
        // The growth edge should remain Unknown (not forced Cut)
        assert_eq!(
            s.edges[s.grid.h_edge(0, 1)],
            EdgeState::Unknown,
            "gemini: should not force Cut on growth edges for size-matched growing component"
        );
    }

    #[test]
    fn gemini_conflicting_sizes_from_two_edges_err() {
        // 2x3: mid-top adjacent to two gemini edges with different sealed sizes.
        // Left sealed at 1, right sealed at 2, mid-top at 1 → conflicting.
        let mut s = make_gemini_solver_2x3(&[0, 1]);
        // Left: monomino (sealed at 1)
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // Right: domino (sealed at 2)
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Uncut);
        // Seal mid-top at 1: cut all its edges
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "gemini: conflicting size requirements (1 vs 2) should be contradiction"
        );
    }

    // === Mingle shape propagation tests ===

    /// Helper: create a 2x2 solver with mingle_shape rule and Cut on v_edge(0,0).
    fn make_mingle_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        s.puzzle.rules.mingle_shape = true;
        let ge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(ge, EdgeState::Cut);
        s
    }

    /// Helper: create a 2x3 solver with mingle_shape rule and Cuts on specified v_edge columns.
    fn make_mingle_solver_2x3(cols: &[usize]) -> Solver {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.mingle_shape = true;
        for &c in cols {
            let ge = s.grid.v_edge(0, c);
            let _ = s.set_edge(ge, EdgeState::Cut);
        }
        s
    }

    #[test]
    fn mingle_both_sealed_same_shape_ok() {
        // Both sides are monominoes (sealed, same shape) → OK
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn mingle_both_sealed_different_shape_err() {
        // Left: domino (0,0)+(1,0), Right: monomino (0,1). Different shapes → Err
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: domino vs monomino should be contradiction"
        );
    }

    #[test]
    fn mingle_one_sealed_size_exceeds_other_err() {
        // Left: sealed monomino. Right: growing domino (size 2).
        // sealed_sz=1, other_sz=2 > 1 → Err
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: sealed monomino (1) vs growing domino (2) should be contradiction"
        );
    }

    #[test]
    fn mingle_one_sealed_size_conflicts_target_err() {
        // Left: sealed monomino (size 1). Right-top has area=3 clue → target 3 ≠ 1 → Err
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        let right_top = s.grid.cell_id(0, 1);
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: right_top,
            value: 3,
        });
        let nc = s.grid.num_cells();
        s.cell_clues_indexed = vec![vec![]; nc];
        for (i, clue) in s.puzzle.cell_clues.iter().enumerate() {
            s.cell_clues_indexed[clue.cell()].push(i);
        }

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: sealed size 1 vs target area 3 should be contradiction"
        );
    }

    #[test]
    fn mingle_sealed_same_size_no_force_cut() {
        // Left-top sealed at 1. Mid-top at size 1 with growth edges.
        // Should NOT force Cut on growth edges (same as gemini behavior).
        let mut s = make_mingle_solver_2x3(&[0]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 2), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
        assert_eq!(
            s.edges[s.grid.h_edge(0, 1)],
            EdgeState::Unknown,
            "mingle: should not force Cut on growth edges for size-matched growing component"
        );
    }

    #[test]
    fn mingle_conflicting_sizes_from_two_edges_err() {
        // Mid-top adjacent to two sealed components with different sizes via mingle → Err
        let mut s = make_mingle_solver_2x3(&[0, 1]);
        // Left: monomino (sealed at 1)
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // Right: domino (sealed at 2)
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Uncut);
        // Seal mid-top at 1: cut all its edges
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: conflicting size requirements (1 vs 2) should be contradiction"
        );
    }

    // === Delta propagation tests ===

    /// Helper: create a 2x2 solver with delta clue on v_edge(0,0).
    fn make_delta_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let de = s.grid.v_edge(0, 0);
        let _ = s.set_edge(de, EdgeState::Cut);
        s.puzzle.edge_clues.push(EdgeClue {
            edge: de,
            kind: EdgeClueKind::Delta,
        });
        s
    }

    #[test]
    fn delta_both_sealed_different_shapes_ok() {
        // Left: domino (0,0)+(1,0), Right: monomino (0,1). Different shapes → OK
        let mut s = make_delta_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn delta_both_sealed_same_shape_err() {
        // Both sides are monominoes (sealed, same shape) → Err
        let mut s = make_delta_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "delta: same shape (monomino vs monomino) should be contradiction"
        );
    }

    // === Mismatch propagation tests ===

    /// Helper: create a 2x2 solver with mismatch rule enabled.
    fn make_mismatch_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        s.puzzle.rules.mismatch = true;
        s
    }

    #[test]
    fn mismatch_sealed_sealed_duplicate_shape_err() {
        // 2x2: two sealed monominoes with mismatch → contradiction.
        // (0,0) and (0,1) both sealed at size 1 → same canonical shape.
        let mut s = make_mismatch_solver_2x2();
        // Cut all 4 internal edges to seal everything as monominoes
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mismatch: two sealed monominoes should be contradiction"
        );
    }

    #[test]
    fn mismatch_sealed_sealed_different_shapes_ok() {
        // 2x2: one monomino and one triomino (L-shape) → different shapes, OK.
        let mut s = make_mismatch_solver_2x2();
        // (0,0) is a monomino: cut all its edges
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // Remaining 3 cells form an L-triomino: keep them connected
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Uncut);

        assert!(
            s.propagate_area_bounds().is_ok(),
            "mismatch: monomino + L-triomino should be valid"
        );
    }

    #[test]
    fn mismatch_growing_no_available_shape_shape_bank() {
        // 2x2 with shape bank containing only the monomino (size 1).
        // One monomino sealed → taken. Another component targeting size 1 → contradiction.
        use crate::polyomino::get_named_shape;
        let mut s = make_mismatch_solver_2x2();
        // Only allow monomino in shape bank
        s.puzzle.rules.shape_bank.push(get_named_shape("o").unwrap());
        // (0,0) sealed as monomino
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // (0,1) targets size 1 (area clue) but only monomino exists and it's taken
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: s.grid.cell_id(0, 1),
            value: 1,
        });
        // Cut edges to seal (0,1) as monomino
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mismatch: only one shape in bank of size 1, already taken → contradiction"
        );
    }

    #[test]
    fn mismatch_growing_available_shape_shape_bank() {
        // 2x2 with shape bank containing monomino and domino.
        // One monomino sealed → taken. Another targeting size 1 has no alternative → err,
        // but this test checks that a size-2 target is fine.
        use crate::polyomino::get_named_shape;
        let mut s = make_mismatch_solver_2x2();
        s.puzzle.rules.shape_bank.push(get_named_shape("o").unwrap());
        s.puzzle.rules.shape_bank.push(get_named_shape("oo").unwrap());
        // (0,0) sealed as monomino (takes the only size-1 shape)
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // Remaining 3 cells target size 2 — domino is available
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: s.grid.cell_id(0, 1),
            value: 2,
        });

        // Should not be a contradiction (domino is available)
        assert!(
            s.propagate_area_bounds().is_ok(),
            "mismatch: domino still available for size 2 target"
        );
    }

    #[test]
    fn mismatch_no_shape_bank_small_size_exhausted() {
        // 2x2 with no shape bank, mismatch enabled.
        // Two sealed monominoes → only 1 free polyomino of size 1, both taken → err.
        let mut s = make_mismatch_solver_2x2();
        // Seal (0,0) as monomino
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        // Seal (0,1) as monomino
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mismatch: two monominoes with no shape bank → only 1 shape of size 1 → err"
        );
    }
}
