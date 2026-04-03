use crate::grid::Grid;
use crate::polyomino;
use crate::types::*;
use std::fmt;

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    pub puzzle: Puzzle,
    pub grid: Grid,
    pub lines: Vec<String>,
    pub grid_start: usize,
    pub grid_end: usize,
    pub pre_cut_edges: Vec<EdgeId>,
}

fn strip_comment(s: &mut String) {
    if let Some(pos) = s.find('#') {
        s.truncate(pos);
    }
    *s = s.trim().to_owned();
}

fn set_precision(rules: &mut GlobalRules, val: usize) {
    rules.minimum = Some(val);
    rules.maximum = Some(val);
}

struct CellToken {
    col: usize,
    text: String,
}

impl Parser {
    pub fn parse<R: std::io::BufRead>(&mut self, reader: R) -> Result<(), ParseError> {
        let mut lines = Vec::new();
        for line in reader.lines() {
            let mut line = line.unwrap_or_default();
            if line.ends_with('\r') {
                line.pop();
            }
            // Preserve leading whitespace (needed for column-position-based parsing)
            // but skip truly blank lines.
            let trimmed_end = line.trim_end();
            if !trimmed_end.trim().is_empty() {
                lines.push(trimmed_end.to_owned());
            }
        }
        self.lines = lines;
        if self.lines.is_empty() {
            return Err(ParseError("empty input".into()));
        }

        self.grid_start = self
            .lines
            .iter()
            .position(|l| {
                let t = l.trim_start();
                t.len() >= 3 && t.starts_with('+') && l.ends_with('+')
            })
            .ok_or_else(|| ParseError("no grid found".into()))?;

        self.grid_end = self.grid_start;
        for i in self.grid_start..self.lines.len() {
            let l = &self.lines[i];
            let t = l.trim_start();
            if !t.is_empty() && t.starts_with('+') && l.ends_with('+') {
                self.grid_end = i;
            } else if t.starts_with('|') && l.ends_with('|') {
                self.grid_end = i;
            } else {
                break;
            }
        }

        self.parse_rules(0, self.grid_start);
        self.parse_visual_grid()?;
        self.parse_supplementary(self.grid_end + 1);
        Ok(())
    }

    fn parse_rules(&mut self, from: usize, to: usize) {
        let mut i = from;
        while i < to {
            let mut line = self.lines[i].clone();
            strip_comment(&mut line);
            if line.is_empty() {
                i += 1;
                continue;
            }

            if line == "precision" {
                i += 1;
                while i < to && self.lines[i].is_empty() {
                    i += 1;
                }
                if i < to {
                    if let Ok(val) = self.lines[i].trim().parse() {
                        set_precision(&mut self.puzzle.rules, val);
                    }
                }
            } else if let Some(rest) = line.strip_prefix("precision ") {
                if let Ok(val) = rest.trim().parse() {
                    set_precision(&mut self.puzzle.rules, val);
                }
            } else if let Some(rest) = line.strip_prefix("minimum ") {
                self.puzzle.rules.minimum = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("maximum ") {
                self.puzzle.rules.maximum = rest.trim().parse().ok();
            } else if line == "mingle shape" {
                self.puzzle.rules.mingle_shape = true;
            } else if line == "size separation" {
                self.puzzle.rules.size_separation = true;
            } else if line == "mismatch" {
                self.puzzle.rules.mismatch = true;
            } else if line == "match" {
                self.puzzle.rules.match_all = true;
            } else if line == "solitude" {
                self.puzzle.rules.solitude = true;
            } else if line == "boxy" {
                self.puzzle.rules.boxy = true;
            } else if line == "non-boxy" {
                self.puzzle.rules.non_boxy = true;
            } else if line == "bricky" {
                self.puzzle.rules.bricky = true;
            } else if line == "loopy" {
                self.puzzle.rules.loopy = true;
            } else if line == "shape bank" || line.starts_with("shape bank ") {
                self.parse_shape_bank(&mut i);
            }
            i += 1;
        }
    }

    fn parse_shape_bank(&mut self, idx: &mut usize) {
        let line = &self.lines[*idx];
        let rest = if line.len() > 11 {
            line[11..].trim().to_owned()
        } else {
            String::new()
        };

        if !rest.is_empty() {
            for name in rest.split_whitespace() {
                if let Some(shape) = polyomino::get_named_shape(name) {
                    self.puzzle.rules.shape_bank.push(shape);
                } else {
                    eprintln!("Unknown shape: {}", name);
                }
            }
            return;
        }

        *idx += 1;
        while *idx < self.grid_start {
            if self.lines[*idx].is_empty() {
                *idx += 1;
                continue;
            }
            let mut shape_lines = Vec::new();
            while *idx < self.grid_start && !self.lines[*idx].is_empty() {
                let sl = &self.lines[*idx];
                if sl.trim().starts_with('#') || !sl.trim().chars().any(|c| c != '#' && c != '.') {
                    shape_lines.push(sl.trim());
                    *idx += 1;
                } else {
                    break;
                }
            }
            if !shape_lines.is_empty() {
                self.puzzle
                    .rules
                    .shape_bank
                    .push(polyomino::parse_shape(&shape_lines));
            }
        }
        *idx -= 1;
    }

    fn parse_visual_grid(&mut self) -> Result<(), ParseError> {
        // --- Step 1: Build global column map ---
        let grid_line_cols = self.build_column_map();

        let cols = grid_line_cols.len().saturating_sub(1);
        if cols < 1 {
            return Err(ParseError("invalid grid width".into()));
        }

        let rows = (self.grid_start..=self.grid_end)
            .filter(|&i| {
                let t = self.lines[i].trim_start();
                !t.is_empty() && t.starts_with('|')
            })
            .count();
        if rows < 1 {
            return Err(ParseError("no cell rows found".into()));
        }

        self.grid = Grid::new(rows, cols, true);

        // --- Step 2: Parse cell rows ---
        let mut row = 0usize;
        for i in self.grid_start..=self.grid_end {
            let line = self.lines[i].clone();
            let t = line.trim_start();
            if t.is_empty() || !t.starts_with('|') {
                continue;
            }

            let tokens = Self::scan_cell_tokens(&line);

            // Map token positions to grid line indices.
            // Multi-char tokens (e.g. <0>) may start before the grid column;
            // match if any byte within the token falls on a grid column.
            let mut token_indices: Vec<usize> = Vec::new();
            let mut token_at_line: Vec<Option<(usize, String)>> =
                vec![None; grid_line_cols.len()];
            for tok in &tokens {
                if let Some(idx) =
                    Self::match_token_to_grid_col(tok.col, tok.text.len(), &grid_line_cols)
                {
                    token_at_line[idx] = Some((tok.col, tok.text.clone()));
                    token_indices.push(idx);
                }
            }
            token_indices.sort();
            token_indices.dedup();

            // Determine covered grid lines (direct tokens only — gaps are NOT cells)
            let mut covered = vec![false; grid_line_cols.len()];
            for &idx in &token_indices {
                covered[idx] = true;
            }

            for c in 0..cols {
                let cid = self.grid.cell_id(row, c);
                if !covered[c] || !covered[c + 1] {
                    self.grid.cell_exists[cid] = false;
                    continue;
                }

                let both_explicit = token_at_line[c].is_some() && token_at_line[c + 1].is_some();

                if both_explicit {
                    // Use actual token byte positions for content extraction,
                    // not grid column positions, because multi-char tokens
                    // may start before the grid column they belong to.
                    let (left_col, left_text) = token_at_line[c].as_ref().unwrap();
                    let (right_col, _) = token_at_line[c + 1].as_ref().unwrap();
                    let content_start = left_col + left_text.len();
                    let content_end = *right_col;
                    let content = if content_start < content_end {
                        line[content_start..content_end].trim()
                    } else {
                        ""
                    };
                    self.parse_cell_content(content, cid);
                }

                if c + 1 < cols {
                    if let Some((_, ref text)) = token_at_line[c + 1] {
                        self.parse_v_separator(row, c, text);
                    }
                }
            }
            // Pre-cut vertical edges where one adjacent cell exists and the other doesn't
            for c in 0..cols.saturating_sub(1) {
                let left_cid = self.grid.cell_id(row, c);
                let right_cid = self.grid.cell_id(row, c + 1);
                if self.grid.cell_exists[left_cid] != self.grid.cell_exists[right_cid] {
                    self.pre_cut_edges.push(self.grid.v_edge(row, c));
                }
            }

            row += 1;
        }

        // --- Step 3: Parse edge rows ---
        let mut edge_row_idx = 0usize;
        for i in self.grid_start..=self.grid_end {
            let line = self.lines[i].clone();
            let t = line.trim_start();
            if t.is_empty() || !t.starts_with('+') {
                continue;
            }

            let r = edge_row_idx as isize - 1;

            // Find + and vertex-symbol positions, map to grid line indices.
            // Watchtower symbols ! @ # $ at internal vertices create vertex clues.
            let mut plus_indices: Vec<usize> = Vec::new();
            for (col, ch) in line.char_indices() {
                if matches!(ch, '+' | '!' | '@' | '#' | '$') {
                    if let Ok(idx) = grid_line_cols.binary_search(&col) {
                        plus_indices.push(idx);
                        if ch != '+' {
                            let val: usize = match ch {
                                '!' => 1, '@' => 2, '#' => 3, '$' => 4, _ => 0,
                            };
                            self.puzzle.vertex_clues.push(VertexClue {
                                vertex: self.grid.vertex(edge_row_idx, idx),
                                value: val,
                            });
                        }
                    }
                }
            }

            // Build lookup: grid line index → has_plus (or vertex symbol)
            let mut has_plus: Vec<bool> = vec![false; grid_line_cols.len()];
            for &idx in &plus_indices {
                has_plus[idx] = true;
            }

            for c in 0..cols {
                if r >= 0 && r < (rows - 1) as isize {
                    let eid = self.grid.h_edge(r as usize, c);

                    if has_plus[c] && has_plus[c + 1] {
                        let left_pos = grid_line_cols[c];
                        let right_pos = grid_line_cols[c + 1];
                        let seg = if left_pos + 1 < right_pos {
                            line[left_pos + 1..right_pos].trim().to_string()
                        } else {
                            String::new()
                        };
                        self.process_edge_segment(&seg, eid);
                    } else {
                        self.pre_cut_edges.push(eid);
                    }
                }
            }
            edge_row_idx += 1;
        }

        Ok(())
    }

    /// Collect `+` (and watchtower-symbol) positions from all edge rows to define grid line columns.
    fn build_column_map(&self) -> Vec<usize> {
        let mut cols_set = std::collections::HashSet::new();
        for i in self.grid_start..=self.grid_end {
            let line = &self.lines[i];
            let t = line.trim_start();
            if t.starts_with('+') {
                for (col, ch) in line.char_indices() {
                    if matches!(ch, '+' | '!' | '@' | '#' | '$') {
                        cols_set.insert(col);
                    }
                }
            }
        }
        let mut cols: Vec<usize> = cols_set.into_iter().collect();
        cols.sort();
        cols
    }

    /// Scan a cell row for structural token positions and their text.
    /// Handles single-char separators (|, ., d, g, <, >, ^, v) plus
    /// the multi-char difference notation <N> (e.g. <3>).
    fn scan_cell_tokens(line: &str) -> Vec<CellToken> {
        let bytes = line.as_bytes();
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            match ch {
                '|' | '.' | 'd' | 'g' | '^' | 'v' | '>' => {
                    tokens.push(CellToken { col: i, text: ch.to_string() });
                    i += 1;
                }
                '<' => {
                    // Check for <N> difference pattern: '<' digits '>'
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j > i + 1 && j < bytes.len() && bytes[j] == b'>' {
                        tokens.push(CellToken { col: i, text: line[i..=j].to_owned() });
                        i = j + 1;
                    } else {
                        tokens.push(CellToken { col: i, text: '<'.to_string() });
                        i += 1;
                    }
                }
                _ => { i += 1; }
            }
        }
        tokens
    }

    /// Find the grid line column index whose byte position falls within a
    /// token's byte range [tok_col, tok_col + tok_len).  Returns None when
    /// no grid column overlaps the token.
    fn match_token_to_grid_col(
        tok_col: usize,
        tok_len: usize,
        grid_line_cols: &[usize],
    ) -> Option<usize> {
        let end = tok_col + tok_len;
        let start_idx = grid_line_cols.partition_point(|&c| c < tok_col);
        if start_idx < grid_line_cols.len() && grid_line_cols[start_idx] < end {
            Some(start_idx)
        } else {
            None
        }
    }

    /// Parse cell content string and register any clues.
    fn parse_cell_content(&mut self, content: &str, cid: CellId) {
        if content.is_empty() {
            self.grid.cell_exists[cid] = false;
        } else if content == "_" {
            // empty cell — no clue
        } else if !content.is_empty() && content.chars().all(|c| c.is_ascii_digit()) {
            self.puzzle.cell_clues.push(CellClue::Area {
                cell: cid,
                value: content.parse().unwrap(),
            });
        } else if content.len() == 1 {
            let ch = content.as_bytes()[0];
            if ch >= b'A' && ch <= b'E' {
                self.puzzle.cell_clues.push(CellClue::Rose {
                    cell: cid,
                    symbol: (ch - b'A') as u8,
                });
            } else if ch >= b'F' && ch <= b'Z' {
                if let Some(shape) = polyomino::get_named_shape(content) {
                    self.puzzle
                        .cell_clues
                        .push(CellClue::Polyomino { cell: cid, shape });
                }
            }
        } else if content.starts_with('p') {
            if let Some(kind) = Self::parse_palistr(content) {
                self.puzzle
                    .cell_clues
                    .push(CellClue::Palisade { cell: cid, kind });
            }
        } else if content == "c" {
            self.puzzle.cell_clues.push(CellClue::Compass {
                cell: cid,
                compass: CompassData::default(),
            });
        } else if Self::is_compact_compass(content) {
            let mut compass = CompassData::default();
            let bytes = content.as_bytes();
            let mut pos = 0;
            while pos < bytes.len() {
                let dir = bytes[pos] as char;
                pos += 1;
                let start = pos;
                while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
                let val: usize = content[start..pos].parse().unwrap_or(0);
                match dir {
                    'N' => compass.n = Some(val),
                    'E' => compass.e = Some(val),
                    'W' => compass.w = Some(val),
                    'S' => compass.s = Some(val),
                    _ => {}
                }
            }
            self.puzzle
                .cell_clues
                .push(CellClue::Compass { cell: cid, compass });
        } else if let Some(shape) = polyomino::get_named_shape(content) {
            self.puzzle
                .cell_clues
                .push(CellClue::Polyomino { cell: cid, shape });
        }
    }

    fn is_compact_compass(s: &str) -> bool {
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            return false;
        }
        let mut pos = 0;
        while pos < bytes.len() {
            if !matches!(bytes[pos], b'N' | b'E' | b'W' | b'S') {
                return false;
            }
            pos += 1;
            if pos >= bytes.len() || !bytes[pos].is_ascii_digit() {
                return false;
            }
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
        }
        true
    }

    /// Process a horizontal edge segment (text between two '+' tokens).
    fn process_edge_segment(&mut self, seg: &str, eid: EdgeId) {
        // Single dot means "unknown edge" — no action
        if seg == "." {
            return;
        }
        // Empty or all-dashes means "cut edge"
        if seg.is_empty() || seg.bytes().all(|b| b == b'-') {
            self.pre_cut_edges.push(eid);
            return;
        }
        // Strip surrounding dashes to get the inner symbol
        let inner = seg.trim_matches('-');
        if inner.len() == 1 {
            let clue_char = inner.as_bytes()[0] as char;
            self.pre_cut_edges.push(eid);
            if let Some(kind) = Self::edge_char_to_kind(clue_char) {
                self.puzzle.edge_clues.push(EdgeClue { edge: eid, kind });
            }
        } else if inner.starts_with('<') && inner.ends_with('>') {
            // Difference clue: <N>
            let num_str = &inner[1..inner.len() - 1];
            if let Ok(val) = num_str.parse::<usize>() {
                self.pre_cut_edges.push(eid);
                self.puzzle.edge_clues.push(EdgeClue {
                    edge: eid,
                    kind: EdgeClueKind::Diff { value: val },
                });
            }
        }
    }

    fn edge_char_to_kind(ch: char) -> Option<EdgeClueKind> {
        Some(match ch {
            '^' | '<' => EdgeClueKind::Inequality {
                smaller_first: true,
            },
            'v' | '>' => EdgeClueKind::Inequality {
                smaller_first: false,
            },
            'd' => EdgeClueKind::Delta,
            'g' => EdgeClueKind::Gemini,
            _ => return None,
        })
    }

    fn parse_v_separator(&mut self, row: usize, col: usize, sep: &str) {
        if sep == "." {
            return;
        }
        let eid = self.grid.v_edge(row, col);
        self.pre_cut_edges.push(eid);

        if sep != " " {
            if sep.len() == 1 {
                let ch = sep.as_bytes()[0] as char;
                if let Some(kind) = Self::edge_char_to_kind(ch) {
                    self.puzzle.edge_clues.push(EdgeClue { edge: eid, kind });
                }
            } else if sep.starts_with('<') && sep.ends_with('>') {
                // Difference clue: <N>
                let inner = &sep[1..sep.len() - 1];
                if let Ok(val) = inner.parse::<usize>() {
                    self.puzzle.edge_clues.push(EdgeClue {
                        edge: eid,
                        kind: EdgeClueKind::Diff { value: val },
                    });
                }
            }
        }
    }

    /// Parse a watchtower value: accepts symbols ! @ # $ (w1–w4) or bare digits.
    fn parse_vertex_value(s: &str) -> usize {
        match s {
            "!" => 1,
            "@" => 2,
            "#" => 3,
            "$" => 4,
            _ => s.parse().unwrap_or(0),
        }
    }

    fn parse_palistr(s: &str) -> Option<PalisadeKind> {
        Some(match s {
            "p0" => PalisadeKind::None,
            "p1" => PalisadeKind::One,
            "p=" => PalisadeKind::Opposite,
            "p2" => PalisadeKind::Adjacent,
            "p3" => PalisadeKind::Three,
            "p4" => PalisadeKind::All,
            _ => return None,
        })
    }

    fn parse_supplementary(&mut self, from: usize) {
        let mut i = from;
        while i < self.lines.len() {
            let mut line = self.lines[i].clone();
            strip_comment(&mut line);
            if line.is_empty() {
                i += 1;
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.is_empty() {
                i += 1;
                continue;
            }
            let cmd = parts[0];

            if cmd == "vertex" {
                // Parse from the raw (unstripped) line so that '#' (watchtower w3)
                // is not consumed by the comment stripper.
                let raw_parts: Vec<&str> = self.lines[i].split_whitespace().collect();
                if raw_parts.len() >= 3 {
                    let addr = raw_parts[1];
                    let val = Self::parse_vertex_value(raw_parts[2]);
                    let r = (addr.as_bytes()[0] - b'a') as usize;
                    let c: usize = addr[1..].parse().unwrap_or(1) - 1;
                    if val > 0 && r <= self.grid.rows && c <= self.grid.cols {
                        self.puzzle.vertex_clues.push(VertexClue {
                            vertex: self.grid.vertex(r, c),
                            value: val,
                        });
                    }
                }
            } else if cmd == "cell" {
                let rest = parts.get(1).unwrap_or(&"");
                let rparts: Vec<&str> = rest.split_whitespace().take(2).collect();
                if rparts.len() >= 2 {
                    let addr = rparts[0];
                    let ctype = rparts[1];
                    let r = (addr.as_bytes()[0] - b'a') as usize;
                    let c: usize = addr[1..].parse().unwrap_or(1) - 1;
                    let cid = self.grid.cell_id(r, c);

                    if ctype == "compass" {
                        let mut compass = CompassData::default();
                        let dirs: Vec<&str> = rest.split_whitespace().skip(2).collect();
                        for d in &dirs {
                            if d.is_empty() {
                                continue;
                            }
                            let ch = d.as_bytes()[0] as char;
                            let v: usize = d[1..].parse().unwrap_or(0);
                            match ch {
                                'e' => compass.e = Some(v),
                                'w' => compass.w = Some(v),
                                's' => compass.s = Some(v),
                                'n' => compass.n = Some(v),
                                _ => {}
                            }
                        }
                        self.puzzle
                            .cell_clues
                            .push(CellClue::Compass { cell: cid, compass });
                    } else if ctype == "poly" {
                        if let Some(name) = rest.split_whitespace().nth(2) {
                            if let Some(shape) = polyomino::get_named_shape(name) {
                                self.puzzle
                                    .cell_clues
                                    .push(CellClue::Polyomino { cell: cid, shape });
                            }
                        } else {
                            i += 1;
                            let mut shape_lines = Vec::new();
                            while i < self.lines.len() {
                                let sl = &self.lines[i];
                                if sl.is_empty() {
                                    break;
                                }
                                if sl.chars().any(|c| c != '#' && c != '.') {
                                    break;
                                }
                                shape_lines.push(sl.as_str());
                                i += 1;
                            }
                            i -= 1;
                            self.puzzle.cell_clues.push(CellClue::Polyomino {
                                cell: cid,
                                shape: polyomino::parse_shape(&shape_lines),
                            });
                        }
                    } else if ctype == "area" {
                        let val: usize = rest
                            .split_whitespace()
                            .nth(2)
                            .unwrap_or("0")
                            .parse()
                            .unwrap_or(0);
                        self.puzzle.cell_clues.push(CellClue::Area {
                            cell: cid,
                            value: val,
                        });
                    } else if ctype == "rose" {
                        let sym = rest.split_whitespace().nth(2).unwrap_or("A").as_bytes()[0];
                        self.puzzle.cell_clues.push(CellClue::Rose {
                            cell: cid,
                            symbol: (sym - b'A') as u8,
                        });
                    } else if ctype == "palisade" {
                        let pval = rest.split_whitespace().nth(2).unwrap_or("");
                        if let Some(kind) = Self::parse_palistr(pval) {
                            self.puzzle
                                .cell_clues
                                .push(CellClue::Palisade { cell: cid, kind });
                        }
                    }
                }
            } else if cmd == "edge" {
                let rest = parts.get(1).unwrap_or(&"");
                let rparts: Vec<&str> = rest.split_whitespace().take(2).collect();
                if rparts.len() >= 2 {
                    let addr = rparts[0];
                    let ctype = rparts[1];
                    let is_h = addr.starts_with('h');
                    let r = (addr.as_bytes()[1] - b'a') as usize;
                    let c: usize = addr[2..].parse().unwrap_or(1) - 1;
                    let eid = if is_h {
                        self.grid.h_edge(r, c)
                    } else {
                        self.grid.v_edge(r, c)
                    };

                    self.pre_cut_edges.push(eid);

                    let clue = match ctype {
                        "d" => Some(EdgeClueKind::Delta),
                        "g" => Some(EdgeClueKind::Gemini),
                        "<" if !is_h => Some(EdgeClueKind::Inequality {
                            smaller_first: true,
                        }),
                        ">" if !is_h => Some(EdgeClueKind::Inequality {
                            smaller_first: false,
                        }),
                        "^" if is_h => Some(EdgeClueKind::Inequality {
                            smaller_first: true,
                        }),
                        "v" if is_h => Some(EdgeClueKind::Inequality {
                            smaller_first: false,
                        }),
                        _ => {
                            // Difference clue: <N>  (e.g. <3>)
                            if let Some(inner) = ctype
                                .strip_prefix('<')
                                .and_then(|s| s.strip_suffix('>'))
                            {
                                if let Ok(val) = inner.parse() {
                                    self.puzzle.edge_clues.push(EdgeClue {
                                        edge: eid,
                                        kind: EdgeClueKind::Diff { value: val },
                                    });
                                }
                            }
                            None
                        }
                    };
                    if let Some(kind) = clue {
                        self.puzzle.edge_clues.push(EdgeClue { edge: eid, kind });
                    }
                }
            }
            i += 1;
        }
    }

    pub fn new() -> Self {
        Self {
            puzzle: Puzzle::default(),
            grid: Grid::new(0, 0, false),
            lines: Vec::new(),
            grid_start: 0,
            grid_end: 0,
            pre_cut_edges: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Parser, ParseError> {
        let mut p = Parser::new();
        p.parse(s.as_bytes())?;
        Ok(p)
    }

    #[test]
    fn empty_input_errors() {
        let mut p = Parser::new();
        assert!(p.parse("".as_bytes()).is_err());
    }

    #[test]
    fn no_grid_errors() {
        let mut p = Parser::new();
        assert!(p.parse("minimum 3\n".as_bytes()).is_err());
    }

    #[test]
    fn simple_grid_dimensions() {
        let p = parse_str(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert_eq!(p.grid.rows, 2);
        assert_eq!(p.grid.cols, 2);
        assert_eq!(p.grid.total_existing_cells(), 4);
    }

    #[test]
    fn precision_sets_min_max() {
        let p = parse_str(
            "\
precision 4
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert_eq!(p.puzzle.rules.minimum, Some(4));
        assert_eq!(p.puzzle.rules.maximum, Some(4));
    }

    #[test]
    fn area_clue_from_cell() {
        let p = parse_str(
            "\
+---+---+
| 3 . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p
            .puzzle
            .cell_clues
            .iter()
            .any(|cl| matches!(cl, CellClue::Area { value: 3, .. })));
    }

    #[test]
    fn rose_clue_from_cell() {
        let p = parse_str(
            "\
+---+---+
| B . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        let rose = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Rose { .. }));
        let CellClue::Rose { symbol, .. } = rose.unwrap() else {
            panic!("expected Rose");
        };
        assert_eq!(*symbol, 1); // B - A = 1
    }

    #[test]
    fn palisade_clue_from_cell() {
        let p = parse_str(
            "\
+----+---+
| p3 . _ |
+ .  + . +
| _  . _ |
+----+---+
",
        )
        .unwrap();
        let pal = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Palisade { .. }));
        let CellClue::Palisade { kind, .. } = pal.unwrap() else {
            panic!("expected Palisade");
        };
        assert_eq!(*kind, PalisadeKind::Three);
    }

    #[test]
    fn edge_clue_from_grid() {
        // The 'd' between rows should become a Delta edge clue
        let p = parse_str(
            "\
+---+---+
| _ . _ |
+ . +-d-+
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p
            .puzzle
            .edge_clues
            .iter()
            .any(|ec| matches!(ec.kind, EdgeClueKind::Delta)));
    }

    #[test]
    fn v_separator_edge_clue() {
        // 'g' between columns should become a Gemini edge clue
        let p = parse_str(
            "\
+---+---+
| _ g _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p
            .puzzle
            .edge_clues
            .iter()
            .any(|ec| matches!(ec.kind, EdgeClueKind::Gemini)));
    }

    #[test]
    fn missing_cell_marks_not_exists() {
        let p = parse_str(
            "\
+---+---+---+
| _ . _ .   |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
",
        )
        .unwrap();
        // Top-right cell should not exist
        assert!(!p.grid.cell_exists[p.grid.cell_id(0, 2)]);
        assert!(p.grid.cell_exists[p.grid.cell_id(0, 0)]);
    }

    #[test]
    fn shape_bank_named() {
        let p = parse_str(
            "\
shape bank T L
+---+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+---+
",
        )
        .unwrap();
        assert_eq!(p.puzzle.rules.shape_bank.len(), 2);
    }

    #[test]
    fn rules_flags() {
        let p = parse_str(
            "\
bricky
mingle shape
solitude
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p.puzzle.rules.bricky);
        assert!(p.puzzle.rules.mingle_shape);
        assert!(p.puzzle.rules.solitude);
    }

    #[test]
    fn supplementary_vertex_clue() {
        let p = parse_str(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
vertex b2 3
",
        )
        .unwrap();
        assert_eq!(p.puzzle.vertex_clues.len(), 1);
        assert_eq!(p.puzzle.vertex_clues[0].value, 3);
        assert_eq!(p.puzzle.vertex_clues[0].vertex, p.grid.vertex(1, 1));
    }

    #[test]
    fn comment_stripped() {
        let p = parse_str(
            "\
bricky # make it bricky
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p.puzzle.rules.bricky);
    }

    #[test]
    fn polyomino_inline_cell() {
        let p = parse_str(
            "\
+---+---+
| T . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        let poly = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Polyomino { .. }));
        let CellClue::Polyomino { shape, .. } = poly.unwrap() else {
            panic!("expected Polyomino");
        };
        assert_eq!(shape.cells.len(), 4); // T tetromino
    }

    #[test]
    fn polyomino_supplementary_inline() {
        let p = parse_str(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
cell a1 poly T
",
        )
        .unwrap();
        let poly = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Polyomino { .. }));
        let CellClue::Polyomino { shape, cell, .. } = poly.unwrap() else {
            panic!("expected Polyomino");
        };
        assert_eq!(*cell, p.grid.cell_id(0, 0));
        assert_eq!(shape.cells.len(), 4);
    }

    #[test]
    fn polyomino_supplementary_multiline_still_works() {
        let p = parse_str(
            "\
+---+---+---+
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+ . + . + . +
| _ . _ . _ |
+---+---+---+
cell a1 poly
###
.#.
",
        )
        .unwrap();
        let poly = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Polyomino { .. }));
        let CellClue::Polyomino { shape, .. } = poly.unwrap() else {
            panic!("expected Polyomino");
        };
        assert_eq!(shape.cells.len(), 4); // T tetromino via multiline
    }

    #[test]
    fn multi_digit_area_clue() {
        let p = parse_str(
            "\
+---+---+
|18 . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        assert!(p
            .puzzle
            .cell_clues
            .iter()
            .any(|cl| matches!(cl, CellClue::Area { value: 18, .. })));
    }

    #[test]
    fn polyomino_inline_cell_two_letter() {
        let p = parse_str(
            "\
+---+---+
|TT . _ |
+ . + . +
| _ . _ |
+---+---+
",
        )
        .unwrap();
        let poly = p
            .puzzle
            .cell_clues
            .iter()
            .find(|cl| matches!(cl, CellClue::Polyomino { .. }));
        let CellClue::Polyomino { shape, .. } = poly.unwrap() else {
            panic!("expected Polyomino");
        };
        assert_eq!(shape.cells.len(), 5); // TT pentomino
    }
}
