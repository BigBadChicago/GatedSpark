//! Checking a board: what is already wrong, and how far along it is.
//!
//! The loop game's half of the same idea as `gs_nonogram::check`. A player asks "how am
//! I doing?" and gets back every square whose number already has too many sides drawn,
//! whatever the drawn lines add up to as a shape, and a count of how many numbers are
//! met.
//!
//! # Nothing here works the puzzle out
//!
//! Counting the lines around a square and asking [`crate::loops::shape_of`] what the
//! drawn edges make are both done on the board in front of the player. No solver runs,
//! nothing is compared against an answer, and there is no answer here to compare
//! against. That is why checking is free while a hint, which does work part of the
//! puzzle out, costs one of three.
//!
//! # Ruling a line out is never checked
//!
//! An edge the player has ruled out is read here as an edge that is simply not drawn.
//! The game says in its own rules that ruling a line out is a note to yourself and
//! nothing checks it, and `is_solved` reads it the same way. So a check faults the
//! lines a player has **drawn**, and never the ones they have crossed off.
//!
//! This means a number can never be reported as impossible to meet: a 3 with one side
//! drawn and the other three ruled out is, by the rule above, a 3 with one side drawn.
//! Too many sides drawn is the one count that cannot be taken back by changing a note.

use gs_core::state::State;

use crate::board::CellId;
use crate::loops::{drawn_in, Shape};
use crate::rules::Slitherlink;

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Every numbered square with more sides drawn than its number allows, as a column
    /// and a row.
    pub too_many: Vec<(u16, u16)>,
    /// Every corner where three or four drawn lines meet, as a column and a row of
    /// dots. Counted here rather than taken from [`Shape`], because a shape is one
    /// verdict and reports a loose end first: a board with both would otherwise have
    /// its real mistake hidden behind the line somebody is still drawing.
    pub crossings: Vec<(u16, u16)>,
    /// What the drawn lines make. [`Shape::LooseEnd`] is not a fault: a line being
    /// drawn is a line on its way somewhere.
    pub shape: Shape,
    /// How many numbers are already met.
    pub done: usize,
    /// How many numbers there are in all.
    pub numbers: usize,
}

impl Checked {
    /// Whether the shape of the drawn lines is a mistake rather than work in progress.
    #[must_use]
    pub fn shape_is_wrong(&self) -> bool {
        !self.crossings.is_empty() || matches!(self.shape, Shape::SeveralLoops(_))
    }

    /// Whether nothing is wrong. Not the same as finished.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.too_many.is_empty() && !self.shape_is_wrong()
    }

    /// The finding in a sentence a player can read.
    #[must_use]
    pub fn plainly(&self) -> String {
        let progress = format!("{} of the {} numbers are met.", self.done, self.numbers);
        if self.is_clear() {
            return format!("Nothing is wrong so far. {progress}");
        }
        let mut parts = Vec::new();
        if !self.too_many.is_empty() {
            let named = name_a_few(&self.too_many);
            parts.push(if self.too_many.len() == 1 {
                format!("{named} has more sides drawn than its number allows")
            } else {
                format!(
                    "{} squares have more sides drawn than their numbers allow: {named}",
                    self.too_many.len()
                )
            });
        }
        if !self.crossings.is_empty() {
            parts.push(format!(
                "too many lines meet at {}",
                name_a_few_dots(&self.crossings)
            ));
        }
        if matches!(self.shape, Shape::SeveralLoops(_)) {
            parts.push(self.shape.plainly().to_owned());
        }
        format!("{}. {progress}", sentence(&parts))
    }
}

/// Join the findings into one sentence, with the first letter made a capital.
fn sentence(parts: &[String]) -> String {
    let joined = parts.join(", and ");
    let mut letters = joined.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => joined,
    }
}

/// Up to four corners by name, then how many more there are.
fn name_a_few_dots(dots: &[(u16, u16)]) -> String {
    const MOST: usize = 4;
    let shown: Vec<String> = dots
        .iter()
        .take(MOST)
        .map(|(column, row)| {
            format!(
                "the corner at dot {}, {}",
                u32::from(*column) + 1,
                u32::from(*row) + 1
            )
        })
        .collect();
    let listed = shown.join(", ");
    if dots.len() > MOST {
        format!("{listed} and {} more", dots.len() - MOST)
    } else {
        listed
    }
}

/// Up to four squares by name, then how many more there are.
fn name_a_few(squares: &[(u16, u16)]) -> String {
    const MOST: usize = 4;
    let shown: Vec<String> = squares
        .iter()
        .take(MOST)
        .map(|(column, row)| {
            format!(
                "the square at row {}, column {}",
                u32::from(*row) + 1,
                u32::from(*column) + 1
            )
        })
        .collect();
    let listed = shown.join(", ");
    if squares.len() > MOST {
        format!("{listed} and {} more", squares.len() - MOST)
    } else {
        listed
    }
}

/// Check a board as the player has it.
#[must_use]
pub fn check(puzzle: &Slitherlink, state: &State) -> Checked {
    let geometry = puzzle.geometry();
    let mut too_many = Vec::new();
    let mut done = 0;
    let mut numbers = 0;

    for (cell, clue) in puzzle.clues.given() {
        numbers += 1;
        let drawn = puzzle.sides_drawn(state, cell);
        if drawn > clue {
            if let Some(place) = place_of(geometry, cell) {
                too_many.push(place);
            }
        } else if drawn == clue {
            done += 1;
        }
    }

    let mut crossings = Vec::new();
    for dot in geometry.dots() {
        let met = geometry
            .edges_of_dot(dot)
            .into_iter()
            .filter(|edge| drawn_in(state, *edge))
            .count();
        if met >= 3 {
            if let Some(place) = geometry.dot_at(dot) {
                crossings.push(place);
            }
        }
    }

    Checked {
        too_many,
        crossings,
        shape: puzzle.shape(state),
        done,
        numbers,
    }
}

fn place_of(geometry: crate::board::Geometry, cell: CellId) -> Option<(u16, u16)> {
    geometry.cell_at(cell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Along;
    use crate::clues::Clues;
    use crate::rules::{Slitherlink, LINE, NO_LINE};
    use gs_core::state::State;

    /// A 2 by 2 board whose answer is the loop round the outside: every square's
    /// number is 2.
    fn ring() -> Slitherlink {
        let geometry = crate::board::Geometry::new(2, 2).expect("a 2 by 2 board");
        let clues = Clues::new(geometry, vec![Some(2), Some(2), Some(2), Some(2)])
            .expect("a 2 by 2 clue grid");
        Slitherlink::new(clues)
    }

    fn blank(puzzle: &Slitherlink) -> State {
        puzzle.blank()
    }

    fn draw(puzzle: &Slitherlink, state: &mut State, along: Along, x: u16, y: u16, on: bool) {
        let geometry = puzzle.geometry();
        let edge = match along {
            Along::Across => geometry.across(x, y),
            Along::Down => geometry.down(x, y),
        }
        .expect("an edge on the board");
        state
            .set(edge.0, if on { LINE } else { NO_LINE })
            .expect("an edge on the board");
    }

    #[test]
    fn a_blank_board_is_not_wrong() {
        let puzzle = ring();
        let found = check(&puzzle, &blank(&puzzle));
        assert!(found.is_clear());
        assert_eq!(found.numbers, 4);
        assert_eq!(found.done, 0);
        assert!(found.plainly().starts_with("Nothing is wrong"));
    }

    #[test]
    fn too_many_sides_drawn_is_found() {
        let puzzle = ring();
        let mut state = blank(&puzzle);
        // All four sides of the top-left square, whose number is 2.
        draw(&puzzle, &mut state, Along::Across, 0, 0, true);
        draw(&puzzle, &mut state, Along::Across, 0, 1, true);
        draw(&puzzle, &mut state, Along::Down, 0, 0, true);
        draw(&puzzle, &mut state, Along::Down, 1, 0, true);
        let found = check(&puzzle, &state);
        assert!(!found.is_clear());
        assert_eq!(found.too_many, vec![(0, 0)]);
        assert!(
            found.plainly().contains("row 1, column 1"),
            "{}",
            found.plainly()
        );
    }

    #[test]
    fn ruling_lines_out_is_never_a_mistake_by_itself() {
        let puzzle = ring();
        let mut state = blank(&puzzle);
        // Every edge of the top-left square ruled out, although its number is 2.
        draw(&puzzle, &mut state, Along::Across, 0, 0, false);
        draw(&puzzle, &mut state, Along::Across, 0, 1, false);
        draw(&puzzle, &mut state, Along::Down, 0, 0, false);
        draw(&puzzle, &mut state, Along::Down, 1, 0, false);
        let found = check(&puzzle, &state);
        assert!(found.is_clear(), "{}", found.plainly());
    }

    #[test]
    fn a_line_on_its_way_somewhere_is_not_a_mistake() {
        let puzzle = ring();
        let mut state = blank(&puzzle);
        draw(&puzzle, &mut state, Along::Across, 0, 0, true);
        let found = check(&puzzle, &state);
        assert!(matches!(found.shape, Shape::LooseEnd(_)));
        assert!(found.is_clear(), "{}", found.plainly());
    }

    #[test]
    fn lines_meeting_at_a_corner_are_a_mistake() {
        let puzzle = ring();
        let mut state = blank(&puzzle);
        // Three edges at the dot in the middle of the board's top edge.
        draw(&puzzle, &mut state, Along::Across, 0, 0, true);
        draw(&puzzle, &mut state, Along::Across, 1, 0, true);
        draw(&puzzle, &mut state, Along::Down, 1, 0, true);
        let found = check(&puzzle, &state);
        // A loose end as well, which `shape` reports first. The crossing is counted
        // here, so it is not hidden behind it.
        assert!(matches!(found.shape, Shape::LooseEnd(_)));
        assert_eq!(found.crossings, vec![(1, 0)]);
        assert!(found.shape_is_wrong());
        assert!(!found.is_clear());
    }
}
