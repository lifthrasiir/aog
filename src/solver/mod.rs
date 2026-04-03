mod edge_state;
mod edges;
mod match_coupled;
mod match_solver;
mod pieces;
mod progress;
mod propagation;
pub(crate) mod shapes;
mod validation;

use crate::grid::Grid;
use crate::types::*;
use std::collections::HashSet;

pub struct Solver {
    pub(crate) puzzle: Puzzle,
    pub(crate) grid: Grid,
    pub(crate) edges: Vec<EdgeState>,
    pub(crate) solution_count: usize,
    pub(crate) first_pieces: Vec<Piece>,
    pub(crate) first_edges: Vec<EdgeState>,
    pub(crate) best_pieces: Vec<Piece>,
    pub(crate) best_edges: Vec<EdgeState>,
    pub(crate) changed: Vec<(EdgeId, EdgeState)>,
    pub(crate) is_pre_cut: Vec<bool>,
    pub(crate) eff_min_area: usize,
    pub(crate) eff_max_area: usize,
    // Piece-based search state
    pub(crate) total_cells: usize,
    pub(crate) shape_transforms: Vec<Vec<Vec<(isize, isize)>>>,
    pub(crate) shape_bank_canonicals: Vec<Shape>,
    // Progress tracking
    pub(crate) node_count: u64,
    pub(crate) total_unknown: usize,
    pub(crate) curr_unknown: usize,
    pub(crate) progress: progress::Progress,
    // Cached component info from last propagate()
    pub(crate) curr_comp_id: Vec<usize>,
    pub(crate) curr_comp_sz: Vec<usize>,
    pub(crate) curr_target_area: Vec<Option<usize>>,
    // Reusable buffers for propagation
    pub(crate) comp_buf: Vec<usize>,
    pub(crate) q_buf: Vec<usize>,
    pub(crate) can_grow_buf: Vec<bool>,
    // Dedup for match coupled solver: tracks seen piece1 cell sets
    pub(crate) seen_partitions: HashSet<Vec<CellId>>,
    // Whether shape bank was auto-populated (affects solve strategy)
    pub(crate) auto_populated_bank: bool,
    // Pre-calculated cell clue info
    pub(crate) cell_clues_indexed: Vec<Vec<usize>>, // indices into self.puzzle.cell_clues
    pub(crate) has_any_clue: Vec<bool>,
    // Optimization: when sum of distinct area values equals total_cells,
    // all cells with the same area number must be in the same piece.
    pub(crate) same_area_groups: bool,
    // Cached from last propagate_area_bounds() for edge selection heuristic
    pub(crate) cached_sealed_neighbor_sizes: Option<Vec<HashSet<usize>>>,
    pub(crate) cached_growth_edge_count: Vec<usize>,
    // Pre-extracted diff clues: (edge_id, value)
    pub(crate) diff_clues: Vec<(EdgeId, usize)>,
}

impl Solver {
    pub fn new(puzzle: Puzzle, grid: Grid) -> Self {
        let n = grid.num_edges();
        let nc = grid.num_cells();

        let mut cell_clues_indexed = vec![vec![]; nc];
        let mut has_any_clue = vec![false; nc];
        for (i, clue) in puzzle.cell_clues.iter().enumerate() {
            let c = clue.cell();
            cell_clues_indexed[c].push(i);
            has_any_clue[c] = true;
        }

        let diff_clues: Vec<(EdgeId, usize)> = puzzle
            .edge_clues
            .iter()
            .filter_map(|cl| {
                if let EdgeClueKind::Diff { value } = cl.kind {
                    Some((cl.edge, value))
                } else {
                    None
                }
            })
            .collect();

        Self {
            puzzle,
            grid,
            edges: vec![EdgeState::Unknown; n],
            solution_count: 0,
            first_pieces: Vec::new(),
            first_edges: Vec::new(),
            best_pieces: Vec::new(),
            best_edges: Vec::new(),
            changed: Vec::new(),
            is_pre_cut: vec![false; n],
            eff_min_area: 1,
            eff_max_area: usize::MAX,
            total_cells: 0,
            shape_transforms: Vec::new(),
            shape_bank_canonicals: Vec::new(),
            node_count: 0,
            total_unknown: 0,
            progress: progress::Progress::new(),
            curr_unknown: n,
            curr_comp_id: Vec::new(),
            curr_comp_sz: Vec::new(),
            curr_target_area: Vec::new(),
            comp_buf: vec![usize::MAX; nc],
            q_buf: Vec::with_capacity(nc),
            can_grow_buf: Vec::new(),
            seen_partitions: HashSet::new(),
            auto_populated_bank: false,
            cell_clues_indexed,
            has_any_clue,
            same_area_groups: false,
            cached_sealed_neighbor_sizes: None,
            cached_growth_edge_count: Vec::new(),
            diff_clues,
        }
    }

    pub fn mark_pre_cut(&mut self, e: EdgeId) {
        self.is_pre_cut[e] = true;
        if self.edges[e] == EdgeState::Unknown {
            self.edges[e] = EdgeState::Cut;
            self.changed.push((e, EdgeState::Unknown));
        }
    }

    pub fn solve(&mut self) -> usize {
        self.progress.reset();
        self.compute_area_bounds();
        self.total_cells = self.grid.total_existing_cells();

        // Detect same-area-groups optimization:
        // If sum of distinct area clue values == total cells, then every distinct
        // area value corresponds to exactly one piece, and all cells with the same
        // area number must be in the same connected piece.
        {
            use std::collections::HashSet;
            let mut seen = HashSet::new();
            let mut distinct_sum = 0usize;
            for clue in &self.puzzle.cell_clues {
                if let CellClue::Area { value, .. } = clue {
                    if seen.insert(*value) {
                        distinct_sum += *value;
                    }
                }
            }
            self.same_area_groups = distinct_sum == self.total_cells;
            if self.same_area_groups {
                eprintln!(
                    "same-area-groups optimization: {} distinct areas sum to {} = total cells",
                    seen.len(),
                    distinct_sum
                );
            }
        }

        // Initial unknown count
        self.curr_unknown = self
            .edges
            .iter()
            .filter(|&&e| e == EdgeState::Unknown)
            .count();

        // Set pre-cut edges for missing cells
        for e in 0..self.grid.num_edges() {
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                if self.edges[e] == EdgeState::Unknown {
                    self.set_edge(e, EdgeState::Cut);
                }
            }
        }
        // Set edge clues to CUT
        let edge_clues_to_set: Vec<EdgeId> = self
            .puzzle
            .edge_clues
            .iter()
            .map(|clue| clue.edge)
            .filter(|&e| self.edges[e] == EdgeState::Unknown)
            .collect();
        for eid in edge_clues_to_set {
            self.set_edge(eid, EdgeState::Cut);
        }
        self.propagate_palisade();

        if self.propagate().is_err() {
            return 0;
        }

        // Debug: count edges after propagation
        let n_cut = self.edges.iter().filter(|&&e| e == EdgeState::Cut).count();
        let n_uncut = self
            .edges
            .iter()
            .filter(|&&e| e == EdgeState::Uncut)
            .count();
        let n_unknown = self
            .edges
            .iter()
            .filter(|&&e| e == EdgeState::Unknown)
            .count();
        eprintln!(
            "after propagation: cut={}, uncut={}, unknown={}, total={}",
            n_cut,
            n_uncut,
            n_unknown,
            n_cut + n_uncut + n_unknown
        );

        // Dump shape bank
        eprintln!("shape bank: {} shapes", self.puzzle.rules.shape_bank.len());
        for (i, s) in self.puzzle.rules.shape_bank.iter().enumerate() {
            eprintln!("  shape {}: {} cells", i, s.cells.len());
        }

        // Dump grid structure
        eprintln!("grid: rows={}, cols={}", self.grid.rows, self.grid.cols);
        for r in 0..self.grid.rows {
            let mut row_str = String::new();
            for c in 0..self.grid.cols {
                let cid = self.grid.cell_id(r, c);
                if self.grid.cell_exists[cid] {
                    // Check for rose clue
                    let rose = self
                        .puzzle
                        .cell_clues
                        .iter()
                        .find(|cl| matches!(cl, CellClue::Rose { cell, .. } if *cell == cid));
                    if let Some(_) = rose {
                        row_str.push('A');
                    } else {
                        row_str.push('_');
                    }
                } else {
                    row_str.push(' ');
                }
            }
            eprintln!("  row {}: {}", r, row_str);
        }

        // Count remaining unknown edges for progress display
        self.total_unknown = self.curr_unknown;

        // For precision puzzles without explicit shape bank, auto-populate
        // with all free polyominoes of the required size (much faster piece-based search)
        self.auto_populate_shape_bank();

        if self.puzzle.rules.match_all {
            self.solve_match();
        } else {
            self.solve_normal();
        }

        // Clear the progress line
        progress::Progress::clear_line();

        self.solution_count
    }

    fn report_progress(&mut self) {
        if let Some(elapsed_secs) = self.progress.should_report(self.node_count) {
            self.progress
                .print(elapsed_secs, self.node_count, self.curr_unknown, self.total_unknown);
        }
    }

    /// Record a solution. Saves the first solution to first_*, and always updates best_*.
    pub(crate) fn save_solution(&mut self, pieces: Vec<Piece>) {
        if self.solution_count == 0 {
            self.first_pieces = pieces.clone();
            self.first_edges = self.edges.clone();
        }
        self.solution_count += 1;
        self.best_pieces = pieces;
        self.best_edges = self.edges.clone();
        self.report_solution(self.solution_count);
    }

    #[cfg(test)]
    pub(crate) fn get_best_pieces(&self) -> &[Piece] {
        &self.best_pieces
    }
    #[cfg(test)]
    pub(crate) fn get_best_edges(&self) -> &[EdgeState] {
        &self.best_edges
    }
    #[cfg(test)]
    pub(crate) fn get_first_pieces(&self) -> &[Piece] {
        &self.first_pieces
    }
    #[cfg(test)]
    pub(crate) fn get_first_edges(&self) -> &[EdgeState] {
        &self.first_edges
    }
    #[cfg(test)]
    pub(crate) fn get_grid(&self) -> &Grid {
        &self.grid
    }

    /// Print the current best solution with a header. Called from all search paths.
    pub(crate) fn report_solution(&self, which: usize) {
        // Clear the progress line on stderr
        progress::Progress::clear_line();
        let header = match which {
            1 => "First solution found:",
            2 => "Second solution found:",
            _ => "Solution found:",
        };
        println!("{}", header);
        print!(
            "{}",
            crate::formatter::format_solution(&self.grid, &self.best_edges, &self.best_pieces)
        );
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use crate::parser::Parser;

    pub fn make_solver(input: &str) -> Solver {
        let mut p = Parser::new();
        p.parse(input.as_bytes()).unwrap();
        let mut s = Solver::new(p.puzzle, p.grid);
        for e in p.pre_cut_edges {
            s.mark_pre_cut(e);
        }
        s
    }
}

