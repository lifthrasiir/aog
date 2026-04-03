The goal is to build a solver for a puzzle game called "The Artisan of Glimmith." The puzzle involves dividing a rectangular grid into multiple connected pieces along grid edges. Clues may be given on grid cells, edges, or vertices, or may be global rules that apply to the entire puzzle. The initial large piece may not be square-shaped, and some grid edges may already be split (pre-cut)---in that case, any piece containing a pre-cut edge is not considered a valid answer. For example, given a 2x3 grid with rows a/b and columns 1/2/3, one valid solution with no clues would be splitting into two pieces: {a1, a2, b1, b2} and {a3, b3}. However, if the edge between b1 and b2 is pre-cut, then {a1, a2, b1, b2} is invalid because it would contain that pre-cut edge. Note that if entire grid cells are cut away (as opposed to edges), that is allowed.

Possible global puzzle rules (can be combined):
- shape bank: Every piece must match one of the given shapes. Rotation and reflection are allowed. Shapes are always connected. Note that quite large shapes or shapes with holes may appear.
- precision, minimum, maximum: A piece must have exactly N cells, at least N cells, or at most N cells. These can be treated as a range.
- mingle shape: Adjacent pieces (pieces sharing an edge) must have the same shape after rotation and/or reflection.
- size separation: Adjacent pieces must have different cell counts.
- mismatch: All pieces must have different shapes from each other after rotation and/or reflection.
- match: All pieces must have identical shapes after rotation and/or reflection.
- solitude: Every piece must contain exactly one clue cell---no more, no less.
- boxy: All pieces must be rectangular.
- non-boxy: All pieces must not be rectangular.
- bricky: No boundary vertex may be in contact with exactly 4 split edges---i.e., no cross-shaped junctions are allowed.
- loopy: No boundary vertex may be in contact with exactly 3 split edges---i.e., no T-shaped junctions are allowed.

Possible cell clues (at most one per cell; no duplicates of the same type):
- rose window (A~E): Up to 5 symbol types may appear on cells. If symbol X appears anywhere in the puzzle, then every piece in the solution must contain exactly one cell with symbol X.
- polyomino: A connected polyomino shape is drawn on the cell.
- palisade: Indicates for each edge surrounding the cell whether it is split or not. Rotation is allowed, giving the following possibilities: none split (p0), one direction split (p1), two opposite directions split (p=), two adjacent directions split (p2), three directions split (p3), all four directions split (p4).
- area number (1 2 3 ...): Indicates the area (cell count) of the piece containing this cell.
- compass (c for empty; otherwise a sequence of direction-number pairs like N1E2W3S4): Relative to the clued cell, gives the number of cells within the same piece in each compass direction. North counts all piece cells with a lower row number, South counts all piece cells with a higher row number, East counts all piece cells with a higher column number, and West counts all piece cells with a lower column number. The clued cell itself is not counted. Each pair consists of an uppercase direction letter (N, E, W, S) followed by a non-negative integer. Directions may be omitted when no information is given. The order of pairs is irrelevant and no direction may appear more than once. An entirely empty compass (just c) can exist and still counts as a clue for the solitude rule.

Possible edge clues (at most one per edge; no duplicates of the same type):
- delta (d): The pieces on either side must have different shapes after rotation and/or reflection.
- gemini (g): The pieces on either side must have the same shape after rotation and/or reflection.
- inequality (horizontal edges: ^ v, vertical edges: < >): The area of the piece on the indicated side must be less than (or greater than) the area of the piece on the other side.
- difference (<0> <1> ...; note no space in between): The area difference between the two adjacent pieces must equal the specified number. It does not specify which piece is larger, and no piece can have zero area.

Possible vertex clues (at most one per vertex; no duplicates of the same type):
- watchtower (! @ # $, corresponding to 1---4 dots): Specifies the number of distinct pieces meeting at that vertex. "Distinct" means that if a piece touches the same vertex twice (e.g., due to a hole), it is still counted only once.

Due to the nature of the puzzle, combining multiple rules may produce special implications beyond what each rule states individually. For example, combining bricky and loopy (meaning a boundary vertex can touch at most 2 split edges) implies that every piece---except the largest one---must fit inside a hole of another piece. Identifying these rule interactions and determining exactly what constraints a given puzzle entails is the first step in solving it.

Given a grid shape and a set of clues, build a program that finds the solution. The program must output one of exactly three results: no solution exists, exactly one solution exists (and display it), or two or more solutions exist (and display any one of them).

