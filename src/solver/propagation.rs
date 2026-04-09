use super::Solver;
use crate::types::*;

impl Solver {
    /// Check if all currently-decided edges are consistent with the known solution.
    /// Used for debug tracing of false contradictions.
    fn on_solution_path(&self) -> bool {
        if self.debug_known_solution.is_empty() || self.in_probing {
            return false;
        }
        self.edges.iter().enumerate().all(|(i, &curr)| {
            if curr == EdgeState::Unknown {
                return true;
            }
            if i >= self.debug_known_solution.len() {
                return true;
            }
            let k = self.debug_known_solution[i];
            k == EdgeState::Unknown || curr == k
        })
    }

    pub(crate) fn propagate(&mut self) -> Result<bool, ()> {
        loop {
            let mut progress = false;

            macro_rules! run_prop {
                ($name:literal, $cond:expr, $call:expr) => {
                    if $cond {
                        self.debug_current_prop = $name;
                        let r = $call;
                        if r.is_err() && self.on_solution_path() {
                            eprintln!(
                                "FALSE_ERR: prop={} depth={} unknown={}",
                                $name, self.search_depth, self.curr_unknown
                            );
                        }
                        progress |= r?;
                    }
                };
            }

            run_prop!(
                "bricky_loopy",
                self.puzzle.rules.bricky || self.puzzle.rules.loopy,
                self.propagate_bricky_loopy()
            );
            run_prop!(
                "vertex_edge_parity",
                !self.puzzle.vertex_clues.is_empty(),
                self.propagate_vertex_edge_parity()
            );
            run_prop!(
                "loop_closure",
                !self.in_probing
                    && self.rose_exact_piece_count.map_or(false, |p| p >= 2),
                self.propagate_loop_closure()
            );
            run_prop!(
                "delta_gemini",
                !self.puzzle.edge_clues.is_empty(),
                self.propagate_delta_gemini_interaction()
            );
            run_prop!("area_bounds", true, self.propagate_area_bounds());
            run_prop!(
                "dual_conn",
                self.rose_exact_piece_count.map_or(false, |p| p >= 2),
                self.propagate_dual_connectivity()
            );
            run_prop!("rose_parity", true, self.propagate_parity());
            run_prop!("rose_sep", true, self.propagate_rose_separation());
            run_prop!("rose_phase3", true, self.propagate_rose_phase3());
            run_prop!(
                "same_area_reach",
                self.same_area_groups,
                self.propagate_same_area_reachability()
            );
            run_prop!(
                "palisade",
                self.has_palisade_clue,
                self.propagate_palisade_constraints()
            );
            run_prop!(
                "compass_basic",
                self.has_compass_clue,
                self.propagate_compass()
            );
            run_prop!("watchtower", true, self.propagate_watchtower());

            if !progress {
                // Failed literal detection (probing): probe each unknown edge
                // to see if one value causes contradiction. in_probing guard
                // prevents recursion when called from within a probe.
                if !self.in_probing && self.curr_unknown > 0 && self.curr_unknown <= 256 {
                    let saved = self.in_probing;
                    self.in_probing = true;
                    self.debug_current_prop = "probe";
                    progress |= self.probe_one_round()?;
                    // Pair probing: probe pairs of edges sharing a vertex.
                    // Higher threshold for loopy+watchtower where vertex-local
                    // constraints make two-edge contradictions common.
                    let pair_threshold: usize =
                        if self.puzzle.rules.loopy && !self.puzzle.vertex_clues.is_empty() {
                            20
                        } else {
                            10
                        };
                    if !progress && self.curr_unknown <= pair_threshold {
                        progress |= self.probe_pair_round()?;
                    }
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
    /// Returns early on first force to let the outer loop cascade.
    fn probe_one_round(&mut self) -> Result<bool, ()> {
        let num_edges = self.grid.num_edges();

        for e in 0..num_edges {
            if self.edges[e] != EdgeState::Unknown {
                continue;
            }

            // Probe Cut
            let cut_ok = self.probe(|s| s.set_edge(e, EdgeState::Cut));

            if !cut_ok {
                // Cut contradicts -> force Uncut
                if self.edges[e] == EdgeState::Unknown && self.set_edge(e, EdgeState::Uncut) {
                    return Ok(true);
                }
                continue;
            }

            if self.edges[e] != EdgeState::Unknown {
                continue; // forced by a previous probe's cascade
            }

            // Probe Uncut
            let uncut_ok = self.probe(|s| s.set_edge(e, EdgeState::Uncut));

            if !uncut_ok {
                // Uncut contradicts -> force Cut
                if self.edges[e] == EdgeState::Unknown && self.set_edge(e, EdgeState::Cut) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Probe pairs of edges sharing a vertex. For each pair (e1, e2),
    /// try all 4 combinations. If 3 contradict, force the 4th.
    /// Only probes pairs where both edges are Unknown and share a vertex.
    fn probe_pair_round(&mut self) -> Result<bool, ()> {
        // Collect unknown edges
        let unknowns: Vec<EdgeId> = (0..self.grid.num_edges())
            .filter(|&e| self.edges[e] == EdgeState::Unknown)
            .collect();

        if unknowns.len() < 2 || unknowns.len() > 30 {
            return Ok(false);
        }

        // Build vertex-to-edge mapping for pairing
        let mut vert_edges: Vec<Vec<EdgeId>> = Vec::new();
        for e in &unknowns {
            let (is_h, r, c) = self.grid.decode_edge(*e);
            let v1 = self.grid.vertex(r, c);
            let v2 = if is_h {
                self.grid.vertex(r + 1, c)
            } else {
                self.grid.vertex(r, c + 1)
            };
            while vert_edges.len() <= v1 {
                vert_edges.push(Vec::new());
            }
            while vert_edges.len() <= v2 {
                vert_edges.push(Vec::new());
            }
            vert_edges[v1].push(*e);
            vert_edges[v2].push(*e);
        }

        // Probe pairs of edges sharing a vertex
        let vals = [EdgeState::Cut, EdgeState::Uncut];
        for v_edges in &vert_edges {
            if v_edges.len() < 2 {
                continue;
            }
            for i in 0..v_edges.len() {
                let e1 = v_edges[i];
                if self.edges[e1] != EdgeState::Unknown {
                    continue;
                }
                for j in (i + 1)..v_edges.len() {
                    let e2 = v_edges[j];
                    if self.edges[e2] != EdgeState::Unknown {
                        continue;
                    }

                    let mut ok_count = 0usize;
                    let mut last_ok = (EdgeState::Cut, EdgeState::Cut);

                    for &v1 in &vals {
                        for &v2 in &vals {
                            let ok = self.probe(|s| {
                                s.set_edge(e1, v1) && s.set_edge(e2, v2)
                            });

                            if ok {
                                ok_count += 1;
                                last_ok = (v1, v2);
                            }
                        }
                    }

                    if ok_count == 1 {
                        // Only one combination works — force it
                        let (v1, v2) = last_ok;
                        if self.edges[e1] == EdgeState::Unknown {
                            let _ = self.set_edge(e1, v1);
                        }
                        if self.edges[e2] == EdgeState::Unknown {
                            let _ = self.set_edge(e2, v2);
                        }
                        return Ok(true);
                    }
                    if ok_count == 0 {
                        // All combinations contradict — current state is invalid
                        return Err(());
                    }

                    if self.edges[e1] != EdgeState::Unknown {
                        break; // e1 was forced by a previous pair probe
                    }
                }
            }
        }

        Ok(false)
    }
}
