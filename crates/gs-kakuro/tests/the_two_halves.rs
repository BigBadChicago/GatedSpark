//! The test file that matters most in this crate.
//!
//! Kakuro's goal predicate is three claims at once: every white square is filled,
//! every run adds up to its total, and no run repeats a digit. None of the three
//! implies the others, and the cheap mistake — the one a reasonable person would
//! make writing this family — is to encode the adding up and assume the repeats
//! follow from it. They do not. `4 + 4` and `3 + 5` both make eight.
//!
//! So each half is checked failing **on its own**, with the other half satisfied
//! exactly. These are this family's version of Slitherlink's two-separate-rings
//! test: a board that passes every check a person would think to write down, and is
//! still wrong.

mod common;

use common::from_answer;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};

/// A run of two squares with a total of eight, and nothing else on the board.
///
/// Deliberately the smallest board that can show the problem. Two squares in one
/// across run, which is also two down runs of one square each.
fn two_squares_making_eight() -> (gs_kakuro::Kakuro, State) {
    from_answer(&[".."], &[3, 5])
}

#[test]
fn the_answer_it_was_built_from_is_accepted() {
    let (puzzle, answer) = two_squares_making_eight();
    assert!(
        puzzle.is_solved(&answer),
        "the board the totals were derived from must be a finished board"
    );
}

#[test]
fn digits_that_add_up_but_repeat_are_refused() {
    let (puzzle, _) = two_squares_making_eight();
    let mut board = puzzle.blank();
    board.set(0, Cell(4)).expect("square exists");
    board.set(1, Cell(4)).expect("square exists");

    assert_eq!(
        puzzle.run_sum(&board, puzzle.layout.runs().next().expect("a run").0),
        Some(8),
        "the adding-up half is satisfied exactly, which is the point of this test"
    );
    assert!(
        !puzzle.is_solved(&board),
        "4 + 4 makes eight and is still not a Kakuro answer, because a digit \
         may not appear twice in one run"
    );
}

#[test]
fn distinct_digits_that_add_up_wrong_are_refused() {
    let (puzzle, _) = two_squares_making_eight();
    let mut board = puzzle.blank();
    board.set(0, Cell(2)).expect("square exists");
    board.set(1, Cell(7)).expect("square exists");

    let across = puzzle.layout.runs().next().expect("a run").0;
    assert!(
        !puzzle.run_repeats(&board, across),
        "the no-repeats half is satisfied exactly"
    );
    assert!(
        !puzzle.is_solved(&board),
        "two and seven are different digits and still add up to nine, not eight"
    );
}

#[test]
fn an_empty_square_is_not_finished() {
    let (puzzle, answer) = two_squares_making_eight();
    let mut board = answer;
    board.set(1, Cell::UNKNOWN).expect("square exists");

    assert!(
        !puzzle.is_solved(&board),
        "an empty white square is a square with no answer in it, so no run \
         containing it can be said to add up"
    );
}

#[test]
fn a_repeat_is_seen_before_the_board_is_full() {
    // A three-square run, two of them filled with the same digit and the third
    // still empty. The page needs to be able to say "that clashes" now, not when
    // the board is full, and this is the method it asks.
    let (puzzle, _) = from_answer(&["..."], &[1, 2, 3]);
    let mut board = puzzle.blank();
    board.set(0, Cell(5)).expect("square exists");
    board.set(2, Cell(5)).expect("square exists");

    let across = puzzle
        .layout
        .runs()
        .find(|(_, run)| run.len() == 3)
        .expect("the across run")
        .0;
    assert!(
        puzzle.run_repeats(&board, across),
        "a clash is a fact about the digits written so far, not about a full board"
    );
    assert!(
        puzzle.run_sum(&board, across).is_none(),
        "and a partly filled run has no sum, so the two checks are independent"
    );
}

#[test]
fn a_repeat_across_two_different_runs_is_allowed() {
    // The rule is "no digit twice in the same run", not "no digit twice on the
    // board". A board where the same digit appears in two different runs is normal
    // and must not be refused, which is the mirror image of the test above and the
    // reason the check is per run rather than per board.
    let (puzzle, answer) = from_answer(&["..", "##", ".."], &[4, 5, 4, 5]);
    assert!(
        puzzle.is_solved(&answer),
        "a four in the top row and a four in the bottom row share no run"
    );
}
