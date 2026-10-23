//! Hints: what a player can be shown next, worked out on the device from the clues.
//!
//! # Why this is not a peek at the answer
//!
//! A hint here is **worked out**, not looked up. Everything below is computed from the
//! clue numbers and from what the player has marked so far, by the same line reasoning
//! in [`crate::line`] and the same bounded solver in [`crate::solve`] that the
//! generation filter uses. Nothing reads a stored answer, because nothing published
//! carries one: a package holds clues, and the proof that they have exactly one answer,
//! and no board.
//!
//! That is what makes hints work with no signal and leaves the published site with no
//! answer files in it.
//!
//! # Three sizes
//!
//! [`Size`] says how much help is being asked for: one square, half a line, or a whole
//! line. A bigger hint is not a different kind of claim — every square in it is one the
//! clues decide — it is simply more of the same work done for the player.
//!
//! # A hint says what to mark; it does not mark it
//!
//! [`next`] returns the squares and the reason. Writing them onto a board is the
//! caller's job, through the ordinary rules, so that a hint goes through the same door
//! as a tap and can be taken back the same way. Nothing here touches a [`State`].
//!
//! # What it looks for, in order
//!
//! 1. **A line that cannot work.** If what the player has marked in some row or column
//!    cannot fit that line's numbers, no square in it is worth filling in: the thing
//!    worth saying is that the row is already wrong. Checked across the whole board
//!    first, because a mistake elsewhere matters more than a free square here.
//! 2. **Squares the clues decide.** For a one-square hint, a square forced by a single
//!    line's numbers, with that line named. For a bigger hint, the line that helps most
//!    and as much of it as the size allows.
//! 3. **The whole puzzle worked out**, within a budget, when nothing is forced outright.
//!    Still no stored answer, and still only the clues as input — but the player is told
//!    it came this way, because "this follows from row 4" and "I worked the whole thing
//!    out" are different claims.
//!
//! Step 3 is also where a player's wrong mark turns up: if the answer the device worked
//! out disagrees with a square the player has filled, the hint says so instead, because
//! being told where the mistake is beats being handed more squares.

use gs_core::state::State;

use crate::line::{feasible, marginals, LineResult, Mark};
use crate::rules::{mark_of, Nonogram};
use crate::solve::{from_state, solve, Budget, Solved};

/// How much help is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// One square.
    OneSquare,
    /// Half of one row or column.
    HalfALine,
    /// A whole row or column.
    AWholeLine,
}

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

/// One square a hint has worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Square {
    pub row: u16,
    pub column: u16,
    /// What the square has to be. Never [`Mark::Unknown`].
    pub mark: Mark,
}

/// What a player can be shown next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Squares the clues decide, with the reason they are decided.
    ///
    /// Never empty: a hint with nothing in it is [`Hint::NothingToSay`].
    Fill {
        squares: Vec<Square>,
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

    /// The line itself, named the way a player points at one.
    #[must_use]
    pub fn line_called(self) -> String {
        match self {
            Because::Row(row) => format!("row {}", u32::from(row) + 1),
            Because::Column(column) => format!("column {}", u32::from(column) + 1),
            Because::TheWholePuzzle => "the board".to_string(),
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
            Hint::Fill { squares, because } => match squares.as_slice() {
                [] => Hint::NothingToSay.plainly(),
                [only] => {
                    let what = match only.mark {
                        Mark::Filled => "has been filled in",
                        Mark::Empty => "has been crossed out",
                        // Not reachable: a worked-out square is never undecided. Said
                        // plainly anyway, because a panic in a hint is worse than a
                        // dull sentence.
                        Mark::Unknown => "cannot be worked out yet",
                    };
                    format!(
                        "Row {}, column {} {}, because of {}.",
                        u32::from(only.row) + 1,
                        u32::from(only.column) + 1,
                        what,
                        because.plainly()
                    )
                }
                // "Settled" rather than "filled in", because some of them are
                // crossed out: a sentence that says a square was filled when it was
                // crossed out would be worse than no sentence.
                many => {
                    let filled = many
                        .iter()
                        .filter(|square| square.mark == Mark::Filled)
                        .count();
                    format!(
                        "{} squares in {} settled for you: {} filled in and {} crossed out, from {}.",
                        many.len(),
                        because.line_called(),
                        filled,
                        many.len() - filled,
                        because.plainly()
                    )
                }
            },
            Hint::LineCannotWork { because } => format!(
                "What is marked in that line cannot match {}. Something there needs changing.",
                because.plainly()
            ),
            Hint::FilledWrong { row, column } => format!(
                "Row {}, column {} is filled in, and it should not be. It has been cleared for you.",
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
        match self {
            Hint::NothingToSay => false,
            Hint::Fill { squares, .. } => !squares.is_empty(),
            _ => true,
        }
    }
}

/// What a player can be shown next on this board, at the size asked for.
///
/// `state` is the board as the player has it, undecided squares included.
#[must_use]
pub fn next(puzzle: &Nonogram, state: &State, budget: Budget, size: Size) -> Hint {
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

    match size {
        Size::OneSquare => one_square(puzzle, &board, budget),
        Size::HalfALine | Size::AWholeLine => a_line(puzzle, &board, budget, size),
    }
}

/// One square, forced by a single line if possible.
fn one_square(puzzle: &Nonogram, board: &[Mark], budget: Budget) -> Hint {
    // Rows first, then columns, and the first one found in that order: the same board
    // always gets the same hint, which matters because a player who asks twice should
    // not be told two different things.
    for (which, cells, runs) in lines(puzzle, board) {
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
                return Hint::Fill {
                    squares: vec![Square {
                        row,
                        column,
                        mark: *mark,
                    }],
                    because: which,
                };
            }
        }
    }

    // Nothing is forced. Work the whole puzzle out, within the budget.
    let dims = puzzle.clues.dims;
    let Some(answer) = worked_out(puzzle, budget) else {
        return Hint::NothingToSay;
    };
    if let Some(wrong) = first_mistake(board, &answer, dims.width) {
        return wrong;
    }
    // A square the answer fills and the player has not. Filled rather than blank,
    // because a filled square moves a Nonogram on and a blank one rarely does.
    for (i, right) in answer.iter().enumerate() {
        if *right == Mark::Filled && board[i] != Mark::Filled {
            let (row, column) = at(dims.width, i);
            return Hint::Fill {
                squares: vec![Square {
                    row,
                    column,
                    mark: Mark::Filled,
                }],
                because: Because::TheWholePuzzle,
            };
        }
    }
    Hint::NothingToSay
}

/// Half a line, or a whole one: the line that helps most, as far as the size allows.
///
/// The squares come from the puzzle's one answer, worked out here from the clues. A
/// whole line is more than line reasoning alone can usually settle, and filling in only
/// the part that happens to be forced would make "a whole row" mean a different amount
/// on every board.
fn a_line(puzzle: &Nonogram, board: &[Mark], budget: Budget, size: Size) -> Hint {
    let dims = puzzle.clues.dims;
    let Some(answer) = worked_out(puzzle, budget) else {
        // No answer within the budget, so there is no honest way to fill a line. Fall
        // back to the one square line reasoning can settle, which is less than was
        // asked for and never wrong.
        return one_square(puzzle, board, budget);
    };
    if let Some(wrong) = first_mistake(board, &answer, dims.width) {
        return wrong;
    }

    // The line with the most squares still to settle is the one a fill helps most.
    // Rows before columns on a tie, and the earliest such line, so the same board
    // always gets the same hint.
    let mut best: Option<(usize, Because)> = None;
    for which in every_line(dims) {
        let waiting = places_in(dims, which)
            .into_iter()
            .filter(|i| board[*i] != answer[*i])
            .count();
        if waiting == 0 {
            continue;
        }
        if best.is_none_or(|(most, _)| waiting > most) {
            best = Some((waiting, which));
        }
    }
    let Some((_, which)) = best else {
        return Hint::NothingToSay;
    };

    let places = places_in(dims, which);
    let wanted = match size {
        // Half, rounded up, so half of a line of five is three rather than two: a
        // player asking for half a line should get at least half of it.
        Size::HalfALine => places.len().div_ceil(2),
        _ => places.len(),
    };
    // The half that helps most, rather than always the left or the top: a row whose
    // left half is already done would otherwise give a hint that fills in nothing.
    let places = if wanted < places.len() {
        // The two halves overlap when the line has an odd length, which is fine: either
        // is a run of `wanted` squares at one end of the line.
        let front = &places[..wanted];
        let back = &places[places.len() - wanted..];
        let waiting = |part: &[usize]| part.iter().filter(|i| board[**i] != answer[**i]).count();
        if waiting(back) > waiting(front) {
            back.to_vec()
        } else {
            front.to_vec()
        }
    } else {
        places
    };

    let squares: Vec<Square> = places
        .into_iter()
        .filter(|i| board[*i] != answer[*i])
        .map(|i| {
            let (row, column) = at(dims.width, i);
            Square {
                row,
                column,
                mark: answer[i],
            }
        })
        .collect();
    if squares.is_empty() {
        return Hint::NothingToSay;
    }
    Hint::Fill {
        squares,
        because: which,
    }
}

/// The puzzle's one answer, worked out here from the clues within a budget.
fn worked_out(puzzle: &Nonogram, budget: Budget) -> Option<Vec<Mark>> {
    let found = solve(puzzle, budget);
    match (found.solved, found.witness.as_ref()) {
        (Solved::One, Some(answer)) => Some(from_state(answer)),
        _ => None,
    }
}

/// A square the player has filled that the answer does not.
///
/// Said before anything else is handed over: a player stuck behind their own mistake is
/// not helped by more squares.
fn first_mistake(board: &[Mark], answer: &[Mark], width: u16) -> Option<Hint> {
    for (i, right) in answer.iter().enumerate() {
        if board.get(i) == Some(&Mark::Filled) && *right != Mark::Filled {
            let (row, column) = at(width, i);
            return Some(Hint::FilledWrong { row, column });
        }
    }
    None
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

/// Every row and then every column, named.
fn every_line(dims: gs_core::state::Dims) -> Vec<Because> {
    (0..dims.height)
        .map(Because::Row)
        .chain((0..dims.width).map(Because::Column))
        .collect()
}

/// Where one line's squares are, in reading order along the line.
fn places_in(dims: gs_core::state::Dims, which: Because) -> Vec<usize> {
    match which {
        Because::Row(row) => (0..dims.width).map(|x| index(dims.width, x, row)).collect(),
        Because::Column(column) => (0..dims.height)
            .map(|y| index(dims.width, column, y))
            .collect(),
        Because::TheWholePuzzle => Vec::new(),
    }
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
