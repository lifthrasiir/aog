/// Union-Find with XOR parity.
///
/// Tracks equivalence classes with a parity bit between elements.
/// `par[i]` stores the XOR-parity from node `i` to its parent.
/// A parity of 0 means "same group" and 1 means "different group"
/// (e.g., same/different piece for cell-level UF, or Cut/Uncut for edge-level UF).

pub fn uf_find(parent: &[usize], par: &[u8], x: usize) -> (usize, u8) {
    let mut cur = x;
    let mut p = 0u8;
    while parent[cur] != cur {
        p ^= par[cur];
        cur = parent[cur];
    }
    (cur, p)
}

/// Union c1 and c2 with parity rel (0=same, 1=different).
/// Returns Ok(true) if newly merged, Ok(false) if already consistent, Err if contradiction.
pub fn uf_union(
    parent: &mut Vec<usize>,
    rank: &mut Vec<u8>,
    par: &mut Vec<u8>,
    c1: usize,
    c2: usize,
    rel: u8,
) -> Result<bool, ()> {
    let (r1, p1) = uf_find(parent, par, c1);
    let (r2, p2) = uf_find(parent, par, c2);
    if r1 == r2 {
        return if (p1 ^ p2) == rel { Ok(false) } else { Err(()) };
    }
    // Merge smaller rank into larger
    if rank[r1] < rank[r2] {
        parent[r1] = r2;
        par[r1] = p1 ^ p2 ^ rel;
    } else if rank[r1] > rank[r2] {
        parent[r2] = r1;
        par[r2] = p1 ^ p2 ^ rel;
    } else {
        parent[r2] = r1;
        par[r2] = p1 ^ p2 ^ rel;
        rank[r1] += 1;
    }
    Ok(true)
}

/// Owning wrapper around the parity union-find arrays.
pub struct ParityUF {
    parent: Vec<usize>,
    rank: Vec<u8>,
    par: Vec<u8>,
}

impl ParityUF {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
            par: vec![0; n],
        }
    }

    pub fn find(&self, x: usize) -> (usize, u8) {
        uf_find(&self.parent, &self.par, x)
    }

    /// Union c1 and c2 with parity rel (0=same, 1=different).
    /// Returns Ok(true) if newly merged, Ok(false) if already consistent, Err if contradiction.
    pub fn union(&mut self, c1: usize, c2: usize, rel: u8) -> Result<bool, ()> {
        uf_union(&mut self.parent, &mut self.rank, &mut self.par, c1, c2, rel)
    }
}
