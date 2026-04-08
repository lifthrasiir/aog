mod clue_placements;
mod edge_state;
mod edges;
mod match_coupled;
mod match_solver;
mod pair;
mod pieces;
mod progress;
mod prop_area;
mod prop_bricky_loopy;
mod prop_compass;
mod prop_delta_gemini;
mod prop_dual;
mod prop_loop;
mod prop_palisade;
mod prop_rose;
mod prop_shape;
mod prop_watchtower;
mod propagation;
pub(crate) mod shapes;
mod validation;
pub use validation::validate_parsed_solution;

use crate::grid::Grid;
use crate::types::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub edges: usize,
    pub manual_diffs: usize,
    pub manual_sames: usize,
}

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
    pub(crate) curr_min_area: Vec<usize>,
    pub(crate) curr_max_area: Vec<usize>,
    // Reusable buffers for propagation
    pub(crate) comp_buf: Vec<usize>,
    pub(crate) comp_buf2: Vec<usize>,
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
    // All edge IDs with any clue (inequality, diff, gemini, delta) — always Cut
    pub(crate) clue_cut_edges: Vec<EdgeId>,
    // Vertices with watchtower clues (static, for edge selection heuristic)
    pub(crate) watchtower_vertices: HashSet<VertexId>,
    // Bitset of rose window symbols present in the puzzle (bit i = symbol i exists)
    pub(crate) rose_bits_all: u8,
    // Pre-computed rose symbol per cell (u8::MAX = no rose symbol, static)
    pub(crate) cell_rose_sym: Vec<u8>,
    // Reusable BFS buffers for rose propagation
    pub(crate) rose_visited: Vec<bool>,
    // Cells per component (populated by build_components, used by sub-propagators)
    pub(crate) comp_cells: Vec<Vec<CellId>>,
    // Growth edges per component (populated by build_components)
    pub(crate) growth_edges: Vec<Vec<EdgeId>>,
    // Recursion guard: prevents probing from running inside a probe's propagation
    pub(crate) in_probing: bool,
    // Pre-computed clue presence flags (set once in new())
    pub(crate) has_palisade_clue: bool,
    pub(crate) has_compass_clue: bool,
    // Cell-pair constraint layer (None if no rose symbols)
    pub(crate) pair_layer: Option<pair::CellPairLayer>,
    // Exact piece count deduced from rose window: if all rose types have the same
    // count N, then exactly N pieces are needed (each piece gets one of each type).
    pub(crate) rose_exact_piece_count: Option<usize>,
    // Reusable BFS buffer for path-finding (pair branching)
    pub(crate) bfs_prev: Vec<Option<(CellId, EdgeId)>>,
    // Manual DIFF constraints from branching (c1, c2)
    pub(crate) manual_diffs: Vec<(CellId, CellId)>,
    // Precomputed set of manual DIFF pairs for fast lookup
    pub(crate) manual_diff_set: HashSet<(CellId, CellId)>,
    // Manual SAME constraints from branching (c1, c2 must be in same piece)
    pub(crate) manual_sames: Vec<(CellId, CellId)>,
    pub(crate) manual_same_set: HashSet<(CellId, CellId)>,
    // Search recursion depth to limit compass branching to top level only
    pub(crate) search_depth: usize,
    // Solver start time for elapsed-time reporting
    pub(crate) start_time: std::time::Instant,
    // Debug: known correct solution edge states (empty = disabled)
    pub(crate) debug_known_solution: Vec<EdgeState>,
    // Debug: name of current propagator (set before each set_edge call)
    pub(crate) debug_current_prop: &'static str,
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

        let clue_cut_edges: Vec<EdgeId> = puzzle.edge_clues.iter().map(|cl| cl.edge).collect();

        let watchtower_vertices: HashSet<VertexId> =
            puzzle.vertex_clues.iter().map(|cl| cl.vertex).collect();

        let mut rose_bits_all: u8 = 0;
        for cl in &puzzle.cell_clues {
            if let CellClue::Rose { symbol, .. } = cl {
                rose_bits_all |= 1 << *symbol;
            }
        }

        // Pre-compute rose symbol per cell (u8::MAX = no rose symbol)
        let mut cell_rose_sym = vec![u8::MAX; nc];
        for c in 0..nc {
            if !grid.cell_exists[c] {
                continue;
            }
            for &clue_idx in &cell_clues_indexed[c] {
                if let CellClue::Rose { symbol, .. } = &puzzle.cell_clues[clue_idx] {
                    cell_rose_sym[c] = *symbol;
                    break;
                }
            }
        }

        let rose_visited = vec![false; nc];

        // Deduce exact piece count from rose window:
        // If all rose types have the same count N, exactly N pieces are needed.
        // If counts differ, no solution exists (some type would be left out).
        let rose_exact_piece_count = if rose_bits_all != 0 {
            let mut type_counts: Vec<usize> = Vec::new();
            for cl in &puzzle.cell_clues {
                if let CellClue::Rose { symbol, .. } = cl {
                    let idx = *symbol as usize;
                    while type_counts.len() <= idx {
                        type_counts.push(0);
                    }
                    type_counts[idx] += 1;
                }
            }
            let nonzero: Vec<usize> = type_counts.iter().copied().filter(|&c| c > 0).collect();
            if nonzero.is_empty() {
                None
            } else if nonzero.iter().all(|&c| c == nonzero[0]) {
                Some(nonzero[0])
            } else {
                // Differing counts → no solution (checked in solve())
                None
            }
        } else {
            None
        };

        let has_palisade_clue = puzzle
            .cell_clues
            .iter()
            .any(|c| matches!(c, CellClue::Palisade { .. }));
        let has_compass_clue = puzzle
            .cell_clues
            .iter()
            .any(|c| matches!(c, CellClue::Compass { .. }));

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
            curr_min_area: Vec::new(),
            curr_max_area: Vec::new(),
            comp_buf: vec![usize::MAX; nc],
            comp_buf2: Vec::new(),
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
            clue_cut_edges,
            watchtower_vertices,
            rose_bits_all,
            cell_rose_sym,
            rose_visited,
            comp_cells: Vec::new(),
            growth_edges: Vec::new(),
            in_probing: false,
            has_palisade_clue,
            has_compass_clue,
            pair_layer: None,
            rose_exact_piece_count,
            bfs_prev: Vec::new(),
            manual_diffs: Vec::new(),
            manual_diff_set: HashSet::new(),
            manual_sames: Vec::new(),
            manual_same_set: HashSet::new(),
            search_depth: 0,
            start_time: std::time::Instant::now(),
            debug_known_solution: Vec::new(),
            debug_current_prop: "init",
        }
    }

    pub fn mark_pre_cut(&mut self, e: EdgeId) {
        self.is_pre_cut[e] = true;
        if self.edges[e] == EdgeState::Unknown {
            self.edges[e] = EdgeState::Cut;
            self.changed.push((e, EdgeState::Unknown));
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            edges: self.changed.len(),
            manual_diffs: self.manual_diffs.len(),
            manual_sames: self.manual_sames.len(),
        }
    }

    pub fn solve(&mut self) -> usize {
        self.progress.reset(self.start_time);
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

        // Rose exact piece count: if type counts differ, no solution.
        if self.rose_bits_all != 0 && self.rose_exact_piece_count.is_none() {
            return 0;
        }

        // Set pre-cut edges for missing cells
        for e in 0..self.grid.num_edges() {
            let (c1, c2) = self.grid.edge_cells(e);
            if (!self.grid.cell_exists[c1] || !self.grid.cell_exists[c2])
                && self.edges[e] == EdgeState::Unknown
            {
                self.set_edge(e, EdgeState::Cut);
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

        // Watchtower !(value=1) replacement on 4-cell vertices:
        // Every cell must belong to a piece, so value=1 always means 0 cut edges.
        // Force all 4 surrounding edges to Uncut and remove the clue.
        // (For border vertices with fewer cells, keep the clue for proper propagation.)
        {
            let cell_pair_indices: [(usize, usize); 4] = [(0, 1), (0, 2), (1, 3), (2, 3)];
            let mut remove_indices = Vec::new();
            let mut edges_to_uncut: Vec<EdgeId> = Vec::new();
            for (idx, clue) in self.puzzle.vertex_clues.iter().enumerate() {
                if clue.value != 1 {
                    continue;
                }
                let (vi, vj) = self.grid.vertex_pos(clue.vertex);
                let cell_opts = self.grid.vertex_cells(vi, vj);
                let n = cell_opts
                    .iter()
                    .copied()
                    .flatten()
                    .filter(|&cid| self.grid.cell_exists[cid])
                    .count();
                if n == 4 {
                    for &(a_idx, b_idx) in &cell_pair_indices {
                        if let (Some(a), Some(b)) = (cell_opts[a_idx], cell_opts[b_idx]) {
                            if self.grid.cell_exists[a] && self.grid.cell_exists[b] {
                                if let Some(eid) = self.grid.edge_between(a, b) {
                                    edges_to_uncut.push(eid);
                                }
                            }
                        }
                    }
                    remove_indices.push(idx);
                }
            }
            for eid in edges_to_uncut {
                if self.edges[eid] == EdgeState::Unknown {
                    let _ = self.set_edge(eid, EdgeState::Uncut);
                }
            }
            // Remove replaced clues (in reverse order to preserve indices)
            for &idx in remove_indices.iter().rev() {
                self.puzzle.vertex_clues.remove(idx);
            }
            if !remove_indices.is_empty() {
                // Rebuild watchtower_vertices set
                self.watchtower_vertices = self
                    .puzzle
                    .vertex_clues
                    .iter()
                    .map(|cl| cl.vertex)
                    .collect();
            }
        }

        // Initialize pair layer for rose puzzles
        if self.rose_bits_all != 0 {
            self.pair_layer = Some(pair::CellPairLayer::new(
                self.grid.num_cells(),
                self.rose_bits_all,
                &self.cell_rose_sym,
            ));
        }

        if self.propagate().is_err() {
            return 0;
        }

        // Vertex-level watchtower config probing: for watchtower vertices,
        // enumerate valid Cut/Uncut configurations. If only one survives, force it.
        // Iterates until no more progress.
        if !self.puzzle.vertex_clues.is_empty() {
            let n_forced = self.probe_watchtower_vertex_configs();
            if n_forced > 0 {
                if self.propagate().is_err() {
                    return 0;
                }
            }
        }

        // Pre-search compass incompatibility: detect incompatible compass pairs
        // and force edge cuts or add manual_diffs before search begins.
        if self.has_compass_clue {
            let n_incompat = self.init_compass_incompatibility();
            if n_incompat > 0 {
                eprintln!("compass incompatibility: {} pairs forced DIFF", n_incompat);
                if self.propagate().is_err() {
                    return 0;
                }
            }
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
                    if rose.is_some() {
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
            self.progress.print(
                elapsed_secs,
                self.node_count,
                self.curr_unknown,
                self.total_unknown,
            );
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
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let header = match which {
            1 => format!("First solution found ({:.1}s):", elapsed),
            2 => format!("Second solution found ({:.1}s):", elapsed),
            _ => format!("Solution found ({:.1}s):", elapsed),
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
