use super::Solver;
use crate::polyomino::{self, canonical};
use crate::types::*;
use std::collections::HashSet;

impl Solver {
    /// Mingle, gemini, delta, mismatch shape-based constraints.
    pub(crate) fn propagate_shape_constraints(&mut self, num_comp: usize) -> Result<(), ()> {
        // Compute canonical shapes for sealed components (shared by mingle_shape, gemini & mismatch)
        let has_mingle = self.puzzle.rules.mingle_shape;
        let has_gemini = self
            .puzzle
            .edge_clues
            .iter()
            .any(|cl| matches!(cl.kind, EdgeClueKind::Gemini));
        let has_mismatch = self.puzzle.rules.mismatch;
        let has_delta = self
            .puzzle
            .edge_clues
            .iter()
            .any(|cl| matches!(cl.kind, EdgeClueKind::Delta));

        if has_mingle || has_gemini || has_mismatch || has_delta {
            let sealed: Vec<usize> = self.sealed(num_comp).collect();
            let mut comp_shape: Vec<Option<Shape>> = vec![None; num_comp];
            for ci in sealed {
                let at_limit = match self.curr_target_area[ci] {
                    Some(t) => self.curr_comp_sz[ci] == t,
                    None => true,
                };
                if !at_limit {
                    continue;
                }
                let cells: Vec<(i32, i32)> = self.comp_cells[ci]
                    .iter()
                    .map(|&c| {
                        let (r, col) = self.grid.cell_pos(c);
                        (r as i32, col as i32)
                    })
                    .collect();
                comp_shape[ci] = Some(canonical(&polyomino::make_shape(&cells)));
            }

            // Mingle shape: adjacent pieces must have the same canonical shape.
            if has_mingle {
                let mut mingle_required_size: Vec<Option<usize>> = vec![None; num_comp];

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

                    // Both sides have known shapes: verify they match
                    match (&comp_shape[ci1], &comp_shape[ci2]) {
                        (Some(s1), Some(s2)) if s1 != s2 => return Err(()),
                        _ => {}
                    }

                    // One side sealed: propagate size constraint to the other
                    let sealed1 = self.is_sealed(ci1);
                    let sealed2 = self.is_sealed(ci2);
                    if sealed1 && sealed2 {
                        continue;
                    }
                    if !sealed1 && !sealed2 {
                        continue;
                    }

                    let (sealed_ci, other_ci) = if sealed1 { (ci1, ci2) } else { (ci2, ci1) };
                    let sealed_sz = self.curr_comp_sz[sealed_ci];

                    // Check for conflicting mingle size requirements
                    if let Some(prev) = mingle_required_size[other_ci] {
                        if prev != sealed_sz {
                            return Err(());
                        }
                    }
                    mingle_required_size[other_ci] = Some(sealed_sz);

                    // If other side has a target area, it must match
                    if let Some(target) = self.curr_target_area[other_ci] {
                        if target != sealed_sz {
                            return Err(());
                        }
                        continue;
                    }

                    // No target on other side: check size compatibility.
                    let other_sz = self.curr_comp_sz[other_ci];
                    if other_sz > sealed_sz {
                        return Err(());
                    }
                }
            }

            // Gemini edge clues: adjacent pieces must have the same canonical shape.
            if has_gemini {
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

                    let sealed1 = self.is_sealed(ci1);
                    let sealed2 = self.is_sealed(ci2);

                    // Both sealed: check canonical shapes match
                    if sealed1 && sealed2 {
                        match (&comp_shape[ci1], &comp_shape[ci2]) {
                            (Some(s1), Some(s2)) if s1 != s2 => return Err(()),
                            _ => {}
                        }
                        continue;
                    }

                    // Both still growing: cannot determine final shapes yet
                    if !sealed1 && !sealed2 {
                        continue;
                    }

                    // Exactly one side sealed: propagate size constraint to the other
                    let (sealed_ci, other_ci) = if sealed1 { (ci1, ci2) } else { (ci2, ci1) };
                    let sealed_sz = self.curr_comp_sz[sealed_ci];
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
                    let other_sz = self.curr_comp_sz[other_ci];
                    if other_sz > sealed_sz {
                        return Err(());
                    }
                }
            }

            // Delta edge clues: adjacent pieces must have different canonical shapes.
            if has_delta {
                for clue in &self.puzzle.edge_clues {
                    if !matches!(clue.kind, EdgeClueKind::Delta) {
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

                    match (&comp_shape[ci1], &comp_shape[ci2]) {
                        (Some(s1), Some(s2)) if s1 == s2 => return Err(()),
                        _ => {}
                    }
                }
            }

            // Mismatch: all pieces must have distinct canonical shapes.
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
                for ci in self.growing(num_comp).collect::<Vec<_>>() {
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
                            let all_shapes = polyomino::enumerate_free_polyominoes(target);
                            any_available = all_shapes.iter().any(|s| !taken_shapes.contains(s));
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;
    use crate::types::{CellClue, EdgeClue, EdgeClueKind};

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
        let mut s = make_gemini_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn gemini_both_sealed_different_shape_err() {
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
        let mut s = make_gemini_solver_2x3(&[0]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 2), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
        assert_eq!(
            s.edges[s.grid.h_edge(0, 1)],
            EdgeState::Unknown,
            "gemini: should not force Cut on growth edges for size-matched growing component"
        );
    }

    #[test]
    fn gemini_conflicting_sizes_from_two_edges_err() {
        let mut s = make_gemini_solver_2x3(&[0, 1]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "gemini: conflicting size requirements (1 vs 2) should be contradiction"
        );
    }

    /// Helper: create a 2x2 solver with mingle_shape rule and Cut on v_edge(0,0).
    fn make_mingle_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        s.puzzle.rules.mingle_shape = true;
        let ge = s.grid.v_edge(0, 0);
        let _ = s.set_edge(ge, EdgeState::Cut);
        s
    }

    /// Helper: create a 2x3 solver with mingle_shape rule and Cuts on specified v_edge columns.
    fn make_mingle_solver_2x3(cols: &[usize]) -> Solver {
        let mut s = make_solver(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        );
        s.puzzle.rules.mingle_shape = true;
        for &c in cols {
            let ge = s.grid.v_edge(0, c);
            let _ = s.set_edge(ge, EdgeState::Cut);
        }
        s
    }

    #[test]
    fn mingle_both_sealed_same_shape_ok() {
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn mingle_both_sealed_different_shape_err() {
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: domino vs monomino should be contradiction"
        );
    }

    #[test]
    fn mingle_one_sealed_size_exceeds_other_err() {
        let mut s = make_mingle_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: sealed monomino (1) vs growing domino (2) should be contradiction"
        );
    }

    #[test]
    fn mingle_one_sealed_size_conflicts_target_err() {
        let mut s = make_mingle_solver_2x2();
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
            "mingle: sealed size 1 vs target area 3 should be contradiction"
        );
    }

    #[test]
    fn mingle_sealed_same_size_no_force_cut() {
        let mut s = make_mingle_solver_2x3(&[0]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(0, 2), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
        assert_eq!(
            s.edges[s.grid.h_edge(0, 1)],
            EdgeState::Unknown,
            "mingle: should not force Cut on growth edges for size-matched growing component"
        );
    }

    #[test]
    fn mingle_conflicting_sizes_from_two_edges_err() {
        let mut s = make_mingle_solver_2x3(&[0, 1]);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 2), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mingle: conflicting size requirements (1 vs 2) should be contradiction"
        );
    }

    /// Helper: create a 2x2 solver with delta clue on v_edge(0,0).
    fn make_delta_solver_2x2() -> Solver {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let de = s.grid.v_edge(0, 0);
        let _ = s.set_edge(de, EdgeState::Cut);
        s.puzzle.edge_clues.push(EdgeClue {
            edge: de,
            kind: EdgeClueKind::Delta,
        });
        s
    }

    #[test]
    fn delta_both_sealed_different_shapes_ok() {
        let mut s = make_delta_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(s.propagate_area_bounds().is_ok());
    }

    #[test]
    fn delta_both_sealed_same_shape_err() {
        let mut s = make_delta_solver_2x2();
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "delta: same shape (monomino vs monomino) should be contradiction"
        );
    }

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
        let mut s = make_mismatch_solver_2x2();
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
        let mut s = make_mismatch_solver_2x2();
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Uncut);
        let _ = s.set_edge(s.grid.v_edge(1, 0), EdgeState::Uncut);

        assert!(
            s.propagate_area_bounds().is_ok(),
            "mismatch: monomino + L-triomino should be valid"
        );
    }

    #[test]
    fn mismatch_growing_no_available_shape_shape_bank() {
        use crate::polyomino::get_named_shape;
        let mut s = make_mismatch_solver_2x2();
        s.puzzle
            .rules
            .shape_bank
            .push(get_named_shape("o").unwrap());
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: s.grid.cell_id(0, 1),
            value: 1,
        });
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mismatch: only one shape in bank of size 1, already taken → contradiction"
        );
    }

    #[test]
    fn mismatch_growing_available_shape_shape_bank() {
        use crate::polyomino::get_named_shape;
        let mut s = make_mismatch_solver_2x2();
        s.puzzle
            .rules
            .shape_bank
            .push(get_named_shape("o").unwrap());
        s.puzzle
            .rules
            .shape_bank
            .push(get_named_shape("oo").unwrap());
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        s.puzzle.cell_clues.push(CellClue::Area {
            cell: s.grid.cell_id(0, 1),
            value: 2,
        });

        assert!(
            s.propagate_area_bounds().is_ok(),
            "mismatch: domino still available for size 2 target"
        );
    }

    #[test]
    fn mismatch_no_shape_bank_small_size_exhausted() {
        let mut s = make_mismatch_solver_2x2();
        let _ = s.set_edge(s.grid.v_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 0), EdgeState::Cut);
        let _ = s.set_edge(s.grid.h_edge(0, 1), EdgeState::Cut);

        assert!(
            s.propagate_area_bounds().is_err(),
            "mismatch: two monominoes with no shape bank → only 1 shape of size 1 → err"
        );
    }
}
