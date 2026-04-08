use crate::types::{CellId, EdgeId, VertexId};

// ── Helper types ─────────────────────────────────────────────────────────────

/// Directional values for the four cardinal directions.
///
/// `IntoIterator` yields items in order: **north, south, west, east**
/// (matches the legacy `cell_edges` array ordering: top=0, bottom=1, left=2, right=3).
#[derive(Clone, Debug)]
pub struct News<T> {
    pub north: T,
    pub south: T,
    pub west: T,
    pub east: T,
}

impl<T> News<T> {
    /// Convert to array `[north, south, west, east]`.
    pub fn into_array(self) -> [T; 4] {
        [self.north, self.south, self.west, self.east]
    }
}

impl<T> IntoIterator for News<T> {
    type Item = T;
    type IntoIter = std::array::IntoIter<T, 4>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_array().into_iter()
    }
}

/// A pair of horizontal-edge values and a pair of vertical-edge values.
///
/// Used as the return type of `Grid::vertex_edges`:
/// - `horiz = (west_h_edge, east_h_edge)` — the two **horizontal** edges meeting at the vertex
/// - `vert  = (north_v_edge, south_v_edge)` — the two **vertical** edges meeting at the vertex
///
/// ### Sorted principle
/// When both entries of a `(T, T)` pair are `Some`, `.0 < .1`.
/// This coincides with geometric order (west < east, north < south)
/// because EdgeIds are assigned small-coordinate-first.
/// When one entry is `None` (boundary), its position encodes direction:
/// `.0` is always the west/north slot, `.1` is always the east/south slot.
///
/// `IntoIterator` yields items in order: **horiz.0, horiz.1, vert.0, vert.1**.
#[derive(Clone, Debug)]
pub struct HorizVert<T> {
    /// Horizontal edge pair: `.0` = west, `.1` = east. When both `Some`, `.0 < .1`.
    pub horiz: T,
    /// Vertical edge pair: `.0` = north, `.1` = south. When both `Some`, `.0 < .1`.
    pub vert: T,
}

impl<T: Copy> IntoIterator for HorizVert<(T, T)> {
    type Item = T;
    type IntoIter = std::array::IntoIter<T, 4>;
    fn into_iter(self) -> Self::IntoIter {
        [self.horiz.0, self.horiz.1, self.vert.0, self.vert.1].into_iter()
    }
}

// ── Grid ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub cell_exists: Vec<bool>,
}

impl Grid {
    pub fn new(rows: usize, cols: usize, default_exists: bool) -> Self {
        Self {
            rows,
            cols,
            cell_exists: vec![default_exists; rows * cols],
        }
    }

    pub fn cell_pos(&self, c: CellId) -> (usize, usize) {
        (c / self.cols, c % self.cols)
    }

    pub fn cell_id(&self, r: usize, c: usize) -> CellId {
        r * self.cols + c
    }

    pub fn num_h_edges(&self) -> usize {
        if self.rows > 1 {
            (self.rows - 1) * self.cols
        } else {
            0
        }
    }

    pub fn num_v_edges(&self) -> usize {
        if self.cols > 1 {
            self.rows * (self.cols - 1)
        } else {
            0
        }
    }

    pub fn num_edges(&self) -> usize {
        self.num_h_edges() + self.num_v_edges()
    }

    pub fn num_cells(&self) -> usize {
        self.rows * self.cols
    }

    /// H(r,c): horizontal edge below cell (r,c), between rows r and r+1.
    /// Runs from vertex(r+1, c) to vertex(r+1, c+1).
    pub fn h_edge(&self, r: usize, c: usize) -> EdgeId {
        c * (self.rows - 1) + r
    }

    /// V(r,c): vertical edge right of cell (r,c), between cols c and c+1.
    /// Runs from vertex(r, c+1) to vertex(r+1, c+1).
    pub fn v_edge(&self, r: usize, c: usize) -> EdgeId {
        self.num_h_edges() + r * (self.cols - 1) + c
    }

    /// Decode edge to (is_horizontal, r, c)
    pub fn decode_edge(&self, e: EdgeId) -> (bool, usize, usize) {
        let nh = self.num_h_edges();
        if e < nh {
            let r = e % (self.rows - 1);
            let c = e / (self.rows - 1);
            (true, r, c)
        } else {
            let idx = e - nh;
            let r = idx / (self.cols - 1);
            let c = idx % (self.cols - 1);
            (false, r, c)
        }
    }

    /// Edge between two adjacent cells, returns None if not adjacent
    pub fn edge_between(&self, a: CellId, b: CellId) -> Option<EdgeId> {
        let (ra, ca) = self.cell_pos(a);
        let (rb, cb) = self.cell_pos(b);
        if ra == rb && ca + 1 == cb {
            return Some(self.v_edge(ra, ca));
        }
        if ra == rb && ca == cb + 1 {
            return Some(self.v_edge(ra, cb));
        }
        if ca == cb && ra + 1 == rb {
            return Some(self.h_edge(ra, ca));
        }
        if ca == cb && ra == rb + 1 {
            return Some(self.h_edge(rb, cb));
        }
        None
    }

    /// The two cells adjacent to an edge, sorted: `.0 < .1` (smaller CellId first).
    pub fn edge_cells(&self, e: EdgeId) -> (CellId, CellId) {
        let (is_h, r, c) = self.decode_edge(e);
        if is_h {
            (self.cell_id(r, c), self.cell_id(r + 1, c))
        } else {
            (self.cell_id(r, c), self.cell_id(r, c + 1))
        }
    }

    /// Vertex at grid point (i,j), 0<=i<=rows, 0<=j<=cols
    pub fn vertex(&self, i: usize, j: usize) -> VertexId {
        i * (self.cols + 1) + j
    }

    /// Decode vertex to (i, j)
    pub fn vertex_pos(&self, v: VertexId) -> (usize, usize) {
        (v / (self.cols + 1), v % (self.cols + 1))
    }

    /// The two endpoint vertices of an edge, sorted: `.0 < .1` (smaller VertexId first).
    ///
    /// - `h_edge(r, c)` → `(vertex(r+1, c), vertex(r+1, c+1))`   left < right
    /// - `v_edge(r, c)` → `(vertex(r, c+1), vertex(r+1, c+1))`   top  < bottom
    pub fn edge_vertices(&self, e: EdgeId) -> (VertexId, VertexId) {
        let (is_h, r, c) = self.decode_edge(e);
        if is_h {
            (self.vertex(r + 1, c), self.vertex(r + 1, c + 1))
        } else {
            (self.vertex(r, c + 1), self.vertex(r + 1, c + 1))
        }
    }

    /// 4 edges adjacent to vertex (i,j).
    ///
    /// Returns `HorizVert` where:
    /// - `horiz = (west_h_edge, east_h_edge)`: the horizontal edges to the left and right
    /// - `vert  = (north_v_edge, south_v_edge)`: the vertical edges going up and down
    ///
    /// Each pair is sorted (`.0 <= .1`, `None < Some`), which coincides with
    /// west < east and north < south due to EdgeId assignment order.
    pub fn vertex_edges(&self, i: usize, j: usize) -> HorizVert<(Option<EdgeId>, Option<EdgeId>)> {
        HorizVert {
            horiz: (
                // west: h_edge(i-1, j-1) — right endpoint is vertex(i, j)
                if i >= 1 && i < self.rows && j >= 1 {
                    Some(self.h_edge(i - 1, j - 1))
                } else {
                    None
                },
                // east: h_edge(i-1, j) — left endpoint is vertex(i, j)
                if i >= 1 && i < self.rows && j < self.cols {
                    Some(self.h_edge(i - 1, j))
                } else {
                    None
                },
            ),
            vert: (
                // north: v_edge(i-1, j-1) — bottom endpoint is vertex(i, j)
                if i >= 1 && j >= 1 && j < self.cols {
                    Some(self.v_edge(i - 1, j - 1))
                } else {
                    None
                },
                // south: v_edge(i, j-1) — top endpoint is vertex(i, j)
                if i < self.rows && j >= 1 && j < self.cols {
                    Some(self.v_edge(i, j - 1))
                } else {
                    None
                },
            ),
        }
    }

    /// Cells sharing a vertex: top-left, top-right, bottom-left, bottom-right
    pub fn vertex_cells(&self, i: usize, j: usize) -> [Option<CellId>; 4] {
        [
            if i > 0 && j > 0 {
                Some(self.cell_id(i - 1, j - 1))
            } else {
                None
            },
            if i > 0 && j < self.cols {
                Some(self.cell_id(i - 1, j))
            } else {
                None
            },
            if i < self.rows && j > 0 {
                Some(self.cell_id(i, j - 1))
            } else {
                None
            },
            if i < self.rows && j < self.cols {
                Some(self.cell_id(i, j))
            } else {
                None
            },
        ]
    }

    /// 4 edges around a cell.
    ///
    /// Returns `News<Option<EdgeId>>` with fields `north`, `south`, `west`, `east`.
    /// `None` means the edge is on the grid boundary (no neighbor in that direction).
    /// `IntoIterator` yields `[north, south, west, east]`.
    pub fn cell_edges(&self, c: CellId) -> News<Option<EdgeId>> {
        let (r, col) = self.cell_pos(c);
        News {
            north: if r > 0 {
                Some(self.h_edge(r - 1, col))
            } else {
                None
            },
            south: if r < self.rows - 1 {
                Some(self.h_edge(r, col))
            } else {
                None
            },
            west: if col > 0 {
                Some(self.v_edge(r, col - 1))
            } else {
                None
            },
            east: if col < self.cols - 1 {
                Some(self.v_edge(r, col))
            } else {
                None
            },
        }
    }

    pub fn total_existing_cells(&self) -> usize {
        self.cell_exists.iter().filter(|&&x| x).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid3x2() -> Grid {
        Grid::new(3, 2, true)
    }

    #[test]
    fn cell_id_pos_roundtrip() {
        let g = grid3x2();
        for r in 0..3 {
            for c in 0..2 {
                assert_eq!(g.cell_pos(g.cell_id(r, c)), (r, c));
            }
        }
    }

    #[test]
    fn h_v_edge_roundtrip() {
        let g = grid3x2();
        for r in 0..2 {
            for c in 0..2 {
                let e = g.h_edge(r, c);
                let (is_h, dr, dc) = g.decode_edge(e);
                assert!(is_h && dr == r && dc == c);
            }
        }
        for r in 0..3 {
            for c in 0..1 {
                let e = g.v_edge(r, c);
                let (is_h, dr, dc) = g.decode_edge(e);
                assert!(!is_h && dr == r && dc == c);
            }
        }
    }

    #[test]
    fn edge_between() {
        let g = grid3x2();
        // Same edge regardless of argument order
        let e = g.edge_between(g.cell_id(1, 0), g.cell_id(1, 1));
        assert_eq!(e, g.edge_between(g.cell_id(1, 1), g.cell_id(1, 0)));
        assert_eq!(e, Some(g.v_edge(1, 0)));
        // Diagonal: no edge
        assert_eq!(g.edge_between(g.cell_id(0, 0), g.cell_id(1, 1)), None);
        // Same cell: no edge
        assert_eq!(g.edge_between(g.cell_id(0, 0), g.cell_id(0, 0)), None);
    }

    #[test]
    fn edge_cells_symmetry() {
        let g = grid3x2();
        // edge_cells should return the two cells that edge_between connects
        for a in 0..g.num_cells() {
            for b in (a + 1)..g.num_cells() {
                if let Some(e) = g.edge_between(a, b) {
                    let (c1, c2) = g.edge_cells(e);
                    assert!((c1 == a && c2 == b) || (c1 == b && c2 == a));
                }
            }
        }
    }

    #[test]
    fn cell_edges_corner() {
        let g = grid3x2();
        // Cell (0,0): no north, no west
        let ce = g.cell_edges(g.cell_id(0, 0));
        assert_eq!(ce.north, None);
        assert_eq!(ce.west, None);
        assert_eq!(ce.south, Some(g.h_edge(0, 0)));
        assert_eq!(ce.east, Some(g.v_edge(0, 0)));
    }

    #[test]
    fn cell_edges_interior() {
        // 3x3 grid: cell (1,1) should have all 4 edges
        let g = Grid::new(3, 3, true);
        let ce = g.cell_edges(g.cell_id(1, 1));
        assert_eq!(ce.north, Some(g.h_edge(0, 1)));
        assert_eq!(ce.south, Some(g.h_edge(1, 1)));
        assert_eq!(ce.west, Some(g.v_edge(1, 0)));
        assert_eq!(ce.east, Some(g.v_edge(1, 1)));
    }

    #[test]
    fn vertex_edges_corner_boundary() {
        let g = grid3x2();
        // Corner vertex (0,0): no west/north h_edge, no north/west v_edge
        let ve = g.vertex_edges(0, 0);
        assert_eq!(ve.horiz.0, None); // west h_edge: needs i>=1, j>=1 — fails both
        assert_eq!(ve.horiz.1, None); // east h_edge: needs i>=1 — fails
        assert_eq!(ve.vert.0, None); // north v_edge: needs i>=1 — fails
        assert_eq!(ve.vert.1, None); // south v_edge: needs j>=1 — fails
    }

    #[test]
    fn vertex_edges_top_edge_boundary() {
        let g = Grid::new(3, 3, true);
        // Top-edge vertex (0,1): i=0, so no h_edges and no north v_edge
        let ve = g.vertex_edges(0, 1);
        assert_eq!(ve.horiz.0, None); // i=0 → fails i>=1
        assert_eq!(ve.horiz.1, None); // i=0 → fails i>=1
        assert_eq!(ve.vert.0, None); // i=0 → fails i>=1
                                     // south v_edge: i=0 < rows=3, j=1 >= 1, j=1 < cols=3 → Some(v_edge(0,0))
        assert_eq!(ve.vert.1, Some(g.v_edge(0, 0)));
    }

    #[test]
    fn vertex_edges_interior() {
        // 3x3 grid, interior vertex (1,1): all 4 edges present
        let g = Grid::new(3, 3, true);
        let ve = g.vertex_edges(1, 1);
        assert_eq!(ve.horiz.0, Some(g.h_edge(0, 0))); // west: h_edge(i-1=0, j-1=0)
        assert_eq!(ve.horiz.1, Some(g.h_edge(0, 1))); // east: h_edge(i-1=0, j=1)
        assert_eq!(ve.vert.0, Some(g.v_edge(0, 0))); // north: v_edge(i-1=0, j-1=0)
        assert_eq!(ve.vert.1, Some(g.v_edge(1, 0))); // south: v_edge(i=1, j-1=0)
    }

    #[test]
    fn vertex_edges_sorted_principle() {
        // When both entries are Some, .0 < .1 (west < east, north < south).
        let g = Grid::new(4, 5, true);
        for i in 0..=g.rows {
            for j in 0..=g.cols {
                let ve = g.vertex_edges(i, j);
                if let (Some(a), Some(b)) = ve.horiz {
                    assert!(a < b, "horiz not sorted when both present at ({i},{j})");
                }
                if let (Some(a), Some(b)) = ve.vert {
                    assert!(a < b, "vert not sorted when both present at ({i},{j})");
                }
            }
        }
    }

    /// Property test: every edge returned by vertex_edges(i,j) must have vertex(i,j)
    /// as one of its endpoints (via edge_vertices).
    #[test]
    fn vertex_edges_touch_vertex() {
        let g = Grid::new(4, 5, true);
        for i in 0..=g.rows {
            for j in 0..=g.cols {
                let v = g.vertex(i, j);
                for maybe_e in g.vertex_edges(i, j) {
                    if let Some(e) = maybe_e {
                        let (v1, v2) = g.edge_vertices(e);
                        assert!(
                            v1 == v || v2 == v,
                            "vertex_edges({i},{j}) returned edge {e}, \
                             but its endpoints are vertex {v1} and {v2}, not {v}"
                        );
                    }
                }
            }
        }
    }

    /// Property test: for every edge e, both endpoint vertices list e in their vertex_edges.
    #[test]
    fn edge_vertices_covered_by_vertex_edges() {
        let g = Grid::new(4, 5, true);
        for e in 0..g.num_edges() {
            let (v1, v2) = g.edge_vertices(e);
            let (i1, j1) = g.vertex_pos(v1);
            let (i2, j2) = g.vertex_pos(v2);
            assert!(
                g.vertex_edges(i1, j1).into_iter().any(|x| x == Some(e)),
                "edge {e}'s vertex ({i1},{j1}) doesn't list it in vertex_edges"
            );
            assert!(
                g.vertex_edges(i2, j2).into_iter().any(|x| x == Some(e)),
                "edge {e}'s vertex ({i2},{j2}) doesn't list it in vertex_edges"
            );
        }
    }

    #[test]
    fn single_cell_grid_no_edges() {
        let g = Grid::new(1, 1, true);
        assert_eq!(g.num_h_edges(), 0);
        assert_eq!(g.num_v_edges(), 0);
    }
}
