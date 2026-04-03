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
        if r == 0 {
            border_row(grid, edges, &mut out, 0);
            out.push('\n');
        }

        cell_row(grid, edges, &cell_piece, &mut out, r);
        out.push('\n');

        border_row(grid, edges, &mut out, r + 1);
        out.push('\n');
    }

    out
}

fn cell_exists_at(grid: &Grid, r: usize, c: usize) -> bool {
    r < grid.rows && c < grid.cols && grid.cell_exists[grid.cell_id(r, c)]
}

fn should_draw_plus(grid: &Grid, vi: usize, vj: usize) -> bool {
    cell_exists_at(grid, vi.wrapping_sub(1), vj.wrapping_sub(1))
        || cell_exists_at(grid, vi.wrapping_sub(1), vj)
        || cell_exists_at(grid, vi, vj.wrapping_sub(1))
        || cell_exists_at(grid, vi, vj)
}

fn h_segment(grid: &Grid, edges: &[EdgeState], vertex_row: usize, c: usize) -> &'static str {
    let above = cell_exists_at(grid, vertex_row.wrapping_sub(1), c);
    let below = cell_exists_at(grid, vertex_row, c);
    if !above && !below {
        "   "
    } else if above && below {
        if edges[grid.h_edge(vertex_row - 1, c)] == EdgeState::Cut {
            "---"
        } else {
            "   "
        }
    } else {
        "---"
    }
}

fn v_separator(grid: &Grid, edges: &[EdgeState], r: usize, left_c: usize) -> char {
    let left = cell_exists_at(grid, r, left_c);
    let right = cell_exists_at(grid, r, left_c + 1);
    if !left && !right {
        ' '
    } else if left && right {
        if edges[grid.v_edge(r, left_c)] == EdgeState::Cut {
            '|'
        } else {
            ' '
        }
    } else {
        '|'
    }
}

fn border_row(grid: &Grid, edges: &[EdgeState], out: &mut String, vertex_row: usize) {
    for c in 0..grid.cols {
        out.push(if should_draw_plus(grid, vertex_row, c) {
            '+'
        } else {
            ' '
        });
        out.push_str(h_segment(grid, edges, vertex_row, c));
    }
    out.push(if should_draw_plus(grid, vertex_row, grid.cols) {
        '+'
    } else {
        ' '
    });
}

fn cell_row(grid: &Grid, edges: &[EdgeState], cell_piece: &[usize], out: &mut String, r: usize) {
    for c in 0..grid.cols {
        // Left border
        if c == 0 {
            out.push(if cell_exists_at(grid, r, 0) { '|' } else { ' ' });
        } else {
            out.push(v_separator(grid, edges, r, c - 1));
        }

        // Cell content
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
    }
    // Right border
    out.push(if cell_exists_at(grid, r, grid.cols - 1) {
        '|'
    } else {
        ' '
    });
}
