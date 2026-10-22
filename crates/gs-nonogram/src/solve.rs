//! Propagation, bounded search, and the difficulty feature vector.
//!
//! **This is not the verification gate.** Nothing here produces a
//! [`gs_core::verification::Outcome`], and that is structural rather than a
//! convention: [`Features`] and [`Solved`] have no conversion to one. The trust
//! boundary is an SMT solver fed by [`crate::smtlib`].
//!
//! What this is for:
//!
//! * **A generation filter.** Reject a candidate before paying for verification.
//!   Being wrong here costs throughput, not trust.
//! * **The difficulty feature vector.** The development plan asks for propagation
//!   work, decision points and forced-move ratio rather than one number, and warns
//!   that a puzzle hard for a solver is not necessarily hard for a human. Nonogram
//!   makes those features explainable: line-solvable without guessing, or how deep
//!   the search had to go.

use gs_core::state::{Cell, Dims, State};

use crate::line::{marginals, LineResult, Mark};
use crate::rules::{mark_of, Nonogram, EMPTY, FILLED};

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solved {
    /// No board satisfies the clues.
    None,
    /// Exactly one board does, within the budget.
    One,
    /// At least two do. Counting stops at two: more than one is already
    /// disqualifying, and the exact number is not a question anybody asks.
    Several,
    /// The budget ran out before the question was decided.
    ///
    /// Distinct from every other variant, for the same reason
    /// [`gs_core::verification::Outcome::Unknown`] is: an exhausted budget is not an
    /// answer. A filter may treat this as "reject and move on"; nothing may treat it
    /// as "unique".
    Undecided,
}

/// The difficulty feature vector, measured while solving.
///
/// Measurements of what the solver actually did, not predictions about a human.
/// Mapping these to player-facing bands needs play data and is deliberately absent:
/// a tunable calibrated against output the system cannot reach fails silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    /// Cells decided by line propagation alone, before any guess.
    pub propagation_only_cells: u32,
    /// Rounds of propagation over all lines.
    pub propagation_rounds: u32,
    /// Branches taken. Zero means the puzzle is solvable by propagation alone,
    /// which is the cleanest difficulty signal this family offers.
    pub decision_points: u32,
    /// Deepest nesting of guesses.
    pub max_search_depth: u32,
    /// Cells on the board.
    pub cells: u32,
}

impl Features {
    /// Whether the puzzle needed no guessing. The property players actually care
    /// about, and the one the family was chosen for.
    #[must_use]
    pub fn line_solvable(&self) -> bool {
        self.decision_points == 0
    }

    /// The share of cells propagation settled on its own, in tenths of a percent, so
    /// the feature vector stays integer-only like everything else.
    #[must_use]
    pub fn forced_ratio_per_mille(&self) -> u32 {
        if self.cells == 0 {
            return 0;
        }
        self.propagation_only_cells.saturating_mul(1000) / self.cells
    }
}

/// What a solve found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub solved: Solved,
    pub features: Features,
    /// The first board found, if any. A candidate witness to hand to the real
    /// verifier — never evidence on its own.
    pub witness: Option<State>,
}

/// Work limits. Every search in this platform is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_nodes: u32,
    pub max_depth: u32,
}

impl Default for Budget {
    fn default() -> Self {
        // Enough for any board the slice generates, small enough that a pathological
        // candidate is rejected rather than chased.
        Budget {
            max_nodes: 20_000,
            max_depth: 64,
        }
    }
}

/// Propagate to a fixpoint, then search if anything is still undecided.
#[must_use]
pub fn solve(puzzle: &Nonogram, budget: Budget) -> Analysis {
    let dims = puzzle.clues.dims;
    let mut board = vec![Mark::Unknown; dims.cell_count()];
    let mut features = Features {
        cells: u32::try_from(dims.cell_count()).unwrap_or(u32::MAX),
        ..Features::default()
    };

    match propagate(puzzle, &mut board, &mut features) {
        Propagation::Contradiction => Analysis {
            solved: Solved::None,
            features,
            witness: None,
        },
        Propagation::Fixpoint => {
            features.propagation_only_cells = decided_count(&board);
            let mut search = Search {
                puzzle,
                budget,
                nodes: 0,
                found: 0,
                witness: None,
                exhausted: false,
                features,
            };
            search.explore(board, 0);
            let solved = if search.exhausted && search.found < 2 {
                Solved::Undecided
            } else {
                match search.found {
                    0 => Solved::None,
                    1 => Solved::One,
                    _ => Solved::Several,
                }
            };
            Analysis {
                solved,
                features: search.features,
                witness: search.witness,
            }
        }
    }
}

enum Propagation {
    Fixpoint,
    Contradiction,
}

/// Apply line marginals to every row and column until nothing more is forced.
fn propagate(puzzle: &Nonogram, board: &mut [Mark], features: &mut Features) -> Propagation {
    let dims = puzzle.clues.dims;
    loop {
        let mut changed = false;
        features.propagation_rounds += 1;

        for y in 0..dims.height {
            let line: Vec<Mark> = (0..dims.width).map(|x| board[idx(dims, x, y)]).collect();
            let runs = match puzzle.clues.rows.get(usize::from(y)) {
                Some(r) => r,
                None => return Propagation::Contradiction,
            };
            match marginals(&line, runs) {
                LineResult::Contradiction => return Propagation::Contradiction,
                LineResult::Forced(forced) => {
                    for (x, mark) in forced.iter().enumerate() {
                        let i = idx(dims, u16::try_from(x).unwrap_or(0), y);
                        if board[i] == Mark::Unknown && *mark != Mark::Unknown {
                            board[i] = *mark;
                            changed = true;
                        }
                    }
                }
            }
        }

        for x in 0..dims.width {
            let line: Vec<Mark> = (0..dims.height).map(|y| board[idx(dims, x, y)]).collect();
            let runs = match puzzle.clues.columns.get(usize::from(x)) {
                Some(r) => r,
                None => return Propagation::Contradiction,
            };
            match marginals(&line, runs) {
                LineResult::Contradiction => return Propagation::Contradiction,
                LineResult::Forced(forced) => {
                    for (y, mark) in forced.iter().enumerate() {
                        let i = idx(dims, x, u16::try_from(y).unwrap_or(0));
                        if board[i] == Mark::Unknown && *mark != Mark::Unknown {
                            board[i] = *mark;
                            changed = true;
                        }
                    }
                }
            }
        }

        if !changed {
            return Propagation::Fixpoint;
        }
    }
}

struct Search<'a> {
    puzzle: &'a Nonogram,
    budget: Budget,
    nodes: u32,
    found: u32,
    witness: Option<State>,
    /// Whether a limit was hit. If so the count is a lower bound, not an answer.
    exhausted: bool,
    features: Features,
}

impl Search<'_> {
    /// Depth-first over the first undecided cell, propagating after each guess.
    ///
    /// Stops at two solutions: the question is "is it unique", and the second answer
    /// settles it.
    fn explore(&mut self, board: Vec<Mark>, depth: u32) {
        if self.found >= 2 {
            return;
        }
        self.nodes += 1;
        if self.nodes > self.budget.max_nodes || depth > self.budget.max_depth {
            self.exhausted = true;
            return;
        }
        self.features.max_search_depth = self.features.max_search_depth.max(depth);

        let Some(undecided) = board.iter().position(|m| *m == Mark::Unknown) else {
            // Fully decided and propagation never contradicted it, so it is a
            // solution. Recorded as a candidate witness, not as proof.
            self.found += 1;
            if self.witness.is_none() {
                self.witness = Some(to_state(self.puzzle.clues.dims, &board));
            }
            return;
        };

        self.features.decision_points += 1;
        for guess in [Mark::Filled, Mark::Empty] {
            let mut branch = board.clone();
            branch[undecided] = guess;
            let mut features = self.features;
            match propagate(self.puzzle, &mut branch, &mut features) {
                Propagation::Contradiction => {}
                Propagation::Fixpoint => {
                    self.features.propagation_rounds = features.propagation_rounds;
                    self.explore(branch, depth + 1);
                }
            }
            if self.found >= 2 {
                return;
            }
        }
    }
}

fn idx(dims: Dims, x: u16, y: u16) -> usize {
    dims.index_of(x, y).unwrap_or(0) as usize
}

fn decided_count(board: &[Mark]) -> u32 {
    u32::try_from(board.iter().filter(|m| **m != Mark::Unknown).count()).unwrap_or(u32::MAX)
}

/// Turn a fully-decided board into engine state, so a witness can be replayed
/// through the real runtime rather than compared against itself.
#[must_use]
pub fn to_state(dims: Dims, board: &[Mark]) -> State {
    let mut state = State::blank(dims);
    for (i, mark) in board.iter().enumerate() {
        let value = match mark {
            Mark::Filled => FILLED,
            Mark::Empty => EMPTY,
            Mark::Unknown => Cell::UNKNOWN,
        };
        let _ = state.set(u32::try_from(i).unwrap_or(u32::MAX), value);
    }
    state
}

/// Read a board back out of engine state, for checking a witness round-trips.
#[must_use]
pub fn from_state(state: &State) -> Vec<Mark> {
    (0..u32::try_from(state.dims().cell_count()).unwrap_or(u32::MAX))
        .map(|i| state.get(i).map_or(Mark::Unknown, mark_of))
        .collect()
}
