mod dlx;
mod formatter;
mod grid;
mod parser;
mod polyomino;
mod solver;
mod types;
mod uf;

use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(true)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 3 {
        eprintln!("Usage: aog [--parse | --solution-kill] [filename]");
        return ExitCode::from(1);
    }

    let mut use_parse = false;
    let mut use_solution_kill = false;
    if args.len() > 1 {
        if args[1] == "--parse" {
            use_parse = true;
        } else if args[1] == "--solution-kill" {
            use_solution_kill = true;
        }
    }
    let filename_idx = if use_parse || use_solution_kill { 2 } else { 1 };

    let reader: Box<dyn std::io::BufRead> = if args.len() > filename_idx {
        let file = match File::open(&args[filename_idx]) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open '{}': {e}", args[filename_idx]);
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

    if use_parse {
        if let Some(solution_edges) = p.parse_solution_edges() {
            let valid = solver::validate_parsed_solution(
                &p.puzzle,
                &p.grid,
                &p.pre_cut_edges,
                &solution_edges,
            );
            if !valid {
                eprintln!("Error: parsed solution is INVALID (fails validation)");
                return ExitCode::FAILURE;
            }
        }
        println!("{}", formatter::format_parse_output(&p));
        return ExitCode::SUCCESS;
    }

    let debug_known = if use_solution_kill {
        match p.parse_solution_edges() {
            Some(edges) => {
                let valid =
                    solver::validate_parsed_solution(&p.puzzle, &p.grid, &p.pre_cut_edges, &edges);
                if !valid {
                    eprintln!("Error: parsed solution is INVALID (fails validation)");
                    return ExitCode::FAILURE;
                }
                edges
            }
            None => {
                eprintln!("Error: no parseable solution found in the input file");
                return ExitCode::from(1);
            }
        }
    } else {
        Vec::new()
    };

    if !debug_known.is_empty() {
        tracing::info!(
            edges = debug_known.len(),
            "solution kill tracing enabled"
        );
    }

    let mut s = solver::Solver::new(p.puzzle, p.grid);
    for e in p.pre_cut_edges {
        s.mark_pre_cut(e);
    }
    s.debug_known_solution = debug_known;

    let count = s.solve();

    if count == 0 {
        println!(
            "No solution ({:.1}s).",
            s.start_time.elapsed().as_secs_f64()
        );
    } else if count == 1 {
        println!(
            "Unique solution found ({:.1}s).",
            s.start_time.elapsed().as_secs_f64()
        );
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

    enum SampleExpected {
        Unique(Vec<String>),
        Multiple(Vec<Vec<String>>),
    }

    /// Extract expected solution(s) from a sample file.
    /// Returns None if the file should be skipped or has no recognized header.
    /// For `# unique solution`: returns Unique with the solution lines.
    /// For `# multiple solutions`: returns Multiple with solution groups separated by `# or`.
    fn parse_sample_solution(content: &str) -> Option<(u64, SampleExpected)> {
        let mut timeout_secs = 1u64;
        let mut in_solution = false;
        let mut is_multiple = false;
        let mut current_group: Vec<String> = Vec::new();
        let mut all_groups: Vec<Vec<String>> = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if !in_solution {
                let (prefix, multiple) = if let Some(r) = trimmed.strip_prefix("# unique solution")
                {
                    (Some(r), false)
                } else if let Some(r) = trimmed.strip_prefix("# multiple solutions") {
                    (Some(r), true)
                } else {
                    (None, false)
                };
                if let Some(rest) = prefix {
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
                    is_multiple = multiple;
                }
            } else if let Some(rest) = trimmed.strip_prefix('#') {
                let rest = rest.trim();
                if is_multiple && rest == "or" {
                    all_groups.push(std::mem::take(&mut current_group));
                } else if !rest.is_empty() {
                    current_group.push(rest.to_string());
                }
            }
        }

        if !in_solution {
            return None;
        }

        if is_multiple {
            if !current_group.is_empty() {
                all_groups.push(current_group);
            }
            if all_groups.is_empty() {
                return None;
            }
            Some((timeout_secs, SampleExpected::Multiple(all_groups)))
        } else {
            if current_group.is_empty() {
                return None;
            }
            Some((timeout_secs, SampleExpected::Unique(current_group)))
        }
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

        let Some((timeout_secs, expected)) = parse_sample_solution(&content) else {
            return; // no recognized header or explicitly skipped
        };

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
                let (first, best) = if count == 0 {
                    ("No solution".to_string(), "No solution".to_string())
                } else {
                    (
                        formatter::format_solution(
                            s.get_grid(),
                            s.get_first_edges(),
                            s.get_first_pieces(),
                        ),
                        formatter::format_solution(
                            s.get_grid(),
                            s.get_best_edges(),
                            s.get_best_pieces(),
                        ),
                    )
                };
                let _ = tx.send((count, first, best));
            })
            .expect("failed to spawn thread");

        match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
            Ok((count, first_output, best_output)) => match expected {
                SampleExpected::Unique(lines) => {
                    assert_eq!(
                        count, 1,
                        "{}: expected unique solution, got {}",
                        path_display, count
                    );
                    let actual = normalize_solution(&best_output);
                    let expected_norm = normalize_solution(&lines.join("\n"));
                    assert_eq!(
                        actual, expected_norm,
                        "{}: solution shape mismatch",
                        path_display
                    );
                }
                SampleExpected::Multiple(groups) => {
                    assert!(
                        count >= 2,
                        "{}: expected multiple solutions, got {}",
                        path_display,
                        count
                    );
                    let expected_norms: Vec<String> = groups
                        .iter()
                        .map(|g| normalize_solution(&g.join("\n")))
                        .collect();
                    let first_norm = normalize_solution(&first_output);
                    let best_norm = normalize_solution(&best_output);
                    assert_ne!(
                        first_norm, best_norm,
                        "{}: two solutions must be different",
                        path_display
                    );
                    assert!(
                        expected_norms.contains(&first_norm),
                        "{}: first solution not among expected\ngot:\n{}\nexpected one of:\n{}",
                        path_display,
                        first_norm,
                        expected_norms.join("\n---\n")
                    );
                    assert!(
                        expected_norms.contains(&best_norm),
                        "{}: second solution not among expected\ngot:\n{}\nexpected one of:\n{}",
                        path_display,
                        best_norm,
                        expected_norms.join("\n---\n")
                    );
                }
            },
            Err(_) => panic!(
                "{}: timed out after {}s (use `# unique solution ({}s):` to increase)",
                path_display,
                timeout_secs,
                timeout_secs + 1,
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
