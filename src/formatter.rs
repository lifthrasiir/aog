use crate::grid::Grid;
use crate::types::{EdgeState, Piece};

pub fn format_solution(grid: &Grid, edges: &[EdgeState], pieces: &[Piece]) -> String {
    let mut cell_piece = vec![0usize; grid.num_cells()];
    for (p, piece) in pieces.iter().enumerate() {
        for &c in &piece.cells {
            cell_piece[c] = p + 1;
        }
    }

    let mut out = String::new();

    for r in 0..grid.rows {
        // Top edge row
        if r == 0 {
            out.push('+');
            for _c in 0..grid.cols {
                out.push_str("---+");
            }
            out.push('\n');
        }

        // Cell row
        out.push('|');
        for c in 0..grid.cols {
            let cid = grid.cell_id(r, c);
            if !grid.cell_exists[cid] {
                out.push_str("   ");
            } else {
                let pid = cell_piece[cid];
                let label = pid.to_string();
                let pad = 3 - label.len();
                let left = pad / 2;
                let right = pad - left;
                for _ in 0..left {
                    out.push(' ');
                }
                out.push_str(&label);
                for _ in 0..right {
                    out.push(' ');
                }
            }

            if c < grid.cols - 1 {
                let left_c = grid.cell_id(r, c);
                let right_c = grid.cell_id(r, c + 1);
                if !grid.cell_exists[left_c] || !grid.cell_exists[right_c] {
                    out.push('|');
                } else {
                    let eid = grid.v_edge(r, c);
                    if edges[eid] == EdgeState::Cut {
                        out.push('|');
                    } else {
                        out.push(' ');
                    }
                }
            }
        }
        out.push_str("|\n");

        // Bottom edge row
        out.push('+');
        for c in 0..grid.cols {
            let cid = grid.cell_id(r, c);
            if r < grid.rows - 1 {
                let below = grid.cell_id(r + 1, c);
                if !grid.cell_exists[cid] || !grid.cell_exists[below] {
                    out.push_str("---+");
                } else {
                    let eid = grid.h_edge(r, c);
                    if edges[eid] == EdgeState::Cut {
                        out.push_str("---+");
                    } else {
                        out.push_str("   +");
                    }
                }
            } else {
                out.push_str("---+");
            }
        }
        out.push('\n');
    }

    out
}
