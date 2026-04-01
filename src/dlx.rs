/// Dancing Links (DLX) — Knuth's Algorithm X for exact cover.
///
/// Node layout:
///   Index 0:           root header
///   Index 1..=N:       column headers for N columns
///   Index N+1..:       data nodes added via `add_row`
///
/// Column selection always picks the column with fewest remaining rows (MRV).
pub struct Dlx {
    left: Vec<usize>,
    right: Vec<usize>,
    up: Vec<usize>,
    down: Vec<usize>,
    col: Vec<usize>,    // column-header node index for each node
    row_id: Vec<usize>, // external row identifier for each data node
    size: Vec<usize>,   // active row count for each column header
}

impl Dlx {
    /// Create a new DLX structure with `num_cols` primary columns.
    pub fn new(num_cols: usize) -> Self {
        let n = num_cols + 1; // root (0) + column headers (1..=num_cols)
        let mut left = vec![0usize; n];
        let mut right = vec![0usize; n];
        // Each column header's up/down initially points to itself (empty column).
        let up: Vec<usize> = (0..n).collect();
        let down: Vec<usize> = (0..n).collect();
        let col: Vec<usize> = (0..n).collect();
        let row_id = vec![0usize; n];
        let size = vec![0usize; n];

        // Circular doubly-linked header list: root ↔ col1 ↔ … ↔ colN ↔ root
        for i in 0..n {
            left[i] = if i == 0 { num_cols } else { i - 1 };
            right[i] = if i == num_cols { 0 } else { i + 1 };
        }

        Dlx {
            left,
            right,
            up,
            down,
            col,
            row_id,
            size,
        }
    }

    /// Add a row that covers the given column indices.
    ///
    /// `cols` must be sorted in ascending order and contain no duplicates.
    /// `row_id` is the caller-defined identifier returned in solutions.
    pub fn add_row(&mut self, row_id: usize, cols: &[usize]) {
        if cols.is_empty() {
            return;
        }
        let base = self.left.len(); // index of the first node we're about to push
        let len = cols.len();

        for (i, &c) in cols.iter().enumerate() {
            let h = c + 1; // column header node for column c
            let node = base + i;

            // Horizontal circular links within the row.
            self.left
                .push(if i == 0 { base + len - 1 } else { node - 1 });
            self.right.push(if i == len - 1 { base } else { node + 1 });

            // Vertical insertion: append this node at the bottom of column h
            // (i.e., just before h in the circular column list).
            let prev_up = self.up[h];
            self.up.push(prev_up);
            self.down.push(h);
            self.col.push(h);
            self.row_id.push(row_id);
            self.size.push(0); // size field unused for data nodes

            self.down[prev_up] = node;
            self.up[h] = node;
            self.size[h] += 1;
        }
    }

    // ── Cover / uncover ────────────────────────────────────────────────────

    fn cover(&mut self, c: usize) {
        // Remove column c from the header list.
        self.right[self.left[c]] = self.right[c];
        self.left[self.right[c]] = self.left[c];

        // For each row that passes through column c, remove that row from all
        // other columns it covers (top to bottom, then left to right).
        let mut i = self.down[c];
        while i != c {
            let mut j = self.right[i];
            while j != i {
                self.up[self.down[j]] = self.up[j];
                self.down[self.up[j]] = self.down[j];
                self.size[self.col[j]] -= 1;
                j = self.right[j];
            }
            i = self.down[i];
        }
    }

    fn uncover(&mut self, c: usize) {
        // Restore rows in reverse order (bottom to top, then right to left).
        let mut i = self.up[c];
        while i != c {
            let mut j = self.left[i];
            while j != i {
                self.size[self.col[j]] += 1;
                self.up[self.down[j]] = j;
                self.down[self.up[j]] = j;
                j = self.left[j];
            }
            i = self.up[i];
        }

        // Restore column c to the header list.
        self.right[self.left[c]] = c;
        self.left[self.right[c]] = c;
    }

    // ── Column selection (MRV) ─────────────────────────────────────────────

    fn choose_column(&self) -> usize {
        let mut best = self.right[0]; // first active column
        let mut best_size = self.size[best];
        let mut c = self.right[best];
        while c != 0 {
            if self.size[c] < best_size {
                best_size = self.size[c];
                best = c;
                if best_size == 0 {
                    break; // can't do better than 0
                }
            }
            c = self.right[c];
        }
        best
    }

    // ── Search ─────────────────────────────────────────────────────────────

    /// Run Algorithm X recursively.
    ///
    /// When all columns are covered `callback` is called with the current
    /// solution (a slice of row IDs).  The callback returns `true` to continue
    /// the search or `false` to stop early.
    ///
    /// Returns `false` if the search was stopped by the callback, `true`
    /// otherwise.
    pub fn search<F>(&mut self, solution: &mut Vec<usize>, callback: &mut F) -> bool
    where
        F: FnMut(&[usize]) -> bool,
    {
        self.search_with_check(solution, &mut |_| true, callback)
    }

    /// Run Algorithm X with early partial-solution checking.
    ///
    /// `row_check` is called after each row is added to the partial solution.
    /// If it returns `false`, the branch is pruned immediately.
    pub fn search_with_check<F, G>(
        &mut self,
        solution: &mut Vec<usize>,
        row_check: &mut G,
        callback: &mut F,
    ) -> bool
    where
        F: FnMut(&[usize]) -> bool,
        G: FnMut(&[usize]) -> bool,
    {
        // All columns covered → report solution.
        if self.right[0] == 0 {
            return callback(solution);
        }

        let c = self.choose_column();

        // Column c has no remaining rows → dead end.
        if self.size[c] == 0 {
            return true;
        }

        self.cover(c);

        let mut r = self.down[c];
        let mut cont = true;
        while r != c && cont {
            // Select row r as part of the solution.
            solution.push(self.row_id[r]);

            // Cover every other column that row r touches.
            let mut j = self.right[r];
            while j != r {
                self.cover(self.col[j]);
                j = self.right[j];
            }

            if row_check(solution) {
                cont = self.search_with_check(solution, row_check, callback);
            }

            // Deselect row r (undo in reverse order).
            solution.pop();
            let mut j = self.left[r];
            while j != r {
                self.uncover(self.col[j]);
                j = self.left[j];
            }

            r = self.down[r];
        }

        self.uncover(c);
        cont
    }
}
