//! One line's feasibility and marginals.
//!
//! Everything the line solver does rests on one question: given a line whose cells
//! are partly decided, can its runs still be placed? [`feasible`] answers it with a
//! memoised walk over run placements.
//!
//! [`marginals`] then asks that question twice per undecided cell — once with the
//! cell forced filled, once forced empty — and reports what is forced. This is
//! deliberately the simple formulation rather than a single clever pass: it is
//! `O(runs x n^3)` per line, which on a 25x25 board is a few hundred thousand
//! operations and not worth optimising, and it is simple enough to be obviously
//! right. The clever version, if it is ever needed, gets tested against this one.

/// A cell in one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Unknown,
    Filled,
    Empty,
}

impl Mark {
    #[must_use]
    pub fn can_be_filled(self) -> bool {
        matches!(self, Mark::Unknown | Mark::Filled)
    }

    #[must_use]
    pub fn can_be_empty(self) -> bool {
        matches!(self, Mark::Unknown | Mark::Empty)
    }
}

/// What a line's marks force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineResult {
    /// No placement of the runs is consistent with the marks.
    Contradiction,
    /// The marks each cell is forced to, `Unknown` where it is still free.
    Forced(Vec<Mark>),
}

/// Can `runs` be placed in `cells` without contradicting what is already marked?
#[must_use]
pub fn feasible(cells: &[Mark], runs: &[u16]) -> bool {
    let n = cells.len();
    // memo[j][p]: Some(answer) for "runs[j..] fit in cells[p..]".
    let mut memo = vec![vec![None; n + 1]; runs.len() + 1];
    place(cells, runs, 0, 0, &mut memo)
}

/// Can `runs[j..]` be placed in `cells[p..]`?
fn place(
    cells: &[Mark],
    runs: &[u16],
    j: usize,
    p: usize,
    memo: &mut Vec<Vec<Option<bool>>>,
) -> bool {
    if let Some(cached) = memo[j][p] {
        return cached;
    }
    let n = cells.len();
    let answer = if j == runs.len() {
        // Nothing left to place: every remaining cell must be able to be empty.
        cells[p..].iter().all(|m| m.can_be_empty())
    } else {
        let len = usize::from(runs[j]);
        let mut found = false;
        // The run may start anywhere from p onwards, with the cells it skips empty.
        let mut start = p;
        while start + len <= n {
            let skipped_can_be_empty = cells[p..start].iter().all(|m| m.can_be_empty());
            if !skipped_can_be_empty {
                // A filled cell was skipped, so no later start can work either.
                break;
            }
            let run_fits = cells[start..start + len].iter().all(|m| m.can_be_filled());
            if run_fits {
                let after = start + len;
                let rest = if after == n {
                    // The run ends at the edge; this must be the last run.
                    j + 1 == runs.len()
                } else if cells[after].can_be_empty() {
                    // One mandatory gap cell, then the remaining runs.
                    place(cells, runs, j + 1, after + 1, memo)
                } else {
                    false
                };
                if rest {
                    found = true;
                    break;
                }
            }
            start += 1;
        }
        found
    };
    memo[j][p] = Some(answer);
    answer
}

/// What the marks on this line force, by asking [`feasible`] with each undecided
/// cell forced each way.
#[must_use]
pub fn marginals(cells: &[Mark], runs: &[u16]) -> LineResult {
    if !feasible(cells, runs) {
        return LineResult::Contradiction;
    }
    let mut out = cells.to_vec();
    let mut probe = cells.to_vec();
    for i in 0..cells.len() {
        if cells[i] != Mark::Unknown {
            continue;
        }
        probe[i] = Mark::Filled;
        let can_fill = feasible(&probe, runs);
        probe[i] = Mark::Empty;
        let can_empty = feasible(&probe, runs);
        probe[i] = Mark::Unknown;

        out[i] = match (can_fill, can_empty) {
            (true, false) => Mark::Filled,
            (false, true) => Mark::Empty,
            (true, true) => Mark::Unknown,
            // The line as a whole is feasible, so at least one of the two must be.
            // Reaching here would mean `feasible` disagrees with itself.
            (false, false) => return LineResult::Contradiction,
        };
    }
    LineResult::Forced(out)
}
