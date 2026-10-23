//! Hints: are they true, are they derived, and do they leave the puzzle alone?
//!
//! The three things that could go wrong here, in the order they would hurt:
//!
//! 1. **A hint that is wrong.** Every hint below is checked against the puzzle's one
//!    answer, which these tests know and the hint code does not.
//! 2. **A hint that reads an answer.** `next` is given the clues and the player's
//!    board, and nothing else; there is no answer in scope to read. The test that
//!    matters for this is the one where a hint is asked for on a blank board and still
//!    gets a true answer out of the numbers alone.
//! 3. **A hint that changes the puzzle.** It takes the board by reference and hands
//!    back a sentence. The test here checks the board is untouched afterwards.

use gs_core::state::{Dims, State};
use gs_nonogram::clues::{Clues, Pattern};
use gs_nonogram::hint::{next, Because, Hint};
use gs_nonogram::line::Mark;
use gs_nonogram::rules::{Nonogram, EMPTY, FILLED};
use gs_nonogram::solve::Budget;

fn puzzle(rows: &[&str]) -> (Nonogram, Pattern) {
    let pattern = Pattern::from_rows(rows).expect("rectangular");
    (Nonogram::new(Clues::of(&pattern)), pattern)
}

fn blank(puzzle: &Nonogram) -> State {
    State::blank(puzzle.clues.dims)
}

/// Whether the one answer has this square filled.
fn answer_fills(pattern: &Pattern, row: u16, column: u16) -> bool {
    pattern.row(row)[usize::from(column)]
}

fn index(dims: Dims, row: u16, column: u16) -> u32 {
    dims.index_of(column, row).expect("on the board")
}

#[test]
fn a_hint_on_a_blank_board_is_true_and_comes_from_a_line() {
    let (p, pattern) = puzzle(&["##.", ".#.", ".##"]);
    let hint = next(&p, &blank(&p), Budget::default());
    let Hint::Square {
        row,
        column,
        mark,
        because,
    } = hint
    else {
        panic!("expected a square to be forced on a blank board, got {hint:?}");
    };
    // A real deduction, from one line's numbers, not from working the puzzle out.
    assert!(
        matches!(because, Because::Row(_) | Because::Column(_)),
        "a blank board should have something forced by a single line, said {because:?}"
    );
    assert_eq!(
        mark == Mark::Filled,
        answer_fills(&pattern, row, column),
        "the hint disagrees with the puzzle's one answer at row {row}, column {column}"
    );
}

#[test]
fn every_hint_along_a_whole_game_agrees_with_the_one_answer() {
    // The load-bearing test. Play the puzzle entirely by hints, and check each one
    // against the answer before making the move. A hint that was ever wrong stops this.
    let (p, pattern) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    let mut given = 0;

    loop {
        let hint = next(&p, &board, Budget::default());
        match hint {
            Hint::Square {
                row, column, mark, ..
            } => {
                assert_eq!(
                    mark == Mark::Filled,
                    answer_fills(&pattern, row, column),
                    "hint {given} is wrong at row {row}, column {column}"
                );
                let value = if mark == Mark::Filled { FILLED } else { EMPTY };
                board
                    .set(index(dims, row, column), value)
                    .expect("on the board");
                given += 1;
            }
            Hint::NothingToSay => break,
            other => panic!("nothing is wrong on this board, so why {other:?}"),
        }
        assert!(given < 1000, "hints are going round in a circle");
    }

    assert!(given > 0, "no hint was ever given");
    // Following hints to the end finishes the puzzle. The hints stop because there is
    // nothing left, not because they ran out of ideas.
    assert!(
        gs_core::ruleset::Ruleset::is_solved(&p, &board),
        "following every hint did not finish the puzzle"
    );
}

#[test]
fn a_row_that_cannot_match_its_numbers_is_said_so() {
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    // Row 1 reads "2": fill all three squares and it can never match.
    for column in 0..3 {
        board
            .set(index(dims, 0, column), FILLED)
            .expect("on the board");
    }
    let hint = next(&p, &board, Budget::default());
    assert_eq!(
        hint,
        Hint::LineCannotWork {
            because: Because::Row(0)
        },
        "got {hint:?}"
    );
    assert!(hint.plainly().contains("row 1"), "said: {}", hint.plainly());
}

#[test]
fn a_square_filled_wrongly_is_pointed_at_rather_than_worked_around() {
    // A mark that is fine for its own row and column on its own, but is not in the
    // answer. Nothing is forced from here, so the hint falls back to the whole puzzle
    // and finds the mistake.
    let (p, pattern) = puzzle(&["#.#", ".#.", "#.#"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    board.set(index(dims, 0, 1), FILLED).expect("on the board");
    assert!(
        !answer_fills(&pattern, 0, 1),
        "and that square is blank in the answer"
    );

    let hint = next(&p, &board, Budget::default());
    match hint {
        Hint::FilledWrong { row, column } => {
            assert!(
                !answer_fills(&pattern, row, column),
                "that square is in the answer"
            );
        }
        // A line reasoning good enough to refuse that mark outright is also a right
        // answer, and a better one. Either is correct; a square to fill is not.
        Hint::LineCannotWork { .. } => {}
        other => panic!("expected the mistake to be found, got {other:?}"),
    }
}

#[test]
fn asking_for_a_hint_leaves_the_board_exactly_as_it_was() {
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let mut board = blank(&p);
    board
        .set(index(p.clues.dims, 0, 0), FILLED)
        .expect("on the board");
    let before = board.canonical_bytes();
    let _ = next(&p, &board, Budget::default());
    let _ = next(&p, &board, Budget::default());
    assert_eq!(
        before,
        board.canonical_bytes(),
        "a hint changed the board, which makes it a move and not a hint"
    );
}

#[test]
fn the_same_board_always_gets_the_same_hint() {
    // A player who asks twice must not be told two different things.
    let (p, _) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let board = blank(&p);
    let first = next(&p, &board, Budget::default());
    for _ in 0..5 {
        assert_eq!(first, next(&p, &board, Budget::default()));
    }
}

#[test]
fn a_finished_board_has_nothing_left_to_say() {
    let (p, pattern) = puzzle(&["##.", ".#.", ".##"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    for row in 0..dims.height {
        for column in 0..dims.width {
            let value = if answer_fills(&pattern, row, column) {
                FILLED
            } else {
                EMPTY
            };
            board
                .set(index(dims, row, column), value)
                .expect("on the board");
        }
    }
    assert_eq!(next(&p, &board, Budget::default()), Hint::NothingToSay);
}

#[test]
fn every_sentence_reads_as_words() {
    // The same guard the page has: no bare code names in front of a player, and no
    // sentence that is only half written. The habit this catches is a new variant
    // added with a `{:?}` in its sentence.
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let said = [
        next(&p, &blank(&p), Budget::default()).plainly(),
        Hint::NothingToSay.plainly(),
        Hint::FilledWrong { row: 2, column: 3 }.plainly(),
        Hint::LineCannotWork {
            because: Because::Column(4),
        }
        .plainly(),
        Hint::Square {
            row: 1,
            column: 1,
            mark: Mark::Empty,
            because: Because::TheWholePuzzle,
        }
        .plainly(),
    ];
    for sentence in said {
        assert!(sentence.ends_with('.'), "not a sentence: {sentence}");
        assert!(
            sentence.chars().next().is_some_and(char::is_uppercase),
            "does not start a sentence: {sentence}"
        );
        assert!(
            !sentence.contains('_') && !sentence.contains('{'),
            "that is code, not words: {sentence}"
        );
        for pair in sentence.chars().collect::<Vec<_>>().windows(2) {
            assert!(
                !(pair[0].is_lowercase() && pair[1].is_uppercase()),
                "a run-together code name in: {sentence}"
            );
        }
    }
}
