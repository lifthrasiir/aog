use super::Solver;
use crate::polyomino::Rotation;
use crate::types::*;
use std::collections::{HashMap, VecDeque};

impl Solver {
    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            if self.puzzle.rules.bricky || self.puzzle.rules.loopy {
                match self.propagate_bricky_loopy() {
                    Ok(p) => progress |= p,
                    Err(()) => return Err(()),
                }
            }
            match self.propagate_area_bounds() {
                Ok(p) => progress |= p,
                Err(()) => return Err(()),
            }
            match self.propagate_palisade_constraints() {
                Ok(p) => progress |= p,
                Err(()) => return Err(()),
            }

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
        let mut comp = vec![usize::MAX; n];

        for c in 0..n {
            if !self.grid.cell_exists[c] || comp[c] != usize::MAX {
                continue;
            }
            self.flood_fill_decided(c, &mut comp);
        }

        let mut comp_map: HashMap<usize, usize> = HashMap::new();
        let mut num_comp = 0usize;
        let mut comp_id = vec![usize::MAX; n];
        for c in 0..n {
            if !self.grid.cell_exists[c] || comp[c] == usize::MAX {
                continue;
            }
            let id = *comp_map.entry(comp[c]).or_insert_with(|| {
                let id = num_comp;
                num_comp += 1;
                id
            });
            comp_id[c] = id;
        }

        let mut comp_sz = vec![0usize; num_comp];
        let mut comp_clues = vec![Vec::new(); num_comp];
        for c in 0..n {
            if self.grid.cell_exists[c] {
                let ci = comp_id[c];
                comp_sz[ci] += 1;
                for clue in &self.puzzle.cell_clues {
                    if clue.cell() == c {
                        comp_clues[ci].push(clue);
                    }
                }
            }
        }

        let mut target_area = vec![None; num_comp];
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

            // Still check for consistent areas if multiple clues present
            if !areas.is_empty() {
                let a0 = areas[0];
                if areas.iter().any(|&a| a != a0) {
                    return Err(());
                }
                target_area[ci] = Some(a0);
                if comp_sz[ci] > a0 {
                    return Err(());
                }
            } else if comp_sz[ci] > self.eff_max_area {
                return Err(());
            }
        }

        // Update cache for select_edge
        self.curr_comp_id = comp_id.clone();
        self.curr_comp_sz = comp_sz.clone();
        self.curr_target_area = target_area.clone();

        // Check Unknown edges to outside
        let mut can_grow = vec![false; num_comp];
        let mut growth_edges = vec![Vec::new(); num_comp];

        for e in 0..self.grid.num_edges() {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }
            let (c1, c2) = self.grid.edge_cells(e);
            if !self.grid.cell_exists[c1] || !self.grid.cell_exists[c2] {
                continue;
            }
            let ci1 = comp_id[c1];
            let ci2 = comp_id[c2];
            if ci1 != ci2 {
                let cannot_merge = if self.puzzle.rules.solitude {
                    target_area[ci1].is_some() && target_area[ci2].is_some()
                } else {
                    if let (Some(a1), Some(a2)) = (target_area[ci1], target_area[ci2]) {
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

                can_grow[ci1] = true;
                can_grow[ci2] = true;
                growth_edges[ci1].push(e);
                growth_edges[ci2].push(e);

                let limit1 = target_area[ci1].unwrap_or(self.eff_max_area);
                let limit2 = target_area[ci2].unwrap_or(self.eff_max_area);

                if comp_sz[ci1] >= limit1 || comp_sz[ci2] >= limit2 {
                    if !self.set_edge(e, EdgeState::Cut) {
                        return Err(());
                    }
                    progress = true;
                }
            }
        }

        for ci in 0..num_comp {
            if let Some(target) = target_area[ci] {
                if comp_sz[ci] < target && !can_grow[ci] {
                    return Err(());
                }
                if comp_sz[ci] == target && can_grow[ci] {
                    for &e in &growth_edges[ci] {
                        if !self.set_edge(e, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    }
                }
            } else if self.eff_min_area > 1 && comp_sz[ci] < self.eff_min_area && !can_grow[ci] {
                return Err(());
            }
        }

        // Non-boxy / boxy: check fully-formed components
        if self.puzzle.rules.non_boxy || self.puzzle.rules.boxy {
            for ci in 0..num_comp {
                if can_grow[ci] {
                    continue;
                }
                let mut min_r = self.grid.rows;
                let mut max_r = 0usize;
                let mut min_c = self.grid.cols;
                let mut max_c = 0usize;
                let mut cell_count = 0usize;
                for c in 0..n {
                    if self.grid.cell_exists[c] && comp_id[c] == ci {
                        let (r, col) = self.grid.cell_pos(c);
                        min_r = min_r.min(r);
                        max_r = max_r.max(r);
                        min_c = min_c.min(col);
                        max_c = max_c.max(col);
                        cell_count += 1;
                    }
                }
                if cell_count == 0 {
                    continue;
                }
                let is_rect = cell_count == (max_r - min_r + 1) * (max_c - min_c + 1);
                if self.puzzle.rules.non_boxy && is_rect {
                    return Err(());
                }
                if self.puzzle.rules.boxy && !is_rect {
                    return Err(());
                }
            }
        }

        // Inequality propagation for Cut edges between known components
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
            let ci1 = comp_id[c1];
            let ci2 = comp_id[c2];
            if ci1 == ci2 {
                continue;
            }
            let (smaller_ci, larger_ci) = if smaller_first {
                (ci1, ci2)
            } else {
                (ci2, ci1)
            };

            let smaller_done = !can_grow[smaller_ci];
            let larger_done = !can_grow[larger_ci];

            // Both fully formed: directly compare
            if smaller_done && larger_done {
                if comp_sz[smaller_ci] >= comp_sz[larger_ci] {
                    return Err(());
                }
                continue;
            }

            // Larger piece fully formed but too small
            if larger_done && comp_sz[larger_ci] <= comp_sz[smaller_ci] {
                return Err(());
            }

            // Smaller piece fully formed but too large
            if smaller_done {
                let max_larger = target_area[larger_ci].unwrap_or(self.eff_max_area);
                if comp_sz[smaller_ci] >= max_larger {
                    return Err(());
                }
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

    pub(crate) fn flood_fill_decided(&self, start: CellId, comp: &mut [usize]) {
        comp[start] = start;
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(cur) = q.pop_front() {
            for eid in self.grid.cell_edges(cur).into_iter().flatten() {
                let (c1, c2) = self.grid.edge_cells(eid);
                let other = if c1 == cur { c2 } else { c1 };
                if !self.grid.cell_exists[other] || comp[other] != usize::MAX {
                    continue;
                }
                if self.edges[eid] == EdgeState::Uncut {
                    comp[other] = start;
                    q.push_back(other);
                }
            }
        }
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
        s.puzzle
            .cell_clues
            .push(CellClue::Palisade {
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
        s.puzzle
            .cell_clues
            .push(CellClue::Palisade {
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
        assert!(result.is_err(), "bricky: 4 cut edges at vertex should be contradiction");
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

        let mut comp = vec![usize::MAX; s.grid.num_cells()];
        s.flood_fill_decided(s.grid.cell_id(0, 0), &mut comp);

        // Cell (0,0) and (0,1) should have same component id
        assert_eq!(comp[s.grid.cell_id(0, 0)], comp[s.grid.cell_id(0, 1)]);
        // Cell (1,0) should be in a different component
        assert_ne!(comp[s.grid.cell_id(0, 0)], comp[s.grid.cell_id(1, 0)]);
    }
}
