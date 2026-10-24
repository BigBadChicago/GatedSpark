//! Hints: that they are right, that they explain themselves, and that they never
//! read an answer that is not there.
//!
//! The claim a hint makes is stronger than "here is a digit". It is that the digit
//! follows from the totals and the board in front of the player, so that a hint is help
//! with reasoning rather than a peek at a stored answer. A published Kakuro carries no
//! answer at all, so there is nothing to peek at — but a hint could still *guess* well
//! and look identical from outside, which is what the first few tests here rule out.

mod common;

use common::from_answer;
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_kakuro::hint::{next, Because, Hint, Size};
use gs_kakuro::rules::Kakuro;
use gs_kakuro::solve::Budget;

/// A 2 by 2 with one answer: 1 5 over 2 6. Rows add to 6 and 8, columns to 3 and 11.
fn a_puzzle() -> Kakuro {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    puzzle
}

fn blank(puzzle: &Kakuro) -> State {
    puzzle.blank()
}

#[test]
fn a_run_of_one_square_is_named_as_what_it_is() {
    // A board with a square alone in a run. The honest hint is to say so, not to dress
    // it up as reasoning: the total *is* the digit, and a player who asked for help on
    // a board with one of these on it should be shown it first.
    let (puzzle, _) = from_answer(&[".#", ".."], &[4, 2, 7]);
    let found = next(&puzzle, &blank(&puzzle), Budget::default(), Size::OneSquare);
    match &found {
        Hint::Fill { squares, because } => {
            assert_eq!(squares.len(), 1);
            assert_eq!(*because, Because::ARunOfOneSquare);
            // The sentence says why in the player's words.
            assert!(
                found.plainly().contains("one square long"),
                "{}",
                found.plainly()
            );
        }
        other => panic!("expected a filled square, got {other:?}"),
    }
}

#[test]
fn a_hint_only_ever_names_the_right_digit() {
    // Walked to the end: ask for a hint, write what it says, ask again. Every digit it
    // produces must be the one in the answer, all the way to a finished board. A hint
    // that was merely plausible would show up here as a board that cannot be finished.
    let (puzzle, answer) = from_answer(&["...", ".#.", "..."], &[1, 2, 5, 3, 7, 4, 6, 8]);
    let mut board = blank(&puzzle);

    for step in 0..100 {
        if puzzle.is_solved(&board) {
            break;
        }
        let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
        let Hint::Fill { squares, .. } = &found else {
            panic!("step {step}: nothing to fill on an unfinished board: {found:?}");
        };
        for filled in squares {
            let white = puzzle
                .layout
                .square(filled.at.column, filled.at.row)
                .and_then(|square| puzzle.layout.white_at(square))
                .expect("a hint names a white square");
            assert_eq!(
                puzzle.digit(&answer, white),
                Some(filled.digit),
                "step {step}: the hint named {} at {:?}, the answer holds {:?}",
                filled.digit,
                filled.at,
                puzzle.digit(&answer, white)
            );
            board
                .set(white.0, Cell(filled.digit))
                .expect("the square exists");
        }
    }
    assert!(puzzle.is_solved(&board), "hints alone should finish it");
}

#[test]
fn a_digit_written_wrong_is_pointed_at_before_anything_else() {
    // The board is otherwise untouched, so there is plenty a hint could usefully say.
    // It says this instead, because a player stuck behind their own mistake is not
    // helped by a free digit somewhere else.
    let puzzle = a_puzzle();
    let mut board = blank(&puzzle);
    // The answer's first square is 1. Write 2 there instead: still a legal move, and
    // still consistent with the no-repeats rule at this point.
    board.set(0, Cell(2)).expect("square exists");

    let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    match &found {
        Hint::WrittenWrong { digit, .. } => {
            assert_eq!(*digit, 2);
            assert!(found.plainly().contains("cleared"), "{}", found.plainly());
        }
        // A wrong digit can also make a run unreachable outright, which is a fair thing
        // to be told and is checked in its own test below. Either is an answer about
        // the mistake; what must not happen is a cheerful hint elsewhere.
        Hint::RunCannotWork { .. } | Hint::DigitTwiceInARun { .. } => {}
        other => panic!("expected the mistake to be pointed at, got {other:?}"),
    }
}

#[test]
fn a_digit_twice_in_one_run_is_named_with_the_digit() {
    let puzzle = a_puzzle();
    let mut board = blank(&puzzle);
    // Both squares of the top row set to 3. They add to 6, which is that row's total,
    // and the puzzle is still wrong — which is exactly why the repeat is checked before
    // the sum. A hint that said "this run cannot reach its total" would be false.
    board.set(0, Cell(3)).expect("square exists");
    board.set(1, Cell(3)).expect("square exists");

    let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    match &found {
        Hint::DigitTwiceInARun { digit, .. } => {
            assert_eq!(*digit, 3);
            let said = found.plainly();
            assert!(said.contains("twice"), "{said}");
            assert!(said.contains('3'), "the digit should be named: {said}");
        }
        other => panic!("expected the repeat to be named, got {other:?}"),
    }
}

#[test]
fn a_run_that_can_no_longer_reach_its_total_is_said_so() {
    // The top row adds to 6. Put a 9 in it and no pair of distinct digits can finish.
    let puzzle = a_puzzle();
    let mut board = blank(&puzzle);
    board.set(0, Cell(9)).expect("square exists");

    let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    match &found {
        Hint::RunCannotWork { run } => {
            let said = found.plainly();
            assert!(said.contains("total"), "{said}");
            // The run is named the way a player points at one, with its total in it.
            assert!(said.contains(&run.total.to_string()), "{said}");
        }
        // Being told the digit is simply wrong is at least as good an answer.
        Hint::WrittenWrong { .. } => {}
        other => panic!("expected the stuck run to be named, got {other:?}"),
    }
}

#[test]
fn the_hints_grow() {
    // One square, then half a run, then a whole one. The sizes are the platform's
    // decision and live in `gs_web::play`; what is checked here is that this family
    // actually honours them.
    let (puzzle, _) = from_answer(&["....", "...."], &[1, 2, 3, 4, 5, 6, 7, 8]);
    let board = blank(&puzzle);

    let one = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    let half = next(&puzzle, &board, Budget::default(), Size::HalfARun);
    let whole = next(&puzzle, &board, Budget::default(), Size::AWholeRun);

    let count = |hint: &Hint| match hint {
        Hint::Fill { squares, .. } => squares.len(),
        other => panic!("expected squares to be filled, got {other:?}"),
    };
    assert_eq!(count(&one), 1);
    assert!(
        count(&half) >= count(&one),
        "half a run should be at least one square"
    );
    assert!(
        count(&whole) >= count(&half),
        "a whole run should be at least half of one: {} then {}",
        count(&half),
        count(&whole)
    );
}

#[test]
fn half_a_run_is_never_nothing() {
    // Rounded up, not down. Rounding down would make the second hint do nothing on the
    // runs that are easiest to give away, which in front of a player is a broken
    // button rather than a small hint.
    let (puzzle, _) = from_answer(&[".#", ".."], &[4, 2, 7]);
    let found = next(&puzzle, &blank(&puzzle), Budget::default(), Size::HalfARun);
    match &found {
        Hint::Fill { squares, .. } => assert!(!squares.is_empty()),
        other => panic!("expected at least one square, got {other:?}"),
    }
}

#[test]
fn a_finished_board_has_nothing_left_to_say() {
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    for size in [Size::OneSquare, Size::HalfARun, Size::AWholeRun] {
        let found = next(&puzzle, &answer, Budget::default(), size);
        assert_eq!(found, Hint::NothingToSay, "{size:?}");
        assert!(
            !found.is_something(),
            "a hint with nothing in it must not be charged for"
        );
    }
}

#[test]
fn the_same_board_always_gets_the_same_hint() {
    // Both tie-breaks in the hint search are fixed. A hint that varied would make the
    // move count, and so the board's fingerprint, depend on something nobody recorded.
    let puzzle = a_puzzle();
    let board = blank(&puzzle);
    let first = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    for _ in 0..5 {
        assert_eq!(
            next(&puzzle, &board, Budget::default(), Size::OneSquare),
            first
        );
    }
}

#[test]
fn every_hint_says_something_a_player_could_read() {
    // These sentences are shown to a player, so a variant name is not good enough.
    // Checked as a property over every hint this family can produce on a board walked
    // from blank to finished, plus the three mistake cases.
    let (puzzle, answer) = from_answer(&["...", "..."], &[1, 2, 3, 4, 5, 6]);
    let mut board = blank(&puzzle);
    let mut seen = Vec::new();

    for _ in 0..40 {
        let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
        seen.push(found.clone());
        let Hint::Fill { squares, .. } = &found else {
            break;
        };
        for filled in squares {
            let white = puzzle
                .layout
                .square(filled.at.column, filled.at.row)
                .and_then(|square| puzzle.layout.white_at(square))
                .expect("a white square");
            board
                .set(white.0, Cell(filled.digit))
                .expect("square exists");
        }
    }

    let mut wrong = puzzle.blank();
    wrong.set(0, Cell(9)).expect("square exists");
    seen.push(next(&puzzle, &wrong, Budget::default(), Size::OneSquare));
    seen.push(next(&puzzle, &answer, Budget::default(), Size::OneSquare));

    assert!(seen.len() > 3, "the walk should have produced some hints");
    for found in seen {
        let said = found.plainly();
        assert!(!said.is_empty(), "{found:?} said nothing");
        assert!(
            said.ends_with('.'),
            "{said:?} should be a sentence, ending with a full stop"
        );
        assert!(
            said.chars().next().is_some_and(char::is_uppercase),
            "{said:?} should start with a capital"
        );
    }
}

#[test]
fn a_hint_marks_nothing_when_the_mistake_is_the_players_to_pick() {
    // A run with a digit twice in it names no square to change, on purpose: which of
    // the two is the wrong one is the player's decision, and clearing one could clear
    // the right one.
    let puzzle = a_puzzle();
    let mut board = blank(&puzzle);
    board.set(0, Cell(3)).expect("square exists");
    board.set(1, Cell(3)).expect("square exists");

    let found = next(&puzzle, &board, Budget::default(), Size::OneSquare);
    assert!(matches!(found, Hint::DigitTwiceInARun { .. }), "{found:?}");
    // It is still worth charging for: it told the player something true and useful.
    assert!(found.is_something());
}
