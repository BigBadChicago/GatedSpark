//! Hints: the next thing a player can work out, found on the device from the clues.
//!
//! # Why this is not a peek at the answer
//!
//! A hint here is a **deduction**, not a lookup. Everything below is computed from the
//! clue numbers and from what the player has marked so far, by the same line reasoning
//! in [`crate::line`] that the generation filter uses. Nothing reads a stored answer,
//! because nothing published carries one: a package holds clues, and the proof that
//! they have exactly one answer, and no board.
//!
//! That is what makes hints work with no signal and leaves the published site with no
//! answer files in it.
//!
//! # Why a hint never touches the board
//!
//! [`next`] returns what a player could do and why. It makes no move. The move stays
//! the player's, which keeps the hint assistance rather than a change to the puzzle:
//! the puzzle, its clues and its one answer are exactly the same whether a hint was
//! asked for or not.
//!
//! # What it looks for, in order
//!
//! 1. **A line that cannot work.** If what the player has marked in some row or column
//!    cannot fit that line's numbers, no square in it is worth pointing at: the thing
//!    worth saying is that the row is already wrong. Checked across the whole board
//!    first, because a mistake elsewhere matters more than a free square here.
//! 2. **A forced square.** A square whose mark follows from one line's numbers and the
//!    marks already around it. This is the hint a player learns something from.
//! 3. **A square from working the whole puzzle out.** When nothing is forced, a hint
//!    falls back to solving the puzzle on the device, within a budget, and pointing at
//!    one square of the answer it worked out. Still no stored answer, and still only
//!    the clues as input — but the player is told it came this way, because "this
//!    follows from row 4" and "I worked the whole thing out" are different claims.
//!
//! The fallback is also where a player's wrong mark turns up: if the answer the device
//! worked out disagrees with a square the player has marked, the hint says so instead,
//! because being told where the mistake is beats being handed another square.

use gs_core::state::State;

use crate::line::{feasible, marginals, LineResult, Mark};
use crate::rules::{mark_of, Nonogram};
use crate::solve::{from_state, solve, Budget, Solved};

/// Which line a deduction came from, or that it came from the whole puzzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Because {
    /// The numbers beside this row, counting from 0 at the top.
    Row(u16),
    /// The numbers above this column, counting from 0 at the left.
    Column(u16),
    /// No single line forced it. The device worked the whole puzzle out.
    TheWholePuzzle,
}

/// What a player can work out next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Mark this square. Rows and columns count from 0 at the top left.
    Square {
        row: u16,
        column: u16,
        /// What the square has to be. Never [`Mark::Unknown`].
        mark: Mark,
        because: Because,
    },
    /// What is marked in this line cannot fit its numbers, whatever happens next.
    LineCannotWork { because: Because },
    /// A square the player has filled in is not filled in the answer.
    FilledWrong { row: u16, column: u16 },
    /// Nothing could be worked out: either the board is finished, or the puzzle needs
    /// more than this device was willing to spend.
    NothingToSay,
}

impl Because {
    /// The reason in a sentence a player can read.
    #[must_use]
    pub fn plainly(self) -> String {
        match self {
            // Lines are counted from 1 for a player, because that is how a person
            // counts rows on a page. The numbering inside is from 0 and stays there.
            Because::Row(row) => format!("the numbers beside row {}", u32::from(row) + 1),
            Because::Column(column) => {
                format!("the numbers above column {}", u32::from(column) + 1)
            }
            Because::TheWholePuzzle => "this puzzle having only one answer".to_string(),
        }
    }
}

impl Hint {
    /// The hint in a sentence a player can read.
    ///
    /// The words live here, with the reasoning, so that there is one copy of them and a
    /// test can read them. A page shows this sentence; it does not write its own.
    #[must_use]
    pub fn plainly(&self) -> String {
        match self {
            Hint::Square {
                row,
                column,
                mark,
                because,
            } => {
                let what = match mark {
                    Mark::Filled => "has to be filled",
                    Mark::Empty => "has to be blank",
                    // Not reachable: a forced mark is never undecided. Said plainly
                    // anyway, because a panic in a hint is worse than a dull sentence.
                    Mark::Unknown => "cannot be worked out yet",
                };
                format!(
                    "Row {}, column {} {}, because of {}.",
                    u32::from(*row) + 1,
                    u32::from(*column) + 1,
                    what,
                    because.plainly()
                )
            }
            Hint::LineCannotWork { because } => format!(
                "What is marked in that line cannot match {}. Something there needs changing.",
                because.plainly()
            ),
            Hint::FilledWrong { row, column } => format!(
                "Row {}, column {} is filled in, and it should not be. Clear it and carry on.",
                u32::from(*row) + 1,
                u32::from(*column) + 1
            ),
            Hint::NothingToSay => {
                "There is nothing this device can work out from here.".to_string()
            }
        }
    }

    /// Whether this hint actually told the player something, and so is worth charging
    /// against a puzzle's allowance.
    #[must_use]
    pub fn is_something(&self) -> bool {
        !matches!(self, Hint::NothingToSay)
    }
}

/// The next thing a player can work out on this board.
///
/// `state` is the board as the player has it, undecided squares included.
#[must_use]
pub fn next(puzzle: &Nonogram, state: &State, budget: Budget) -> Hint {
    let dims = puzzle.clues.dims;
    let board: Vec<Mark> = (0..dims.cell_count())
        .map(|i| {
            state
                .get(u32::try_from(i).unwrap_or(u32::MAX))
                .map_or(Mark::Unknown, mark_of)
        })
        .collect();

    // 1. A line that cannot work, anywhere on the board.
    for (which, cells, runs) in lines(puzzle, &board) {
        if !feasible(&cells, &runs) {
            return Hint::LineCannotWork { because: which };
        }
    }

    // 2. A forced square. Rows first, then columns, and the first one found in that
    // order: the same board always gets the same hint, which matters because a player
    // who asks twice should not be told two different things.
    for (which, cells, runs) in lines(puzzle, &board) {
        if let LineResult::Forced(forced) = marginals(&cells, &runs) {
            for (along, mark) in forced.iter().enumerate() {
                if cells[along] != Mark::Unknown || *mark == Mark::Unknown {
                    continue;
                }
                let along = u16::try_from(along).unwrap_or(0);
                let (row, column) = match which {
                    Because::Row(row) => (row, along),
                    Because::Column(column) => (along, column),
                    Because::TheWholePuzzle => continue,
                };
                return Hint::Square {
                    row,
                    column,
                    mark: *mark,
                    because: which,
                };
            }
        }
    }

    // 3. Nothing is forced. Work the whole puzzle out, within the budget, and point at
    // one square of what came back.
    let found = solve(puzzle, budget);
    let (Solved::One, Some(answer)) = (found.solved, found.witness.as_ref()) else {
        return Hint::NothingToSay;
    };
    let answer = from_state(answer);

    // A square the player has filled that the answer does not. Said first: a player
    // stuck behind their own mistake is not helped by another square.
    for (i, right) in answer.iter().enumerate() {
        if board[i] == Mark::Filled && *right != Mark::Filled {
            let (row, column) = at(dims.width, i);
            return Hint::FilledWrong { row, column };
        }
    }

    // Otherwise a square the answer fills and the player has not. Filled rather than
    // blank, because a filled square moves a Nonogram on and a blank one rarely does.
    for (i, right) in answer.iter().enumerate() {
        if *right == Mark::Filled && board[i] != Mark::Filled {
            let (row, column) = at(dims.width, i);
            return Hint::Square {
                row,
                column,
                mark: Mark::Filled,
                because: Because::TheWholePuzzle,
            };
        }
    }

    Hint::NothingToSay
}

/// Every row and then every column, as a line of marks and its numbers.
fn lines(puzzle: &Nonogram, board: &[Mark]) -> Vec<(Because, Vec<Mark>, Vec<u16>)> {
    let dims = puzzle.clues.dims;
    let mut out = Vec::new();
    for y in 0..dims.height {
        let cells: Vec<Mark> = (0..dims.width)
            .map(|x| board[index(dims.width, x, y)])
            .collect();
        let runs = puzzle
            .clues
            .rows
            .get(usize::from(y))
            .cloned()
            .unwrap_or_default();
        out.push((Because::Row(y), cells, runs));
    }
    for x in 0..dims.width {
        let cells: Vec<Mark> = (0..dims.height)
            .map(|y| board[index(dims.width, x, y)])
            .collect();
        let runs = puzzle
            .clues
            .columns
            .get(usize::from(x))
            .cloned()
            .unwrap_or_default();
        out.push((Because::Column(x), cells, runs));
    }
    out
}

fn index(width: u16, x: u16, y: u16) -> usize {
    usize::from(y) * usize::from(width) + usize::from(x)
}

fn at(width: u16, index: usize) -> (u16, u16) {
    let width = usize::from(width).max(1);
    (
        u16::try_from(index / width).unwrap_or(0),
        u16::try_from(index % width).unwrap_or(0),
    )
}
