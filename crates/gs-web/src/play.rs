//! Playing a loaded puzzle.
//!
//! # The one rule this module exists to keep
//!
//! **It decides nothing.** Every question a player's move raises is answered by
//! `gs-core` and `gs-nonogram`:
//!
//! - whether a move is legal → `Ruleset::transition`
//! - what the board becomes → `Delta::apply`, through the timeline
//! - whether undo and redo are possible → `Timeline`
//! - **whether the puzzle is solved → `Ruleset::is_solved`**
//!
//! The last one is the whole reason this module is thin. Working out for itself whether
//! a board is finished is the single easiest mistake to make here, and the most
//! expensive: there would be two answers to that question, they would disagree one day,
//! and the wrong one would be the one in front of the player. `Nonogram::is_solved`
//! checks the clues — not a stored answer — and that is the only definition of solved
//! this project has.
//!
//! So nothing below computes anything about the board. It holds a timeline, hands moves
//! to it, and reads answers out.
//!
//! # Why a session owns its puzzle
//!
//! A [`Session`] is built from a [`Playable`](crate::Playable), which can only come from
//! [`load`](crate::load). You cannot start a session on a puzzle whose signature was not
//! checked, because there is no other way to get the thing a session needs.

use gs_core::hash::StateHash;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_core::timeline::{Timeline, TimelineError};
use gs_nonogram::rules::{mark_of, Action, Nonogram, EMPTY, FILLED};

use crate::Playable;

/// What a player can do to a square.
///
/// Three values, not two. Marking a square known-empty is a real move a player makes on
/// purpose, and it is how a Nonogram is actually solved; `Clear` is how they take either
/// mark back. All three go through the same rules as anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The square is part of the picture.
    Filled,
    /// The square is definitely not part of the picture.
    Empty,
    /// Undecided. A player clearing a mark has not undone a move, they have made one.
    Unknown,
}

impl Mark {
    fn as_cell(self) -> Cell {
        match self {
            Mark::Filled => FILLED,
            Mark::Empty => EMPTY,
            Mark::Unknown => Cell::UNKNOWN,
        }
    }

    fn of_cell(cell: Cell) -> Mark {
        match mark_of(cell) {
            gs_nonogram::line::Mark::Filled => Mark::Filled,
            gs_nonogram::line::Mark::Empty => Mark::Empty,
            gs_nonogram::line::Mark::Unknown => Mark::Unknown,
        }
    }
}

/// Why a move did nothing.
///
/// Every one of these is ordinary during play, not an error in the usual sense: tapping
/// a square that is already filled, or undoing at the start. They are reported rather
/// than ignored so a page can tell the difference between "that did nothing" and "that
/// worked".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoChange {
    /// The square is already marked that way.
    AlreadyThatWay,
    /// There is no such square on this board.
    NoSuchSquare,
    /// Nothing left to undo.
    AtTheStart,
    /// Nothing left to redo.
    NothingToRedo,
    /// The rules refused the move for a reason of their own. Kept distinct from the
    /// cases above: those are expected, and this one means something is wrong.
    TheRulesRefused,
}

impl std::fmt::Display for NoChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = match self {
            NoChange::AlreadyThatWay => "that square is already marked that way",
            NoChange::NoSuchSquare => "there is no such square on this board",
            NoChange::AtTheStart => "there is nothing left to undo",
            NoChange::NothingToRedo => "there is nothing to redo",
            NoChange::TheRulesRefused => "the rules did not allow that move",
        };
        f.write_str(said)
    }
}

impl std::error::Error for NoChange {}

fn why(e: TimelineError) -> NoChange {
    match e {
        TimelineError::AtRoot => NoChange::AtTheStart,
        TimelineError::NoRedo => NoChange::NothingToRedo,
        TimelineError::Illegal(gs_core::ruleset::Illegal::NoOp) => NoChange::AlreadyThatWay,
        TimelineError::Illegal(gs_core::ruleset::Illegal::OutOfBounds) => NoChange::NoSuchSquare,
        _ => NoChange::TheRulesRefused,
    }
}

/// One person playing one puzzle.
///
/// Holds the puzzle's rules and a timeline of everything done so far. Every move, undo
/// and redo goes through the timeline, which is what makes undo exact rather than
/// approximate: it replays recorded deltas instead of guessing at an inverse.
pub struct Session {
    puzzle: Playable,
    rules: Nonogram,
    timeline: Timeline<Nonogram>,
}

impl Session {
    /// Start playing a puzzle that passed every check.
    ///
    /// The board starts blank — every square undecided — which is a real state and not
    /// an absence of one.
    #[must_use]
    pub fn start(puzzle: Playable) -> Self {
        let rules = Nonogram::new(puzzle.clues.clone());
        let blank = State::blank(puzzle.clues.dims);
        let timeline = Timeline::new(&rules, blank);
        Session {
            puzzle,
            rules,
            timeline,
        }
    }

    #[must_use]
    pub fn puzzle(&self) -> &Playable {
        &self.puzzle
    }

    /// Mark one square, counting from the top-left along each row.
    ///
    /// The rules decide whether this is allowed. Nothing here checks the index, the
    /// value, or whether the square already holds it.
    pub fn mark(&mut self, index: u32, mark: Mark) -> Result<(), NoChange> {
        self.timeline
            .play(
                &self.rules,
                Action::Set {
                    index,
                    value: mark.as_cell(),
                },
            )
            .map(|_| ())
            .map_err(why)
    }

    /// Mark one square by its row and column, which is how a page thinks about a board.
    ///
    /// Out-of-range coordinates become `NoSuchSquare` here rather than wrapping into a
    /// valid index somewhere else on the board, which is the bug this method exists to
    /// not have.
    pub fn mark_at(&mut self, row: u16, column: u16, mark: Mark) -> Result<(), NoChange> {
        let dims = self.puzzle.clues.dims;
        if row >= dims.height || column >= dims.width {
            return Err(NoChange::NoSuchSquare);
        }
        let index = u32::from(row) * u32::from(dims.width) + u32::from(column);
        self.mark(index, mark)
    }

    pub fn undo(&mut self) -> Result<(), NoChange> {
        self.timeline.undo().map(|_| ()).map_err(why)
    }

    pub fn redo(&mut self) -> Result<(), NoChange> {
        self.timeline.redo().map(|_| ()).map_err(why)
    }

    /// What one square currently holds.
    ///
    /// A square off the board reads as `Unknown` rather than throwing, because a page
    /// draws a board in a loop and an error there is a blank page.
    #[must_use]
    pub fn mark_of(&self, index: u32) -> Mark {
        self.timeline
            .state()
            .get(index)
            .map(Mark::of_cell)
            .unwrap_or(Mark::Unknown)
    }

    #[must_use]
    pub fn mark_at_of(&self, row: u16, column: u16) -> Mark {
        let dims = self.puzzle.clues.dims;
        if row >= dims.height || column >= dims.width {
            return Mark::Unknown;
        }
        self.mark_of(u32::from(row) * u32::from(dims.width) + u32::from(column))
    }

    /// Whether the puzzle is solved.
    ///
    /// Asked of the rules, every time, with no cached answer and no shortcut. The whole
    /// module exists so that this line is the only answer to this question.
    ///
    /// What the rules say, since 2026-10-23: the **filled** squares must match the
    /// clues, and crossing out the blanks is optional. See `Nonogram::is_solved`.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        self.rules.is_solved(self.timeline.state())
    }

    /// How many moves have been made along the current line of play.
    #[must_use]
    pub fn moves_made(&self) -> u32 {
        self.timeline.depth()
    }

    /// The fingerprint of the board as it stands.
    ///
    /// The same number the published report binds a stored answer to, which is what
    /// lets a saved game be checked later against something that was signed.
    #[must_use]
    pub fn board_fingerprint(&self) -> StateHash {
        self.timeline.hash()
    }

    /// The board in the one exact form the engine agrees on.
    #[must_use]
    pub fn board_bytes(&self) -> Vec<u8> {
        self.timeline.state().canonical_bytes()
    }
}
