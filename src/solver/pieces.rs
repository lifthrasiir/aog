use super::Solver;
use crate::dlx::Dlx;
use crate::polyomino::{canonical, Rotation};
use crate::types::*;
use std::collections::HashSet;

impl Solver {
    fn is_placement_valid(
        &self,
        cells: &[CellId],
        shape_idx: usize,
        cell_clues: &[Vec<&CellClue>],
        has_any_clue: &[bool],
        rose_symbols: &[u8],
    ) -> bool {
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
                if has_any_clue[cid] {
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
                if cell_clues[cid]
                    .iter()
                    .any(|cl| matches!(cl, CellClue::Rose { symbol, .. } if *symbol == sym))
                {
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
            for clue in &cell_clues[cid] {
                match clue {
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
                        let mut ec = 0;
                        let mut wc = 0;
                        let mut sc = 0;
                        let mut nc = 0;
                        for &ocid in cells {
                            let (pr, pc) = self.grid.cell_pos(ocid);
                            let dr = (pr as isize) - (cr as isize);
                            let dc = (pc as isize) - (cc as isize);
                            if dr == 0 && dc == 1 {
                                ec += 1;
                            } else if dr == 0 && dc == -1 {
                                wc += 1;
                            } else if dr == 1 && dc == 0 {
                                sc += 1;
                            } else if dr == -1 && dc == 0 {
                                nc += 1;
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

    fn generate_placements(&self) -> Vec<Piece> {
        let mut placements = Vec::new();

        let mut cell_clues: Vec<Vec<&CellClue>> = vec![vec![]; self.grid.num_cells()];
        let mut has_any_clue = vec![false; self.grid.num_cells()];
        let mut rose_symbols_set = HashSet::new();
        for clue in &self.puzzle.cell_clues {
            cell_clues[clue.cell()].push(clue);
            has_any_clue[clue.cell()] = true;
            if let CellClue::Rose { symbol, .. } = clue {
                rose_symbols_set.insert(*symbol);
            }
        }
        let rose_symbols: Vec<u8> = rose_symbols_set.into_iter().collect();

        for (si, transforms) in self.shape_transforms.iter().enumerate() {
            for transform in transforms {
                for r in 0..self.grid.rows {
                    for c in 0..self.grid.cols {
                        let mut cells = Vec::with_capacity(transform.len());
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
                            cells.push(cid);
                        }
                        if !valid {
                            continue;
                        }

                        cells.sort();

                        if self.is_placement_valid(
                            &cells,
                            si,
                            &cell_clues,
                            &has_any_clue,
                            &rose_symbols,
                        ) {
                            placements.push(Piece {
                                cells,
                                area: transform.len(),
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

            let mut solution = Vec::new();
            dlx.search(&mut solution, &mut |sol_rows| {
                let pieces: Vec<Piece> = sol_rows
                    .iter()
                    .map(|&idx| placements[idx].1.clone())
                    .collect();
                let old_edges = self.edges.clone();
                for piece in &pieces {
                    for &cid in &piece.cells {
                        for eid in grid.cell_edges(cid).into_iter().flatten() {
                            let (c1, c2) = grid.edge_cells(eid);
                            let other = if c1 == cid { c2 } else { c1 };
                            if !grid.cell_exists[other]
                                || piece.cells.binary_search(&other).is_err()
                            {
                                self.edges[eid] = EdgeState::Cut;
                            } else {
                                self.edges[eid] = EdgeState::Uncut;
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
                self.edges = old_edges;
                self.solution_count < 2
            });
        } else {
            let placements = self.generate_placements();
            for (i, p) in placements.iter().enumerate() {
                let mut cols: Vec<usize> = p.cells.iter().map(|&c| cell_to_col[c]).collect();
                cols.sort();
                dlx.add_row(i, &cols);
            }

            let mut solution = Vec::new();
            dlx.search(&mut solution, &mut |sol_rows| {
                let pieces: Vec<Piece> = sol_rows
                    .iter()
                    .map(|&idx| placements[idx].clone())
                    .collect();
                let old_edges = self.edges.clone();
                for piece in &pieces {
                    for &cid in &piece.cells {
                        for eid in grid.cell_edges(cid).into_iter().flatten() {
                            let (c1, c2) = grid.edge_cells(eid);
                            let other = if c1 == cid { c2 } else { c1 };
                            if !grid.cell_exists[other]
                                || piece.cells.binary_search(&other).is_err()
                            {
                                self.edges[eid] = EdgeState::Cut;
                            } else {
                                self.edges[eid] = EdgeState::Uncut;
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
                self.edges = old_edges;
                self.solution_count < 2
            });
        }
    }
}
