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
