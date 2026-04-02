use super::Solver;
use crate::polyomino::{self, canonical};
use crate::types::*;
use std::collections::HashMap;

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
        } else {
            self.backtrack_edges();
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
