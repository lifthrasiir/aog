use super::Solver;
use crate::polyomino::{self, canonical};
use crate::types::*;
use std::collections::{HashMap, HashSet};

impl Solver {
    const MAX_ENUM_SIZE: usize = 10;

    pub(crate) fn solve_normal(&mut self) {
        // When distinct area sum = total cells, use grouped area search
        if self.same_area_groups {
            self.solve_grouped_areas();
            return;
        }

        let total_clue_area: usize = self
            .puzzle
            .cell_clues
            .iter()
            .map(|cl| match cl {
                CellClue::Area { value, .. } => *value,
                CellClue::Polyomino { shape, .. } => shape.cells.len(),
                _ => 0,
            })
            .sum();

        if !self.puzzle.rules.shape_bank.is_empty() || total_clue_area == self.total_cells {
            if !self.puzzle.rules.shape_bank.is_empty() {
                self.prepare_shape_transforms();
                self.shape_bank_canonicals = self
                    .puzzle
                    .rules
                    .shape_bank
                    .iter()
                    .map(|s| canonical(s))
                    .collect();
            }
            self.backtrack_pieces();
        } else if total_clue_area > 0 && self.should_try_hybrid(total_clue_area) {
            self.solve_hybrid();
        } else {
            self.backtrack_edges();
        }
    }

    /// Check if hybrid (piece placements + edge search) is appropriate:
    /// area clues cover a significant fraction of cells and there are multiple
    /// area clues of the same value (suggesting piece-based enumeration helps).
    fn should_try_hybrid(&self, total_clue_area: usize) -> bool {
        if total_clue_area == 0 || total_clue_area >= self.total_cells {
            return false;
        }
        if self.puzzle.rules.size_separation {
            return true;
        }
        total_clue_area > self.total_cells * 2 / 3
    }

    /// Hybrid search: enumerate placements for area-clue pieces, try
    /// non-overlapping combinations, then edge-search the remaining cells.
    fn solve_hybrid(&mut self) {
        let placements = self.generate_clue_placements();

        // Group by clue index
        let num_clues = self
            .puzzle
            .cell_clues
            .iter()
            .filter(|cl| matches!(cl, CellClue::Area { .. }))
            .count();
        let mut groups: Vec<Vec<(usize, Vec<CellId>)>> = vec![Vec::new(); num_clues];
        for (clue_idx, piece) in &placements {
            groups[*clue_idx].push((*clue_idx, piece.cells.clone()));
        }

        for (i, g) in groups.iter().enumerate() {
            eprintln!("clue {}: {} placements", i, g.len());
            if g.is_empty() {
                return; // no valid placements for this clue
            }
        }

        // Sort by fewest placements first (most constrained)
        let mut order: Vec<usize> = (0..num_clues).collect();
        order.sort_by_key(|&i| groups[i].len());

        let n = self.grid.num_cells();
        let mut used = vec![false; n];
        let mut solution: Vec<(usize, Vec<CellId>)> = Vec::new();
        self.hybrid_backtrack(&order, &groups, &mut used, &mut solution);

        // Phase 2: fast uniqueness check.
        // After finding the first solution, fix two clue placements at a time
        // and enumerate alternatives for the third. This is O(sum of alt placements)
        // instead of O(product of all placements).
        if self.solution_count == 1 {
            // Extract known cell sets per clue from the solution
            let mut known: Vec<HashSet<CellId>> = vec![HashSet::new(); num_clues];
            for &(clue_idx, ref cells) in &solution {
                known[clue_idx] = cells.iter().copied().collect();
            }

            // For each clue, try alternative placements while fixing others
            for target_clue in 0..num_clues {
                if self.solution_count >= 2 {
                    break;
                }
                let alt_count = groups[target_clue]
                    .iter()
                    .filter(|(_, cells)| {
                        cells.iter().copied().collect::<HashSet<_>>() != known[target_clue]
                    })
                    .count();
                eprintln!(
                    "uniqueness check clue {}: {} alternatives",
                    target_clue, alt_count
                );
                for (_, cells) in &groups[target_clue] {
                    if self.solution_count >= 2 {
                        break;
                    }
                    // Skip if same as known
                    let cells_set: HashSet<CellId> = cells.iter().copied().collect();
                    if cells_set == known[target_clue] {
                        continue;
                    }
                    // Check overlap with other clues' known placements
                    let mut overlaps = false;
                    for (other_idx, other_known) in known.iter().enumerate() {
                        if other_idx != target_clue && cells.iter().any(|c| other_known.contains(c))
                        {
                            overlaps = true;
                            break;
                        }
                    }
                    if overlaps {
                        continue;
                    }

                    // Set edges for all known placements + this trial placement
                    let snap = self.changed.len();
                    let mut all_cells: Vec<(usize, Vec<CellId>)> = Vec::new();
                    for (ci, c) in known.iter().enumerate() {
                        if ci == target_clue {
                            all_cells.push((ci, cells.clone()));
                        } else {
                            all_cells.push((ci, c.iter().copied().collect()));
                        }
                    }
                    let placed_set: HashSet<CellId> = all_cells
                        .iter()
                        .flat_map(|(_, c)| c.iter().copied())
                        .collect();
                    for (_, cells) in &all_cells {
                        for &cid in cells {
                            for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                                let (c1, c2) = self.grid.edge_cells(eid);
                                let other = if c1 == cid { c2 } else { c1 };
                                if !self.grid.cell_exists[other] {
                                    continue;
                                }
                                if placed_set.contains(&other)
                                    && all_cells.iter().any(|(_, c)| c.contains(&other))
                                {
                                    if self.edges[eid] == EdgeState::Unknown {
                                        let _ = self.set_edge(eid, EdgeState::Uncut);
                                    }
                                } else if placed_set.contains(&other) {
                                    if self.edges[eid] == EdgeState::Unknown {
                                        let _ = self.set_edge(eid, EdgeState::Cut);
                                    }
                                }
                            }
                        }
                    }
                    self.curr_unknown = self
                        .edges
                        .iter()
                        .filter(|&&e| e == EdgeState::Unknown)
                        .count();
                    if self.propagate().is_ok() {
                        self.backtrack_edges();
                    }
                    self.restore(snap);
                }
            }
        }
    }

    fn hybrid_backtrack(
        &mut self,
        order: &[usize],
        groups: &[Vec<(usize, Vec<CellId>)>],
        used: &mut Vec<bool>,
        solution: &mut Vec<(usize, Vec<CellId>)>,
    ) {
        if self.solution_count >= 2 {
            return;
        }

        let depth = solution.len();
        if depth == order.len() {
            // All clue pieces placed. Set edges, propagate, edge-search remaining cells.
            let snap = self.changed.len();

            // Build a set of placed cells for fast lookup
            let placed_set: HashSet<CellId> = solution
                .iter()
                .flat_map(|(_, cells)| cells.iter().copied())
                .collect();

            // Set edges: Uncut within each piece, Cut between pieces / placed vs unplaced
            for (_, cells) in solution.iter() {
                for &cid in cells {
                    for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let other = if c1 == cid { c2 } else { c1 };
                        if !self.grid.cell_exists[other] {
                            continue;
                        }
                        if placed_set.contains(&other) && cells.contains(&other) {
                            // Same piece → Uncut
                            if self.edges[eid] == EdgeState::Unknown {
                                let _ = self.set_edge(eid, EdgeState::Uncut);
                            }
                        } else if placed_set.contains(&other) {
                            // Different piece → Cut
                            if self.edges[eid] == EdgeState::Unknown {
                                let _ = self.set_edge(eid, EdgeState::Cut);
                            }
                        }
                        // Unplaced cells: leave Unknown for edge-based search
                    }
                }
            }

            // Recount remaining unknowns and run edge-based search
            self.curr_unknown = self
                .edges
                .iter()
                .filter(|&&e| e == EdgeState::Unknown)
                .count();

            if self.propagate().is_ok() {
                self.backtrack_edges();
            }

            self.restore(snap);
            return;
        }

        let gi = order[depth];
        let group = &groups[gi];

        for (clue_idx, cells) in group {
            if cells.iter().any(|&c| used[c]) {
                continue;
            }

            // Quick adjacency check: size separation forbids same-size adjacent pieces
            let area = cells.len();
            let mut adj_ok = true;
            let _cell_set: HashSet<CellId> = cells.iter().copied().collect();
            for (prev_gi, prev_cells) in solution.iter() {
                let prev_area = self.puzzle.cell_clues[*prev_gi]
                    .cell_area()
                    .unwrap_or(prev_cells.len());
                if area == prev_area {
                    // Check if any cell in current piece is adjacent to previous piece
                    for &cid in cells {
                        for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                            let (c1, c2) = self.grid.edge_cells(eid);
                            let other = if c1 == cid { c2 } else { c1 };
                            if self.grid.cell_exists[other] && prev_cells.contains(&other) {
                                adj_ok = false;
                                break;
                            }
                        }
                        if !adj_ok {
                            break;
                        }
                    }
                }
            }

            if !adj_ok {
                continue;
            }

            // Mark used, recurse, unmark
            for &c in cells {
                used[c] = true;
            }
            solution.push((*clue_idx, cells.clone()));

            self.hybrid_backtrack(order, groups, used, solution);

            solution.pop();
            for &c in cells {
                used[c] = false;
            }

            if self.solution_count >= 2 {
                return;
            }
        }
    }

    pub(crate) fn solve_match(&mut self) {
        let rules = &self.puzzle.rules;

        // Contradiction checks
        if rules.match_all && rules.mismatch && self.total_cells > 1 {
            return;
        }
        if rules.match_all && rules.size_separation && self.total_cells > 1 {
            return;
        }

        let total = self.total_cells;
        let rose_count = self.count_rose_symbols();

        if !self.puzzle.rules.shape_bank.is_empty() {
            // match + shape bank: try each bank shape whose area divides total
            let bank_shapes: Vec<Shape> = self.puzzle.rules.shape_bank.clone();
            for shape in &bank_shapes {
                if self.solution_count >= 2 {
                    return;
                }
                let area = shape.cells.len();
                if total % area != 0 || area >= total {
                    continue;
                }
                if area < self.eff_min_area || area > self.eff_max_area {
                    continue;
                }
                let num_pieces = total / area;
                if rose_count > 0 && num_pieces != rose_count {
                    continue;
                }
                eprintln!("\rmatch+bank: trying shape (area={})...", area);
                self.try_single_shape(shape);
            }
        } else {
            // match without shape bank: enumerate free polyominoes
            let candidates = Self::divisors_in_range(total, self.eff_min_area, self.eff_max_area);

            for area in candidates {
                if self.solution_count >= 2 {
                    return;
                }
                let num_pieces = total / area;
                if rose_count > 0 && num_pieces != rose_count {
                    continue;
                }

                if area <= Self::MAX_ENUM_SIZE {
                    let shapes = polyomino::enumerate_free_polyominoes(area);
                    eprintln!(
                        "\rmatch: area={}, {} pieces, {} free polyominoes",
                        area,
                        num_pieces,
                        shapes.len()
                    );
                    for shape in &shapes {
                        if self.solution_count >= 2 {
                            return;
                        }
                        self.try_single_shape(shape);
                    }
                } else {
                    // Coupled rigid-motion DFS for 2-piece match with rose anchors
                    if rose_count == 2 {
                        eprintln!(
                            "\rmatch: area={}, coupled rigid-motion DFS (2 pieces)",
                            area
                        );
                        self.solve_match_2piece_coupled();
                        if self.solution_count > 0 {
                            return;
                        }
                    }
                    // Fallback: edge search with tightened bounds
                    eprintln!("\rmatch: area={} > max, edge search fallback", area);
                    let old_min = self.eff_min_area;
                    let old_max = self.eff_max_area;
                    self.eff_min_area = area;
                    self.eff_max_area = area;
                    self.backtrack_edges();
                    self.eff_min_area = old_min;
                    self.eff_max_area = old_max;
                    return;
                }
            }
        }

        // If no enumeration path found a solution, fall back to edge search
        if self.solution_count == 0 {
            eprintln!("\rmatch: edge search fallback");
            self.backtrack_edges();
        }
    }

    pub(crate) fn count_rose_symbols(&self) -> usize {
        let mut counts: HashMap<u8, usize> = HashMap::new();
        for clue in &self.puzzle.cell_clues {
            if let CellClue::Rose { symbol, .. } = clue {
                *counts.entry(*symbol).or_insert(0) += 1;
            }
        }
        counts.into_values().next().unwrap_or(0)
    }

    pub(crate) fn divisors_in_range(n: usize, lo: usize, hi: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut d = 1usize;
        while d * d <= n {
            if n % d == 0 {
                if d >= lo && d <= hi && d < n {
                    result.push(d);
                }
                let q = n / d;
                if q != d && q >= lo && q <= hi && q < n {
                    result.push(q);
                }
            }
            d += 1;
        }
        result.sort();
        result
    }

    pub(crate) fn try_single_shape(&mut self, shape: &Shape) {
        let saved_bank = std::mem::take(&mut self.puzzle.rules.shape_bank);
        let saved_transforms = std::mem::take(&mut self.shape_transforms);
        let saved_canonicals = std::mem::take(&mut self.shape_bank_canonicals);

        self.puzzle.rules.shape_bank = vec![shape.clone()];
        self.prepare_shape_transforms();
        self.shape_bank_canonicals = vec![canonical(shape)];

        self.backtrack_pieces();

        self.puzzle.rules.shape_bank = saved_bank;
        self.shape_transforms = saved_transforms;
        self.shape_bank_canonicals = saved_canonicals;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisors_in_range_basic() {
        assert_eq!(Solver::divisors_in_range(12, 2, 6), vec![2, 3, 4, 6]);
    }

    #[test]
    fn divisors_in_range_narrow() {
        assert_eq!(Solver::divisors_in_range(12, 3, 4), vec![3, 4]);
    }

    #[test]
    fn divisors_in_range_no_match() {
        assert_eq!(Solver::divisors_in_range(7, 2, 3), Vec::<usize>::new());
    }

    #[test]
    fn divisors_in_range_excludes_self() {
        // 4's divisors: 1,2,4 but 4==n so excluded. lo=2 excludes 1.
        assert_eq!(Solver::divisors_in_range(4, 2, 10), vec![2]);
    }

    #[test]
    fn divisors_in_range_prime() {
        // 7 is prime, only divisor is 1 (included if lo<=1). lo=2 excludes it.
        assert_eq!(Solver::divisors_in_range(7, 2, 10), Vec::<usize>::new());
    }

    #[test]
    fn count_rose_symbols_basic() {
        use crate::solver::test_helpers::make_solver;

        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        // Directly set rose clues on the puzzle
        let cell0 = s.grid.cell_id(0, 0);
        let cell1 = s.grid.cell_id(1, 1);
        s.puzzle.cell_clues.push(CellClue::Rose {
            cell: cell0,
            symbol: 1,
        });
        s.puzzle.cell_clues.push(CellClue::Rose {
            cell: cell1,
            symbol: 1,
        });
        assert_eq!(s.count_rose_symbols(), 2);
    }

    #[test]
    fn count_rose_symbols_none() {
        use crate::solver::test_helpers::make_solver;

        let s = make_solver(
            "\
+---+---+
| _ . _ |
+---+---+
",
        );
        assert_eq!(s.count_rose_symbols(), 0);
    }
}
