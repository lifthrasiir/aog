use super::Solver;
use crate::dlx::Dlx;
use crate::polyomino::{canonical, Rotation};
use crate::types::*;
use std::collections::HashSet;

impl Solver {
    fn is_placement_valid(&self, cells: &[CellId], shape_idx: usize, rose_symbols: &[u8]) -> bool {
        // Internal edges must not be Cut or Pre-cut
        for i in 0..cells.len() {
            for j in (i + 1)..cells.len() {
                if let Some(e) = self.grid.edge_between(cells[i], cells[j]) {
                    if self.is_pre_cut[e] || self.edges[e] == EdgeState::Cut {
                        return false;
                    }
                }
            }
        }

        // Boundary edges must not be Uncut
        for &cid in cells {
            for eid in self.grid.cell_edges(cid).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == cid { c2 } else { c1 };
                if !self.grid.cell_exists[other] || cells.binary_search(&other).is_err() {
                    if self.edges[eid] == EdgeState::Uncut {
                        return false;
                    }
                }
            }
        }

        // Solitude rule: exactly one cell with any clue
        if self.puzzle.rules.solitude {
            let mut clue_count = 0;
            for &cid in cells {
                if self.has_any_clue[cid] {
                    clue_count += 1;
                }
            }
            if clue_count != 1 {
                return false;
            }
        }

        // Rose window rule: exactly one of each symbol present in the puzzle
        for &sym in rose_symbols {
            let mut found = false;
            for &cid in cells {
                let has_sym = self.cell_clues_indexed[cid].iter().any(|&idx| {
                    if let CellClue::Rose { symbol, .. } = &self.puzzle.cell_clues[idx] {
                        *symbol == sym
                    } else {
                        false
                    }
                });
                if has_sym {
                    if found {
                        return false;
                    } // already found one
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }

        // Local cell clues
        for &cid in cells {
            for &idx in &self.cell_clues_indexed[cid] {
                match &self.puzzle.cell_clues[idx] {
                    CellClue::Area { value, .. } => {
                        if cells.len() != *value {
                            return false;
                        }
                    }
                    CellClue::Polyomino { shape, .. } => {
                        if self.shape_bank_canonicals[shape_idx].cells != canonical(shape).cells {
                            return false;
                        }
                    }
                    CellClue::Palisade { kind, .. } => {
                        let mut num_cut = 0usize;
                        let mut cut_mask = 0u8;
                        for (k, eid) in self.grid.cell_edges(cid).into_iter().flatten().enumerate()
                        {
                            let (c1, c2) = self.grid.edge_cells(eid);
                            let other = if c1 == cid { c2 } else { c1 };
                            if !self.grid.cell_exists[other] || cells.binary_search(&other).is_err()
                            {
                                num_cut += 1;
                                cut_mask |= 1 << k;
                            }
                        }
                        let valid = Rotation::all().iter().any(|rot| {
                            let (ec, em) = kind.pattern_at_rotation(rot.index());
                            num_cut == ec && (cut_mask & em) == em
                        });
                        if !valid {
                            return false;
                        }
                    }
                    CellClue::Compass { compass, .. } => {
                        let (cr, cc) = self.grid.cell_pos(cid);
                        let (cr, cc) = (cr as isize, cc as isize);
                        let (mut nc, mut sc, mut ec, mut wc) = (0, 0, 0, 0);
                        for &ocid in cells {
                            let (pr, pc) = self.grid.cell_pos(ocid);
                            let dr = pr as isize - cr;
                            let dc = pc as isize - cc;
                            if dr < 0 {
                                nc += 1;
                            }
                            if dr > 0 {
                                sc += 1;
                            }
                            if dc > 0 {
                                ec += 1;
                            }
                            if dc < 0 {
                                wc += 1;
                            }
                        }
                        if let Some(v) = compass.e {
                            if v != ec {
                                return false;
                            }
                        }
                        if let Some(v) = compass.w {
                            if v != wc {
                                return false;
                            }
                        }
                        if let Some(v) = compass.s {
                            if v != sc {
                                return false;
                            }
                        }
                        if let Some(v) = compass.n {
                            if v != nc {
                                return false;
                            }
                        }
                    }
                    CellClue::Rose { .. } => {}
                }
            }
        }

        true
    }

    fn generate_placements(&self, cell_min: &[usize], cell_max: &[usize]) -> Vec<Piece> {
        let mut placements = Vec::new();

        let mut rose_symbols_set = HashSet::new();
        for clue in &self.puzzle.cell_clues {
            if let CellClue::Rose { symbol, .. } = clue {
                rose_symbols_set.insert(*symbol);
            }
        }
        let rose_symbols: Vec<u8> = rose_symbols_set.into_iter().collect();

        for (si, transforms) in self.shape_transforms.iter().enumerate() {
            for transform in transforms {
                let area = transform.len();
                for r in 0..self.grid.rows {
                    for c in 0..self.grid.cols {
                        let mut cells = Vec::with_capacity(area);
                        let mut valid = true;
                        for &(dr, dc) in transform {
                            let nr = r as isize + dr;
                            let nc = c as isize + dc;
                            if nr < 0
                                || nr >= self.grid.rows as isize
                                || nc < 0
                                || nc >= self.grid.cols as isize
                            {
                                valid = false;
                                break;
                            }
                            let cid = self.grid.cell_id(nr as usize, nc as usize);
                            if !self.grid.cell_exists[cid] {
                                valid = false;
                                break;
                            }
                            // Area bounds check from inequality constraints
                            if area < cell_min[cid] || area > cell_max[cid] {
                                valid = false;
                                break;
                            }
                            cells.push(cid);
                        }
                        if !valid {
                            continue;
                        }

                        cells.sort();

                        if self.is_placement_valid(&cells, si, &rose_symbols) {
                            placements.push(Piece {
                                cells,
                                area,
                                canonical: self.shape_bank_canonicals[si].clone(),
                            });
                        }
                    }
                }
            }
        }

        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for p in placements {
            if seen.insert(p.cells.clone()) {
                unique.push(p);
            }
        }
        unique
    }

    pub(crate) fn backtrack_pieces(&mut self) {
        if self.solution_count >= 2 {
            return;
        }

        self.node_count += 1;
        self.report_progress();

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

        let use_clue_mode =
            self.puzzle.rules.shape_bank.is_empty() && total_clue_area == self.total_cells;

        let num_cells = self.total_cells;
        let num_clues = if use_clue_mode {
            self.puzzle.cell_clues.len()
        } else {
            0
        };

        let mut cell_to_col = vec![usize::MAX; self.grid.num_cells()];
        let mut active_count = 0;
        for c in 0..self.grid.num_cells() {
            if self.grid.cell_exists[c] {
                cell_to_col[c] = active_count;
                active_count += 1;
            }
        }

        let mut dlx = Dlx::new(num_cells + num_clues);
        let grid = self.grid.clone();

        if use_clue_mode {
            let placements = self.generate_clue_placements();
            for (i, (clue_idx, p)) in placements.iter().enumerate() {
                let mut cols: Vec<usize> = p.cells.iter().map(|&c| cell_to_col[c]).collect();
                cols.push(num_cells + clue_idx);
                cols.sort();
                dlx.add_row(i, &cols);
            }

            let mut cell_to_piece = vec![usize::MAX; num_cells];
            let mut solution = Vec::new();
            dlx.search(&mut solution, &mut |sol_rows| {
                let snap = self.changed.len();
                let pieces: Vec<Piece> = sol_rows
                    .iter()
                    .enumerate()
                    .map(|(pi, &idx)| {
                        let p = placements[idx].1.clone();
                        for &cid in &p.cells {
                            cell_to_piece[cid] = pi;
                        }
                        p
                    })
                    .collect();

                // Temporary set edges based on pieces for validation
                for piece in &pieces {
                    for &cid in &piece.cells {
                        for eid in grid.cell_edges(cid).into_iter().flatten() {
                            let (c1, c2) = grid.edge_cells(eid);
                            let other = if c1 == cid { c2 } else { c1 };
                            if !grid.cell_exists[other]
                                || cell_to_piece[other] != cell_to_piece[cid]
                            {
                                self.set_edge(eid, EdgeState::Cut);
                            } else {
                                self.set_edge(eid, EdgeState::Uncut);
                            }
                        }
                    }
                }

                if self.validate(&pieces) {
                    self.solution_count += 1;
                    self.best_pieces = pieces;
                    self.best_edges = self.edges.clone();
                    self.report_solution(self.solution_count);
                }
                self.restore(snap);
                self.solution_count < 2
            });
        } else {
            // Compute cell area bounds from inequality constraints (arc consistency)
            let n = self.grid.num_cells();
            let mut cell_min = vec![self.eff_min_area; n];
            let mut cell_max = vec![self.eff_max_area; n];

            let mut ineq_pairs: Vec<(CellId, CellId)> = Vec::new();
            for cl in &self.puzzle.edge_clues {
                if let EdgeClueKind::Inequality { smaller_first } = cl.kind {
                    let (c1, c2) = self.grid.edge_cells(cl.edge);
                    if self.grid.cell_exists[c1] && self.grid.cell_exists[c2] {
                        if smaller_first {
                            ineq_pairs.push((c1, c2)); // area(c1) < area(c2)
                        } else {
                            ineq_pairs.push((c2, c1)); // area(c2) < area(c1)
                        }
                    }
                }
            }

            if !ineq_pairs.is_empty() {
                let mut changed = true;
                while changed {
                    changed = false;
                    for &(small, large) in &ineq_pairs {
                        let new_max_small = cell_max[large].saturating_sub(1);
                        if cell_max[small] > new_max_small {
                            cell_max[small] = new_max_small;
                            changed = true;
                        }
                        let new_min_large = cell_min[small].saturating_add(1);
                        if cell_min[large] < new_min_large {
                            cell_min[large] = new_min_large;
                            changed = true;
                        }
                    }
                }
                eprintln!(
                    "inequality bounds: {} constraints, narrowed {} cells",
                    ineq_pairs.len(),
                    (0..n)
                        .filter(|&c| self.grid.cell_exists[c]
                            && (cell_min[c] != self.eff_min_area
                                || cell_max[c] != self.eff_max_area))
                        .count()
                );
            }

            let placements = self.generate_placements(&cell_min, &cell_max);
            for (i, p) in placements.iter().enumerate() {
                let mut cols: Vec<usize> = p.cells.iter().map(|&c| cell_to_col[c]).collect();
                cols.sort();
                dlx.add_row(i, &cols);
            }

            // Check if incremental edge-clue checking is beneficial
            let has_edge_constraints = self.puzzle.edge_clues.iter().any(|cl| {
                matches!(
                    cl.kind,
                    EdgeClueKind::Inequality { .. }
                        | EdgeClueKind::Delta
                        | EdgeClueKind::Gemini
                        | EdgeClueKind::Diff { .. }
                )
            }) || self.puzzle.rules.size_separation
                || self.puzzle.rules.mingle_shape;

            if has_edge_constraints {
                eprintln!(
                    "piece-based search with incremental edge-clue check ({} placements)",
                    placements.len()
                );

                // Pre-compute edge constraint pairs: (cell1, cell2, kind)
                let edge_constraints: Vec<(CellId, CellId, EdgeClueKind)> = self
                    .puzzle
                    .edge_clues
                    .iter()
                    .filter_map(|cl| {
                        let (c1, c2) = self.grid.edge_cells(cl.edge);
                        if self.grid.cell_exists[c1] && self.grid.cell_exists[c2] {
                            Some((c1, c2, cl.kind))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Pre-compute all adjacent cell pairs for size_separation/mingle_shape checks
                let adjacent_pairs: Vec<(CellId, CellId)> = (0..self.grid.num_edges())
                    .filter_map(|e| {
                        let (c1, c2) = self.grid.edge_cells(e);
                        if self.grid.cell_exists[c1] && self.grid.cell_exists[c2] {
                            Some((c1, c2))
                        } else {
                            None
                        }
                    })
                    .collect();

                let num_cells_total = self.grid.num_cells();
                let mut cell_to_piece = vec![usize::MAX; num_cells_total];
                let check_size_sep = self.puzzle.rules.size_separation;
                let check_mingle = self.puzzle.rules.mingle_shape;

                let mut row_check = |solution: &[usize]| -> bool {
                    // Rebuild cell→piece mapping from solution
                    cell_to_piece.fill(usize::MAX);
                    for (pi, &row_id) in solution.iter().enumerate() {
                        for &c in &placements[row_id].cells {
                            cell_to_piece[c] = pi;
                        }
                    }
                    // Check each edge constraint where both cells are assigned
                    for &(c1, c2, kind) in &edge_constraints {
                        let p1 = cell_to_piece[c1];
                        let p2 = cell_to_piece[c2];
                        if p1 == usize::MAX || p2 == usize::MAX {
                            continue;
                        }
                        let a1 = placements[solution[p1]].area;
                        let a2 = placements[solution[p2]].area;
                        let s1 = &placements[solution[p1]].canonical;
                        let s2 = &placements[solution[p2]].canonical;
                        match kind {
                            EdgeClueKind::Inequality { smaller_first } => {
                                if smaller_first && a1 >= a2 {
                                    return false;
                                }
                                if !smaller_first && a2 >= a1 {
                                    return false;
                                }
                            }
                            EdgeClueKind::Delta => {
                                if s1 == s2 {
                                    return false;
                                }
                            }
                            EdgeClueKind::Gemini => {
                                if s1 != s2 {
                                    return false;
                                }
                            }
                            EdgeClueKind::Diff { value } => {
                                if a1.abs_diff(a2) != value {
                                    return false;
                                }
                            }
                        }
                    }

                    // Size separation: adjacent pieces must have different areas
                    if check_size_sep {
                        for &(c1, c2) in &adjacent_pairs {
                            let p1 = cell_to_piece[c1];
                            let p2 = cell_to_piece[c2];
                            if p1 == usize::MAX || p2 == usize::MAX || p1 == p2 {
                                continue;
                            }
                            if placements[solution[p1]].area == placements[solution[p2]].area {
                                return false;
                            }
                        }
                    }

                    // Mingle shape: adjacent pieces must have same shape
                    if check_mingle {
                        for &(c1, c2) in &adjacent_pairs {
                            let p1 = cell_to_piece[c1];
                            let p2 = cell_to_piece[c2];
                            if p1 == usize::MAX || p2 == usize::MAX || p1 == p2 {
                                continue;
                            }
                            if placements[solution[p1]].canonical != placements[solution[p2]].canonical {
                                return false;
                            }
                        }
                    }

                    true
                };

                let mut cell_to_piece_final = vec![usize::MAX; num_cells_total];
                let mut solution = Vec::new();
                dlx.search_with_check(&mut solution, &mut row_check, &mut |sol_rows| {
                    let snap = self.changed.len();
                    let pieces: Vec<Piece> = sol_rows
                        .iter()
                        .enumerate()
                        .map(|(pi, &idx)| {
                            let p = placements[idx].clone();
                            for &cid in &p.cells {
                                cell_to_piece_final[cid] = pi;
                            }
                            p
                        })
                        .collect();

                    // Temporary set edges based on pieces for validation
                    for piece in &pieces {
                        for &cid in &piece.cells {
                            for eid in grid.cell_edges(cid).into_iter().flatten() {
                                let (c1, c2) = grid.edge_cells(eid);
                                let other = if c1 == cid { c2 } else { c1 };
                                if !grid.cell_exists[other]
                                    || cell_to_piece_final[other] != cell_to_piece_final[cid]
                                {
                                    self.set_edge(eid, EdgeState::Cut);
                                } else {
                                    self.set_edge(eid, EdgeState::Uncut);
                                }
                            }
                        }
                    }

                    if self.validate(&pieces) {
                        self.solution_count += 1;
                        self.best_pieces = pieces;
                        self.best_edges = self.edges.clone();
                        self.report_solution(self.solution_count);
                    }
                    self.restore(snap);
                    self.solution_count < 2
                });
            } else {
                let mut cell_to_piece_simple = vec![usize::MAX; num_cells];
                let mut solution = Vec::new();
                dlx.search(&mut solution, &mut |sol_rows| {
                    let snap = self.changed.len();
                    let pieces: Vec<Piece> = sol_rows
                        .iter()
                        .enumerate()
                        .map(|(pi, &idx)| {
                            let p = placements[idx].clone();
                            for &cid in &p.cells {
                                cell_to_piece_simple[cid] = pi;
                            }
                            p
                        })
                        .collect();

                    // Temporary set edges based on pieces for validation
                    for piece in &pieces {
                        for &cid in &piece.cells {
                            for eid in grid.cell_edges(cid).into_iter().flatten() {
                                let (c1, c2) = grid.edge_cells(eid);
                                let other = if c1 == cid { c2 } else { c1 };
                                if !grid.cell_exists[other]
                                    || cell_to_piece_simple[other] != cell_to_piece_simple[cid]
                                {
                                    self.set_edge(eid, EdgeState::Cut);
                                } else {
                                    self.set_edge(eid, EdgeState::Uncut);
                                }
                            }
                        }
                    }

                    if self.validate(&pieces) {
                        self.solution_count += 1;
                        self.best_pieces = pieces;
                        self.best_edges = self.edges.clone();
                        self.report_solution(self.solution_count);
                    }
                    self.restore(snap);
                    self.solution_count < 2
                });
            }
        }
    }
}
