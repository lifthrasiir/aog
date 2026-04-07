use super::Solver;
use crate::grid::Grid;
use crate::polyomino::{self, canonical, is_rectangular, Rotation};
use crate::types::*;
use std::collections::{HashSet, VecDeque};

/// Validate a parsed solution (edge states) against the puzzle rules and clues.
/// Creates a temporary solver, applies the edges, and runs the full validation.
pub fn validate_parsed_solution(
    puzzle: &Puzzle,
    grid: &Grid,
    pre_cut_edges: &[EdgeId],
    solution_edges: &[EdgeState],
) -> bool {
    let mut s = Solver::new(puzzle.clone(), grid.clone());
    for &e in pre_cut_edges {
        s.mark_pre_cut(e);
    }
    for e in 0..grid.num_edges() {
        if solution_edges[e] != EdgeState::Unknown {
            s.edges[e] = solution_edges[e];
        } else {
            let (c1, c2) = grid.edge_cells(e);
            if !grid.cell_exists[c1] || !grid.cell_exists[c2] {
                s.edges[e] = EdgeState::Cut;
            }
        }
    }
    let pieces = s.compute_pieces();
    s.validate(&pieces)
}

impl Solver {
    pub(crate) fn compute_pieces(&self) -> Vec<Piece> {
        let n = self.grid.num_cells();
        let mut comp = vec![usize::MAX; n];
        let mut num_pieces = 0usize;

        for c in 0..n {
            if !self.grid.cell_exists[c] || comp[c] != usize::MAX {
                continue;
            }
            comp[c] = num_pieces;
            let mut q = VecDeque::new();
            q.push_back(c);
            while let Some(cur) = q.pop_front() {
                for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                    let (c1, c2) = self.grid.edge_cells(eid);
                    let other = if c1 == cur { c2 } else { c1 };
                    if !self.grid.cell_exists[other] || comp[other] != usize::MAX {
                        continue;
                    }
                    if self.edges[eid] != EdgeState::Cut {
                        comp[other] = num_pieces;
                        q.push_back(other);
                    }
                }
            }
            num_pieces += 1;
        }

        let mut pieces = vec![Piece::default(); num_pieces];
        for c in 0..n {
            if !self.grid.cell_exists[c] || comp[c] == usize::MAX {
                continue;
            }
            pieces[comp[c]].cells.push(c);
        }
        for p in &mut pieces {
            p.area = p.cells.len();
            let sc: Vec<(i32, i32)> = p
                .cells
                .iter()
                .map(|&c| {
                    let (r, col) = self.grid.cell_pos(c);
                    (r as i32, col as i32)
                })
                .collect();
            p.canonical = canonical(&polyomino::make_shape(&sc));
        }
        pieces
    }

    pub(crate) fn validate(&self, pieces: &[Piece]) -> bool {
        if pieces.is_empty() {
            return false;
        }
        let n = self.grid.num_cells();
        let mut cell_piece = vec![usize::MAX; n];
        for (p, piece) in pieces.iter().enumerate() {
            for &c in &piece.cells {
                cell_piece[c] = p;
            }
        }

        if !self.validate_structure(pieces, &cell_piece) {
            return false;
        }
        if !self.validate_cell_clues(pieces, &cell_piece) {
            return false;
        }
        if !self.validate_rose_window(pieces) {
            return false;
        }
        if !self.validate_edge_clues(pieces, &cell_piece) {
            return false;
        }
        if !self.validate_vertex_clues(&cell_piece) {
            return false;
        }

        true
    }

    fn validate_structure(&self, pieces: &[Piece], cell_piece: &[usize]) -> bool {
        if pieces.iter().any(|p| p.cells.is_empty()) {
            return false;
        }
        if self
            .is_pre_cut
            .iter()
            .enumerate()
            .any(|(e, &pre)| pre && self.edges[e] != EdgeState::Cut)
        {
            return false;
        }

        // Pre-cut edge straddle check: no piece should have cells on
        // both sides of a pre-cut edge (even if connected indirectly).
        for e in 0..self.grid.num_edges() {
            if !self.is_pre_cut[e] {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            if cell_piece[c1] == cell_piece[c2] {
                return false;
            }
        }
        if self.edges.contains(&EdgeState::Unknown) {
            return false;
        }

        let rules = &self.puzzle.rules;
        for p in pieces {
            if let Some(min) = rules.minimum {
                if p.area < min {
                    return false;
                }
            }
            if let Some(max) = rules.maximum {
                if p.area > max {
                    return false;
                }
            }
            let rect = is_rectangular(p, &self.grid);
            if rules.boxy && !rect {
                return false;
            }
            if rules.non_boxy && rect {
                return false;
            }
        }
        if rules.match_all
            && pieces.len() > 1
            && !pieces
                .iter()
                .skip(1)
                .all(|p| p.canonical == pieces[0].canonical)
        {
            return false;
        }
        if rules.mismatch {
            for i in 0..pieces.len() {
                for j in (i + 1)..pieces.len() {
                    if pieces[i].canonical == pieces[j].canonical {
                        return false;
                    }
                }
            }
        }
        if rules.size_separation {
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let p1 = cell_piece[c1];
                let p2 = cell_piece[c2];
                if p1 != p2 && pieces[p1].area == pieces[p2].area {
                    return false;
                }
            }
        }
        if !rules.shape_bank.is_empty() {
            for p in pieces {
                let found = rules
                    .shape_bank
                    .iter()
                    .any(|bs| canonical(&p.canonical) == canonical(bs));
                if !found {
                    return false;
                }
            }
        }
        if rules.mingle_shape {
            for e in 0..self.grid.num_edges() {
                if self.edges[e] != EdgeState::Cut {
                    continue;
                }
                let (c1, c2) = self.grid.edge_cells(e);
                if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                    continue;
                }
                let p1 = cell_piece[c1];
                let p2 = cell_piece[c2];
                if p1 != p2 && pieces[p1].canonical != pieces[p2].canonical {
                    return false;
                }
            }
        }
        if rules.solitude {
            for p in pieces {
                let cnt = self
                    .puzzle
                    .cell_clues
                    .iter()
                    .filter(|cl| self.grid.cell_exists[cl.cell()] && p.cells.contains(&cl.cell()))
                    .count();
                if cnt != 1 {
                    return false;
                }
            }
        }
        true
    }

    fn validate_cell_clues(&self, pieces: &[Piece], cell_piece: &[usize]) -> bool {
        for clue in &self.puzzle.cell_clues {
            if !self.grid.cell_exists[clue.cell()] {
                return false;
            }
            let pid = cell_piece[clue.cell()];
            if pid == usize::MAX {
                return false;
            }
            let piece = &pieces[pid];
            match clue {
                CellClue::Area { value, .. } => {
                    if piece.area != *value {
                        return false;
                    }
                }
                CellClue::Rose { .. } => {}
                CellClue::Polyomino { shape, .. } => {
                    if canonical(&piece.canonical) != canonical(shape) {
                        return false;
                    }
                }
                CellClue::Palisade { cell, kind } => {
                    let mut num_cut = 0usize;
                    let mut cut_mask = 0u8;
                    for (k, eid) in self
                        .grid
                        .cell_edges(*cell)
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        let (c1, c2) = self.grid.edge_cells(eid);
                        let is_boundary = !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2];
                        if is_boundary || self.edges[eid] == EdgeState::Cut {
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
                CellClue::Compass { cell, compass } => {
                    let (cr, cc) = self.grid.cell_pos(*cell);
                    let (cr, cc) = (cr as isize, cc as isize);
                    let (mut nc, mut sc, mut ec, mut wc) = (0, 0, 0, 0);
                    for &c in &piece.cells {
                        let (pr, pc) = self.grid.cell_pos(c);
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
                    if let Some(e) = compass.e {
                        if e != ec {
                            return false;
                        }
                    }
                    if let Some(w) = compass.w {
                        if w != wc {
                            return false;
                        }
                    }
                    if let Some(s) = compass.s {
                        if s != sc {
                            return false;
                        }
                    }
                    if let Some(n) = compass.n {
                        if n != nc {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn validate_rose_window(&self, pieces: &[Piece]) -> bool {
        // Rose window: each piece must contain exactly one rose of each symbol
        let mut has_rose = [false; 8];
        for cl in &self.puzzle.cell_clues {
            if let CellClue::Rose { symbol, .. } = cl {
                has_rose[*symbol as usize] = true;
            }
        }
        for sym in 0..8u8 {
            if !has_rose[sym as usize] {
                continue;
            }
            for p in pieces {
                let cnt = p.cells.iter()
                    .filter(|&&c| {
                        self.puzzle.cell_clues.iter().any(|cl| {
                            matches!(cl, CellClue::Rose { symbol, cell, .. } if *symbol == sym && *cell == c)
                        })
                    })
                    .count();
                if cnt != 1 {
                    return false;
                }
            }
        }
        true
    }

    fn validate_edge_clues(&self, pieces: &[Piece], cell_piece: &[usize]) -> bool {
        for clue in &self.puzzle.edge_clues {
            if self.edges[clue.edge] != EdgeState::Cut {
                return false;
            }
            let (c1, c2) = self.grid.edge_cells(clue.edge);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                return false;
            }
            let p1 = cell_piece[c1];
            let p2 = cell_piece[c2];
            if p1 == p2 {
                return false;
            }
            let a1 = pieces[p1].area;
            let a2 = pieces[p2].area;
            match clue.kind {
                EdgeClueKind::Delta => {
                    if pieces[p1].canonical == pieces[p2].canonical {
                        return false;
                    }
                }
                EdgeClueKind::Gemini => {
                    if pieces[p1].canonical != pieces[p2].canonical {
                        return false;
                    }
                }
                EdgeClueKind::Inequality { smaller_first } => {
                    if smaller_first && a1 >= a2 {
                        return false;
                    }
                    if !smaller_first && a2 >= a1 {
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
        true
    }

    fn validate_vertex_clues(&self, cell_piece: &[usize]) -> bool {
        for clue in &self.puzzle.vertex_clues {
            let (vi, vj) = self.grid.vertex_pos(clue.vertex);
            let distinct: HashSet<_> = self
                .grid
                .vertex_cells(vi, vj)
                .into_iter()
                .flatten()
                .filter(|&cid| self.grid.cell_exists[cid] && cell_piece[cid] != usize::MAX)
                .map(|cid| cell_piece[cid])
                .collect();
            if distinct.len() != clue.value {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

    #[test]
    fn compute_pieces_all_uncut() {
        let input = "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
";
        let mut s = make_solver(input);
        // Mark all edges as Uncut (except boundaries which are pre-cut)
        for e in 0..s.grid.num_edges() {
            if s.edges[e] == EdgeState::Unknown {
                let _ = s.set_edge(e, EdgeState::Uncut);
            }
        }
        let pieces = s.compute_pieces();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].area, 4);
    }

    #[test]
    fn compute_pieces_all_cut() {
        let input = "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
";
        let mut s = make_solver(input);
        for e in 0..s.grid.num_edges() {
            if s.edges[e] == EdgeState::Unknown {
                let _ = s.set_edge(e, EdgeState::Cut);
            }
        }
        let pieces = s.compute_pieces();
        assert_eq!(pieces.len(), 4);
        assert!(pieces.iter().all(|p| p.area == 1));
    }
}
