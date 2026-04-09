use super::super::{EdgeForcer, Solver};
use crate::polyomino::Rotation;
use crate::types::*;

impl Solver {
    pub(crate) fn propagate_palisade(&mut self) {
        let mut ef = EdgeForcer::new();
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
                        ef.force(eid, state);
                    }
                }
            }
        }
        let _ = ef.apply(self);
    }

    /// Full palisade propagation: enumerate compatible rotations and force edges
    /// where all compatible rotations agree on the state.
    pub(crate) fn propagate_palisade_constraints(&mut self) -> Result<bool, ()> {
        // First pass: collect all deductions
        let mut ef = EdgeForcer::new();
        let mut contradiction = false;

        for clue in &self.puzzle.cell_clues {
            let CellClue::Palisade { cell, kind } = clue else {
                continue;
            };
            if !self.grid.cell_exists[*cell] {
                continue;
            }

            let edges = self.grid.cell_edges(*cell).into_array();
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
                    ef.force_cut(eid);
                } else if !can_be_cut[k] && can_be_uncut[k] {
                    ef.force_uncut(eid);
                }
            }
        }

        if contradiction {
            return Err(());
        }

        // Second pass: apply deductions
        let progress = if ef.is_empty() {
            false
        } else {
            ef.apply(self)?
        };

        Ok(progress)
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
        s.puzzle.cell_clues.push(CellClue::Palisade {
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
        s.puzzle.cell_clues.push(CellClue::Palisade {
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
}
