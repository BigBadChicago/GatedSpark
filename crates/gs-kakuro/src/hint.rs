//! Hints: what a player can be shown next, worked out on the device from the totals.
//!
//! The same arrangement as `gs_nonogram::hint` and `gs_slitherlink::hint`, and for the
//! same reasons. A hint is **worked out** from the totals and the digits the player has
//! written so far. Nothing reads a stored answer, because a published puzzle carries
//! none. So hints work with no signal, and the published site still has no answer in it
//! anywhere.
//!
//! [`next`] says what to write and why. Putting it on the board is the caller's job,
//! through the ordinary rules, so a hint goes through the same door as a typed digit and
//! can be taken back the same way.
//!
//! # The deductions are the solver's, not this module's
//!
//! Everything about which digits a square could still hold comes from
//! [`crate::solve::narrow_from`]. This module decides **which** of those facts to show
//! and how to say it; it works none of them out itself. That split matters more here
//! than in either other family, because our narrowing uses a deliberate relaxation when
//! it reasons about sums: a second copy of it written for hints would be a second
//! approximation, the two would differ at the edges, and the version a player was shown
//! would be whichever one the hint button happened to call.
//!
//! # Three sizes, in this game's own units
//!
//! A Kakuro has no rows to fill — it has runs, and a run is the only line in this game
//! that means anything, since a row of the grid may be several runs with black squares
//! between them. So [`Size`] counts squares and runs: one square, half a run, or a whole
//! run. Picking the grid's rows instead would have given hints that filled squares from
//! three unrelated runs and explained none of them.
//!
//! # What it looks for, in order
//!
//! 1. **Something that cannot work**: a run with a digit twice in it, or a run whose
//!    total can no longer be reached by what is left. A player stuck behind their own
//!    mistake is not helped by a free digit somewhere else.
//! 2. **A digit that is written and wrong.** Cleared, and said plainly. A puzzle with
//!    one answer makes this a fact rather than an opinion.
//! 3. **Digits the totals decide**, with the reason named — for a bigger hint, the run
//!    that helps most.
//! 4. **The whole puzzle worked out**, within a budget, when nothing is forced outright.

use gs_core::state::State;

use crate::layout::{Along, RunId, WhiteId};
use crate::rules::Kakuro;
use crate::solve::{narrow_from, settled_digit, solve, Budget, Candidates};
use crate::totals::{LEAST_DIGIT, MOST_DIGIT};

/// How much help is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// One square.
    OneSquare,
    /// Half of one run.
    HalfARun,
    /// A whole run.
    AWholeRun,
}

/// Where a square is on the grid, counting from 0 at the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Where {
    pub column: u16,
    pub row: u16,
}

impl Where {
    /// Named the way a player points at a square. Counted from 1, because that is how
    /// a person counts squares on a page; the numbering inside stays at 0.
    #[must_use]
    pub fn plainly(self) -> String {
        format!(
            "the square at {} across, {} down",
            u32::from(self.column) + 1,
            u32::from(self.row) + 1
        )
    }
}

/// One run, named the way a player points at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhichRun {
    /// Where the run's first white square is.
    pub starts_at: Where,
    pub along: Along,
    pub total: u16,
    pub squares: u16,
}

impl WhichRun {
    #[must_use]
    pub fn plainly(self) -> String {
        format!(
            "the run of {} squares going {} from {}, adding up to {}",
            self.squares,
            match self.along {
                Along::Across => "across",
                Along::Down => "down",
            },
            self.starts_at.plainly(),
            self.total
        )
    }
}

/// One square a hint has worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filled {
    pub at: Where,
    /// The digit to write, one to nine.
    pub digit: i16,
}

/// Why a digit is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Because {
    /// The totals leave this square only one digit it could hold.
    OnlyDigitLeft,
    /// This run has one square in it, so its total names the digit outright.
    ///
    /// A separate reason from [`Because::OnlyDigitLeft`] although the deduction is the
    /// same shape, because it is the one reason a player will feel cheated by. It is
    /// said in its own words rather than dressed up as reasoning.
    ARunOfOneSquare,
    /// This run, taken as a whole.
    TheRun(WhichRun),
    /// Nothing single decided it. The device worked the whole puzzle out.
    TheWholePuzzle,
}

impl Because {
    /// The reason in a sentence a player can read.
    #[must_use]
    pub fn plainly(self) -> String {
        match self {
            Because::OnlyDigitLeft => {
                "it being the only digit left that fits both of its totals".to_string()
            }
            Because::ARunOfOneSquare => {
                "its run being one square long, so the total is the digit".to_string()
            }
            Because::TheRun(run) => run.plainly(),
            Because::TheWholePuzzle => "this puzzle having only one answer".to_string(),
        }
    }
}

/// What a player can be shown next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Digits the totals decide, with the reason they are decided.
    ///
    /// Never empty: a hint with nothing in it is [`Hint::NothingToSay`].
    Fill {
        squares: Vec<Filled>,
        because: Because,
    },
    /// The same digit appears twice in one run.
    DigitTwiceInARun { run: WhichRun, digit: i16 },
    /// This run's total can no longer be reached by what is left in it.
    RunCannotWork { run: WhichRun },
    /// A digit the player has written is not the one in the answer.
    WrittenWrong { at: Where, digit: i16 },
    /// Nothing could be worked out: either the board is finished, or the puzzle needs
    /// more than this device was willing to spend.
    NothingToSay,
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
                [only] => format!(
                    "{} has been filled in with {}, because of {}.",
                    sentence_case(&only.at.plainly()),
                    only.digit,
                    because.plainly()
                ),
                many => format!(
                    "{} squares filled in for you, from {}.",
                    many.len(),
                    because.plainly()
                ),
            },
            Hint::DigitTwiceInARun { run, digit } => format!(
                "{} has {} in it twice, and no digit may repeat inside one run. One of \
                 them needs changing.",
                sentence_case(&run.plainly()),
                digit
            ),
            Hint::RunCannotWork { run } => format!(
                "{} can no longer reach its total with what is left in it. Something in \
                 it needs changing.",
                sentence_case(&run.plainly())
            ),
            Hint::WrittenWrong { at, digit } => format!(
                "{} holds {}, and the answer does not. It has been cleared for you.",
                sentence_case(&at.plainly()),
                digit
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

fn sentence_case(said: &str) -> String {
    let mut letters = said.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

/// What a player can be shown next on this board, at the size asked for.
///
/// `state` is the board as the player has it, empty squares included.
#[must_use]
pub fn next(puzzle: &Kakuro, state: &State, budget: Budget, size: Size) -> Hint {
    // 1. Anything that cannot work, in run order so that the same board always gets
    //    the same hint.
    for (id, _) in puzzle.layout.runs() {
        if let Some(wrong) = what_is_wrong_with(puzzle, state, id) {
            return wrong;
        }
    }

    // 2. A digit that is written and wrong. This needs the answer, so it is asked for
    //    once and reused below.
    let answer = solve(puzzle, budget).witness;
    if let Some(answer) = &answer {
        for white in puzzle.layout.whites() {
            let (Some(written), Some(right)) =
                (puzzle.digit(state, white), puzzle.digit(answer, white))
            else {
                continue;
            };
            if written != right {
                return Hint::WrittenWrong {
                    at: at(puzzle, white),
                    digit: written,
                };
            }
        }
    }

    // 3 and 4. What the totals decide, at the size asked for.
    let narrowed = narrow_from(puzzle, state);
    match size {
        Size::OneSquare => one_square(puzzle, state, narrowed.as_ref(), answer.as_ref()),
        Size::HalfARun | Size::AWholeRun => {
            a_run(puzzle, state, narrowed.as_ref(), answer.as_ref(), size)
        }
    }
}

/// Why this run cannot work, if it cannot.
///
/// Two separate faults, checked in this order because a repeat is the more specific
/// thing to say: a run holding 4 and 4 against a total of 8 adds up correctly and is
/// still wrong, and telling the player its total is unreachable would be a false
/// statement about a real mistake.
fn what_is_wrong_with(puzzle: &Kakuro, state: &State, id: RunId) -> Option<Hint> {
    let run = puzzle.layout.run(id)?;
    let total = puzzle.totals.get(id)?;
    let which = which_run(puzzle, id)?;

    if puzzle.run_repeats(state, id) {
        // Name the digit that repeats rather than just saying one does. Smallest
        // first, so the message is fixed for a given board.
        for digit in LEAST_DIGIT..=MOST_DIGIT {
            let times = run
                .squares
                .iter()
                .filter(|white| puzzle.digit(state, **white) == Some(digit))
                .count();
            if times > 1 {
                return Some(Hint::DigitTwiceInARun { run: which, digit });
            }
        }
    }

    // What is written, and how little or much the empty squares could still add.
    let mut written: i32 = 0;
    let mut used: Vec<i16> = Vec::new();
    let mut empty = 0_usize;
    for white in &run.squares {
        match puzzle.digit(state, *white) {
            Some(digit) => {
                written += i32::from(digit);
                used.push(digit);
            }
            None => empty += 1,
        }
    }

    // The empty squares hold distinct digits none of which is already in this run, so
    // the smallest and largest they could add are the sums of the smallest and largest
    // such digits. Exact, not a relaxation: distinctness within one run is the whole
    // of the rule here, so there is nothing left to approximate.
    let free: Vec<i16> = (LEAST_DIGIT..=MOST_DIGIT)
        .filter(|digit| !used.contains(digit))
        .collect();
    if empty > free.len() {
        // More empty squares than digits left to put in them. The layout refuses runs
        // longer than nine, so this can only mean digits are already repeated, which
        // the check above would have caught.
        return Some(Hint::RunCannotWork { run: which });
    }
    let least: i32 = free.iter().take(empty).map(|d| i32::from(*d)).sum();
    let most: i32 = free.iter().rev().take(empty).map(|d| i32::from(*d)).sum();

    if written + least > i32::from(total) || written + most < i32::from(total) {
        return Some(Hint::RunCannotWork { run: which });
    }
    None
}

/// One square, forced outright if possible.
fn one_square(
    puzzle: &Kakuro,
    state: &State,
    narrowed: Option<&Candidates>,
    answer: Option<&State>,
) -> Hint {
    if let Some(narrowed) = narrowed {
        // A run of one square first. It is the least interesting deduction on the
        // board, and that is exactly why it goes first: if a player is stuck on a
        // board that still has one of these on it, the honest help is to point at it
        // and say what it is, not to spend their hint on clever reasoning elsewhere.
        for (id, run) in puzzle.layout.runs() {
            if run.len() != 1 {
                continue;
            }
            let Some(white) = run.squares.first().copied() else {
                continue;
            };
            if puzzle.digit(state, white).is_some() {
                continue;
            }
            let Some(total) = puzzle.totals.get(id) else {
                continue;
            };
            if let Ok(digit) = i16::try_from(total) {
                if (LEAST_DIGIT..=MOST_DIGIT).contains(&digit) {
                    return Hint::Fill {
                        squares: vec![Filled {
                            at: at(puzzle, white),
                            digit,
                        }],
                        because: Because::ARunOfOneSquare,
                    };
                }
            }
        }

        // Then a square the totals have settled between them.
        for white in puzzle.layout.whites() {
            if puzzle.digit(state, white).is_some() {
                continue;
            }
            if let Some(digit) = settled_digit(narrowed, white) {
                return Hint::Fill {
                    squares: vec![Filled {
                        at: at(puzzle, white),
                        digit,
                    }],
                    because: Because::OnlyDigitLeft,
                };
            }
        }
    }

    // Nothing is forced. Fall back to the whole puzzle, worked out within the budget.
    let Some(answer) = answer else {
        return Hint::NothingToSay;
    };
    for white in puzzle.layout.whites() {
        if puzzle.digit(state, white).is_some() {
            continue;
        }
        if let Some(digit) = puzzle.digit(answer, white) {
            return Hint::Fill {
                squares: vec![Filled {
                    at: at(puzzle, white),
                    digit,
                }],
                because: Because::TheWholePuzzle,
            };
        }
    }
    Hint::NothingToSay
}

/// Half a run, or a whole one: whichever run helps most.
///
/// "Helps most" is the run with the most empty squares this hint could fill. A run
/// that is nearly done is a worse hint than an untouched one even though it is cheaper
/// to work out, because a player asking for a whole run is asking to be unstuck.
/// Ties go to the earlier run, so one board always gives one hint.
fn a_run(
    puzzle: &Kakuro,
    state: &State,
    narrowed: Option<&Candidates>,
    answer: Option<&State>,
    size: Size,
) -> Hint {
    let mut best: Option<(RunId, Vec<Filled>)> = None;
    for (id, run) in puzzle.layout.runs() {
        let Some(which) = which_run(puzzle, id) else {
            continue;
        };
        let _ = which;
        let mut settled: Vec<Filled> = Vec::new();
        for white in &run.squares {
            if puzzle.digit(state, *white).is_some() {
                continue;
            }
            // The forced digit if there is one, and the answer's digit otherwise. A
            // whole-run hint is the largest help on offer, so falling back to the
            // answer here is the point rather than a shortcut — it is what makes the
            // third hint able to unstick a board where nothing is forced at all.
            let digit = narrowed
                .and_then(|n| settled_digit(n, *white))
                .or_else(|| answer.and_then(|a| puzzle.digit(a, *white)));
            if let Some(digit) = digit {
                settled.push(Filled {
                    at: at(puzzle, *white),
                    digit,
                });
            }
        }
        if settled.is_empty() {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(_, found)| settled.len() > found.len())
        {
            best = Some((id, settled));
        }
    }

    let Some((id, mut squares)) = best else {
        return Hint::NothingToSay;
    };
    if size == Size::HalfARun {
        // Half, rounded up, so that a run of one square still gives a square. Rounding
        // down would make the second hint do nothing on exactly the runs that are
        // easiest to give away, which would look like a broken button.
        let keep = squares.len().div_ceil(2);
        squares.truncate(keep);
    }
    let Some(which) = which_run(puzzle, id) else {
        return Hint::NothingToSay;
    };
    Hint::Fill {
        squares,
        because: Because::TheRun(which),
    }
}

/// Where a white square is on the grid.
fn at(puzzle: &Kakuro, white: WhiteId) -> Where {
    let (column, row) = puzzle
        .layout
        .square_of(white)
        .and_then(|square| puzzle.layout.square_at(square))
        .unwrap_or((0, 0));
    Where { column, row }
}

/// A run, named the way a player points at one.
fn which_run(puzzle: &Kakuro, id: RunId) -> Option<WhichRun> {
    let run = puzzle.layout.run(id)?;
    let total = puzzle.totals.get(id)?;
    let starts_at = puzzle
        .layout
        .square_at(run.start)
        .map(|(column, row)| Where { column, row })?;
    Some(WhichRun {
        starts_at,
        along: run.along,
        total,
        squares: u16::try_from(run.len()).unwrap_or(u16::MAX),
    })
}
