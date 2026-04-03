mod dlx;
mod formatter;
mod grid;
mod parser;
mod polyomino;
mod solver;
mod types;

use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 {
        eprintln!("Usage: aog [filename]");
        return ExitCode::from(1);
    }

    let reader: Box<dyn std::io::BufRead> = if args.len() == 2 {
        let file = match File::open(&args[1]) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open '{}': {e}", args[1]);
                return ExitCode::from(1);
            }
        };
        Box::new(BufReader::new(file))
    } else {
        Box::new(BufReader::new(std::io::stdin().lock()))
    };
    let mut p = parser::Parser::new();
    if let Err(e) = p.parse(reader) {
        eprintln!("Failed to parse input: {e}");
        return ExitCode::from(1);
    }

    let mut s = solver::Solver::new(p.puzzle, p.grid);
    for e in p.pre_cut_edges {
        s.mark_pre_cut(e);
    }

    let count = s.solve();

    if count == 0 {
        println!("No solution");
    } else if count == 1 {
        println!("Unique solution found.");
    } else {
        println!("Multiple solutions found ({} shown above).", count);
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve_input(input: &str) -> (usize, String) {
        let mut p = parser::Parser::new();
        p.parse(input.as_bytes()).unwrap();
        let mut s = solver::Solver::new(p.puzzle, p.grid);
        for e in p.pre_cut_edges {
            s.mark_pre_cut(e);
        }
        let count = s.solve();
        let output = if count == 0 {
            "No solution".to_string()
        } else {
            formatter::format_solution(s.get_grid(), s.get_best_edges(), s.get_best_pieces())
        };
        (count, output)
    }

    #[test]
    fn test_o_puzzle() {
        let (count, output) = solve_input(
            "shape bank O\n\
             +---+---+\n\
             | _ . _ |\n\
             + . + . +\n\
             | _ . _ |\n\
             +---+---+\n",
        );
        assert_eq!(count, 1);
        assert_eq!(
            output,
            "+---+---+\n\
             | 1   1 |\n\
             +   +   +\n\
             | 1   1 |\n\
             +---+---+\n"
        );
    }

    #[test]
    fn test_t_puzzle() {
        let (count, output) = solve_input(
            "shape bank O\n\
             +---+---+---+---+\n\
             | _ . _ . _ . _ |\n\
             + . + . + . + . +\n\
             | _ . _ . _ . _ |\n\
             +   +   +   +   +\n\
             | _ . _ . _ . _ |\n\
             + . + . + . + . +\n\
             | _ . _ . _ . _ |\n\
             +---+---+---+---+\n",
        );
        assert_eq!(count, 1);
        assert_eq!(
            output,
            "+---+---+---+---+\n\
             | 1   1 | 2   2 |\n\
             +   +   +   +   +\n\
             | 1   1 | 2   2 |\n\
             +---+---+---+---+\n\
             | 3   3 | 4   4 |\n\
             +   +   +   +   +\n\
             | 3   3 | 4   4 |\n\
             +---+---+---+---+\n"
        );
    }

    #[test]
    fn test_input_puzzle() {
        // The main 7x7 test with T and L shapes, edge clues
        let input = "\
shape bank T L
+---+---+---+---+---+---+---+
| _ . _ . _ . _ . _ . _ . _ |
+ . + . + . + . + . + . + . +
| _ . _ . _ . _ . _ . _ . _ |
+ . + . + . + . +-g-+ . + . +
| _ . _ g _ . _ . _ d _ . _ |
+ . + . +-d-+---+ . + . + . +
| _ . _ d _ |   | _ . _ . _ |
+ . +-g-+ . +---+ . + . + . +
| _ . _ . _ . _ d _ . _ . _ |
+ . + . + . +-g-+-g-+ . + . +
| _ . _ . _ g _ . _ . _ . _ |
+ . + . + . + . + . + . + . +
| _ . _ . _ . _ . _ . _ . _ |
+---+---+---+---+---+---+---+
";
        let (count, _) = solve_input(input);
        assert_eq!(count, 1);
    }

    /// Extract expected solution lines from a sample file.
    /// Returns None if the file should be skipped (no `# unique solution` or contains `skip`).
    /// Otherwise returns (timeout_secs, expected_lines).
    fn parse_sample_solution(content: &str) -> Option<(u64, Vec<String>)> {
        let mut timeout_secs = 1u64;
        let mut in_solution = false;
        let mut lines = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if !in_solution {
                if let Some(rest) = trimmed.strip_prefix("# unique solution") {
                    let rest = rest.trim_end_matches(':').trim();
                    if rest.contains("skip") {
                        return None;
                    }
                    if let Some(ts) = rest.strip_suffix("s)").and_then(|s| s.strip_prefix("(")) {
                        if let Ok(t) = ts.parse::<u64>() {
                            timeout_secs = t;
                        }
                    }
                    in_solution = true;
                }
            } else if let Some(rest) = trimmed.strip_prefix('#') {
                let rest = rest.trim();
                if !rest.is_empty() {
                    lines.push(rest.to_string());
                }
            }
        }

        if !in_solution || lines.is_empty() {
            return None;
        }

        Some((timeout_secs, lines))
    }

    /// Normalize a solution string: replace digits with spaces, trim each line, remove empty lines.
    fn normalize_solution(s: &str) -> String {
        s.lines()
            .map(|line| {
                line.chars()
                    .map(|c| if c.is_ascii_digit() { ' ' } else { c })
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn test_sample_file(path: &std::path::Path) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));

        let Some((timeout_secs, expected_lines)) = parse_sample_solution(&content) else {
            return; // no `# unique solution` or explicitly skipped
        };

        let expected = normalize_solution(&expected_lines.join("\n"));

        let path_display = path.display().to_string();
        let path_display = std::sync::Arc::new(path_display);
        let path_display_clone = path_display.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let child = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let mut p = parser::Parser::new();
                p.parse(content.as_bytes())
                    .unwrap_or_else(|e| panic!("parse error in {}: {}", *path_display_clone, e));
                let mut s = solver::Solver::new(p.puzzle, p.grid);
                for e in p.pre_cut_edges {
                    s.mark_pre_cut(e);
                }
                let count = s.solve();
                let output = formatter::format_solution(s.get_grid(), s.get_best_edges(), s.get_best_pieces());
                let _ = tx.send((count, output));
            })
            .expect("failed to spawn thread");

        match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
            Ok((count, output)) => {
                assert_eq!(
                    count, 1,
                    "{}: expected unique solution, got {}",
                    path_display, count
                );
                let actual = normalize_solution(&output);
                assert_eq!(actual, expected, "{}: solution shape mismatch", path_display);
            }
            Err(_) => panic!(
                "{}: timed out after {}s (use `# unique solution ({}s):` to increase)",
                path_display, timeout_secs, timeout_secs + 1,
            ),
        }
        drop(child);
    }

    // Sample-based integration tests (auto-discovered from samples/ directory)
    #[test]
    fn test_samples() {
        let samples_dir = std::path::Path::new("samples");
        let mut entries: Vec<_> = std::fs::read_dir(samples_dir)
            .expect("failed to read samples/ directory")
            .map(|e| e.expect("failed to read directory entry"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }
            test_sample_file(&path);
        }
    }

    #[test]
    fn no_solution_when_impossible() {
        // 1x2 grid, minimum 4 → impossible
        let (count, output) = solve_input(
            "minimum 4\n\
             +---+---+\n\
             | _ . _ |\n\
             +---+---+\n",
        );
        assert_eq!(count, 0);
        assert_eq!(output, "No solution");
    }
}
