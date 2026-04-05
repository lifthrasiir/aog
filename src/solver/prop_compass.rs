use super::Solver;
use crate::types::*;

impl Solver {
    pub(crate) fn propagate_compass(&mut self) -> Result<bool, ()> {
        // Collect compass clues upfront to avoid borrow conflicts with set_edge
        let entries: Vec<(CellId, CompassData)> = self
            .puzzle
            .cell_clues
            .iter()
            .filter_map(|cl| match cl {
                CellClue::Compass { cell, compass } => {
                    if self.grid.cell_exists[*cell] {
                        Some((*cell, compass.clone()))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        let mut progress = false;

        for (cell, compass) in &entries {
            let (r, c) = self.grid.cell_pos(*cell);

            for &(dr, dc, val) in &[
                (-1isize, 0, compass.n),
                (0, 1, compass.e),
                (1, 0, compass.s),
                (0, -1, compass.w),
            ] {
                let Some(v) = val else { continue };

                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0
                    || nr >= self.grid.rows as isize
                    || nc < 0
                    || nc >= self.grid.cols as isize
                {
                    continue; // v > 0 is fine: detour possible via other cells
                }

                let nid = self.grid.cell_id(nr as usize, nc as usize);
                if !self.grid.cell_exists[nid] {
                    continue; // v > 0 is fine: detour possible via other cells
                }

                let Some(edge) = self.grid.edge_between(*cell, nid) else {
                    continue;
                };

                if v == 0 {
                    // No cells in this direction: direct edge must be Cut
                    if self.edges[edge] == EdgeState::Unknown {
                        if !self.set_edge(edge, EdgeState::Cut) {
                            return Err(());
                        }
                        progress = true;
                    } else if self.edges[edge] != EdgeState::Cut {
                        return Err(());
                    }
                }
                // v > 0: cells can join via detour, so no edge constraint
            }
        }

        Ok(progress)
    }
}
