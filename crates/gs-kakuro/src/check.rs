//! Checking a board: what is already wrong, and how far along it is.
//!
//! The digit game's half of the same idea as `gs_nonogram::check`. A player asks "how
//! am I doing?" and gets back every run that already breaks one of the three rules,
//! and a count of how many runs are finished.
//!
//! # Nothing here works the puzzle out
//!
//! Each fault below is found by adding up the digits the player wrote and comparing
//! them with the total printed on the board. No solver runs, nothing is compared
//! against an answer, and there is no answer here to compare against. That is why
//! checking is free while a hint, which does work part of the puzzle out, costs one of
//! three.
//!
//! # The three faults, and why no fourth
//!
//! - **A digit twice in one run.** Already shown as you play, and repeated here so one
//!   question has one answer.
//! - **Past the total.** The digits written so far add up to more than the run's total,
//!   so no digit in the squares still empty can rescue it.
//! - **Full and wrong.** Every square in the run has a digit and they do not add up.
//!
//! A fifth kind of fault — a run that cannot *reach* its total with the squares it has
//! left — would need the digits still available worked out, which is reasoning rather
//! than counting, and is the hint's job. The line is drawn at what a player can check
//! for themselves with a pencil.

use gs_core::state::State;

use crate::layout::RunId;
use crate::rules::Kakuro;

/// What is wrong with one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The same digit appears twice in it.
    ADigitTwice,
    /// What is written already adds up to more than the total.
    PastTheTotal,
    /// Every square has a digit, and they do not add up to the total.
    FullAndWrong,
}

impl Why {
    /// The fault in words, as part of a sentence about a run.
    #[must_use]
    pub fn plainly(self) -> &'static str {
        match self {
            Why::ADigitTwice => "has the same digit in it twice",
            Why::PastTheTotal => "is already past its total",
            Why::FullAndWrong => "is full and does not add up to its total",
        }
    }
}

/// One run with something wrong with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wrong {
    pub run: RunId,
    pub why: Why,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Every run with something wrong with it, in the layout's own order.
    pub wrong: Vec<Wrong>,
    /// How many runs already add up, with no digit twice.
    pub done: usize,
    /// How many runs there are in all.
    pub runs: usize,
}

impl Checked {
    /// Whether nothing is wrong. Not the same as finished.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.wrong.is_empty()
    }

    /// The finding in a sentence a player can read.
    #[must_use]
    pub fn plainly(&self) -> String {
        let progress = format!("{} of the {} runs add up.", self.done, self.runs);
        if self.wrong.is_empty() {
            return format!("Nothing is wrong so far. {progress}");
        }
        if let [only] = self.wrong.as_slice() {
            return format!("One run {}. {progress}", only.why.plainly());
        }
        // Counted by kind rather than listed one by one. A run has no name a player
        // would recognise — "run 14" means nothing on a board where the runs are not
        // numbered — so the honest thing to report is how many and of what kind, with
        // the squares themselves marked on the board.
        let mut twice = 0;
        let mut past = 0;
        let mut full = 0;
        for wrong in &self.wrong {
            match wrong.why {
                Why::ADigitTwice => twice += 1,
                Why::PastTheTotal => past += 1,
                Why::FullAndWrong => full += 1,
            }
        }
        let mut parts = Vec::new();
        if twice > 0 {
            parts.push(format!("{twice} have a digit in them twice"));
        }
        if past > 0 {
            parts.push(format!("{past} are already past their totals"));
        }
        if full > 0 {
            parts.push(format!("{full} are full and do not add up"));
        }
        format!(
            "{} runs are wrong: {}. {progress}",
            self.wrong.len(),
            parts.join(", and ")
        )
    }
}

/// Check a board as the player has it.
#[must_use]
pub fn check(puzzle: &Kakuro, state: &State) -> Checked {
    let mut wrong = Vec::new();
    let mut done = 0;
    let mut runs = 0;

    for (id, run) in puzzle.layout.runs() {
        runs += 1;
        let total = puzzle.totals.get(id);

        if puzzle.run_repeats(state, id) {
            wrong.push(Wrong {
                run: id,
                why: Why::ADigitTwice,
            });
            continue;
        }

        // What is written so far, which is not the same question as `run_sum`: that
        // one answers nothing at all until every square in the run has a digit.
        let mut written = 0u16;
        let mut empty = 0usize;
        for white in &run.squares {
            match puzzle.digit(state, *white) {
                Some(digit) => written = written.saturating_add(u16::try_from(digit).unwrap_or(0)),
                None => empty += 1,
            }
        }

        let Some(total) = total else { continue };
        if written > total {
            wrong.push(Wrong {
                run: id,
                why: Why::PastTheTotal,
            });
        } else if empty == 0 {
            if written == total {
                done += 1;
            } else {
                wrong.push(Wrong {
                    run: id,
                    why: Why::FullAndWrong,
                });
            }
        }
    }

    Checked { wrong, done, runs }
}

/// Every white square of every run with something wrong with it, as a column and a row.
///
/// Separate from [`check`] because the page marks squares while the sentence talks
/// about runs, and working out which squares belong to a run means knowing the board's
/// shape — which is the layout's to say, not a page's to re-derive.
#[must_use]
pub fn squares_to_mark(puzzle: &Kakuro, found: &Checked) -> Vec<(u16, u16)> {
    let layout = &puzzle.layout;
    let mut out = Vec::new();
    for wrong in &found.wrong {
        let Some(run) = layout.run(wrong.run) else {
            continue;
        };
        for white in &run.squares {
            if let Some(place) = layout
                .square_of(*white)
                .and_then(|square| layout.square_at(square))
            {
                out.push(place);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;
    use crate::totals::Totals;

    /// A small board whose runs are all two squares long and all want 5, which two
    /// different digits can make (1 + 4, or 2 + 3) and can also fall short of.
    fn small() -> Kakuro {
        // A 3 by 3 grid: the top row and left column are black, the rest white.
        let blocks = vec![
            true, true, true, //
            true, false, false, //
            true, false, false,
        ];
        let layout = Layout::new(3, 3, blocks).expect("a 3 by 3 layout");
        // Four runs of two squares each, every one of them wanting 5.
        let totals = Totals::new(&layout, vec![5; layout.run_count()]).expect("totals");
        Kakuro::new(layout, totals).expect("a puzzle")
    }

    fn write(puzzle: &Kakuro, state: &mut State, column: u16, row: u16, digit: i16) {
        let layout = &puzzle.layout;
        let square = layout.square(column, row).expect("a square on the board");
        let white = layout.white_at(square).expect("a white square");
        // The board's state is indexed by white square, not by grid square: the black
        // ones hold nothing a player can change and so take up no room in it.
        state
            .set(white.0, gs_core::state::Cell(digit))
            .expect("a white square on the board");
    }

    #[test]
    fn the_test_board_is_shaped_as_expected() {
        let puzzle = small();
        assert_eq!(puzzle.layout.white_count(), 4);
        assert_eq!(puzzle.layout.run_count(), 4);
    }

    #[test]
    fn a_blank_board_is_not_wrong() {
        let puzzle = small();
        let found = check(&puzzle, &puzzle.blank());
        assert!(found.is_clear());
        assert_eq!(found.done, 0);
        assert!(found.plainly().starts_with("Nothing is wrong"));
    }

    #[test]
    fn a_digit_twice_in_a_run_is_found() {
        let puzzle = small();
        let mut state = puzzle.blank();
        write(&puzzle, &mut state, 1, 1, 1);
        write(&puzzle, &mut state, 2, 1, 1);
        let found = check(&puzzle, &state);
        assert!(!found.is_clear());
        assert!(found
            .wrong
            .iter()
            .any(|wrong| wrong.why == Why::ADigitTwice));
        assert!(!squares_to_mark(&puzzle, &found).is_empty());
    }

    #[test]
    fn going_past_the_total_is_found() {
        let puzzle = small();
        let mut state = puzzle.blank();
        // One square of a run that wants 5, with a 9 in it.
        write(&puzzle, &mut state, 1, 1, 9);
        let found = check(&puzzle, &state);
        assert!(found
            .wrong
            .iter()
            .any(|wrong| wrong.why == Why::PastTheTotal));
    }

    #[test]
    fn a_full_run_that_does_not_add_up_is_found() {
        let puzzle = small();
        let mut state = puzzle.blank();
        // Full, no repeat, and 1 + 2 is 3, which is short of 5.
        write(&puzzle, &mut state, 1, 2, 1);
        write(&puzzle, &mut state, 2, 2, 2);
        let found = check(&puzzle, &state);
        assert!(found
            .wrong
            .iter()
            .any(|wrong| wrong.why == Why::FullAndWrong));
    }

    #[test]
    fn a_run_that_adds_up_is_counted() {
        let puzzle = small();
        let mut state = puzzle.blank();
        write(&puzzle, &mut state, 1, 2, 1);
        write(&puzzle, &mut state, 2, 2, 4);
        let found = check(&puzzle, &state);
        assert!(found.done >= 1, "{}", found.plainly());
    }
}
