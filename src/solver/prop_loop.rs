use super::Solver;
use crate::types::*;

impl Solver {
    /// Detect premature loop closure in the cut-edge graph.
    ///
    /// Cut edges form a graph on grid vertices. For puzzles with loopy + @,
    /// the cut graph should eventually be a single connected structure.
    /// If an interior loop closes while open path segments still exist,
    /// those segments can never merge with the closed loop → contradiction.
    ///
    /// For puzzles with a known piece count (rose_exact_piece_count),
    /// the number of closed loops cannot exceed pieces - 1.
    pub(crate) fn propagate_loop_closure(&mut self) -> Result<bool, ()> {
        // Only meaningful when piece count is known (need it for loop limit)
        let max_loops = match self.rose_exact_piece_count {
            Some(p) if p >= 2 => p - 1,
            _ => return Ok(false),
        };

        let ne = self.grid.num_edges();
        let nv = (self.grid.rows + 1) * (self.grid.cols + 1);

        // Build cut-edge graph: vertices connected by Cut edges
        let mut vp: Vec<usize> = (0..nv).collect();
        let mut vr: Vec<u8> = vec![0; nv];
        let mut cd: Vec<u8> = vec![0; nv]; // cut degree per vertex
        let mut any_cut = false;

        for e in 0..ne {
            if self.edges[e] != EdgeState::Cut {
                continue;
            }
            any_cut = true;
            let (v1, v2) = self.grid.edge_vertices(e);
            cd[v1] += 1;
            cd[v2] += 1;
            // Union
            let mut r1 = v1;
            while vp[r1] != r1 {
                r1 = vp[r1];
            }
            let mut r2 = v2;
            while vp[r2] != r2 {
                r2 = vp[r2];
            }
            if r1 != r2 {
                if vr[r1] < vr[r2] {
                    vp[r1] = r2;
                } else if vr[r1] > vr[r2] {
                    vp[r2] = r1;
                } else {
                    vp[r2] = r1;
                    vr[r1] += 1;
                }
            }
        }

        if !any_cut {
            return Ok(false);
        }

        // Count odd-degree vertices per component root
        // Odd-degree vertices are path endpoints; even means loop or empty
        let mut root_odd: Vec<u8> = vec![0; nv];
        for v in 0..nv {
            if cd[v] == 0 || cd[v] % 2 == 0 {
                continue;
            }
            let mut r = v;
            while vp[r] != r {
                r = vp[r];
            }
            root_odd[r] += 1;
        }

        // Classify components: only process roots that have cut edges
        let mut num_loops = 0usize;
        let mut num_open = 0usize;
        // Track which roots we've already processed
        let mut seen_root = vec![false; nv];
        for v in 0..nv {
            if cd[v] == 0 {
                continue;
            }
            let mut r = v;
            while vp[r] != r {
                r = vp[r];
            }
            if seen_root[r] {
                continue;
            }
            seen_root[r] = true;

            match root_odd[r] {
                0 => num_loops += 1,   // all vertices even degree → loop
                2 => num_open += 1,    // exactly 2 odd-degree vertices → path
                _ => {
                    // Branching (3+ odd-degree vertices) — shouldn't happen
                    // with @ + loopy but handle gracefully
                    num_open += 1;
                }
            }
        }

        // Premature closure check:
        // - Always: num_loops > max_loops is a contradiction
        // - For loopy + @ (each vertex needs exactly degree 2): open paths
        //   can't merge with existing loops (loop vertices already have degree 2),
        //   so each open path will form at least 1 more loop
        // num_loops > max_loops is always a contradiction (fully determined loops)
        if num_loops > max_loops {
            return Err(());
        }

        // --- Boundary-edge constraint for loopy puzzles ---
        // For a 2-piece loopy puzzle, the cut edges must form exactly 1 closed
        // loop. A Cut edge at a grid-boundary vertex (a vertex with ≤1 total
        // edges) creates a path endpoint that can NEVER be extended (no more
        // edges at that vertex), making the path permanently stuck. Such a
        // path can never close into a loop, so the required single-loop
        // structure is impossible.
        //
        // Therefore, for max_loops == 1 (2-piece), all edges touching
        // boundary vertices must be Uncut. Force unknowns to Uncut; any
        // existing Cut is a contradiction.
        //
        // Boundary vertices are those at i=0, i=rows, j=0, or j=cols.
        // The edges touching them are:
        //   - v_edge(0, j-1) for j=1..cols-1 (top row vertical edges)
        //   - v_edge(rows-1, j-1) for j=1..cols-1 (bottom row vertical edges)
        //   - h_edge(i-1, 0) for i=1..rows-1 (left column horizontal edges)
        //   - h_edge(i-1, cols-1) for i=1..rows-1 (right column horizontal edges)
        if max_loops == 1 && self.puzzle.rules.loopy {
            let mut progress = false;
            // Top boundary: v_edge(0, j-1) for j=1..cols-1
            for j in 1..self.grid.cols {
                let e = self.grid.v_edge(0, j - 1);
                match self.edges[e] {
                    EdgeState::Cut => return Err(()),
                    EdgeState::Unknown => {
                        if !self.set_edge(e, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    EdgeState::Uncut => {}
                }
            }
            // Bottom boundary: v_edge(rows-1, j-1) for j=1..cols-1
            for j in 1..self.grid.cols {
                let e = self.grid.v_edge(self.grid.rows - 1, j - 1);
                match self.edges[e] {
                    EdgeState::Cut => return Err(()),
                    EdgeState::Unknown => {
                        if !self.set_edge(e, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    EdgeState::Uncut => {}
                }
            }
            // Left boundary: h_edge(i-1, 0) for i=1..rows-1
            for i in 1..self.grid.rows {
                let e = self.grid.h_edge(i - 1, 0);
                match self.edges[e] {
                    EdgeState::Cut => return Err(()),
                    EdgeState::Unknown => {
                        if !self.set_edge(e, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    EdgeState::Uncut => {}
                }
            }
            // Right boundary: h_edge(i-1, cols-1) for i=1..rows-1
            for i in 1..self.grid.rows {
                let e = self.grid.h_edge(i - 1, self.grid.cols - 1);
                match self.edges[e] {
                    EdgeState::Cut => return Err(()),
                    EdgeState::Unknown => {
                        if !self.set_edge(e, EdgeState::Uncut) {
                            return Err(());
                        }
                        progress = true;
                    }
                    EdgeState::Uncut => {}
                }
            }
            if progress {
                return Ok(true);
            }
        }

        // For loopy + @ (each vertex needs exactly degree 2): open paths
        // can't merge with existing loops (loop vertices already have degree 2),
        // so each open path will form at least 1 more loop
        let all_vertices_degree2 = self.puzzle.rules.loopy
            && !self.puzzle.vertex_clues.is_empty()
            && self.puzzle.vertex_clues.iter().all(|cl| cl.value == 2);
        if all_vertices_degree2 {
            let min_total_loops = num_loops + if num_open > 0 { 1 } else { 0 };
            if min_total_loops > max_loops {
                return Err(());
            }
        }

        // --- Single-loop saturation rule ---
        // When max_loops == 1 and at least 1 loop already exists, any @ vertex
        // (needs degree 2) NOT on the existing loop can never get its required
        // cut edges — they would form a second loop (contradiction).
        // Therefore: force all unknown edges at non-loop @ vertices to Uncut,
        // and any existing cut edge at such a vertex is a contradiction.
        if max_loops == 1 && num_loops >= 1 && !self.puzzle.vertex_clues.is_empty() {
            // Collect roots that are loops
            let mut loop_root: Vec<bool> = vec![false; nv];
            for v in 0..nv {
                if cd[v] == 0 {
                    continue;
                }
                let mut r = v;
                while vp[r] != r {
                    r = vp[r];
                }
                if root_odd[r] == 0 {
                    loop_root[r] = true;
                }
            }

            // Check @ vertices: contradiction if not on loop but has cut edges
            // Collect unknowns to force at non-loop @ vertices with cd==0
            let mut unks_to_force: Vec<EdgeId> = Vec::new();
            for cl in &self.puzzle.vertex_clues {
                if cl.value != 2 {
                    continue;
                }
                let v = cl.vertex;
                if cd[v] > 0 {
                    let mut r = v;
                    while vp[r] != r {
                        r = vp[r];
                    }
                    if !loop_root[r] {
                        return Err(());
                    }
                } else {
                    // cd[v] == 0: force all unknown edges to Uncut
                    let (vi, vj) = self.grid.vertex_pos(v);
                    for maybe_e in self.grid.vertex_edges(vi, vj) {
                        if let Some(e) = maybe_e {
                            if self.edges[e] == EdgeState::Unknown {
                                unks_to_force.push(e);
                            }
                        }
                    }
                }
            }

            let mut progress = false;
            for e in unks_to_force {
                if self.edges[e] == EdgeState::Unknown {
                    if !self.set_edge(e, EdgeState::Uncut) {
                        return Err(());
                    }
                    progress = true;
                }
            }
            if progress {
                return Ok(true);
            }
        }

        Ok(false)
    }
}
