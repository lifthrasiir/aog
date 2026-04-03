use super::Solver;
use crate::polyomino::{self, canonical, Rotation};
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

impl Solver {
    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            if self.puzzle.rules.bricky || self.puzzle.rules.loopy {
                progress |= self.propagate_bricky_loopy()?;
            }
            progress |= self.propagate_area_bounds()?;
            progress |= self.propagate_same_area_reachability()?;
            progress |= self.propagate_palisade_constraints()?;
            progress |= self.propagate_compass()?;
            progress |= self.propagate_watchtower()?;

            if !progress {
                return Ok(true);
            }
        }
    }

    pub(crate) fn propagate_bricky_loopy(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        for i in 1..self.grid.rows {
            for j in 1..self.grid.cols {
                let mut cut_count = 0usize;
                let mut unk_edges = Vec::new();
                for eid in self.grid.vertex_edges(i, j).into_iter().flatten() {
                    match self.edges[eid] {
                        EdgeState::Cut => cut_count += 1,
                        EdgeState::Unknown => unk_edges.push(eid),
                        _ => {}
                    }
                }
                let max_cut = if self.puzzle.rules.loopy { 2 } else { 3 };

                if cut_count > max_cut {
                    return Err(());
                }
                if cut_count + unk_edges.len() > max_cut {
                    let must_uncut = cut_count + unk_edges.len() - max_cut;
                    for &eid in &unk_edges[..must_uncut] {
                        if !self.set_edge(eid, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            }
        }
        Ok(progress)
    }

    pub(crate) fn propagate_area_bounds(&mut self) -> Result<bool, ()> {
        let mut progress = false;
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
        let mut comp_cells: Vec<Vec<CellId>> = vec![Vec::new(); num_comp];
        let mut comp_clues = vec![Vec::new(); num_comp]; // This one is still a bit heavy, but clues are few
        for c in 0..n {
            if self.grid.cell_exists[c] {
                let ci = self.curr_comp_id[c];
                self.curr_comp_sz[ci] += 1;
                comp_cells[ci].push(c);
                for &clue_idx in &self.cell_clues_indexed[c] {
                    comp_clues[ci].push(&self.puzzle.cell_clues[clue_idx]);
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
        let mut growth_edges = vec![Vec::new(); num_comp];
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
                    progress = true;
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
                growth_edges[ci1].push(e);
                growth_edges[ci2].push(e);

                let limit1 = self.curr_target_area[ci1].unwrap_or(self.eff_max_area);
                let limit2 = self.curr_target_area[ci2].unwrap_or(self.eff_max_area);

                if self.curr_comp_sz[ci1] >= limit1 || self.curr_comp_sz[ci2] >= limit2 {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    progress = true;
                }
            }
        }

        // Apply same-area forced Uncuts (collected above to avoid stale component state)
        for &e in &same_area_forced_uncuts {
            if self.edges[e] == EdgeState::Unknown {
                if !self.set_edge(e, EdgeState::Uncut) {
                    return Err(());
                }
                progress = true;
            }
        }

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
                    for &e in &growth_edges[ci] {
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
                    for &e in &growth_edges[ci] {
                        if self.edges[e] == EdgeState::Unknown {
                            if !self.set_edge(e, EdgeState::Cut) {
                                return Err(());
                            }
                            progress = true;
                        }
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
            for &c in &comp_cells[ci] {
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
                    for &e in &growth_edges[ci] {
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
                for &c in &comp_cells[ci] {
                    let (r, col) = self.grid.cell_pos(c);
                    min_r[ci] = min_r[ci].min(r);
                    max_r[ci] = max_r[ci].max(r);
                    min_c[ci] = min_c[ci].min(col);
                    max_c[ci] = max_c[ci].max(col);
                }
            }
            for ci in 0..num_comp {
                if self.can_grow_buf[ci] {
                    continue;
                }
                let cell_count = self.curr_comp_sz[ci];
                if cell_count == 0 {
                    continue;
                }
                let is_rect = cell_count == (max_r[ci] - min_r[ci] + 1) * (max_c[ci] - min_c[ci] + 1);
                if self.puzzle.rules.non_boxy && is_rect {
                    return Err(());
                }
                if self.puzzle.rules.boxy && !is_rect {
                    return Err(());
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
                let max_larger = self.curr_target_area[larger_ci].unwrap_or_else(|| {
                    // Sum unique adjacent component sizes through remaining Unknown edges
                    let mut adj_sz: HashSet<usize> = HashSet::new();
                    for &ge in &growth_edges[larger_ci] {
                        if self.edges[ge] != EdgeState::Unknown {
                            continue;
                        }
                        let (gc1, gc2) = self.grid.edge_cells(ge);
                        let other_ci =
                            if self.curr_comp_id[gc1] == larger_ci { self.curr_comp_id[gc2] } else { self.curr_comp_id[gc1] };
                        adj_sz.insert(self.curr_comp_sz[other_ci]);
                    }
                    (self.curr_comp_sz[larger_ci] + adj_sz.iter().sum::<usize>()).min(self.eff_max_area)
                });
                if self.curr_comp_sz[smaller_ci] >= max_larger {
                    return Err(());
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
                    for &ge in &growth_edges[other_ci] {
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

        // Compute canonical shapes for sealed components (shared by mingle_shape, gemini & mismatch)
        let has_mingle = self.puzzle.rules.mingle_shape;
        let has_gemini = self
            .puzzle
            .edge_clues
            .iter()
            .any(|cl| matches!(cl.kind, EdgeClueKind::Gemini));
        let has_mismatch = self.puzzle.rules.mismatch;

        if has_mingle || has_gemini || has_mismatch {
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
                let cells: Vec<(i32, i32)> = comp_cells[ci]
                    .iter()
                    .map(|&c| {
                        let (r, col) = self.grid.cell_pos(c);
                        (r as i32, col as i32)
                    })
                    .collect();
                comp_shape[ci] = Some(canonical(&polyomino::make_shape(&cells)));
            }

            // Mingle shape: adjacent finished components must have the same shape
            if has_mingle {
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
                    match (&comp_shape[ci1], &comp_shape[ci2]) {
                        (Some(s1), Some(s2)) if s1 != s2 => return Err(()),
                        _ => {}
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

    pub(crate) fn propagate_palisade(&mut self) {
        let mut to_set: Vec<(EdgeId, EdgeState)> = Vec::new();
        for clue in &self.puzzle.cell_clues {
            let CellClue::Palisade { cell, kind } = clue else {
                continue;
            };
            if !self.grid.cell_exists[*cell] {
                continue;
            }
            let num_cut = kind.cut_count();
            if num_cut == 0 || num_cut == 4 {
                let state = if num_cut == 0 {
                    EdgeState::Uncut
                } else {
                    EdgeState::Cut
                };
                for eid in self.grid.cell_edges(*cell).into_iter().flatten() {
                    if self.edges[eid] == EdgeState::Unknown {
                        to_set.push((eid, state));
                    }
                }
            }
        }
        for (eid, state) in to_set {
            let _ = self.set_edge(eid, state);
        }
    }

    /// Full palisade propagation: enumerate compatible rotations and force edges
    /// where all compatible rotations agree on the state.
    pub(crate) fn propagate_palisade_constraints(&mut self) -> Result<bool, ()> {
        // First pass: collect all deductions
        let mut all_forced: Vec<(EdgeId, EdgeState)> = Vec::new();
        let mut contradiction = false;

        for clue in &self.puzzle.cell_clues {
            let CellClue::Palisade { cell, kind } = clue else {
                continue;
            };
            if !self.grid.cell_exists[*cell] {
                continue;
            }

            let edges: [Option<EdgeId>; 4] = self.grid.cell_edges(*cell);
            let states: [EdgeState; 4] =
                edges.map(|e| e.map(|eid| self.edges[eid]).unwrap_or(EdgeState::Cut));

            let mut known_cuts = 0u8;
            let mut known_uncuts = 0u8;
            let mut known_cut_mask = 0u8;
            for k in 0..4 {
                match states[k] {
                    EdgeState::Cut => {
                        known_cuts += 1;
                        known_cut_mask |= 1 << k;
                    }
                    EdgeState::Uncut => {
                        known_uncuts += 1;
                    }
                    EdgeState::Unknown => {}
                }
            }

            let mut can_be_cut = [false; 4];
            let mut can_be_uncut = [false; 4];
            let mut any_compatible = false;

            for rot in Rotation::all() {
                let (ec, em) = kind.pattern_at_rotation(rot.index());

                let unknown_count = 4 - known_cuts - known_uncuts;
                if (known_cuts as usize) > ec {
                    continue;
                }
                if (known_cuts as usize) + (unknown_count as usize) < ec {
                    continue;
                }
                if (known_cut_mask & em) != known_cut_mask {
                    continue;
                }

                let known_uncut_mask: u8 = (0..4u8)
                    .filter(|&k| states[k as usize] == EdgeState::Uncut)
                    .fold(0, |m, k| m | (1 << k));
                if (known_uncut_mask & em) != 0 {
                    continue;
                }

                any_compatible = true;

                for k in 0..4 {
                    if (em >> k) & 1 == 1 {
                        can_be_cut[k] = true;
                    } else {
                        can_be_uncut[k] = true;
                    }
                }
            }

            if !any_compatible {
                contradiction = true;
                break;
            }

            for k in 0..4 {
                if states[k] != EdgeState::Unknown {
                    continue;
                }
                let eid = match edges[k] {
                    Some(e) => e,
                    None => continue,
                };
                if can_be_cut[k] && !can_be_uncut[k] {
                    all_forced.push((eid, EdgeState::Cut));
                } else if !can_be_cut[k] && can_be_uncut[k] {
                    all_forced.push((eid, EdgeState::Uncut));
                }
            }
        }

        if contradiction {
            return Err(());
        }

        // Second pass: apply deductions
        let mut progress = false;
        for (eid, state) in all_forced {
            if !self.set_edge(eid, state) {
                return Err(());
            }
            progress = true;
        }

        Ok(progress)
    }

    /// Propagate watchtower (vertex) clues.
    ///
    /// For an interior vertex with 4 surrounding cells forming a 2×2 block,
    /// the 4 internal edges between those cells determine connectivity:
    ///   k internal cut edges → max(1, k) distinct pieces at the vertex.
    /// So value=v means exactly v of those 4 internal edges must be Cut.
    pub(crate) fn propagate_watchtower(&mut self) -> Result<bool, ()> {
        let mut progress = false;
        // Collect constraints upfront to avoid borrow conflicts
        let constraints: Vec<(usize, Vec<EdgeId>)> = self
            .puzzle
            .vertex_clues
            .iter()
            .filter_map(|clue| {
                let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                if vi == 0 || vj == 0 {
                    return None; // border vertex
                }
                let tl = self.grid.cell_id(vi - 1, vj - 1);
                let tr = self.grid.cell_id(vi - 1, vj);
                let bl = self.grid.cell_id(vi, vj - 1);
                let br = self.grid.cell_id(vi, vj);
                if !self.grid.cell_exists[tl] || !self.grid.cell_exists[tr]
                    || !self.grid.cell_exists[bl] || !self.grid.cell_exists[br]
                {
                    return None;
                }
                // 4 internal edges of the 2×2 cell group
                Some((
                    clue.value,
                    vec![
                        self.grid.v_edge(vi - 1, vj - 1), // TL-TR
                        self.grid.h_edge(vi - 1, vj - 1), // TL-BL
                        self.grid.h_edge(vi - 1, vj),     // TR-BR
                        self.grid.v_edge(vi, vj - 1),     // BL-BR
                    ],
                ))
            })
            .collect();

        for (value, edge_ids) in constraints {
            let mut n_cut = 0usize;
            let mut unk = Vec::new();
            for &eid in &edge_ids {
                match self.edges[eid] {
                    EdgeState::Cut => n_cut += 1,
                    EdgeState::Unknown => unk.push(eid),
                    EdgeState::Uncut => {}
                }
            }

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
            } else {
                let needed = (value as usize).saturating_sub(n_cut);
                if needed > unk.len() {
                    return Err(());
                }
                if needed == 0 && !unk.is_empty() {
                    for eid in unk {
                        if !self.set_edge(eid, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                } else if needed == unk.len() && !unk.is_empty() {
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

    /// When same_area_groups is true, check that all anchors of each area value
    /// are still potentially reachable from each other through available cells.
    /// "Available" = not Cut-edge-separated from the group's components.
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
    use crate::types::{CellClue, PalisadeKind};

    #[test]
    fn propagate_palisade_none_forces_all_uncut() {
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
        // Add a palisade p0 clue at center cell (1,1)
        let center = s.grid.cell_id(1, 1);
        s.puzzle.cell_clues.push(CellClue::Palisade {
            cell: center,
            kind: PalisadeKind::None,
        });

        s.propagate_palisade();

        // All 4 edges around center should be Uncut
        for eid in s.grid.cell_edges(center).into_iter().flatten() {
            assert_eq!(
                s.edges[eid],
                EdgeState::Uncut,
                "palisade p0: edge {eid} should be Uncut"
            );
        }
    }

    #[test]
    fn propagate_palisade_all_forces_all_cut() {
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
        let center = s.grid.cell_id(1, 1);
        s.puzzle.cell_clues.push(CellClue::Palisade {
            cell: center,
            kind: PalisadeKind::All,
        });

        s.propagate_palisade();

        for eid in s.grid.cell_edges(center).into_iter().flatten() {
            assert_eq!(
                s.edges[eid],
                EdgeState::Cut,
                "palisade p4: edge {eid} should be Cut"
            );
        }
    }

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
