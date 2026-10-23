//! Playing a loaded puzzle, in whichever game it belongs to.
//!
//! # The one rule this module exists to keep
//!
//! **It decides nothing.** Every question a player's move raises is answered by
//! `gs-core` and by that game's own crate:
//!
//! - whether a move is legal → `Ruleset::transition`
//! - what the board becomes → `Delta::apply`, through the timeline
//! - whether undo and redo are possible → `Timeline`
//! - **whether the puzzle is solved → `Ruleset::is_solved`**
//!
//! The last one is the whole reason these sessions are thin. Working out for itself
//! whether a board is finished is the single easiest mistake to make here, and the most
//! expensive: there would be two answers to that question, they would disagree one day,
//! and the wrong one would be the one in front of the player. Each game's `is_solved`
//! checks the clues — not a stored answer — and that is the only definition of solved
//! this project has.
//!
//! So nothing below computes anything about a board. It holds a timeline, hands moves to
//! it, and reads answers out.
//!
//! # Why a session owns its puzzle
//!
//! A session is built from a [`Playable`](crate::Playable), which can only come from
//! [`load`](crate::load). You cannot start a session on a puzzle whose signature was not
//! checked, because there is no other way to get the thing a session needs.
//!
//! # Why there are two session types and one [`Session`]
//!
//! A Nonogram move marks a square; a Slitherlink move marks a line between two dots.
//! Those are not the same move wearing different words, and a single `mark(index)` for
//! both would only have hidden which one a caller meant. So each game has its own
//! session with its own moves.
//!
//! What they do share is everything that is not a move: undo, redo, how many moves have
//! been made, the board's fingerprint, and whether the puzzle is solved. [`Session`]
//! holds either game and answers those, so a page can show a board, a move counter and a
//! finished message without knowing which game it has. To make a move it asks for the
//! game it wants and gets nothing if that is not what it is holding.

pub mod nonogram;
pub mod slitherlink;

use gs_core::hash::StateHash;
use gs_core::timeline::TimelineError;

use crate::Playable;

/// Why a move did nothing.
///
/// Every one of these is ordinary during play, not an error in the usual sense: tapping
/// a square that is already filled, or undoing at the start. They are reported rather
/// than ignored so a page can tell the difference between "that did nothing" and "that
/// worked".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoChange {
    /// The square, or the line, is already marked that way.
    AlreadyThatWay,
    /// There is no such square, or no such line, on this board.
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
            NoChange::AlreadyThatWay => "that is already marked that way",
            NoChange::NoSuchSquare => "there is no such square or line on this board",
            NoChange::AtTheStart => "there is nothing left to undo",
            NoChange::NothingToRedo => "there is nothing to redo",
            NoChange::TheRulesRefused => "the rules did not allow that move",
        };
        f.write_str(said)
    }
}

impl std::error::Error for NoChange {}

pub(crate) fn why(e: TimelineError) -> NoChange {
    match e {
        TimelineError::AtRoot => NoChange::AtTheStart,
        TimelineError::NoRedo => NoChange::NothingToRedo,
        TimelineError::Illegal(gs_core::ruleset::Illegal::NoOp) => NoChange::AlreadyThatWay,
        TimelineError::Illegal(gs_core::ruleset::Illegal::OutOfBounds) => NoChange::NoSuchSquare,
        _ => NoChange::TheRulesRefused,
    }
}

/// One person playing one puzzle, of either game.
pub enum Session {
    Nonogram(nonogram::Session),
    Slitherlink(slitherlink::Session),
}

impl Session {
    /// Start playing a puzzle that passed every check.
    ///
    /// There is no failure case and no default. Every game [`load`](crate::load) will
    /// hand back is a game with a session, because the list of games it accepts and the
    /// list of games below are the same two, and the compiler holds them together: a
    /// third game added to `KNOWN` without a session here stops this matching.
    #[must_use]
    pub fn start(puzzle: Playable) -> Self {
        match puzzle.board {
            crate::Board::Nonogram(_) => Session::Nonogram(
                nonogram::Session::start(puzzle)
                    .expect("a Nonogram board makes a Nonogram session"),
            ),
            crate::Board::Slitherlink(_) => Session::Slitherlink(
                slitherlink::Session::start(puzzle)
                    .expect("a Slitherlink board makes a Slitherlink session"),
            ),
        }
    }

    #[must_use]
    pub fn puzzle(&self) -> &Playable {
        match self {
            Session::Nonogram(game) => game.puzzle(),
            Session::Slitherlink(game) => game.puzzle(),
        }
    }

    /// The Nonogram being played, to make a Nonogram move on. Nothing if this is some
    /// other game.
    pub fn as_nonogram(&mut self) -> Option<&mut nonogram::Session> {
        match self {
            Session::Nonogram(game) => Some(game),
            Session::Slitherlink(_) => None,
        }
    }

    /// The Slitherlink being played, to make a Slitherlink move on. Nothing if this is
    /// some other game.
    pub fn as_slitherlink(&mut self) -> Option<&mut slitherlink::Session> {
        match self {
            Session::Slitherlink(game) => Some(game),
            Session::Nonogram(_) => None,
        }
    }

    pub fn undo(&mut self) -> Result<(), NoChange> {
        match self {
            Session::Nonogram(game) => game.undo(),
            Session::Slitherlink(game) => game.undo(),
        }
    }

    pub fn redo(&mut self) -> Result<(), NoChange> {
        match self {
            Session::Nonogram(game) => game.redo(),
            Session::Slitherlink(game) => game.redo(),
        }
    }

    /// Whether the puzzle is solved. Asked of that game's rules, every time.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        match self {
            Session::Nonogram(game) => game.is_solved(),
            Session::Slitherlink(game) => game.is_solved(),
        }
    }

    /// How many moves have been made along the current line of play.
    #[must_use]
    pub fn moves_made(&self) -> u32 {
        match self {
            Session::Nonogram(game) => game.moves_made(),
            Session::Slitherlink(game) => game.moves_made(),
        }
    }

    /// The fingerprint of the board as it stands.
    #[must_use]
    pub fn board_fingerprint(&self) -> StateHash {
        match self {
            Session::Nonogram(game) => game.board_fingerprint(),
            Session::Slitherlink(game) => game.board_fingerprint(),
        }
    }

    /// The board in the one exact form the engine agrees on.
    #[must_use]
    pub fn board_bytes(&self) -> Vec<u8> {
        match self {
            Session::Nonogram(game) => game.board_bytes(),
            Session::Slitherlink(game) => game.board_bytes(),
        }
    }
}
