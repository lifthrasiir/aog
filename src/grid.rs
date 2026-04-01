use crate::types::{CellId, EdgeId, VertexId};

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

    /// H(r,c): horizontal edge below cell (r,c)
    pub fn h_edge(&self, r: usize, c: usize) -> EdgeId {
        c * (self.rows - 1) + r
    }

    /// V(r,c): vertical edge right of cell (r,c)
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

    /// The two cells adjacent to an edge
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

    /// 4 edges adjacent to vertex (i,j), None if boundary
    pub fn vertex_edges(&self, i: usize, j: usize) -> [Option<EdgeId>; 4] {
        [
            if i >= 1 && i < self.rows {
                Some(self.h_edge(i - 1, j))
            } else {
                None
            },
            if i < self.rows - 1 {
                Some(self.h_edge(i, j))
            } else {
                None
            },
            if j >= 1 && j < self.cols {
                Some(self.v_edge(i, j - 1))
            } else {
                None
            },
            if j < self.cols - 1 {
                Some(self.v_edge(i, j))
            } else {
                None
            },
        ]
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

    /// 4 edges around a cell: top, bottom, left, right. None if boundary.
    pub fn cell_edges(&self, c: CellId) -> [Option<EdgeId>; 4] {
        let (r, col) = self.cell_pos(c);
        [
            if r > 0 {
                Some(self.h_edge(r - 1, col))
            } else {
                None
            },
            if r < self.rows - 1 {
                Some(self.h_edge(r, col))
            } else {
                None
            },
            if col > 0 {
                Some(self.v_edge(r, col - 1))
            } else {
                None
            },
            if col < self.cols - 1 {
                Some(self.v_edge(r, col))
            } else {
                None
            },
        ]
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
    fn vertex_edges_corner_boundary() {
        let g = grid3x2();
        // Corner (0,0): no top edge, no left edge
        let edges = g.vertex_edges(0, 0);
        assert!(edges[0].is_none()); // top
        assert!(edges[2].is_none()); // left
        assert!(edges[1].is_some()); // bottom
        assert!(edges[3].is_some()); // right
    }

    #[test]
    fn vertex_edges_all_interior() {
        // Need 0 < i < rows and 0 < j < cols for all 4 edges
        let g = Grid::new(3, 3, true);
        let edges = g.vertex_edges(1, 1);
        assert!(edges.iter().all(|e| e.is_some()));
    }

    #[test]
    fn cell_edges_corner() {
        let g = grid3x2();
        // Cell (0,0): no top, no left
        let edges = g.cell_edges(g.cell_id(0, 0));
        assert!(edges[0].is_none()); // top
        assert!(edges[2].is_none()); // left
        assert!(edges[1].is_some()); // bottom
        assert!(edges[3].is_some()); // right
    }

    #[test]
    fn single_cell_grid_no_edges() {
        let g = Grid::new(1, 1, true);
        assert_eq!(g.num_h_edges(), 0);
        assert_eq!(g.num_v_edges(), 0);
    }
}
