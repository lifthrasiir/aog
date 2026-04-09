use super::{Snapshot, Solver};
use crate::types::*;

/// Collects forced edge assignments (cut/uncut) and applies them in batch.
/// Replaces the repeated pattern of local `Vec<EdgeId>` / `Vec<(EdgeId, EdgeState)>`
/// creation, population, and manual apply loops.
pub(crate) struct EdgeForcer {
    edges: Vec<(EdgeId, EdgeState)>,
}

impl EdgeForcer {
    pub(crate) fn new() -> Self {
        Self { edges: Vec::new() }
    }

    pub(crate) fn force_cut(&mut self, e: EdgeId) {
        self.edges.push((e, EdgeState::Cut));
    }

    pub(crate) fn force_uncut(&mut self, e: EdgeId) {
        self.edges.push((e, EdgeState::Uncut));
    }

    pub(crate) fn force(&mut self, e: EdgeId, s: EdgeState) {
        self.edges.push((e, s));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    #[expect(unused)]
    pub(crate) fn has_cuts(&self) -> bool {
        self.edges.iter().any(|&(_, s)| s == EdgeState::Cut)
    }

    /// Apply all collected forced edges to the solver, then clear the buffer.
    /// Returns Ok(progress) or Err(()) on contradiction.
    pub(crate) fn apply(&mut self, solver: &mut Solver) -> Result<bool, ()> {
        let progress = solver.apply_forced_edges(&self.edges)?;
        self.edges.clear();
        Ok(progress)
    }
}

impl Solver {
    pub(crate) fn set_edge(&mut self, e: EdgeId, s: EdgeState) -> bool {
        if self.edges[e] == s {
            return true;
        }
        if self.edges[e] != EdgeState::Unknown {
            return false;
        }
        // Debug: detect when we're forcing an edge to a value that contradicts
        // the known correct solution while still on the solution path.
        if !self.debug_known_solution.is_empty()
            && !self.in_probing
            && e < self.debug_known_solution.len()
            && self.debug_current_prop != "branch"
        {
            let known = self.debug_known_solution[e];
            if known != EdgeState::Unknown && known != s {
                // Check if all currently-set edges are consistent with known solution.
                // If yes, this set_edge is the first kill of the solution path.
                let on_path = self.edges.iter().enumerate().all(|(i, &curr)| {
                    if curr == EdgeState::Unknown {
                        return true;
                    }
                    if i >= self.debug_known_solution.len() {
                        return true;
                    }
                    let k = self.debug_known_solution[i];
                    k == EdgeState::Unknown || curr == k
                });
                if on_path {
                    let (c1, c2) = self.grid.edge_cells(e);
                    eprintln!(
                        "SOLUTION_KILL: prop={} edge={} cells={:?}->{:?} \
                         forced={:?} known={:?} depth={} unknown={}",
                        self.debug_current_prop,
                        e,
                        self.grid.cell_pos(c1),
                        self.grid.cell_pos(c2),
                        s,
                        known,
                        self.search_depth,
                        self.curr_unknown,
                    );
                }
            }
        }
        self.edges[e] = s;
        self.curr_unknown -= 1;
        self.changed.push((e, EdgeState::Unknown));
        true
    }

    pub(crate) fn apply_forced_edges(
        &mut self,
        forced: &[(EdgeId, EdgeState)],
    ) -> Result<bool, ()> {
        let mut progress = false;
        for &(e, state) in forced {
            if self.edges[e] == EdgeState::Unknown {
                if !self.set_edge(e, state) {
                    return Err(());
                }
                progress = true;
            }
        }
        Ok(progress)
    }

    pub(crate) fn set_edges_from_pieces(
        &mut self,
        pieces: &[crate::types::Piece],
        cell_to_piece: &[usize],
    ) {
        let grid = &self.grid;
        // Collect edges to set first, then apply (avoids borrow conflict with set_edge).
        let mut to_set: Vec<(EdgeId, EdgeState)> = Vec::new();
        for piece in pieces {
            for &cid in &piece.cells {
                for eid in grid.cell_edges(cid).into_iter().flatten() {
                    let (c1, c2) = grid.edge_cells(eid);
                    let other = if c1 == cid { c2 } else { c1 };
                    let state =
                        if !grid.cell_exists[other] || cell_to_piece[other] != cell_to_piece[cid] {
                            EdgeState::Cut
                        } else {
                            EdgeState::Uncut
                        };
                    to_set.push((eid, state));
                }
            }
        }
        for (eid, state) in to_set {
            self.set_edge(eid, state);
        }
    }

    pub(crate) fn restore(&mut self, snap: Snapshot) {
        while self.changed.len() > snap.edges {
            let (e, old_state) = self.changed.pop().unwrap();
            if self.edges[e] != EdgeState::Unknown && old_state == EdgeState::Unknown {
                self.curr_unknown += 1;
            }
            self.edges[e] = old_state;
        }
        self.manual_diffs.truncate(snap.manual_diffs);
        self.manual_diff_set
            .retain(|pair| self.manual_diffs.iter().any(|d| *d == *pair));
        self.manual_sames.truncate(snap.manual_sames);
        self.manual_same_set
            .retain(|pair| self.manual_sames.iter().any(|d| *d == *pair));
    }

    /// Run `setup` to modify edge state. If setup returns true, propagate.
    /// Always restores state afterward. Returns true if setup succeeded AND propagation succeeded.
    ///
    /// This encapsulates the repeated `snapshot → set edges → propagate → restore` pattern.
    pub(crate) fn probe(&mut self, setup: impl FnOnce(&mut Self) -> bool) -> bool {
        let snap = self.snapshot();
        let ok = setup(self) && self.propagate().is_ok();
        self.restore(snap);
        ok
    }

    // Component sealed/growing helpers (reads can_grow_buf directly)

    #[inline]
    pub(crate) fn is_sealed(&self, ci: usize) -> bool {
        ci < self.can_grow_buf.len() && !self.can_grow_buf[ci]
    }

    #[inline]
    pub(crate) fn is_growing(&self, ci: usize) -> bool {
        ci < self.can_grow_buf.len() && self.can_grow_buf[ci]
    }

    pub(crate) fn sealed(&self, num_comp: usize) -> impl Iterator<Item = usize> + '_ {
        (0..num_comp).filter(|&ci| self.is_sealed(ci))
    }

    pub(crate) fn growing(&self, num_comp: usize) -> impl Iterator<Item = usize> + '_ {
        (0..num_comp).filter(|&ci| self.is_growing(ci))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

    #[test]
    fn set_edge_and_restore() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        assert_eq!(s.edges[e], EdgeState::Unknown);

        // Set to Cut
        assert!(s.set_edge(e, EdgeState::Cut));
        assert_eq!(s.edges[e], EdgeState::Cut);
        let snap = s.snapshot();

        // Set another edge
        let e2 = s.grid.h_edge(0, 0);
        let _ = s.set_edge(e2, EdgeState::Uncut);
        assert_eq!(s.edges[e2], EdgeState::Uncut);

        // Restore to before e2
        s.restore(snap);
        assert_eq!(s.edges[e], EdgeState::Cut);
        assert_eq!(s.edges[e2], EdgeState::Unknown);
    }

    #[test]
    fn set_edge_idempotent() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        s.edges[e] = EdgeState::Cut;
        // Setting same state returns true without pushing to changed
        let snap = s.snapshot();
        assert!(s.set_edge(e, EdgeState::Cut));
        assert_eq!(s.changed.len(), snap.edges);
    }

    #[test]
    fn set_edge_conflict_returns_false() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        s.edges[e] = EdgeState::Cut;
        // Trying to set to Uncut when already Cut should fail
        assert!(!s.set_edge(e, EdgeState::Uncut));
    }
}
