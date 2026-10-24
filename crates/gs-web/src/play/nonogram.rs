//! Playing a loaded Nonogram.
//!
//! The rules of this module are the ones in [`crate::play`], which every game's session
//! follows: it decides nothing, and every question a move raises is answered by
//! `gs-core` and `gs-nonogram`.

use gs_core::hash::StateHash;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_core::timeline::Timeline;
use gs_nonogram::rules::{mark_of, Action, Nonogram, EMPTY, FILLED};

use super::{why, Advice, HintSize, NoChange, NoHint, Place, HINTS_PER_PUZZLE};
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

    /// The same three states, as the line solver and the hints talk about them.
    #[must_use]
    pub fn of_line(mark: gs_nonogram::line::Mark) -> Mark {
        match mark {
            gs_nonogram::line::Mark::Filled => Mark::Filled,
            gs_nonogram::line::Mark::Empty => Mark::Empty,
            gs_nonogram::line::Mark::Unknown => Mark::Unknown,
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

/// One person playing one puzzle.
///
/// Holds the puzzle's rules and a timeline of everything done so far. Every move, undo
/// and redo goes through the timeline, which is what makes undo exact rather than
/// approximate: it replays recorded deltas instead of guessing at an inverse.
pub struct Session {
    puzzle: Playable,
    rules: Nonogram,
    timeline: Timeline<Nonogram>,
    /// How many hints the player has spent on this puzzle.
    hints_used: u8,
    /// The last hint actually given, so that asking for the same one again is free.
    last_hint: Option<String>,
}

impl Session {
    /// Start playing a Nonogram that passed every check, or nothing if it is not a
    /// Nonogram at all.
    ///
    /// Nothing rather than an empty board: a session of the wrong game cannot be made
    /// here, and a blank board of this game would look like a puzzle rather than like a
    /// mistake. A caller that does not already know which game it has asks
    /// [`Playable::family`], or uses [`crate::play::Session::start`], which takes any
    /// game and always succeeds.
    ///
    /// The board starts blank — every square undecided — which is a real state and not
    /// an absence of one.
    #[must_use]
    pub fn start(puzzle: Playable) -> Option<Self> {
        let clues = puzzle.nonogram()?;
        let rules = Nonogram::new(clues.clone());
        let blank = State::blank(clues.dims);
        let timeline = Timeline::new(&rules, blank);
        Some(Session {
            puzzle,
            rules,
            timeline,
            hints_used: 0,
            last_hint: None,
        })
    }

    /// The clue numbers, which a Nonogram session always has.
    fn clues(&self) -> &gs_nonogram::clues::Clues {
        self.puzzle
            .nonogram()
            .expect("a Nonogram session can only be started on a Nonogram")
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
        let dims = self.clues().dims;
        if row >= dims.height || column >= dims.width {
            return Err(NoChange::NoSuchSquare);
        }
        let index = u32::from(row) * u32::from(dims.width) + u32::from(column);
        self.mark(index, mark)
    }

    /// Work the next part of the puzzle out for the player, and mark it in.
    ///
    /// Worked out here and now from the clue numbers and the board in front of them, by
    /// `gs_nonogram::hint`. There is no answer in this session to look at: a published
    /// puzzle carries clues and a proof, and no board. That is what lets a hint work
    /// with no signal.
    ///
    /// The hints grow — one square, half a row or column, then a whole one — and every
    /// square they fill in goes through `mark`, the same door a tap goes through. So a
    /// hint can be undone square by square, and nothing about the puzzle changes.
    ///
    /// A hint that could say nothing is not charged for, and neither is the same hint
    /// again, which can only happen when it had nothing to mark.
    pub fn hint(&mut self) -> Result<Advice, NoHint> {
        let spent = self.hints_used >= HINTS_PER_PUZZLE;
        let found = gs_nonogram::hint::next(
            &self.rules,
            self.timeline.state(),
            gs_nonogram::solve::Budget::default(),
            match HintSize::after(self.hints_used) {
                HintSize::One => gs_nonogram::hint::Size::OneSquare,
                HintSize::HalfALine => gs_nonogram::hint::Size::HalfALine,
                HintSize::AWholeLine => gs_nonogram::hint::Size::AWholeLine,
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

        // Mark it in. Every square goes through the rules, so a hint cannot write
        // something a player could not have written themselves.
        let mut filled = Vec::new();
        match &found {
            gs_nonogram::hint::Hint::Fill { squares, .. } => {
                for square in squares {
                    if self
                        .mark_at(square.row, square.column, Mark::of_line(square.mark))
                        .is_ok()
                    {
                        filled.push(Place::Square {
                            row: square.row,
                            column: square.column,
                        });
                    }
                }
            }
            // The player's own wrong square, cleared rather than left for them to
            // find. Clearing is a move like any other.
            gs_nonogram::hint::Hint::FilledWrong { row, column } => {
                // Cleared first, then reported, rather than inside the test: a match
                // guard that changes the board would be a trap for the next reader.
                let cleared = self.mark_at(*row, *column, Mark::Unknown);
                if cleared.is_ok() {
                    filled.push(Place::Square {
                        row: *row,
                        column: *column,
                    });
                }
            }
            // A line that cannot work marks nothing: which of those squares is wrong is
            // the player's to decide, and guessing for them could undo the right one.
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
        let dims = self.clues().dims;
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
