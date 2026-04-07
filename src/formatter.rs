use crate::grid::Grid;
use crate::parser::Parser;
use crate::polyomino;
use crate::types::{CellClue, CellId, EdgeClueKind, EdgeId, EdgeState, PalisadeKind, Piece, VertexId};
use std::collections::{HashMap, HashSet};

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

// ── JSON parse output ──────────────────────────────────────────────────────────

fn cell_pos_str(grid: &Grid, cid: CellId) -> String {
    let (r, c) = grid.cell_pos(cid);
    format!("{}{}", (b'a' + r as u8) as char, c + 1)
}

fn shape_to_name(shape: &crate::types::Shape) -> Option<&'static str> {
    let canon = polyomino::canonical(shape);
    let names = [
        "o", "oo", "ooo", "8o", "I", "O", "T", "S", "Z", "L", "J", "F", "P", "N", "U", "V",
        "W", "X", "Y", "II", "LL", "TT", "ZZ",
    ];
    for &name in &names {
        if let Some(named) = polyomino::get_named_shape(name) {
            if canon == polyomino::canonical(&named) {
                return Some(name);
            }
        }
    }
    None
}

fn shape_to_grid(shape: &crate::types::Shape) -> String {
    let mut rows = vec![vec!['.'; shape.width as usize]; shape.height as usize];
    for &(r, c) in &shape.cells {
        rows[r as usize][c as usize] = '#';
    }
    rows.iter().map(|row| row.iter().collect::<String>()).collect::<Vec<_>>().join("\n")
}

fn format_cell_clue(cl: &CellClue) -> String {
    match cl {
        CellClue::Area { value, .. } => value.to_string(),
        CellClue::Rose { symbol, .. } => {
            format!("{}", (b'A' + *symbol as u8) as char)
        }
        CellClue::Polyomino { shape, .. } => match shape_to_name(shape) {
            Some(name) => name.to_string(),
            None => shape_to_grid(shape),
        },
        CellClue::Palisade { kind, .. } => match kind {
            PalisadeKind::None => "p0",
            PalisadeKind::One => "p1",
            PalisadeKind::Opposite => "p=",
            PalisadeKind::Adjacent => "p2",
            PalisadeKind::Three => "p3",
            PalisadeKind::All => "p4",
        }
        .to_string(),
        CellClue::Compass { compass, .. } => {
            let mut parts = Vec::new();
            if let Some(n) = compass.n {
                parts.push(format!("N{}", n));
            }
            if let Some(e) = compass.e {
                parts.push(format!("E{}", e));
            }
            if let Some(w) = compass.w {
                parts.push(format!("W{}", w));
            }
            if let Some(s) = compass.s {
                parts.push(format!("S{}", s));
            }
            if parts.is_empty() {
                "c".to_string()
            } else {
                parts.join("")
            }
        }
    }
}

fn format_edge_clue(kind: EdgeClueKind, is_horizontal: bool) -> String {
    match kind {
        EdgeClueKind::Delta => "d".to_string(),
        EdgeClueKind::Gemini => "g".to_string(),
        EdgeClueKind::Inequality { smaller_first } => {
            if is_horizontal {
                if smaller_first {
                    "^"
                } else {
                    "v"
                }
            } else if smaller_first {
                "<"
            } else {
                ">"
            }
            .to_string()
        }
        EdgeClueKind::Diff { value } => format!("<{}>", value),
    }
}

fn format_vertex_clue(value: usize) -> String {
    match value {
        1 => "!",
        2 => "@",
        3 => "#",
        4 => "$",
        _ => "?",
    }
    .to_string()
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn parse_solution_piece_map(parser: &Parser) -> Option<Vec<(String, usize)>> {
    let start = parser.lines.iter().position(|l| {
        let t = l.trim_start();
        t.strip_prefix('#')
            .map(|r| r.trim_start().starts_with('+'))
            .unwrap_or(false)
    })?;

    let mut piece_tokens: Vec<usize> = Vec::new();
    for line in &parser.lines[start..] {
        let t = line.trim_start();
        let Some(inner) = t.strip_prefix('#') else {
            break;
        };
        let bytes = inner.as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            if bytes[j].is_ascii_digit() {
                let s = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if let Ok(n) = inner[s..j].parse::<usize>() {
                    piece_tokens.push(n);
                }
            } else {
                j += 1;
            }
        }
    }

    let existing: Vec<usize> = (0..parser.grid.num_cells())
        .filter(|&c| parser.grid.cell_exists[c])
        .collect();

    if piece_tokens.len() != existing.len() {
        return None;
    }

    Some(
        existing
            .iter()
            .zip(piece_tokens.iter())
            .map(|(&c, &p)| (cell_pos_str(&parser.grid, c), p))
            .collect(),
    )
}

/// Format grouped JSON entries: consecutive entries with the same row key
/// are placed on one line, separated by ", "; different rows get new lines
/// with a trailing comma on the previous row.
fn format_grouped(entries: &[(usize, String)]) -> String {
    let mut result = String::new();
    let mut current_row = usize::MAX;
    let mut first_in_line = true;
    for &(row, ref entry) in entries {
        if row != current_row {
            if current_row != usize::MAX {
                result.push_str(",\n");
            }
            current_row = row;
            first_in_line = true;
        }
        if !first_in_line {
            result.push_str(", ");
        }
        first_in_line = false;
        result.push_str(entry);
    }
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

/// Wrap grouped entries into a JSON object body with indentation.
fn format_grouped_object(entries: &[(usize, String)], indent: &str) -> String {
    let body = format_grouped(entries);
    if body.is_empty() {
        "{}".to_string()
    } else {
        let indented: String = body
            .lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{}{}", indent, line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("{{\n{}}}", indented.trim_end())
    }
}

pub fn format_parse_output(parser: &Parser) -> String {
    let grid = &parser.grid;
    let puzzle = &parser.puzzle;

    let pre_cut: HashSet<EdgeId> = parser.pre_cut_edges.iter().copied().collect();
    let edge_clues: HashMap<EdgeId, &crate::types::EdgeClue> =
        puzzle.edge_clues.iter().map(|ec| (ec.edge, ec)).collect();
    let vertex_clues: HashMap<VertexId, &crate::types::VertexClue> =
        puzzle.vertex_clues.iter().map(|vc| (vc.vertex, vc)).collect();
    let cell_clues: HashMap<CellId, &CellClue> =
        puzzle.cell_clues.iter().map(|cc| (cc.cell(), cc)).collect();

    // Rules
    let mut rules: Vec<String> = Vec::new();
    let r = &puzzle.rules;
    if r.mingle_shape {
        rules.push("mingle shape".into());
    }
    if r.size_separation {
        rules.push("size separation".into());
    }
    if r.mismatch {
        rules.push("mismatch".into());
    }
    if r.match_all {
        rules.push("match".into());
    }
    if r.solitude {
        rules.push("solitude".into());
    }
    if r.boxy {
        rules.push("boxy".into());
    }
    if r.non_boxy {
        rules.push("non-boxy".into());
    }
    if r.bricky {
        rules.push("bricky".into());
    }
    if r.loopy {
        rules.push("loopy".into());
    }
    if let Some(v) = r.minimum {
        if let Some(u) = r.maximum {
            if v == u {
                rules.push(format!("precision {}", v));
            } else {
                rules.push(format!("minimum {}", v));
                rules.push(format!("maximum {}", u));
            }
        } else {
            rules.push(format!("minimum {}", v));
        }
    } else if let Some(u) = r.maximum {
        rules.push(format!("maximum {}", u));
    }
    if !r.shape_bank.is_empty() {
        let mut parts: Vec<String> = Vec::new();
        let mut grid_shapes: Vec<String> = Vec::new();
        for shape in &r.shape_bank {
            match shape_to_name(shape) {
                Some(name) => parts.push(name.to_string()),
                None => grid_shapes.push(shape_to_grid(shape)),
            }
        }
        if !parts.is_empty() {
            let mut s = "shape bank ".to_string();
            s.push_str(&parts.join(" "));
            if !grid_shapes.is_empty() {
                s.push('\n');
                s.push_str(&grid_shapes.join("\n\n"));
            }
            rules.push(s);
        } else {
            let mut s = "shape bank\n".to_string();
            s.push_str(&grid_shapes.join("\n\n"));
            rules.push(s);
        }
    }

    // Cells: (row, "key": "value")
    let mut cells: Vec<(usize, String)> = Vec::new();
    for cid in 0..grid.num_cells() {
        if !grid.cell_exists[cid] {
            continue;
        }
        let (r, _) = grid.cell_pos(cid);
        let pos = cell_pos_str(grid, cid);
        let value = match cell_clues.get(&cid) {
            Some(cl) => json_escape(&format_cell_clue(cl)),
            None => "_".to_string(),
        };
        cells.push((r, format!("\"{}\": \"{}\"", pos, value)));
    }

    // Edges: (row of first cell, "key": "value")
    let mut edges: Vec<(usize, String)> = Vec::new();
    for e in 0..grid.num_edges() {
        let (c1, c2) = grid.edge_cells(e);
        if !grid.cell_exists[c1] || !grid.cell_exists[c2] {
            continue;
        }

        let (r1, _) = grid.cell_pos(c1);
        let (r2, _) = grid.cell_pos(c2);
        let is_h = r1 != r2;
        let pos1 = cell_pos_str(grid, c1);
        let pos2 = cell_pos_str(grid, c2);
        let key = if is_h {
            format!("{}-{}", pos1, pos2)
        } else {
            format!("{}|{}", pos1, pos2)
        };

        let value = if let Some(ec) = edge_clues.get(&e) {
            format_edge_clue(ec.kind, is_h)
        } else if pre_cut.contains(&e) {
            if is_h {
                "-"
            } else {
                "|"
            }
            .to_string()
        } else {
            continue;
        };

        edges.push((r1, format!("\"{}\": \"{}\"", key, value)));
    }

    // Vertices: (row of top-left cell, "key": "value")
    let mut vertices: Vec<(usize, String)> = Vec::new();
    for i in 1..grid.rows {
        for j in 1..grid.cols {
            let tl = grid.cell_id(i - 1, j - 1);
            let br = grid.cell_id(i, j);
            if !grid.cell_exists[tl] || !grid.cell_exists[br] {
                continue;
            }

            let vid = grid.vertex(i, j);
            let Some(&vc) = vertex_clues.get(&vid) else {
                continue;
            };

            let key = format!("{}+{}", cell_pos_str(grid, tl), cell_pos_str(grid, br));
            vertices.push((i - 1, format!("\"{}\": \"{}\"", key, format_vertex_clue(vc.value))));
        }
    }

    // Solution (optional): (row, "key": value)
    let solution = parse_solution_piece_map(parser).map(|sol| {
        sol.into_iter()
            .map(|(pos, idx)| {
                let row = (pos.as_bytes()[0] - b'a') as usize;
                (row, format!("\"{}\": {}", pos, idx))
            })
            .collect::<Vec<_>>()
    });

    // Assemble JSON
    let mut json = String::from("{\n");
    json.push_str("  \"rules\": ");
    json.push_str(&format_json_array(&rules, "    "));
    json.push_str(",\n");
    json.push_str("  \"cells\": ");
    json.push_str(&format_grouped_object(&cells, "    "));
    json.push_str(",\n");
    json.push_str("  \"edges\": ");
    json.push_str(&format_grouped_object(&edges, "    "));
    json.push_str(",\n");
    json.push_str("  \"vertices\": ");
    json.push_str(&format_grouped_object(&vertices, "    "));
    if let Some(ref sol) = solution {
        json.push_str(",\n  \"solution\": ");
        json.push_str(&format_grouped_object(sol, "    "));
        json.push('\n');
    } else {
        json.push('\n');
    }
    json.push_str("}\n");
    json
}

fn format_json_array(items: &[String], indent: &str) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let mut body = String::new();
    for (i, item) in items.iter().enumerate() {
        body.push_str(indent);
        body.push('"');
        body.push_str(&json_escape(item));
        body.push('"');
        if i + 1 < items.len() {
            body.push(',');
        }
        body.push('\n');
    }
    format!("[\n{}]", body.trim_end())
}
