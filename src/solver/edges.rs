use super::Solver;
use crate::types::*;
use std::collections::HashSet;

/// Cached data used exclusively by the edge selection heuristic.
/// Populated by area propagation, consumed by `select_edge` / `prefer_cut_first`.
pub(crate) struct EdgeSelectionCache {
    /// All edge IDs with any clue (inequality, diff, gemini, delta) — always Cut.
    pub(crate) clue_cut_edges: Vec<EdgeId>,
    /// Sealed neighbor sizes per component, for size_separation heuristic.
    pub(crate) sealed_neighbor_sizes: Option<Vec<HashSet<usize>>>,
    /// Growth edge count per component.
    pub(crate) growth_edge_count: Vec<usize>,
    /// Vertices with watchtower clues (rebuilt after initial optimization pass).
    pub(crate) watchtower_vertices: HashSet<VertexId>,
}

impl EdgeSelectionCache {
    pub(crate) fn new(clue_cut_edges: Vec<EdgeId>, watchtower_vertices: HashSet<VertexId>) -> Self {
        Self {
            clue_cut_edges,
            sealed_neighbor_sizes: None,
            growth_edge_count: Vec::new(),
            watchtower_vertices,
        }
    }
}

impl Solver {
    fn select_edge(&self) -> Option<(EdgeId, i32)> {
        let num_edges = self.grid.num_edges();
        if self.curr_comp_id.is_empty() {
            for e in 0..num_edges {
                if self.edges[e] == EdgeState::Unknown {
                    return Some((e, 0));
                }
            }
            return None;
        }

        let num_comp = self.curr_comp_sz.len();

        // Precompute clue-constrained components: components adjacent to any
        // clue edge (inequality, diff, gemini, delta). These components have
        // shape or size constraints from edge clues.
        let has_clue_edges = !self.edge_selection.clue_cut_edges.is_empty();
        let mut clue_constrained_comp: Vec<bool> = if has_clue_edges {
            vec![false; num_comp]
        } else {
            Vec::new()
        };
        if has_clue_edges {
            for &ce in &self.edge_selection.clue_cut_edges {
                if self.edges[ce] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(ce);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let ci1 = self.curr_comp_id[c1];
                let ci2 = self.curr_comp_id[c2];
                clue_constrained_comp[ci1] = true;
                clue_constrained_comp[ci2] = true;
            }
        }

        // Precompute compass info per component:
        // - comp_has_compass: component has at least one compass clue
        // - comp_compass_count: number of compass clues in component
        // - comp_dir_at_limit: component has a compass direction at its value limit
        let (_comp_has_compass, comp_compass_count, comp_dir_at_limit) =
            if self.has_compass_clue && !self.curr_comp_id.is_empty() {
                let mut has_compass = vec![false; num_comp];
                let mut compass_count = vec![0u8; num_comp];
                let mut dir_at_limit = vec![false; num_comp];
                for cl in &self.puzzle.cell_clues {
                    if let CellClue::Compass { cell, compass } = cl {
                        if !self.grid.cell_exists[*cell] {
                            continue;
                        }
                        let ci = self.curr_comp_id[*cell];
                        if ci == usize::MAX || ci >= num_comp {
                            continue;
                        }
                        has_compass[ci] = true;
                        compass_count[ci] += 1;
                        // Check if any direction is at its compass limit
                        let (cr, cc) = self.grid.cell_pos(*cell);
                        let (cr_i, cc_i) = (cr as isize, cc as isize);
                        let counts: [usize; 4] = [
                            self.comp_cells[ci]
                                .iter()
                                .filter(|&&c| (self.grid.cell_pos(c).0 as isize) < cr_i)
                                .count(),
                            self.comp_cells[ci]
                                .iter()
                                .filter(|&&c| (self.grid.cell_pos(c).0 as isize) > cr_i)
                                .count(),
                            self.comp_cells[ci]
                                .iter()
                                .filter(|&&c| (self.grid.cell_pos(c).1 as isize) > cc_i)
                                .count(),
                            self.comp_cells[ci]
                                .iter()
                                .filter(|&&c| (self.grid.cell_pos(c).1 as isize) < cc_i)
                                .count(),
                        ];
                        for &(val, cnt) in &[
                            (compass.n, counts[0]),
                            (compass.s, counts[1]),
                            (compass.e, counts[2]),
                            (compass.w, counts[3]),
                        ] {
                            if let Some(v) = val {
                                if cnt == v {
                                    dir_at_limit[ci] = true;
                                }
                            }
                        }
                    }
                }
                (has_compass, compass_count, dir_at_limit)
            } else {
                (vec![], vec![], vec![])
            };

        // Precompute compass-adjacent cells for edge bonus
        let compass_adjacent: Vec<bool> = if self.has_compass_clue {
            let mut adj = vec![false; self.grid.num_cells()];
            for cl in &self.puzzle.cell_clues {
                if let CellClue::Compass { cell, .. } = cl {
                    if self.grid.cell_exists[*cell] {
                        adj[*cell] = true;
                    }
                }
            }
            adj
        } else {
            Vec::new()
        };

        let mut best_e = None;
        let mut best_score = i32::MIN;

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }

            let mut score = 0i32;
            // Cell 1
            let ci1 = self.curr_comp_id[c1];
            let sz1 = self.curr_comp_sz[ci1];
            if let Some(target) = self.curr_target_area[ci1] {
                score += if sz1 < target { 100 } else { 1 };
            } else {
                score += 10;
            }
            // Cell 2
            let ci2 = self.curr_comp_id[c2];
            let sz2 = self.curr_comp_sz[ci2];
            if let Some(target) = self.curr_target_area[ci2] {
                score += if sz2 < target { 100 } else { 1 };
            } else {
                score += 10;
            }

            let sealed1 = self.is_sealed(ci1);
            let sealed2 = self.is_sealed(ci2);

            // General bonuses (apply regardless of size_separation)

            // Bonus: cutting would seal a component that has a target area
            if ci1 < self.edge_selection.growth_edge_count.len()
                && self.edge_selection.growth_edge_count[ci1] == 1
                && self.curr_target_area[ci1].is_some()
            {
                score += 75;
            }
            if ci2 < self.edge_selection.growth_edge_count.len()
                && self.edge_selection.growth_edge_count[ci2] == 1
                && self.curr_target_area[ci2].is_some()
            {
                score += 75;
            }

            // Bonus: edge between sealed and target component
            if sealed1 ^ sealed2 {
                let (other_ci, _) = if sealed1 { (ci2, ci1) } else { (ci1, ci2) };
                if self.curr_target_area[other_ci].is_some() {
                    score += 50;
                }
            }

            // Bonus: edge adjacent to a clue-constrained component
            // (component touching a gemini/delta/inequality/diff edge)
            if has_clue_edges && (clue_constrained_comp[ci1] || clue_constrained_comp[ci2]) {
                score += 30;
            }

            // Bonus: edge incident to a watchtower vertex
            if !self.edge_selection.watchtower_vertices.is_empty() {
                let (is_h, r, c) = self.grid.decode_edge(e);
                let v1 = self.grid.vertex(r, c);
                let v2 = if is_h {
                    self.grid.vertex(r + 1, c)
                } else {
                    self.grid.vertex(r, c + 1)
                };
                if self.edge_selection.watchtower_vertices.contains(&v1)
                    || self.edge_selection.watchtower_vertices.contains(&v2)
                {
                    score += 25;
                }
            }

            // Slitherlink cut-path endpoint bonus: for loopy+watchtower puzzles,
            // edges at cut-path endpoints (vertices with exactly 1 cut edge) are
            // the most constrained — they must extend the path. Resolving these
            // early prevents deep wrong branches in Slitherlink-like puzzles.
            if self.puzzle.rules.loopy && !self.edge_selection.watchtower_vertices.is_empty() {
                let (is_h, r, c) = self.grid.decode_edge(e);
                let (v1i, v1j) = (r, c);
                let (v2i, v2j) = if is_h { (r + 1, c) } else { (r, c + 1) };
                for &(vi, vj) in &[(v1i, v1j), (v2i, v2j)] {
                    let cut_deg: usize = self
                        .grid
                        .vertex_edges(vi, vj)
                        .into_iter()
                        .flatten()
                        .filter(|eid| self.edges[*eid] == EdgeState::Cut)
                        .count();
                    if cut_deg == 1 {
                        score += 45; // path endpoint: high priority
                        break;
                    }
                }
            }

            // Rose cell proximity bonus: prefer edges near rose cells.
            // The boundary between pieces must separate rose cells of the same type,
            // so edges near them are more likely to be on the boundary.
            if self.rose_bits_all != 0
                && (self.cell_rose_sym[c1] != u8::MAX || self.cell_rose_sym[c2] != u8::MAX)
            {
                score += 80;
            }
            // Also bonus edges whose cells are in different rose-containing components
            if self.rose_bits_all != 0 && !self.curr_comp_id.is_empty() {
                let ci1_sym = self.cell_rose_sym[c1] != u8::MAX;
                let ci2_sym = self.cell_rose_sym[c2] != u8::MAX;
                // Edge between a rose cell and a non-rose cell is a strong boundary candidate
                if ci1_sym ^ ci2_sym {
                    score += 40;
                }
                // Edge between two cells with same-type rose symbols → must be DIFF (Cut)
                if ci1_sym && ci2_sym && self.cell_rose_sym[c1] == self.cell_rose_sym[c2] {
                    score += 200; // very high: this edge MUST be Cut
                }
            }

            // Compass-aware edge selection bonuses
            if self.has_compass_clue {
                // Bonus: edge adjacent to a compass cell
                if !compass_adjacent.is_empty() && (compass_adjacent[c1] || compass_adjacent[c2]) {
                    score += 40;
                }

                // Bonus: growth edge of component with compass direction at limit
                if !comp_dir_at_limit.is_empty() {
                    if ci1 < comp_dir_at_limit.len() && comp_dir_at_limit[ci1] {
                        score += 60;
                    }
                    if ci2 < comp_dir_at_limit.len() && comp_dir_at_limit[ci2] {
                        score += 60;
                    }
                }

                // Bonus: component with multiple compass clues
                if !comp_compass_count.is_empty() {
                    if ci1 < comp_compass_count.len() && comp_compass_count[ci1] >= 2 {
                        score += 30;
                    }
                    if ci2 < comp_compass_count.len() && comp_compass_count[ci2] >= 2 {
                        score += 30;
                    }
                }
            }

            // Size separation heuristic bonuses
            if self.puzzle.rules.size_separation {
                // Bonus: Uncut would create merge-size conflict with sealed neighbor
                if let Some(ref sns) = self.edge_selection.sealed_neighbor_sizes {
                    if ci1 < sns.len() && ci2 < sns.len() {
                        let merged_sz = sz1 + sz2;
                        if sns[ci1].contains(&merged_sz) || sns[ci2].contains(&merged_sz) {
                            score += 200; // immediate contradiction if Uncut
                        }
                    }
                }

                // Bonus: edge between same-size no-target components
                // (size separation requires them to differ)
                if sz1 == sz2
                    && self.curr_target_area[ci1].is_none()
                    && self.curr_target_area[ci2].is_none()
                {
                    score += 80;
                }

                // Additional sealing bonus: cutting would seal any component
                // (even without target, sealing helps size_separation propagation)
                if ci1 < self.edge_selection.growth_edge_count.len()
                    && self.edge_selection.growth_edge_count[ci1] == 1
                {
                    score += 30;
                }
                if ci2 < self.edge_selection.growth_edge_count.len()
                    && self.edge_selection.growth_edge_count[ci2] == 1
                {
                    score += 30;
                }
            }

            if score > best_score {
                best_score = score;
                best_e = Some((e, score));
                if score >= 200 {
                    break;
                }
            }
        }

        best_e
    }

    /// For the selected edge, determine whether to try Cut or Uncut first.
    /// Returns true if Cut should be tried first (default), false if Uncut first.
    fn prefer_cut_first(&self, e: EdgeId) -> bool {
        if self.curr_comp_id.is_empty() {
            return true;
        }
        let (c1, c2) = self.grid.edge_cells(e);
        if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
            return true;
        }
        let ci1 = self.curr_comp_id[c1];
        let ci2 = self.curr_comp_id[c2];
        let sz1 = self.curr_comp_sz[ci1];
        let sz2 = self.curr_comp_sz[ci2];

        // If Uncut would create a merge-size conflict, definitely try Cut first
        if self.puzzle.rules.size_separation {
            if let Some(ref sns) = self.edge_selection.sealed_neighbor_sizes {
                if ci1 < sns.len() && ci2 < sns.len() {
                    let merged_sz = sz1 + sz2;
                    if sns[ci1].contains(&merged_sz) || sns[ci2].contains(&merged_sz) {
                        return true;
                    }
                }
            }
        }

        // If both components are same-size no-target, Uncut first:
        // merging them creates a larger component that's more likely to be
        // uniquely sized, while cutting leaves two same-size neighbors
        // that will need further constraint.
        if sz1 == sz2
            && self.curr_target_area[ci1].is_none()
            && self.curr_target_area[ci2].is_none()
            && self.puzzle.rules.size_separation
        {
            return false;
        }

        // If one side is needy (below target), prefer Uncut to help it grow
        if let Some(target) = self.curr_target_area[ci1] {
            if sz1 < target {
                return false;
            }
        }
        if let Some(target) = self.curr_target_area[ci2] {
            if sz2 < target {
                return false;
            }
        }

        true
    }

    pub(crate) fn backtrack_edges(&mut self) {
        if self.solution_count >= 2 {
            return;
        }

        self.node_count += 1;
        self.report_progress();
        self.search_depth += 1;

        if self.curr_unknown == 0 {
            let pieces = self.compute_pieces();
            if self.validate(&pieces) {
                // Deduplicate: skip if same edge assignment as previous solution
                if self.solution_count > 0 && self.edges == self.best_edges {
                    self.search_depth -= 1;
                    return;
                }
                self.save_solution(pieces);
            }
            self.search_depth -= 1;
            return;
        }

        // Edge selection
        let (e, best_edge_score) = match self.select_edge() {
            Some((e, score)) => (e, score),
            None => {
                self.search_depth -= 1;
                return;
            }
        };

        // Compass membership branching: for compass-only puzzles (no rose),
        // collect independent compass pairs and branch on all of them first.
        // Run at shallow depths to catch new opportunities after edge decisions.
        if self.search_depth <= 3
            && self.has_compass_clue
            && self.rose_bits_all == 0
            && self.curr_unknown <= 80
            && self.curr_unknown < self.total_unknown
        // not at search root
        {
            let max_pairs = if self.search_depth <= 2 { 5 } else { 2 };
            let pairs = self.select_compass_branches_flat(max_pairs);
            if !pairs.is_empty() {
                self.branch_compass_flat(pairs);
                self.search_depth -= 1;
                return;
            }
        }

        // Pair branching: for rose puzzles, try branching on cell pairs
        // before falling back to edge branching.
        if self.rose_bits_all != 0 && self.pair_layer.is_some() && self.curr_unknown <= 80 {
            if let Some((c1, c2)) = self.select_rose_pair(best_edge_score) {
                self.branch_on_pair(c1, c2);
                return;
            }
        }

        // Edge branching
        let cut_first = self.prefer_cut_first(e);
        let order: &[EdgeState; 2] = if cut_first {
            &[EdgeState::Cut, EdgeState::Uncut]
        } else {
            &[EdgeState::Uncut, EdgeState::Cut]
        };

        for &val in order {
            let snap = self.snapshot();
            self.debug_current_prop = "branch";
            if !self.set_edge(e, val) {
                continue;
            }
            match self.propagate() {
                Ok(_) => {
                    self.backtrack_edges();
                }
                Err(_) => {
                    let (c1, c2) = self.grid.edge_cells(e);
                    // If we're on the solution path, this branch failure is wrong
                    if !self.in_probing && !self.debug_known_solution.is_empty() {
                        let sol = if e < self.debug_known_solution.len() {
                            self.debug_known_solution[e]
                        } else {
                            EdgeState::Unknown
                        };
                        if sol == val {
                            eprintln!(
                                "SOLUTION_KILL: branch edge={} cells={:?}->{:?} val={:?} depth={} unk={} prop={}",
                                e, self.grid.cell_pos(c1), self.grid.cell_pos(c2), val,
                                self.search_depth, self.curr_unknown, self.debug_current_prop
                            );
                        }
                    }
                }
            }
            self.restore(snap);
        }

        self.search_depth -= 1;
    }
}
