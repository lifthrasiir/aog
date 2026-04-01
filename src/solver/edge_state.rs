use super::Solver;
use crate::types::*;

impl Solver {
    pub(crate) fn set_edge(&mut self, e: EdgeId, s: EdgeState) -> bool {
        if self.edges[e] == s {
            return true;
        }
        if self.edges[e] != EdgeState::Unknown {
            return false;
        }
        self.edges[e] = s;
        self.changed.push((e, EdgeState::Unknown));
        true
    }

    pub(crate) fn restore(&mut self, snap: usize) {
        while self.changed.len() > snap {
            let (e, old_state) = self.changed.pop().unwrap();
            self.edges[e] = old_state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::test_helpers::make_solver;

    #[test]
    fn set_edge_and_restore() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        assert_eq!(s.edges[e], EdgeState::Unknown);

        // Set to Cut
        assert!(s.set_edge(e, EdgeState::Cut));
        assert_eq!(s.edges[e], EdgeState::Cut);
        let snap = s.changed.len();

        // Set another edge
        let e2 = s.grid.h_edge(0, 0);
        assert!(s.set_edge(e2, EdgeState::Uncut));
        assert_eq!(s.edges[e2], EdgeState::Uncut);

        // Restore to before e2
        s.restore(snap);
        assert_eq!(s.edges[e], EdgeState::Cut);
        assert_eq!(s.edges[e2], EdgeState::Unknown);
    }

    #[test]
    fn set_edge_idempotent() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        s.edges[e] = EdgeState::Cut;
        // Setting same state returns true without pushing to changed
        let snap = s.changed.len();
        assert!(s.set_edge(e, EdgeState::Cut));
        assert_eq!(s.changed.len(), snap);
    }

    #[test]
    fn set_edge_conflict_returns_false() {
        let mut s = make_solver(
            "\
+---+---+
| _ . _ |
+ . + . +
| _ . _ |
+---+---+
",
        );
        let e = s.grid.v_edge(0, 0);
        s.edges[e] = EdgeState::Cut;
        // Trying to set to Uncut when already Cut should fail
        assert!(!s.set_edge(e, EdgeState::Uncut));
    }
}
