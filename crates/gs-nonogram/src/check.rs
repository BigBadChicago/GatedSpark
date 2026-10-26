//! Checking a board: what is already wrong, and how far along it is.
//!
//! # What this is, and what it deliberately is not
//!
//! A player asks "how am I doing?" and gets two things back: every row or column whose
//! marks can no longer match its numbers, and a count of how many lines already do.
//!
//! It is **not** a comparison against an answer. There is no answer here to compare
//! against — a published puzzle carries clues and a proof that they have exactly one
//! answer, and no board — and nothing below works one out either, unlike
//! [`crate::hint`], which may spend a solver budget. Everything here is counting and
//! fitting, on the clues and the player's own marks.
//!
//! That is why checking is free and unlimited while hints are rationed. A check tells a
//! player nothing they could not have worked out by reading their own board against the
//! numbers beside it. A hint tells them something new, so it costs one of three.
//!
//! # Crossing out is never checked
//!
//! A crossed-out square is read here as a square that is simply not filled in yet, the
//! same way [`crate::rules::Nonogram::is_solved`] reads it. The user settled that in so
//! many words: the puzzle is done when the **filled** squares match, and crossing the
//! blanks out is a note the player keeps for themselves.
//!
//! So a check faults a player's filled squares and nothing else. Crossing out every
//! square on the board is not a mistake this will report, because by the rule above it
//! is not a mistake. The alternative — treating a crossed-out square as a promise and
//! holding the player to it — would make a note that "nothing checks" into the thing
//! that fails them, which is the opposite of what was decided.
//!
//! # Why a whole line, and not a square
//!
//! When a line cannot work, which square in it is wrong is not decided: three filled
//! squares where the numbers allow two is a mistake in that line, and the player chose
//! those three. Naming one of them would be a guess, and a guess that could send
//! somebody to undo the square they had right. So a check names the line and stops.

use gs_core::state::State;

use crate::clues::runs_of;
use crate::hint::Because;
use crate::line::{feasible, Mark};
use crate::rules::{mark_of, Nonogram};

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Every row and column whose marks cannot match its numbers, whatever is done to
    /// the squares still blank. In reading order: rows down the board, then columns
    /// across it.
    pub cannot_work: Vec<Because>,
    /// How many rows and columns already match their numbers exactly.
    pub done: usize,
    /// How many rows and columns there are in all.
    pub lines: usize,
}

impl Checked {
    /// Whether nothing is wrong. Not the same as finished.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.cannot_work.is_empty()
    }

    /// The finding in a sentence a player can read.
    ///
    /// The words live here, next to the judgement, for the same reason the hint's words
    /// live next to its reasoning: one copy, which a test can read, and no page writing
    /// its own version of what the rules mean.
    #[must_use]
    pub fn plainly(&self) -> String {
        if self.cannot_work.is_empty() {
            return format!(
                "Nothing is wrong so far. {} of the {} rows and columns already match their numbers.",
                self.done, self.lines
            );
        }
        let named = name_a_few(&self.cannot_work);
        if self.cannot_work.len() == 1 {
            format!("{named} cannot match its numbers. Something in it needs changing.")
        } else {
            format!(
                "{} lines cannot match their numbers: {}. Something in each needs changing.",
                self.cannot_work.len(),
                named
            )
        }
    }
}

/// Up to four lines by name, then how many more there are.
///
/// Four because a sentence listing eleven rows is a sentence nobody reads, and because
/// a player only needs somewhere to start.
fn name_a_few(lines: &[Because]) -> String {
    const MOST: usize = 4;
    let shown: Vec<String> = lines
        .iter()
        .take(MOST)
        .map(|line| line.line_called())
        .collect();
    let listed = shown.join(", ");
    if lines.len() > MOST {
        format!("{listed} and {} more", lines.len() - MOST)
    } else {
        listed
    }
}

/// Check a board as the player has it.
///
/// `state` is the board with the player's marks on it, undecided squares included.
#[must_use]
pub fn check(puzzle: &Nonogram, state: &State) -> Checked {
    let dims = puzzle.clues.dims;
    let board: Vec<Mark> = (0..dims.cell_count())
        .map(|i| {
            let mark = state
                .get(u32::try_from(i).unwrap_or(u32::MAX))
                .map_or(Mark::Unknown, mark_of);
            // Crossed out is read as not yet filled; see the note at the top.
            if mark == Mark::Empty {
                Mark::Unknown
            } else {
                mark
            }
        })
        .collect();

    let mut cannot_work = Vec::new();
    let mut done = 0;
    let mut lines = 0;

    for (which, cells, runs) in every_line(puzzle, &board) {
        lines += 1;
        if !feasible(&cells, &runs) {
            cannot_work.push(which);
            continue;
        }
        // A line that already matches. Undecided counts as not filled, which is the
        // same reading `is_solved` uses: crossing the blanks out is a note the player
        // keeps for themselves and nothing checks it.
        let filled: Vec<bool> = cells.iter().map(|mark| *mark == Mark::Filled).collect();
        if runs_of(&filled) == runs {
            done += 1;
        }
    }

    Checked {
        cannot_work,
        done,
        lines,
    }
}

/// Every row and then every column, with its marks and its numbers.
///
/// A second copy of the walk in [`crate::hint`] would drift from it, so the shape is
/// the same on purpose: rows first, in order, then columns. A player who checks and
/// then asks for a hint is told about the same line first.
fn every_line(puzzle: &Nonogram, board: &[Mark]) -> Vec<(Because, Vec<Mark>, Vec<u16>)> {
    let dims = puzzle.clues.dims;
    let width = usize::from(dims.width);
    let mut out = Vec::new();
    for y in 0..dims.height {
        let cells: Vec<Mark> = (0..dims.width)
            .map(|x| board[usize::from(y) * width + usize::from(x)])
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
            .map(|y| board[usize::from(y) * width + usize::from(x)])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clues::{Clues, Pattern};
    use crate::rules::{EMPTY, FILLED};
    use gs_core::ruleset::Ruleset;
    use gs_core::state::State;

    /// A small puzzle whose answer is a plus sign.
    fn plus() -> Nonogram {
        let pattern = Pattern::from_rows(&[".#.", "###", ".#."]).expect("a square pattern");
        Nonogram::new(Clues::of(&pattern))
    }

    fn with(puzzle: &Nonogram, marks: &[(u16, u16, bool)]) -> State {
        let mut state = State::blank(puzzle.clues.dims);
        for (row, column, filled) in marks {
            let index = u32::from(*row) * 3 + u32::from(*column);
            state
                .set(index, if *filled { FILLED } else { EMPTY })
                .expect("a square on the board");
        }
        state
    }

    #[test]
    fn a_blank_board_is_not_wrong() {
        let puzzle = plus();
        let found = check(&puzzle, &State::blank(puzzle.clues.dims));
        assert!(found.is_clear());
        assert_eq!(found.lines, 6);
        // The middle row and middle column are `1` lines on this puzzle, so a blank
        // board matches none of the six.
        assert_eq!(found.done, 0);
    }

    #[test]
    fn too_many_filled_in_a_line_is_found() {
        let puzzle = plus();
        // Row 0 wants one filled square. Three is not one, and no amount of further
        // marking makes it one.
        let state = with(&puzzle, &[(0, 0, true), (0, 1, true), (0, 2, true)]);
        let found = check(&puzzle, &state);
        assert!(!found.is_clear());
        assert!(found.cannot_work.contains(&Because::Row(0)));
        assert!(found.plainly().contains("row 1"));
    }

    #[test]
    fn a_line_that_matches_is_counted() {
        let puzzle = plus();
        // The middle row, filled right across, is exactly its own clue of 3.
        let state = with(&puzzle, &[(1, 0, true), (1, 1, true), (1, 2, true)]);
        let found = check(&puzzle, &state);
        assert!(found.is_clear());
        // Three lines, not one: filling the middle row right across also settles
        // column 1 and column 3, each of which wants a single filled square.
        assert_eq!(found.done, 3);
        assert!(found.plainly().contains("3 of the 6"));
    }

    #[test]
    fn the_right_answer_checks_out_completely() {
        let puzzle = plus();
        let state = with(
            &puzzle,
            &[
                (0, 1, true),
                (1, 0, true),
                (1, 1, true),
                (1, 2, true),
                (2, 1, true),
            ],
        );
        let found = check(&puzzle, &state);
        assert!(found.is_clear());
        assert_eq!(found.done, found.lines);
        assert!(puzzle.is_solved(&state));
    }

    #[test]
    fn crossing_a_square_out_is_never_a_mistake_by_itself() {
        let puzzle = plus();
        // Every square crossed out. Wrong as an answer, but nothing here can say so:
        // an undecided square and a crossed-out one are both "not filled", and the
        // player is entitled to cross out as a note to themselves.
        let mut marks = Vec::new();
        for row in 0..3u16 {
            for column in 0..3u16 {
                marks.push((row, column, false));
            }
        }
        let state = with(&puzzle, &marks);
        let found = check(&puzzle, &state);
        assert!(found.is_clear());
    }

    #[test]
    fn several_wrong_lines_are_all_named() {
        let puzzle = plus();
        // Filling the top row and the left column right across breaks both of them,
        // each of which wants a single filled square.
        let state = with(
            &puzzle,
            &[
                (0, 0, true),
                (0, 1, true),
                (0, 2, true),
                (1, 0, true),
                (2, 0, true),
            ],
        );
        let found = check(&puzzle, &state);
        assert!(found.cannot_work.len() >= 2);
        let said = found.plainly();
        assert!(said.contains("lines cannot match"), "said: {said}");
    }
}
