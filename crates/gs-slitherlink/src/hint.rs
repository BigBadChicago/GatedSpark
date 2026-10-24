//! Hints: what a player can be shown next, worked out on the device from the numbers.
//!
//! The same arrangement as `gs_nonogram::hint`, and for the same reasons. A hint is
//! **worked out** from the numbers in the squares and the lines the player has marked
//! so far. Nothing reads a stored answer, because a published puzzle carries none. So
//! hints work with no signal, and the published site still has no answer in it
//! anywhere.
//!
//! [`next`] says what to mark and why. Writing it onto a board is the caller's job,
//! through the ordinary rules, so a hint goes through the same door as a tap and can be
//! taken back the same way.
//!
//! # Three sizes, in this game's own units
//!
//! A Slitherlink has no rows of squares to fill, so [`Size`] counts lines: one line,
//! half a row or column of them, or a whole row or column. A row here means every line
//! running left to right between one row of dots and the next, and a column means every
//! line running top to bottom down one column of dots.
//!
//! # What it looks for, in order
//!
//! 1. **Something that cannot work**: a square whose number can no longer be met, or a
//!    dot with three lines at it, or a dot with one line and nowhere for it to go. A
//!    player stuck behind their own mistake is not helped by a free line somewhere else.
//! 2. **Lines the numbers decide.** For a one-line hint, a line that follows from a
//!    square's number or from the nought-or-two rule at a dot, with that reason named.
//!    For a bigger hint, the row or column of lines that helps most.
//! 3. **The whole puzzle worked out**, within a budget, when nothing is forced outright.

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

/// How much help is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// One line.
    OneLine,
    /// Half of one row or column of lines.
    HalfALine,
    /// A whole row or column of lines.
    AWholeLine,
}

/// Why a line is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Because {
    /// The number in this square. Columns and rows count from 0 at the top left.
    Number { column: u16, row: u16 },
    /// The dot at this corner, which has either no lines at it or exactly two.
    Dot { column: u16, row: u16 },
    /// Every line running left to right between one row of dots and the next.
    AcrossRow(u16),
    /// Every line running top to bottom down one column of dots.
    DownColumn(u16),
    /// Nothing single decided it. The device worked the whole puzzle out.
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

/// One line a hint has worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub at: Where,
    /// True to draw it, false to rule it out.
    pub drawn: bool,
}

/// What a player can be shown next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Lines the numbers decide, with the reason they are decided.
    ///
    /// Never empty: a hint with nothing in it is [`Hint::NothingToSay`].
    Fill { lines: Vec<Line>, because: Because },
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
                "the number in the square at {} across, {} down",
                u32::from(column) + 1,
                u32::from(row) + 1
            ),
            Because::Dot { column, row } => format!(
                "the dot at {} across, {} down having either no lines at it or exactly two",
                u32::from(column) + 1,
                u32::from(row) + 1
            ),
            Because::AcrossRow(_) | Because::DownColumn(_) | Because::TheWholePuzzle => {
                "this puzzle having only one answer".to_string()
            }
        }
    }

    /// The run of lines itself, named the way a player points at one.
    #[must_use]
    pub fn line_called(self) -> String {
        match self {
            Because::AcrossRow(row) => format!(
                "the lines running left to right along row {}",
                u32::from(row) + 1
            ),
            Because::DownColumn(column) => format!(
                "the lines running top to bottom down column {}",
                u32::from(column) + 1
            ),
            _ => "the board".to_string(),
        }
    }
}

impl Where {
    fn plainly(self) -> String {
        format!(
            "the line running {} from the dot at {} across, {} down",
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
            Hint::Fill { lines, because } => match lines.as_slice() {
                [] => Hint::NothingToSay.plainly(),
                [only] => format!(
                    "{} {}, because of {}.",
                    sentence_case(&only.at.plainly()),
                    if only.drawn {
                        "has been drawn in for you"
                    } else {
                        "has been ruled out for you"
                    },
                    because.plainly()
                ),
                many => {
                    let drawn = many.iter().filter(|line| line.drawn).count();
                    format!(
                        "{} of {} settled for you: {} drawn and {} ruled out, from {}.",
                        many.len(),
                        because.line_called(),
                        drawn,
                        many.len() - drawn,
                        because.plainly()
                    )
                }
            },
            Hint::NumberCannotWork { column, row } => format!(
                "The number in the square at {} across, {} down can no longer be met. \
                 Something around it needs changing.",
                u32::from(*column) + 1,
                u32::from(*row) + 1
            ),
            Hint::DotCannotWork { column, row } => format!(
                "The lines at the dot at {} across, {} down cannot work: a dot has \
                 either no lines at it or exactly two.",
                u32::from(*column) + 1,
                u32::from(*row) + 1
            ),
            Hint::DrawnWrong { at } => format!(
                "{} is drawn, and it is not part of the loop. It has been cleared for you.",
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
        match self {
            Hint::NothingToSay => false,
            Hint::Fill { lines, .. } => !lines.is_empty(),
            _ => true,
        }
    }
}

fn sentence_case(said: &str) -> String {
    let mut letters = said.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

/// What a player can be shown next on this board, at the size asked for.
///
/// `state` is the board as the player has it, undecided lines included.
#[must_use]
pub fn next(puzzle: &Slitherlink, state: &State, budget: Budget, size: Size) -> Hint {
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

    match size {
        Size::OneLine => one_line(puzzle, &how, budget),
        Size::HalfALine | Size::AWholeLine => a_run_of_lines(puzzle, &how, budget, size),
    }
}

/// One line, forced outright if possible.
fn one_line(puzzle: &Slitherlink, how: &dyn Fn(EdgeId) -> How, budget: Budget) -> Hint {
    let geometry = puzzle.geometry();

    // Numbers first, because a number is the more instructive reason, and in board
    // order after that: the same board always gets the same hint.
    for (cell, clue) in puzzle.clues.given() {
        let Some((drawn, undecided)) = at_cell(geometry, cell, how) else {
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
            return one(
                geometry,
                undecided[0],
                drawn_now,
                Because::Number { column, row },
            );
        }
    }
    for dot in geometry.dots() {
        let (drawn, undecided) = at_dot(geometry, dot, how);
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
            return one(
                geometry,
                undecided[0],
                drawn_now,
                Because::Dot { column, row },
            );
        }
    }

    // Nothing is forced. Work the whole puzzle out, within the budget.
    let Some(answer) = worked_out(puzzle, budget) else {
        return Hint::NothingToSay;
    };
    if let Some(wrong) = first_mistake(geometry, how, &answer) {
        return wrong;
    }
    for edge in geometry.edges() {
        if drawn_in(&answer, edge) && how(edge) != How::Drawn {
            return one(geometry, edge, true, Because::TheWholePuzzle);
        }
    }
    Hint::NothingToSay
}

/// Half a run of lines, or a whole one: the row or column that helps most.
///
/// The lines come from the puzzle's one answer, worked out here from the numbers. A
/// whole row is more than the local rules can usually settle, and filling in only the
/// part that happens to be forced would make "a whole row" mean a different amount on
/// every board.
fn a_run_of_lines(
    puzzle: &Slitherlink,
    how: &dyn Fn(EdgeId) -> How,
    budget: Budget,
    size: Size,
) -> Hint {
    let geometry = puzzle.geometry();
    let Some(answer) = worked_out(puzzle, budget) else {
        // No answer within the budget, so there is no honest way to fill a run of
        // lines. Fall back to the one line the local rules can settle, which is less
        // than was asked for and never wrong.
        return one_line(puzzle, how, budget);
    };
    if let Some(wrong) = first_mistake(geometry, how, &answer) {
        return wrong;
    }

    let settled = |edge: EdgeId| {
        let belongs = drawn_in(&answer, edge);
        how(edge) == if belongs { How::Drawn } else { How::RuledOut }
    };

    // The run with the most lines still to settle is the one a fill helps most. Rows
    // before columns on a tie, and the earliest such run, so the same board always gets
    // the same hint.
    let mut best: Option<(usize, Because)> = None;
    for which in every_run(geometry) {
        let waiting = edges_in(geometry, which)
            .into_iter()
            .filter(|edge| !settled(*edge))
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

    let edges = edges_in(geometry, which);
    let wanted = match size {
        // Half, rounded up, so half of a run of five is three rather than two: a player
        // asking for half should get at least half.
        Size::HalfALine => edges.len().div_ceil(2),
        _ => edges.len(),
    };
    // The half that helps most, rather than always the first: a run whose first half is
    // already settled would otherwise give a hint that fills in nothing.
    let edges = if wanted < edges.len() {
        // The two halves overlap when the run has an odd length, which is fine: either
        // is a run of `wanted` lines at one end of it.
        let front = &edges[..wanted];
        let back = &edges[edges.len() - wanted..];
        let waiting = |part: &[EdgeId]| part.iter().filter(|e| !settled(**e)).count();
        if waiting(back) > waiting(front) {
            back.to_vec()
        } else {
            front.to_vec()
        }
    } else {
        edges
    };

    let lines: Vec<Line> = edges
        .into_iter()
        .filter(|edge| !settled(*edge))
        .filter_map(|edge| {
            Some(Line {
                at: place(geometry, edge)?,
                drawn: drawn_in(&answer, edge),
            })
        })
        .collect();
    if lines.is_empty() {
        return Hint::NothingToSay;
    }
    Hint::Fill {
        lines,
        because: which,
    }
}

/// The puzzle's one answer, worked out here from the numbers within a budget.
fn worked_out(puzzle: &Slitherlink, budget: Budget) -> Option<State> {
    let found = solve(puzzle, budget);
    match (found.solved, found.witness) {
        (Solved::One, Some(answer)) => Some(answer),
        _ => None,
    }
}

/// A line the player has drawn that the answer does not.
fn first_mistake(geometry: Geometry, how: &dyn Fn(EdgeId) -> How, answer: &State) -> Option<Hint> {
    for edge in geometry.edges() {
        if how(edge) == How::Drawn && !drawn_in(answer, edge) {
            return Some(Hint::DrawnWrong {
                at: place(geometry, edge)?,
            });
        }
    }
    None
}

fn one(geometry: Geometry, edge: EdgeId, drawn: bool, because: Because) -> Hint {
    match place(geometry, edge) {
        Some(at) => Hint::Fill {
            lines: vec![Line { at, drawn }],
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

/// Every row of across-lines, then every column of down-lines.
fn every_run(geometry: Geometry) -> Vec<Because> {
    (0..geometry.dots_down())
        .map(Because::AcrossRow)
        .chain((0..geometry.dots_across()).map(Because::DownColumn))
        .collect()
}

/// The lines in one run, in order along it.
fn edges_in(geometry: Geometry, which: Because) -> Vec<EdgeId> {
    match which {
        Because::AcrossRow(row) => (0..geometry.width())
            .filter_map(|x| geometry.across(x, row))
            .collect(),
        Because::DownColumn(column) => (0..geometry.height())
            .filter_map(|y| geometry.down(column, y))
            .collect(),
        _ => Vec::new(),
    }
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
