//! Playing a loaded Kakuro.
//!
//! The rules of this module are the ones in [`crate::play`], which every game's session
//! follows: it decides nothing, and every question a move raises is answered by
//! `gs-core` and `gs-kakuro`.
//!
//! # What a player writes here, and why it is the interesting case
//!
//! A digit, one to nine, in a white square. That is the first move in this project that
//! carries a **value the player chooses** rather than a mark they switch on and off.
//! Nonogram has filled, crossed out and blank; Slitherlink has drawn, ruled out and
//! undecided. Both are small fixed sets, and every layer above them — this session, the
//! page, the keyboard handling, the hints — was written against that assumption. A
//! Kakuro square has ten states, and the point of this family being third was to find
//! out what that breaks.
//!
//! What it broke, in the end, is smaller than expected and all of it is here:
//!
//! * A move needs a value as well as a place, so [`Session::write`] takes a digit and
//!   [`Session::clear`] exists as its own method rather than being "write nothing".
//! * A page cannot show a square's state as a class name from a fixed list. It has to
//!   render the number, which is why [`Session::digit_at`] returns the digit rather
//!   than a filled-or-not answer.
//! * A digit a player types has to be refused **before** it reaches the rules if it is
//!   not a digit at all. See [`Session::write`].
//!
//! Nothing below the session needed changing, which is the finding worth recording: the
//! timeline, the delta, the hash and the publication gate are all indifferent to how
//! many values a cell can take, because none of them ever enumerated them.
//!
//! # Black squares are not a state a player can reach
//!
//! The board's shape is part of the puzzle. A black square has no logical state at all —
//! it is not in the state vector, which holds one entry per **white** square — so there
//! is no move that could turn a square black or white. A page asking about a black
//! square gets `None`, not a blank digit.

use gs_core::hash::StateHash;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_core::timeline::Timeline;
use gs_kakuro::layout::{Along, Layout, RunId, WhiteId};
use gs_kakuro::rules::{Action, Kakuro};
use gs_kakuro::totals::{LEAST_DIGIT, MOST_DIGIT};

use super::{why, Advice, HintSize, NoChange, NoHint, Place, HINTS_PER_PUZZLE};
use crate::Playable;

/// A total, and which way the run it belongs to goes.
///
/// A page draws these in the black square at the head of a run: the across total above
/// the diagonal, the down total below it. That is how a printed Kakuro does it, and
/// doing it any other way would make a familiar puzzle unreadable to somebody who knows
/// the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heading {
    /// The total for the run going right from here, if one starts here.
    pub across: Option<u16>,
    /// The total for the run going down from here, if one starts here.
    pub down: Option<u16>,
}

impl Heading {
    /// Whether this black square carries anything at all. Most do not.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.across.is_none() && self.down.is_none()
    }
}

/// What one square on the grid is.
///
/// Three cases and not two, because a black square with totals on it and a black square
/// with nothing on it are drawn differently and a page needs to tell them apart without
/// asking a second question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Square {
    /// A white square: the digit written in it, or nothing if it is empty.
    White { digit: Option<i16> },
    /// A black square, carrying whichever totals start at it.
    Black(Heading),
    /// Not on the board at all.
    OffTheBoard,
}

/// One person playing one Kakuro.
///
/// Holds the puzzle's rules and a timeline of everything done so far. Every move, undo
/// and redo goes through the timeline, which is what makes undo exact rather than
/// approximate: it replays recorded deltas instead of guessing at an inverse.
pub struct Session {
    puzzle: Playable,
    rules: Kakuro,
    timeline: Timeline<Kakuro>,
    /// How many hints the player has spent on this puzzle.
    hints_used: u8,
    /// The last hint actually given, so that asking for the same one again is free.
    last_hint: Option<String>,
}

impl Session {
    /// Start playing a Kakuro that passed every check, or nothing if it is not a Kakuro
    /// at all.
    ///
    /// The board starts blank — every white square empty — which is a real state and not
    /// an absence of one.
    #[must_use]
    pub fn start(puzzle: Playable) -> Option<Self> {
        let rules = puzzle.kakuro()?.clone();
        let blank = rules.blank();
        let timeline = Timeline::new(&rules, blank);
        Some(Session {
            puzzle,
            rules,
            timeline,
            hints_used: 0,
            last_hint: None,
        })
    }

    #[must_use]
    pub fn puzzle(&self) -> &Playable {
        &self.puzzle
    }

    /// The board's shape: where the black squares are and which runs they make.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.rules.layout
    }

    /// What is at one place on the grid.
    ///
    /// A page draws from this, square by square, and never has to know that white
    /// squares are numbered separately underneath.
    #[must_use]
    pub fn square_at(&self, column: u16, row: u16) -> Square {
        let layout = self.layout();
        let Some(square) = layout.square(column, row) else {
            return Square::OffTheBoard;
        };
        match layout.white_at(square) {
            Some(white) => Square::White {
                digit: self.rules.digit(self.timeline.state(), white),
            },
            None => Square::Black(self.heading_at(column, row)),
        }
    }

    /// The totals written on the black square at `column`, `row`.
    ///
    /// Worked out by asking which runs **start** at the square just right of here and
    /// just below here. A run's head is the black square before it, which is where a
    /// printed Kakuro puts the number, and the rim counts as a head in the same way —
    /// a run starting in the first column has no black square to its left, so its total
    /// is drawn outside the grid by the page.
    #[must_use]
    pub fn heading_at(&self, column: u16, row: u16) -> Heading {
        Heading {
            across: self.total_of_run_starting_at(column.checked_add(1), Some(row), Along::Across),
            down: self.total_of_run_starting_at(Some(column), row.checked_add(1), Along::Down),
        }
    }

    /// The total of the run that **begins at this white square** and goes `across`, or
    /// down when `across` is false. Nothing if no run begins here.
    ///
    /// This is [`Session::heading_at`] asked from the other side, and it exists for the
    /// runs a page cannot otherwise draw. A run's total is normally written in the black
    /// square in front of it, but a run starting hard against the left or top rim has no
    /// square in front of it, so its total has to be drawn in the margin — and a page
    /// looking only at black squares would silently lose it. On a sparse board that is
    /// most of the runs.
    #[must_use]
    pub fn heading_for_rim(&self, column: u16, row: u16, across: bool) -> Option<u16> {
        self.total_of_run_starting_at(
            Some(column),
            Some(row),
            if across { Along::Across } else { Along::Down },
        )
    }

    /// The total for the run of `along` that begins at this square, if one does.
    fn total_of_run_starting_at(
        &self,
        column: Option<u16>,
        row: Option<u16>,
        along: Along,
    ) -> Option<u16> {
        let layout = self.layout();
        let square = layout.square(column?, row?)?;
        let white = layout.white_at(square)?;
        let (across, down) = layout.runs_of(white)?;
        let id = match along {
            Along::Across => across,
            Along::Down => down,
        };
        let run = layout.run(id)?;
        // Only if this white square is where that run begins. Every white square is in
        // an across run and a down run, so without this check every square would look
        // like the head of two.
        if run.squares.first().copied() != Some(white) {
            return None;
        }
        self.rules.totals.get(id)
    }

    /// Write a digit in the white square at `column`, `row`.
    ///
    /// A value outside one to nine is refused here, before the rules see it. That is
    /// not duplicated judgement: the rules would refuse it too, and this exists because
    /// a page reads a keystroke and a keystroke is not a digit. Catching it here means a
    /// page can pass on whatever it read without first deciding what counts as a digit,
    /// which is the decision this module would rather own than leave to a page.
    pub fn write(&mut self, column: u16, row: u16, digit: i16) -> Result<(), NoChange> {
        if !(LEAST_DIGIT..=MOST_DIGIT).contains(&digit) {
            return Err(NoChange::TheRulesRefused);
        }
        let square = self.white_at(column, row).ok_or(NoChange::NoSuchSquare)?;
        self.set(square, Cell(digit))
    }

    /// Empty the white square at `column`, `row`.
    ///
    /// Its own method rather than `write(0)`. Zero is not a Kakuro digit — the family
    /// refuses it outright, because a nought would satisfy a total of nine beside a nine
    /// and the no-repeats rule would not catch it — so there is no number that means
    /// "empty", and an empty square is `Cell::UNKNOWN`.
    ///
    /// A player clearing a square has not undone a move, they have made one.
    pub fn clear(&mut self, column: u16, row: u16) -> Result<(), NoChange> {
        let square = self.white_at(column, row).ok_or(NoChange::NoSuchSquare)?;
        self.set(square, Cell::UNKNOWN)
    }

    fn set(&mut self, square: WhiteId, value: Cell) -> Result<(), NoChange> {
        self.timeline
            .play(&self.rules, Action::Write { square, value })
            .map(|_| ())
            .map_err(why)
    }

    fn white_at(&self, column: u16, row: u16) -> Option<WhiteId> {
        let layout = self.layout();
        layout.white_at(layout.square(column, row)?)
    }

    /// The digit in one white square, or nothing if it is empty, black, or off the
    /// board.
    ///
    /// Three different reasons for the same answer, deliberately flattened: a page
    /// drawing a grid in a loop wants one question per square, and it already has
    /// [`Session::square_at`] when it needs to tell them apart.
    #[must_use]
    pub fn digit_at(&self, column: u16, row: u16) -> Option<i16> {
        self.rules
            .digit(self.timeline.state(), self.white_at(column, row)?)
    }

    /// What the digits written in one run add up to so far, and what it needs.
    ///
    /// Asked of the rules, so that the sum a player is shown beside a total and the sum
    /// the rules judge are the same sum. A page adding the digits up itself would be the
    /// second opinion this module exists not to have.
    ///
    /// The sum is `None` while any square in the run is still empty, which is what the
    /// rules mean by it: a run with a gap has no sum yet, rather than a sum that happens
    /// to be short.
    #[must_use]
    pub fn run_so_far(&self, run: RunId) -> Option<(Option<u16>, u16)> {
        let total = self.rules.totals.get(run)?;
        Some((self.rules.run_sum(self.timeline.state(), run), total))
    }

    /// Which runs do not yet add up to their totals.
    ///
    /// Not "wrong": a run that is not yet met is the ordinary state of a puzzle being
    /// played, and every run on a blank board is in this list. A page may use it to show
    /// a player where to look, and nothing here decides whether it should.
    #[must_use]
    pub fn runs_not_met(&self) -> Vec<RunId> {
        self.rules.runs_not_met(self.timeline.state()).collect()
    }

    /// Which runs have a digit twice in them.
    ///
    /// This one *is* a mistake, and it is the one Kakuro mistake a player can see at a
    /// glance once it is pointed out. Kept separate from [`Session::runs_not_met`] for
    /// that reason: a page may want to mark a repeat in a way it would not mark an
    /// unfinished run.
    #[must_use]
    pub fn runs_with_a_repeat(&self) -> Vec<RunId> {
        self.layout()
            .runs()
            .map(|(id, _)| id)
            .filter(|id| self.rules.run_repeats(self.timeline.state(), *id))
            .collect()
    }

    /// Work the next part of the puzzle out for the player, and write it in.
    ///
    /// Worked out here and now from the totals and the board in front of them, by
    /// `gs_kakuro::hint`. There is no answer in this session to look at, which is what
    /// lets a hint work with no signal.
    ///
    /// The hints grow — one square, half a run, then a whole run — and every digit they
    /// settle goes through `write`, the same door a typed digit goes through. So a hint
    /// can be undone square by square, and nothing about the puzzle changes.
    ///
    /// A hint that could say nothing is not charged for, and neither is the same hint
    /// again, which can only happen when it had nothing to write.
    pub fn hint(&mut self) -> Result<Advice, NoHint> {
        let spent = self.hints_used >= HINTS_PER_PUZZLE;
        let found = gs_kakuro::hint::next(
            &self.rules,
            self.timeline.state(),
            gs_kakuro::solve::Budget::default(),
            match HintSize::after(self.hints_used) {
                HintSize::One => gs_kakuro::hint::Size::OneSquare,
                HintSize::HalfALine => gs_kakuro::hint::Size::HalfARun,
                HintSize::AWholeLine => gs_kakuro::hint::Size::AWholeRun,
            },
        );
        if !found.is_something() {
            return Err(NoHint::NothingToSay);
        }
        let says = found.plainly();
        let again = self.last_hint.as_deref() == Some(says.as_str());
        if spent && !again {
            return Err(NoHint::NoneLeft);
        }

        // Write it in. Every digit goes through the rules, so a hint cannot write
        // something a player could not have written themselves.
        let mut filled = Vec::new();
        match &found {
            gs_kakuro::hint::Hint::Fill { squares, .. } => {
                for square in squares {
                    if self
                        .write(square.at.column, square.at.row, square.digit)
                        .is_ok()
                    {
                        filled.push(Place::Digit {
                            column: square.at.column,
                            row: square.at.row,
                        });
                    }
                }
            }
            // The player's own wrong digit, cleared rather than left for them to find.
            gs_kakuro::hint::Hint::WrittenWrong { at, .. } => {
                let cleared = self.clear(at.column, at.row).is_ok();
                if cleared {
                    filled.push(Place::Digit {
                        column: at.column,
                        row: at.row,
                    });
                }
            }
            // A run that cannot work, or one with a repeat in it, writes nothing: which
            // digit in it is the wrong one is the player's to decide, and guessing could
            // clear the right one.
            _ => {}
        }

        if !again {
            self.hints_used = self.hints_used.saturating_add(1);
            self.last_hint = Some(says.clone());
        }
        Ok(Advice {
            says,
            filled,
            hints_left: self.hints_left(),
        })
    }

    /// How many hints are left on this puzzle.
    #[must_use]
    pub fn hints_left(&self) -> u8 {
        HINTS_PER_PUZZLE.saturating_sub(self.hints_used)
    }

    pub fn undo(&mut self) -> Result<(), NoChange> {
        self.timeline.undo().map(|_| ()).map_err(why)
    }

    pub fn redo(&mut self) -> Result<(), NoChange> {
        self.timeline.redo().map(|_| ()).map_err(why)
    }

    /// Whether the puzzle is solved.
    ///
    /// Asked of the rules, every time, with no cached answer and no shortcut. The whole
    /// module exists so that this line is the only answer to this question.
    ///
    /// What the rules say: every white square holds a digit, every run adds up to its
    /// total, and no run has a digit twice. Unlike both other games there is nothing
    /// optional about finishing here — a Nonogram is done whether or not the blanks were
    /// crossed out, and a Slitherlink whether or not the unused lines were ruled out,
    /// but a Kakuro with an empty square has a run that does not meet its total.
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

    /// The board as the engine holds it, for a caller that wants to ask the rules
    /// something this session does not expose.
    #[must_use]
    pub fn board(&self) -> &State {
        self.timeline.state()
    }
}
