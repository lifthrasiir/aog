use crate::grid::Grid;
use crate::types::{Piece, Shape};
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn all() -> [Self; 4] {
        [Self::R0, Self::R90, Self::R180, Self::R270]
    }

    pub fn transform(self, r: i32, c: i32) -> (i32, i32) {
        match self {
            Self::R0 => (r, c),
            Self::R90 => (c, -r),
            Self::R180 => (-r, -c),
            Self::R270 => (-c, r),
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

pub fn normalize(cells: &[(i32, i32)]) -> Vec<(i32, i32)> {
    if cells.is_empty() {
        return vec![];
    }
    let min_r = cells.iter().map(|&(r, _)| r).min().unwrap();
    let min_c = cells.iter().map(|&(_, c)| c).min().unwrap();
    let mut out: Vec<_> = cells.iter().map(|&(r, c)| (r - min_r, c - min_c)).collect();
    out.sort();
    out
}

pub fn make_shape(cells: &[(i32, i32)]) -> Shape {
    let cells = normalize(cells);
    if cells.is_empty() {
        return Shape::default();
    }
    let max_r = cells.iter().map(|&(r, _)| r).max().unwrap();
    let max_c = cells.iter().map(|&(_, c)| c).max().unwrap();
    Shape {
        height: max_r + 1,
        width: max_c + 1,
        cells,
    }
}

fn rotate(cells: &[(i32, i32)], rot: Rotation, flip: bool) -> Vec<(i32, i32)> {
    cells
        .iter()
        .map(|&(r, c)| {
            let (nr, nc) = rot.transform(r, c);
            if flip {
                (nr, -nc)
            } else {
                (nr, nc)
            }
        })
        .collect()
}

pub fn canonical(s: &Shape) -> Shape {
    let mut best = s.clone();
    for rot in Rotation::all() {
        for flip in [false, true] {
            let cand = make_shape(&rotate(&s.cells, rot, flip));
            if cand < best {
                best = cand;
            }
        }
    }
    best
}

pub fn parse_shape(lines: &[&str]) -> Shape {
    let mut cells = vec![];
    for (r, line) in lines.iter().enumerate() {
        for (c, ch) in line.chars().enumerate() {
            if ch == '#' {
                cells.push((r as i32, c as i32));
            }
        }
    }
    make_shape(&cells)
}

static NAMED_SHAPES: LazyLock<BTreeMap<&'static str, Vec<(i32, i32)>>> = LazyLock::new(|| {
    let mut m = BTreeMap::new();
    // Monomino
    // []
    m.insert("o", vec![(0, 0)]);
    // Domino
    // [][]
    m.insert("oo", vec![(0, 0), (0, 1)]);
    // Triominoes
    // [][][]    []
    //           [][]
    // ooo       8o
    m.insert("ooo", vec![(0, 0), (0, 1), (0, 2)]);
    m.insert("8o", vec![(0, 0), (1, 0), (1, 1)]);
    // Tetrominoes
    //                           []  []
    //           [][]  [][][]    []  []      [][]  [][]
    // [][][][]  [][]    []    [][]  [][]  [][]      [][]
    // I         O     T       J  =  L     S   =   Z
    m.insert("I", vec![(0, 0), (0, 1), (0, 2), (0, 3)]);
    m.insert("O", vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    m.insert("T", vec![(0, 0), (0, 1), (0, 2), (1, 1)]);
    m.insert("S", vec![(0, 1), (0, 2), (1, 0), (1, 1)]);
    m.insert("Z", vec![(0, 0), (0, 1), (1, 1), (1, 2)]);
    m.insert("L", vec![(0, 0), (1, 0), (2, 0), (2, 1)]);
    m.insert("J", vec![(0, 0), (0, 1), (1, 0), (2, 0)]);
    // Pentominoes
    //         []          []
    //         []  []      []                                                  []
    //   [][]  []  []      []  [][]  [][][]          []      []        []    [][]  [][]
    // [][]    []  []    [][]  [][]    []    []  []  []      [][]    [][][]    []    []
    //   []    []  [][]  []    []      []    [][][]  [][][]    [][]    []      []    [][]
    // F       II  LL    N     P     TT      U       V       W       X       Y     ZZ
    m.insert("F", vec![(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)]);
    m.insert("P", vec![(0, 0), (0, 1), (1, 0), (1, 1), (2, 0)]);
    m.insert("N", vec![(0, 0), (1, 0), (1, 1), (2, 1), (3, 1)]);
    m.insert("U", vec![(0, 0), (0, 2), (1, 0), (1, 1), (1, 2)]);
    m.insert("V", vec![(0, 0), (1, 0), (2, 0), (2, 1), (2, 2)]);
    m.insert("W", vec![(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)]);
    m.insert("X", vec![(0, 1), (1, 0), (1, 1), (1, 2), (2, 1)]);
    m.insert("Y", vec![(0, 1), (1, 0), (1, 1), (2, 1), (3, 1)]);
    // Pentominoes sharing names with tetrominoes
    m.insert("II", vec![(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
    m.insert("LL", vec![(0, 0), (1, 0), (2, 0), (3, 0), (3, 1)]);
    m.insert("TT", vec![(0, 0), (0, 1), (0, 2), (1, 1), (2, 1)]);
    m.insert("ZZ", vec![(0, 0), (0, 1), (1, 1), (2, 1), (2, 2)]);
    m
});

pub fn get_named_shape(name: &str) -> Option<Shape> {
    NAMED_SHAPES.get(name).map(|cells| make_shape(cells))
}

/// Enumerate all distinct free polyominoes of size n.
/// Uses Redelmeier's algorithm with canonical deduplication.
/// Returns canonical shapes sorted by (height, width, cells).
pub fn enumerate_free_polyominoes(n: usize) -> Vec<Shape> {
    if n == 0 {
        return vec![];
    }
    let mut seen: HashSet<Vec<(i32, i32)>> = HashSet::new();
    let mut results: Vec<Shape> = Vec::new();

    let mut cells: Vec<(i32, i32)> = vec![(0, 0)];
    let mut placed: HashSet<(i32, i32)> = [(0, 0)].into_iter().collect();
    let mut untried: Vec<(i32, i32)> = Vec::new();
    for &(dr, dc) in &[(0i32, 1i32), (1, 0), (0, -1), (-1, 0)] {
        if placed.insert((dr, dc)) {
            untried.push((dr, dc));
        }
    }

    poly_rec(
        n,
        &mut cells,
        &untried,
        &mut placed,
        &mut seen,
        &mut results,
    );
    results.sort();
    results
}

/// Redelmeier-style recursive enumeration.
/// `parent_untried` cells at indices < i are permanently discarded for this branch,
/// ensuring each fixed polyomino is generated at most once.
fn poly_rec(
    target: usize,
    cells: &mut Vec<(i32, i32)>,
    parent_untried: &[(i32, i32)],
    placed: &mut HashSet<(i32, i32)>,
    seen: &mut HashSet<Vec<(i32, i32)>>,
    results: &mut Vec<Shape>,
) {
    if cells.len() == target {
        let canon = canonical(&make_shape(cells));
        if seen.insert(canon.cells.clone()) {
            results.push(canon);
        }
        return;
    }

    for i in 0..parent_untried.len() {
        let pos = parent_untried[i];

        // Discard cells before index i; keep cells after
        let mut new_untried: Vec<(i32, i32)> = parent_untried[i + 1..].to_vec();

        // Track placed additions for undo
        let mut placed_added: Vec<(i32, i32)> = Vec::new();

        cells.push(pos);
        if placed.insert(pos) {
            placed_added.push(pos);
        }

        // Add new frontier neighbors
        for &(dr, dc) in &[(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let nb = (pos.0 + dr, pos.1 + dc);
            if placed.insert(nb) {
                new_untried.push(nb);
                placed_added.push(nb);
            }
        }

        // Pruning: check after computing new_untried (includes growth from neighbors)
        if cells.len() + new_untried.len() >= target {
            poly_rec(target, cells, &new_untried, placed, seen, results);
        }

        cells.pop();
        for p in &placed_added {
            placed.remove(p);
        }
    }
}

/// Check if a Shape fills its bounding box completely (is rectangular).
pub fn is_rectangular_shape(shape: &Shape) -> bool {
    if shape.cells.is_empty() {
        return false;
    }
    (shape.height * shape.width) as usize == shape.cells.len()
}

pub fn is_rectangular(piece: &Piece, grid: &Grid) -> bool {
    if piece.cells.is_empty() {
        return false;
    }
    let mut min_r = grid.rows;
    let mut max_r = 0usize;
    let mut min_c = grid.cols;
    let mut max_c = 0usize;
    for &c in &piece.cells {
        let (r, col) = grid.cell_pos(c);
        min_r = min_r.min(r);
        max_r = max_r.max(r);
        min_c = min_c.min(col);
        max_c = max_c.max(col);
    }
    let expected = (max_r - min_r + 1) * (max_c - min_c + 1);
    piece.cells.len() == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rotation is an involution for 180°, and 90°+90°=180°, 90°+270°=identity
    #[test]
    fn rotation_composition() {
        let pt = (3, 5);
        let (r1, c1) = Rotation::R90.transform(pt.0, pt.1);
        let (r2, c2) = Rotation::R90.transform(r1, c1);
        assert_eq!((r2, c2), Rotation::R180.transform(pt.0, pt.1));
        let (r3, c3) = Rotation::R90.transform(r2, c2);
        assert_eq!((r3, c3), Rotation::R270.transform(pt.0, pt.1));
        let (r4, c4) = Rotation::R90.transform(r3, c3);
        assert_eq!((r4, c4), pt); // full circle
    }

    // canonical of canonical is idempotent
    #[test]
    fn canonical_idempotent() {
        let shapes = [
            "I", "O", "T", "S", "Z", "L", "J", "F", "P", "N", "U", "V", "W", "X", "Y",
        ];
        for &name in &shapes {
            let s = get_named_shape(name).unwrap();
            assert_eq!(
                canonical(&canonical(&s)),
                canonical(&s),
                "canonical not idempotent for {name}"
            );
        }
    }

    // canonical of a rotated shape equals canonical of original
    #[test]
    fn canonical_invariant_under_rotation() {
        let shapes = ["I", "O", "T", "S", "Z", "L", "J"];
        for &name in &shapes {
            let s = get_named_shape(name).unwrap();
            let c = canonical(&s);
            for rot in Rotation::all() {
                let rotated = make_shape(&rotate(&s.cells, rot, false));
                assert_eq!(
                    canonical(&rotated),
                    c,
                    "canonical differs for {name} rotated {:?}",
                    rot
                );
            }
        }
    }

    // canonical of a flipped shape equals canonical of original
    #[test]
    fn canonical_invariant_under_flip() {
        let shapes = ["S", "Z", "L", "J"]; // asymmetrical shapes
        for &name in &shapes {
            let s = get_named_shape(name).unwrap();
            let c = canonical(&s);
            let flipped = make_shape(&rotate(&s.cells, Rotation::R0, true));
            assert_eq!(
                canonical(&flipped),
                c,
                "canonical differs for {name} flipped"
            );
        }
    }

    // S and Z are distinct canonical forms
    // S and Z are mirror images — canonical should be the same under reflection
    #[test]
    fn s_and_z_same_under_canonical() {
        let s = canonical(&get_named_shape("S").unwrap());
        let z = canonical(&get_named_shape("Z").unwrap());
        assert_eq!(
            s, z,
            "S and Z should be same canonical form (mirror images)"
        );
    }

    // L and J are the same under flip
    #[test]
    fn l_and_j_same_under_flip() {
        let l = canonical(&get_named_shape("L").unwrap());
        let j = canonical(&get_named_shape("J").unwrap());
        assert_eq!(l, j, "L and J should be same canonical form");
    }

    #[test]
    fn parse_shape_basic() {
        let lines = ["##.", ".#.", ".##"];
        let shape = parse_shape(&lines);
        assert_eq!(shape.cells.len(), 5);
        assert_eq!(shape.height, 3);
        assert_eq!(shape.width, 3);
    }

    #[test]
    fn is_rectangular_shape_basic() {
        let i = get_named_shape("I").unwrap(); // 1x4 rectangle
        assert!(is_rectangular_shape(&i));
        let o = get_named_shape("O").unwrap(); // 2x2 rectangle
        assert!(is_rectangular_shape(&o));
        let t = get_named_shape("T").unwrap(); // T-shape
        assert!(!is_rectangular_shape(&t));
        let l = get_named_shape("L").unwrap(); // L-shape
        assert!(!is_rectangular_shape(&l));
        let x = get_named_shape("X").unwrap(); // X-pentomino
        assert!(!is_rectangular_shape(&x));
    }

    #[test]
    fn is_rectangular_with_grid() {
        let g = Grid::new(3, 3, true);
        // 2x2 block = rectangular
        let p = Piece {
            cells: vec![
                g.cell_id(0, 0),
                g.cell_id(0, 1),
                g.cell_id(1, 0),
                g.cell_id(1, 1),
            ],
            ..Piece::default()
        };
        assert!(is_rectangular(&p, &g));
        // L-shape = not rectangular
        let p2 = Piece {
            cells: vec![
                g.cell_id(0, 0),
                g.cell_id(1, 0),
                g.cell_id(2, 0),
                g.cell_id(2, 1),
            ],
            ..Piece::default()
        };
        assert!(!is_rectangular(&p2, &g));
    }

    // OEIS A000105: number of free polyominoes
    #[test]
    fn enumerate_counts() {
        let expected = [1, 1, 2, 5, 12, 35, 108, 369];
        for (i, &exp) in expected.iter().enumerate() {
            let n = i + 1;
            let shapes = enumerate_free_polyominoes(n);
            assert_eq!(
                shapes.len(),
                exp,
                "enumerate_free_polyominoes({n}) = {}, expected {exp}",
                shapes.len()
            );
        }
    }

    #[test]
    fn enumerate_all_canonical() {
        for n in 1..=8 {
            for shape in enumerate_free_polyominoes(n) {
                assert_eq!(
                    shape,
                    canonical(&shape),
                    "enumerated shape of size {n} is not in canonical form"
                );
            }
        }
    }

    #[test]
    fn enumerate_n1_monomino() {
        let shapes = enumerate_free_polyominoes(1);
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].cells, vec![(0, 0)]);
    }
}
