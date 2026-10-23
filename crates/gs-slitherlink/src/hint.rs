//! Hints: the next line a player can work out, found on the device from the numbers.
//!
//! The same arrangement as `gs_nonogram::hint`, and for the same reasons. A hint is a
//! **deduction** from the numbers in the squares and the lines the player has marked so
//! far. Nothing reads a stored answer, because a published puzzle carries none. So
//! hints work with no signal, and the published site still has no answer in it
//! anywhere.
//!
//! [`next`] makes no move. It says what a player could mark and why, and the marking
//! stays theirs, which is what keeps a hint assistance and not a change to the puzzle.
//!
//! # What it looks for, in order
//!
//! 1. **Something that cannot work**: a square whose number can no longer be met, or a
//!    dot with three lines at it, or a dot with one line and nowhere for it to go. A
//!    player stuck behind their own mistake is not helped by a free line somewhere else.
//! 2. **A forced line**: one that follows from a square's number, or from the rule that
//!    a dot has either no lines at it or exactly two. These are the deductions a person
//!    makes, which is why they are also the hints worth giving.
//! 3. **A line from working the whole puzzle out**, within a budget, when nothing at all
//!    is forced. The player is told the hint came this way, because "this follows from
//!    that 3" and "I worked the whole thing out" are different claims.

use gs_core::state::State;

use crate::board::{Along, CellId, DotId, EdgeId, Geometry};
use crate::loops::drawn_in;
use crate::rules::{Slitherlink, LINE, NO_LINE};
use crate::solve::{solve, Budget, Solved};

/// How one line is marked on the board in front of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    Drawn,
    RuledOut,
    Undecided,
}

/// Why a line is forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Because {
    /// The number in this square. Columns and rows count from 0 at the top left.
    Number { column: u16, row: u16 },
    /// The dot at this corner, which has either no lines at it or exactly two.
    Dot { column: u16, row: u16 },
    /// Nothing single forced it. The device worked the whole puzzle out.
    TheWholePuzzle,
}

/// Where a line is: which way it runs, and the dot it starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Where {
    /// True for a line running left to right, false for one running top to bottom.
    pub across: bool,
    pub column: u16,
    pub row: u16,
}

/// What a player can work out next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Mark this line, drawn or ruled out.
    Line {
        at: Where,
        /// True to draw it, false to rule it out.
        drawn: bool,
        because: Because,
    },
    /// This square's number can no longer be met by what is marked around it.
    NumberCannotWork { column: u16, row: u16 },
    /// Too many lines meet at this dot, or one line reaches it and cannot leave.
    DotCannotWork { column: u16, row: u16 },
    /// A line the player has drawn is not part of the loop.
    DrawnWrong { at: Where },
    /// Nothing could be worked out: either the board is finished, or the puzzle needs
    /// more than this device was willing to spend.
    NothingToSay,
}

impl Because {
    /// The reason in a sentence a player can read.
    #[must_use]
    pub fn plainly(self) -> String {
        match self {
            // Counted from 1 for a player, because that is how a person counts squares
            // on a page. The numbering inside is from 0 and stays there.
            Because::Number { column, row } => format!(
                "the number in the square {} across and {} down",
                u32::from(column) + 1,
                u32::from(row) + 1
            ),
            Because::Dot { column, row } => format!(
                "the dot {} across and {} down having either no lines at it or exactly two",
                u32::from(column) + 1,
                u32::from(row) + 1
            ),
            Because::TheWholePuzzle => "this puzzle having only one answer".to_string(),
        }
    }
}

impl Where {
    fn plainly(self) -> String {
        format!(
            "the line running {} from the dot {} across and {} down",
            if self.across { "right" } else { "down" },
            u32::from(self.column) + 1,
            u32::from(self.row) + 1
        )
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
            Hint::Line { at, drawn, because } => format!(
                "{} {}, because of {}.",
                sentence_case(&at.plainly()),
                if *drawn {
                    "is part of the loop"
                } else {
                    "is not part of the loop"
                },
                because.plainly()
            ),
            Hint::NumberCannotWork { column, row } => format!(
                "The number in the square {} across and {} down can no longer be met. \
                 Something around it needs changing.",
                u32::from(*column) + 1,
                u32::from(*row) + 1
            ),
            Hint::DotCannotWork { column, row } => format!(
                "The lines at the dot {} across and {} down cannot work: a dot has \
                 either no lines at it or exactly two.",
                u32::from(*column) + 1,
                u32::from(*row) + 1
            ),
            Hint::DrawnWrong { at } => format!(
                "{} is drawn, and it is not part of the loop. Clear it and carry on.",
                sentence_case(&at.plainly())
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

fn sentence_case(said: &str) -> String {
    let mut letters = said.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

/// The next thing a player can work out on this board.
///
/// `state` is the board as the player has it, undecided lines included.
#[must_use]
pub fn next(puzzle: &Slitherlink, state: &State, budget: Budget) -> Hint {
    let geometry = puzzle.geometry();
    let how = |edge: EdgeId| -> How {
        match state.get(edge.0) {
            Ok(value) if value == LINE => How::Drawn,
            Ok(value) if value == NO_LINE => How::RuledOut,
            _ => How::Undecided,
        }
    };

    // 1. Anything that cannot work. Numbers first, then dots.
    for (cell, clue) in puzzle.clues.given() {
        let Some((drawn, undecided)) = at_cell(geometry, cell, &how) else {
            continue;
        };
        let clue = usize::from(clue);
        if drawn > clue || drawn + undecided.len() < clue {
            let (column, row) = geometry.cell_at(cell).unwrap_or((0, 0));
            return Hint::NumberCannotWork { column, row };
        }
    }
    for dot in geometry.dots() {
        let (drawn, undecided) = at_dot(geometry, dot, &how);
        if drawn > 2 || (drawn == 1 && undecided.is_empty()) {
            let (column, row) = geometry.dot_at(dot).unwrap_or((0, 0));
            return Hint::DotCannotWork { column, row };
        }
    }

    // 2. A forced line. Numbers first, because a number is the more instructive reason,
    // and in board order after that: the same board always gets the same hint.
    for (cell, clue) in puzzle.clues.given() {
        let Some((drawn, undecided)) = at_cell(geometry, cell, &how) else {
            continue;
        };
        let clue = usize::from(clue);
        if undecided.is_empty() {
            continue;
        }
        let settles = if drawn == clue {
            Some(false)
        } else if drawn + undecided.len() == clue {
            Some(true)
        } else {
            None
        };
        if let Some(drawn_now) = settles {
            let (column, row) = geometry.cell_at(cell).unwrap_or((0, 0));
            return line_hint(
                geometry,
                undecided[0],
                drawn_now,
                Because::Number { column, row },
            );
        }
    }
    for dot in geometry.dots() {
        let (drawn, undecided) = at_dot(geometry, dot, &how);
        if undecided.is_empty() {
            continue;
        }
        // Two lines already: nothing else can join them. One line and one way out: it
        // has to be that way. No lines and one way out: drawing it would leave a single
        // line at this dot, which the rules do not allow.
        let settles = if drawn == 2 {
            Some(false)
        } else if drawn == 1 && undecided.len() == 1 {
            Some(true)
        } else if drawn == 0 && undecided.len() == 1 {
            Some(false)
        } else {
            None
        };
        if let Some(drawn_now) = settles {
            let (column, row) = geometry.dot_at(dot).unwrap_or((0, 0));
            return line_hint(
                geometry,
                undecided[0],
                drawn_now,
                Because::Dot { column, row },
            );
        }
    }

    // 3. Nothing is forced. Work the whole puzzle out, within the budget.
    let found = solve(puzzle, budget);
    let (Solved::One, Some(answer)) = (found.solved, found.witness.as_ref()) else {
        return Hint::NothingToSay;
    };

    for edge in geometry.edges() {
        let belongs = drawn_in(answer, edge);
        if how(edge) == How::Drawn && !belongs {
            let Some(at) = place(geometry, edge) else {
                continue;
            };
            return Hint::DrawnWrong { at };
        }
    }
    for edge in geometry.edges() {
        if drawn_in(answer, edge) && how(edge) != How::Drawn {
            return line_hint(geometry, edge, true, Because::TheWholePuzzle);
        }
    }

    Hint::NothingToSay
}

fn line_hint(geometry: Geometry, edge: EdgeId, drawn: bool, because: Because) -> Hint {
    match place(geometry, edge) {
        Some(at) => Hint::Line {
            at,
            drawn,
            because,
        },
        None => Hint::NothingToSay,
    }
}

fn place(geometry: Geometry, edge: EdgeId) -> Option<Where> {
    let (along, column, row) = geometry.edge_at(edge)?;
    Some(Where {
        across: along == Along::Across,
        column,
        row,
    })
}

/// How many of a square's four sides are drawn, and which are still undecided.
fn at_cell(
    geometry: Geometry,
    cell: CellId,
    how: &dyn Fn(EdgeId) -> How,
) -> Option<(usize, Vec<EdgeId>)> {
    let sides = geometry.edges_of_cell(cell)?;
    let mut drawn = 0;
    let mut undecided = Vec::new();
    for side in sides {
        match how(side) {
            How::Drawn => drawn += 1,
            How::Undecided => undecided.push(side),
            How::RuledOut => {}
        }
    }
    Some((drawn, undecided))
}

/// How many lines meet at a dot, and which of its lines are still undecided.
fn at_dot(geometry: Geometry, dot: DotId, how: &dyn Fn(EdgeId) -> How) -> (usize, Vec<EdgeId>) {
    let mut drawn = 0;
    let mut undecided = Vec::new();
    for edge in geometry.edges_of_dot(dot) {
        match how(edge) {
            How::Drawn => drawn += 1,
            How::Undecided => undecided.push(edge),
            How::RuledOut => {}
        }
    }
    (drawn, undecided)
}
