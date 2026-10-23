//! Playing a loaded Slitherlink.
//!
//! The rules of this module are the ones in [`crate::play`], which every game's session
//! follows: it decides nothing, and every question a move raises is answered by
//! `gs-core` and `gs-slitherlink`.
//!
//! # What a player marks here
//!
//! Not squares. The lines between the dots. A board of `w` by `h` squares has
//! `w * (h + 1)` lines running left to right and `h * (w + 1)` running top to bottom,
//! and the numbers in the squares are fixed clues a player never touches.
//!
//! A page therefore asks for a line by saying which way it runs and where it starts,
//! which is how a person points at one. The numbering underneath is `gs-slitherlink`'s
//! and is not repeated here.

use gs_core::hash::StateHash;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_core::timeline::Timeline;
use gs_slitherlink::board::{DotId, EdgeId, Geometry};
use gs_slitherlink::loops::Shape;
use gs_slitherlink::rules::{Action, Slitherlink, LINE, NO_LINE};

use super::{why, Advice, NoChange, NoHint, PointsAt, HINTS_PER_PUZZLE};
use crate::Playable;

/// Which way a line runs.
///
/// The page's own word for it, kept separate from `gs_slitherlink::board::Along` so a
/// page is not writing against the board's internals. They mean the same two things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// Left to right, along the top of a row of squares.
    Across,
    /// Top to bottom, down the side of a column of squares.
    Down,
}

/// What a player can do to one line.
///
/// Three values, not two. Ruling a line out is a real move a player makes on purpose,
/// and it is how a Slitherlink is actually solved; `Unknown` is how they take either
/// mark back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The line is part of the loop.
    Drawn,
    /// The line is definitely not part of the loop.
    RuledOut,
    /// Undecided. A player clearing a mark has not undone a move, they have made one.
    Unknown,
}

impl Line {
    fn as_cell(self) -> Cell {
        match self {
            Line::Drawn => LINE,
            Line::RuledOut => NO_LINE,
            Line::Unknown => Cell::UNKNOWN,
        }
    }

    fn of_cell(cell: Cell) -> Line {
        if cell == LINE {
            Line::Drawn
        } else if cell == NO_LINE {
            Line::RuledOut
        } else {
            Line::Unknown
        }
    }
}

/// One person playing one Slitherlink.
///
/// Holds the puzzle's rules and a timeline of everything done so far. Every move, undo
/// and redo goes through the timeline, which is what makes undo exact rather than
/// approximate: it replays recorded deltas instead of guessing at an inverse.
pub struct Session {
    puzzle: Playable,
    rules: Slitherlink,
    timeline: Timeline<Slitherlink>,
    /// How many hints the player has spent on this puzzle.
    hints_used: u8,
}

impl Session {
    /// Start playing a Slitherlink that passed every check, or nothing if it is not a
    /// Slitherlink at all.
    ///
    /// The board starts blank — every line undecided — which is a real state and not an
    /// absence of one.
    #[must_use]
    pub fn start(puzzle: Playable) -> Option<Self> {
        let clues = puzzle.slitherlink()?;
        let rules = Slitherlink::new(clues.clone());
        let blank = State::blank(rules.geometry().state_dims());
        let timeline = Timeline::new(&rules, blank);
        Some(Session {
            puzzle,
            rules,
            timeline,
            hints_used: 0,
        })
    }

    #[must_use]
    pub fn puzzle(&self) -> &Playable {
        &self.puzzle
    }

    /// The board's shape: how many squares, dots and lines there are, and where each
    /// one is. A page draws from this.
    #[must_use]
    pub fn geometry(&self) -> Geometry {
        self.rules.geometry()
    }

    /// The number in one square, if it has one.
    ///
    /// Most squares have none, and that is the puzzle. A square off the board also
    /// reads as no number, because a page draws a board in a loop and an error there is
    /// a blank page.
    #[must_use]
    pub fn number_at(&self, column: u16, row: u16) -> Option<u8> {
        let cell = self.geometry().cell(column, row)?;
        self.puzzle.slitherlink()?.at(cell)
    }

    /// Mark the line running `which` way from the dot at `column`, `row`.
    ///
    /// Dots are counted from the top-left corner of the board, so a board of `w` by `h`
    /// squares has dots from `0` to `w` across and `0` to `h` down. A line that is not
    /// on the board is `NoSuchSquare` here rather than becoming some other line
    /// somewhere else, which is the bug this method exists to not have.
    pub fn mark(
        &mut self,
        which: Which,
        column: u16,
        row: u16,
        line: Line,
    ) -> Result<(), NoChange> {
        let edge = match which {
            Which::Across => self.geometry().across(column, row),
            Which::Down => self.geometry().down(column, row),
        };
        let Some(edge) = edge else {
            return Err(NoChange::NoSuchSquare);
        };
        self.mark_edge(edge, line)
    }

    /// The same, for a line the page already holds by its number.
    ///
    /// The rules decide whether this is allowed. Nothing here checks the number, the
    /// value, or whether the line already holds it.
    pub fn mark_edge(&mut self, edge: EdgeId, line: Line) -> Result<(), NoChange> {
        self.timeline
            .play(
                &self.rules,
                Action::Set {
                    edge,
                    value: line.as_cell(),
                },
            )
            .map(|_| ())
            .map_err(why)
    }

    /// How one line is currently marked.
    #[must_use]
    pub fn line_at(&self, which: Which, column: u16, row: u16) -> Line {
        let edge = match which {
            Which::Across => self.geometry().across(column, row),
            Which::Down => self.geometry().down(column, row),
        };
        edge.map_or(Line::Unknown, |edge| self.line_of(edge))
    }

    /// The same, for a line the page already holds by its number. A line off the board
    /// reads as undecided rather than throwing.
    #[must_use]
    pub fn line_of(&self, edge: EdgeId) -> Line {
        self.timeline
            .state()
            .get(edge.0)
            .map(Line::of_cell)
            .unwrap_or(Line::Unknown)
    }

    /// The two dots a line joins, for a page that wants to draw it.
    #[must_use]
    pub fn ends_of(&self, edge: EdgeId) -> Option<(DotId, DotId)> {
        self.geometry().ends_of(edge)
    }

    /// How many sides of one square the player has drawn so far, against its number.
    ///
    /// Asked of the rules, so that the count a player is shown beside a clue and the
    /// count the rules judge are the same count. A page working this out from its own
    /// marks would be the second opinion this module exists not to have.
    #[must_use]
    pub fn sides_drawn(&self, column: u16, row: u16) -> Option<u8> {
        let cell = self.geometry().cell(column, row)?;
        Some(self.rules.sides_drawn(self.timeline.state(), cell))
    }

    /// What the drawn lines add up to at the moment: one loop, nothing yet, a loose
    /// end, a crossing, or several separate loops.
    ///
    /// This is the one thing a Slitherlink page can usefully say that a Nonogram page
    /// cannot, and it comes from the rules rather than from here. `Shape::plainly` puts
    /// it in a sentence a player can read.
    #[must_use]
    pub fn shape(&self) -> Shape {
        self.rules.shape(self.timeline.state())
    }

    /// Which squares' numbers are not met by the lines drawn so far.
    ///
    /// Not "wrong": a square whose number is not yet met is the ordinary state of a
    /// puzzle being played. A page may use it to show a player where to look, and
    /// nothing here decides whether it should.
    #[must_use]
    pub fn numbers_not_met(&self) -> Vec<(u16, u16)> {
        let geometry = self.geometry();
        self.rules
            .clues_not_met(self.timeline.state())
            .filter_map(|cell| geometry.cell_at(cell))
            .collect()
    }

    /// The next line the player could work out, and why.
    ///
    /// Worked out here and now from the numbers in the squares and the board in front
    /// of them, by `gs_slitherlink::hint`. There is no answer in this session to look
    /// at, which is what lets a hint work with no signal.
    ///
    /// A hint that could say nothing is not charged for.
    pub fn hint(&mut self) -> Result<Advice, NoHint> {
        if self.hints_used >= HINTS_PER_PUZZLE {
            return Err(NoHint::NoneLeft);
        }
        let found = gs_slitherlink::hint::next(
            &self.rules,
            self.timeline.state(),
            gs_slitherlink::solve::Budget::default(),
        );
        if !found.is_something() {
            return Err(NoHint::NothingToSay);
        }
        self.hints_used = self.hints_used.saturating_add(1);
        let points_at = match found {
            gs_slitherlink::hint::Hint::Line { at, .. }
            | gs_slitherlink::hint::Hint::DrawnWrong { at } => PointsAt::Line {
                across: at.across,
                column: at.column,
                row: at.row,
            },
            _ => PointsAt::Nowhere,
        };
        Ok(Advice {
            says: found.plainly(),
            points_at,
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
    /// What the rules say: the drawn lines must form **one closed loop**, and every
    /// square with a number must have exactly that many of its four sides drawn. A line
    /// left undecided counts as not drawn, so a player never has to rule lines out to
    /// finish — though it is usually how they get there.
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
