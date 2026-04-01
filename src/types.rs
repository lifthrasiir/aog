pub type CellId = usize;
pub type EdgeId = usize;
pub type VertexId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeState {
    Unknown,
    Cut,
    Uncut,
}

/// Ordered by (height, width, cells) so canonical forms can be compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shape {
    pub height: i32,
    pub width: i32,
    pub cells: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct Piece {
    pub cells: Vec<CellId>,
    pub area: usize,
    pub canonical: Shape,
}

#[derive(Clone, Debug, Default)]
pub struct CompassData {
    pub e: Option<usize>,
    pub w: Option<usize>,
    pub s: Option<usize>,
    pub n: Option<usize>,
}

/// Palisade clue: how the 4 edges around a cell are cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PalisadeKind {
    /// p0: no edges cut
    None,
    /// p1: exactly one edge cut
    One,
    /// p=: two opposite edges cut
    Opposite,
    /// p2: two adjacent edges cut
    Adjacent,
    /// p3: three edges cut
    Three,
    /// p4: all four edges cut
    All,
}

impl PalisadeKind {
    pub fn cut_count(self) -> usize {
        match self {
            Self::None => 0,
            Self::One => 1,
            Self::Opposite | Self::Adjacent => 2,
            Self::Three => 3,
            Self::All => 4,
        }
    }

    /// Returns (expected_cut_count, edge_mask) for a given rotation.
    /// The mask bits correspond to the cell_edges ordering: top(0), bottom(1), left(2), right(3).
    /// Rotations go clockwise: top → right → bottom → left, so the cycle in bit positions is
    /// [0, 3, 1, 2] (top=bit0, right=bit3, bottom=bit1, left=bit2).
    pub fn pattern_at_rotation(self, rot: usize) -> (usize, u8) {
        // Clockwise cycle in cell_edges bit positions: top=0, right=3, bottom=1, left=2
        const CYCLE: [u8; 4] = [0, 3, 1, 2];
        let bit = |i: usize| -> u8 { 1 << CYCLE[i % 4] };
        match self {
            Self::None => (0, 0),
            Self::One => (1, bit(rot)),
            Self::Opposite => (2, bit(rot) | bit(rot + 2)),
            Self::Adjacent => (2, bit(rot) | bit(rot + 1)),
            Self::Three => (3, 0xF & !bit(rot)),
            Self::All => (4, 0xF),
        }
    }
}

#[derive(Clone, Debug)]
pub enum CellClue {
    Area { cell: CellId, value: usize },
    Rose { cell: CellId, symbol: u8 },
    Polyomino { cell: CellId, shape: Shape },
    Palisade { cell: CellId, kind: PalisadeKind },
    Compass { cell: CellId, compass: CompassData },
}

impl CellClue {
    pub fn cell(&self) -> CellId {
        match self {
            Self::Area { cell, .. }
            | Self::Rose { cell, .. }
            | Self::Polyomino { cell, .. }
            | Self::Palisade { cell, .. }
            | Self::Compass { cell, .. } => *cell,
        }
    }
}

/// Edge clue: semantic meaning of a cut edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeClueKind {
    Delta,
    Gemini,
    /// true = cell with smaller row/col index has smaller area
    Inequality {
        smaller_first: bool,
    },
    Diff {
        value: usize,
    },
}

#[derive(Clone, Debug)]
pub struct EdgeClue {
    pub edge: EdgeId,
    pub kind: EdgeClueKind,
}

#[derive(Clone, Debug)]
pub struct VertexClue {
    pub vertex: VertexId,
    pub value: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GlobalRules {
    pub shape_bank: Vec<Shape>,
    pub minimum: Option<usize>,
    pub maximum: Option<usize>,
    pub mingle_shape: bool,
    pub size_separation: bool,
    pub mismatch: bool,
    pub match_all: bool,
    pub solitude: bool,
    pub boxy: bool,
    pub non_boxy: bool,
    pub bricky: bool,
    pub loopy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Puzzle {
    pub cell_clues: Vec<CellClue>,
    pub edge_clues: Vec<EdgeClue>,
    pub vertex_clues: Vec<VertexClue>,
    pub rules: GlobalRules,
}
