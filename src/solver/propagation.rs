use super::Solver;
use crate::types::*;

impl Solver {
    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            if self.puzzle.rules.bricky || self.puzzle.rules.loopy {
                progress |= self.propagate_bricky_loopy()?;
            }
            progress |= self.propagate_area_bounds()?;
            progress |= self.propagate_rose_separation()?;
            progress |= self.propagate_same_area_reachability()?;
            progress |= self.propagate_palisade_constraints()?;
            progress |= self.propagate_compass()?;
            progress |= self.propagate_watchtower()?;

            if !progress {
                // Failed literal detection (probing): probe each unknown edge
                // to see if one value causes contradiction. Uses recursion guard
                // to prevent infinite loop when called from within a probe.
                if !self.in_probing
                    && self.rose_bits_all != 0
                    && self.curr_unknown > 0
                    && self.curr_unknown <= 256
                {
                    let saved = self.in_probing;
                    self.in_probing = true;
                    progress |= self.probe_one_round()?;
                    self.in_probing = saved;
                }

                if !progress {
                    return Ok(true);
                }
            }
        }
    }

    /// Single round of failed literal detection: for each unknown edge,
    /// temporarily assign Cut and Uncut, run propagation, and if one causes
    /// contradiction, force the opposite value.
    fn probe_one_round(&mut self) -> Result<bool, ()> {
        let mut forced = 0usize;
        let num_edges = self.grid.num_edges();

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }

            // Probe Cut
            let snap = self.changed.len();
            let cut_ok = self.set_edge(e, EdgeState::Cut)
                && self.propagate().is_ok();
            self.restore(snap);

            if !cut_ok {
                // Cut contradicts → force Uncut
                if self.edges[e] == EdgeState::Unknown {
                    if self.set_edge(e, EdgeState::Uncut) {
                        forced += 1;
                        self.propagate()?;
                    }
                }
                continue;
            }

            if self.edges[e] != EdgeState::Unknown {
                continue; // forced by a previous probe's cascade
            }

            // Probe Uncut
            let snap = self.changed.len();
            let uncut_ok = self.set_edge(e, EdgeState::Uncut)
                && self.propagate().is_ok();
            self.restore(snap);

            if !uncut_ok {
                // Uncut contradicts → force Cut
                if self.edges[e] == EdgeState::Unknown {
                    if self.set_edge(e, EdgeState::Cut) {
                        forced += 1;
                        self.propagate()?;
                    }
                }
            }
        }

        Ok(forced > 0)
    }

    /// Multi-round probing (standalone, for initial solve phase).
    /// Calls propagate() internally which includes integrated probing,
    /// so this is equivalent to just running propagate() with the
    /// probing threshold already handled.
    pub(crate) fn probe_edges(&mut self) -> Result<(), ()> {
        // Just run propagate; the integrated probing handles everything
        self.propagate().map(|_| ())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

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
        assert!(
            result.is_err(),
            "bricky: 4 cut edges at vertex should be contradiction"
        );
    }
}
