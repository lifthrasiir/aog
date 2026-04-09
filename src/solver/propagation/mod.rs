mod area;
mod bricky_loopy;
mod compass;
mod delta_gemini;
mod dual;
mod loop_closure;
mod palisade;
mod rose;
mod shape;
mod watchtower;

use super::Solver;
use crate::types::*;

/// State and reusable buffers used exclusively within the propagation subsystem.
pub(crate) struct PropagationState {
    /// Whether any palisade cell clue is present (pre-computed once in new()).
    pub(crate) has_palisade_clue: bool,
    /// Pre-extracted diff clues: (edge_id, value). Consumed by `propagate_diff_clues`.
    pub(crate) diff_clues: Vec<(EdgeId, usize)>,
    /// When sum of distinct area values equals total_cells, all same-area cells
    /// must share a piece — enables grouped-area search path.
    pub(crate) same_area_groups: bool,
    /// Exact piece count deduced from rose window (None if undetermined or absent).
    pub(crate) exact_piece_count: Option<usize>,
    /// Per-component minimum area (updated each propagation round).
    pub(crate) curr_min_area: Vec<usize>,
    /// Per-component maximum area (updated each propagation round).
    pub(crate) curr_max_area: Vec<usize>,
    /// Growth edges per component (populated by build_components).
    pub(crate) growth_edges: Vec<Vec<EdgeId>>,
    /// Number of clue cells per component (populated by build_components, solitude only).
    pub(crate) comp_clue_cells: Vec<usize>,
    /// Rose symbol bitmask per component (reusable buffer, populated by build_components).
    pub(crate) comp_rose: Vec<u8>,
    /// Pre-computed indices into puzzle.cell_clues for compass clues with existing cells.
    pub(crate) compass_clue_indices: Vec<usize>,
    /// Reusable BFS buffer for component traversal.
    pub(crate) comp_buf: Vec<usize>,
    /// Secondary reusable BFS buffer (ID mapping etc.).
    pub(crate) comp_buf2: Vec<usize>,
}

impl PropagationState {
    pub(crate) fn new(
        has_palisade_clue: bool,
        diff_clues: Vec<(EdgeId, usize)>,
        same_area_groups: bool,
        exact_piece_count: Option<usize>,
        nc: usize,
    ) -> Self {
        Self {
            has_palisade_clue,
            diff_clues,
            same_area_groups,
            exact_piece_count,
            curr_min_area: Vec::new(),
            curr_max_area: Vec::new(),
            growth_edges: Vec::new(),
            comp_clue_cells: Vec::new(),
            comp_rose: Vec::new(),
            compass_clue_indices: Vec::new(),
            comp_buf: vec![usize::MAX; nc],
            comp_buf2: Vec::new(),
        }
    }
}

impl Solver {
    /// Check if all currently-decided edges are consistent with the known solution.
    /// Used for debug tracing of false contradictions.
    pub(crate) fn on_solution_path(&self) -> bool {
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
        let _prop_span = tracing::debug_span!(
            "propagate",
            depth = self.search_depth,
            probe = self.in_probing
        )
        .entered();

        loop {
            let mut progress = false;

            macro_rules! run_prop {
                ($name:literal, $cond:expr, $call:expr) => {
                    if $cond {
                        self.debug_current_prop = $name;
                        let r = $call;
                        if r.is_err() && self.on_solution_path() {
                            tracing::warn!(
                                prop = $name,
                                depth = self.search_depth,
                                unknown = self.curr_unknown,
                                "FALSE_ERR"
                            );
                        }
                        let made = r?;
                        if made {
                            tracing::trace!(
                                prop = $name,
                                depth = self.search_depth,
                                unk = self.curr_unknown,
                                "propagator made progress"
                            );
                        }
                        progress |= made;
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
                !self.in_probing && self.rose_bits_all != 0 && self.prop.exact_piece_count.map_or(false, |p| p >= 2),
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
                self.rose_bits_all != 0 && self.prop.exact_piece_count.map_or(false, |p| p >= 2),
                self.propagate_dual_connectivity()
            );
            run_prop!("rose_parity", true, self.propagate_parity());
            run_prop!("rose_sep", true, self.propagate_rose_separation());
            run_prop!("rose_phase3", true, self.propagate_rose_phase3());
            run_prop!(
                "same_area_reach",
                self.prop.same_area_groups,
                self.propagate_same_area_reachability()
            );
            run_prop!(
                "palisade",
                self.prop.has_palisade_clue,
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
            let cut_ok = {
                let _span = tracing::trace_span!(
                    "probe",
                    edge = e,
                    val = "Cut",
                    depth = self.search_depth,
                    unk = self.curr_unknown
                )
                .entered();
                self.probe(|s| s.set_edge(e, EdgeState::Cut))
            };
            tracing::trace!(edge = e, cut_ok, unk = self.curr_unknown, "probe Cut result");

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
            let uncut_ok = {
                let _span = tracing::trace_span!(
                    "probe",
                    edge = e,
                    val = "Uncut",
                    depth = self.search_depth,
                    unk = self.curr_unknown
                )
                .entered();
                self.probe(|s| s.set_edge(e, EdgeState::Uncut))
            };
            tracing::trace!(edge = e, uncut_ok, unk = self.curr_unknown, "probe Uncut result");

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
                            let ok = self.probe(|s| s.set_edge(e1, v1) && s.set_edge(e2, v2));

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
