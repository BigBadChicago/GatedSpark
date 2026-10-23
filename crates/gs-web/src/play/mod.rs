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

/// How many hints a player gets on one puzzle.
///
/// Three, which is what the project owner asked for. It lives here rather than in
/// either game because it is about the help on offer, not about the rules: both games
/// get the same allowance, and neither game's crate has any say in it.
///
/// The allowance belongs to a session, so it starts again when a puzzle is opened
/// again. That is deliberate: a hint is help, not a score, and there is nothing here
/// worth defending with bookkeeping that follows a player around.
///
/// Two things are deliberately not charged for: a hint that could say nothing, and the
/// same hint again, which can only happen when it had nothing to mark.
pub const HINTS_PER_PUZZLE: u8 = 3;

/// How big each hint is, in the order they are given.
///
/// The hints grow: the first settles one square or line, the second half a row or
/// column, the third a whole one. The project owner asked for exactly this, and the
/// shape of it is the point — somebody a little stuck gets a nudge, and somebody
/// thoroughly stuck gets enough to carry on with.
///
/// A list rather than a rule, so that the sizes and the allowance cannot disagree:
/// [`HINTS_PER_PUZZLE`] is this list's length, and a test holds them together.
pub const HINT_SIZES: [HintSize; 3] = [HintSize::One, HintSize::HalfALine, HintSize::AWholeLine];

/// How much of the board one hint settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintSize {
    /// One square, or one line.
    One,
    /// Half a row or column.
    HalfALine,
    /// A whole row or column.
    AWholeLine,
}

impl HintSize {
    /// Which size the hint after `spent` hints should be.
    ///
    /// Past the end of the list it stays at the largest. That cannot happen while the
    /// allowance and the list are the same length, and it is the right answer if they
    /// ever are not.
    #[must_use]
    pub fn after(spent: u8) -> HintSize {
        *HINT_SIZES
            .get(usize::from(spent))
            .unwrap_or(&HintSize::AWholeLine)
    }
}

/// One place on a board, in the one shape a page can draw whichever game it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// One square of a Nonogram, counting from 0 at the top left.
    Square { row: u16, column: u16 },
    /// One line of a Slitherlink: which way it runs, and the dot it starts from.
    Line { across: bool, column: u16, row: u16 },
}

/// A hint, after it has been marked on the board.
///
/// The sentence comes from the game's own crate, where the reasoning is. Nothing here
/// writes it, for the same reason nothing here decides whether a board is finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    /// What to tell the player.
    pub says: String,
    /// The places the hint marked, so a page can show what just changed. Empty when
    /// there was nothing to mark: a hint saying a row cannot work marks nothing,
    /// because which square in it is wrong is the player's to decide.
    pub filled: Vec<Place>,
    /// How many hints are left on this puzzle after this one.
    pub hints_left: u8,
}

/// Why there is no hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHint {
    /// The allowance for this puzzle is spent.
    NoneLeft,
    /// Nothing could be worked out from here. Not charged against the allowance: a
    /// player should not pay for an answer that never came.
    NothingToSay,
}

impl std::fmt::Display for NoHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = match self {
            NoHint::NoneLeft => "That is all three hints for this puzzle.",
            NoHint::NothingToSay => "There is nothing this device can work out from here.",
        };
        f.write_str(said)
    }
}

impl std::error::Error for NoHint {}

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

    /// Work the next part of the puzzle out for the player, and mark it in.
    ///
    /// Worked out on the device from the clues, by the game's own reasoning. No answer
    /// is stored or fetched, which is why this works with no signal and why nothing
    /// published alongside a puzzle has to be kept secret.
    ///
    /// The hints grow: one square, then half a row or column, then a whole one. See
    /// [`HINT_SIZES`].
    ///
    /// **The puzzle itself does not change.** A hint marks squares the clues already
    /// decide, through the ordinary rules, so every mark it makes is one the player
    /// could have made and can take back with undo.
    pub fn hint(&mut self) -> Result<Advice, NoHint> {
        match self {
            Session::Nonogram(game) => game.hint(),
            Session::Slitherlink(game) => game.hint(),
        }
    }

    /// How many hints are left on this puzzle.
    #[must_use]
    pub fn hints_left(&self) -> u8 {
        match self {
            Session::Nonogram(game) => game.hints_left(),
            Session::Slitherlink(game) => game.hints_left(),
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
