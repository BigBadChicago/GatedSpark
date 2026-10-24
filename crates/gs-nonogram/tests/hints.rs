//! Hints: are they true, are they derived, and does a bigger one stay true?
//!
//! The three things that could go wrong here, in the order they would hurt:
//!
//! 1. **A hint that is wrong.** Every square of every hint below is checked against the
//!    puzzle's one answer, which these tests know and the hint code does not. A hint
//!    that fills in a whole row has fifteen chances to be wrong instead of one, so this
//!    matters more now than it did when a hint was a single square.
//! 2. **A hint that reads an answer.** `next` is given the clues and the player's
//!    board, and nothing else; there is no answer in scope to read. The test that
//!    matters for this is the one where a hint is asked for on a blank board and still
//!    gets a true answer out of the numbers alone.
//! 3. **A hint that changes the puzzle.** It takes the board by reference and hands
//!    back a list of squares. The test here checks the board is untouched afterwards:
//!    marking them in is the caller's job, through the ordinary rules.

use gs_core::state::{Dims, State};
use gs_nonogram::clues::{Clues, Pattern};
use gs_nonogram::hint::{next, Because, Hint, Size};
use gs_nonogram::line::Mark;
use gs_nonogram::rules::{Nonogram, EMPTY, FILLED};
use gs_nonogram::solve::Budget;

const EVERY_SIZE: [Size; 3] = [Size::OneSquare, Size::HalfALine, Size::AWholeLine];

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

/// Write a hint's squares onto a board, the way a session does.
fn mark_in(board: &mut State, dims: Dims, squares: &[gs_nonogram::hint::Square]) {
    for square in squares {
        let value = match square.mark {
            Mark::Filled => FILLED,
            Mark::Empty => EMPTY,
            Mark::Unknown => continue,
        };
        board
            .set(index(dims, square.row, square.column), value)
            .expect("on the board");
    }
}

#[test]
fn a_hint_of_every_size_is_true_on_a_blank_board() {
    let (p, pattern) = puzzle(&["##.", ".#.", ".##"]);
    for size in EVERY_SIZE {
        let hint = next(&p, &blank(&p), Budget::default(), size);
        let Hint::Fill { squares, .. } = &hint else {
            panic!("expected squares on a blank board at {size:?}, got {hint:?}");
        };
        assert!(!squares.is_empty(), "a hint with nothing in it at {size:?}");
        for square in squares {
            assert_eq!(
                square.mark == Mark::Filled,
                answer_fills(&pattern, square.row, square.column),
                "{size:?} disagrees with the one answer at row {}, column {}",
                square.row,
                square.column
            );
        }
    }
}

#[test]
fn the_smallest_hint_comes_from_one_line_and_settles_one_square() {
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let hint = next(&p, &blank(&p), Budget::default(), Size::OneSquare);
    let Hint::Fill { squares, because } = hint else {
        panic!("expected a square to be forced on a blank board");
    };
    assert_eq!(squares.len(), 1, "the small hint is one square");
    assert!(
        matches!(because, Because::Row(_) | Because::Column(_)),
        "a blank board should have something forced by a single line, said {because:?}"
    );
}

#[test]
fn the_sizes_really_do_get_bigger() {
    // The load-bearing test for what the project owner asked for. Measured on one
    // board, from the same starting point, so the three are comparable.
    let (p, _) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let board = blank(&p);
    let how_many = |size| match next(&p, &board, Budget::default(), size) {
        Hint::Fill { squares, .. } => squares.len(),
        other => panic!("expected squares at {size:?}, got {other:?}"),
    };
    let one = how_many(Size::OneSquare);
    let half = how_many(Size::HalfALine);
    let whole = how_many(Size::AWholeLine);
    println!("one {one}, half {half}, whole {whole}");

    assert_eq!(one, 1);
    assert!(
        half > one,
        "half a line ({half}) is no bigger than one square"
    );
    assert!(
        whole > half,
        "a whole line ({whole}) is no bigger than half ({half})"
    );
    // And a whole line is a whole line: this board is 5 by 5 and nothing is marked, so
    // every square of the line it picks is still to be settled.
    assert_eq!(whole, 5, "a whole line of a 5 by 5 board is five squares");
    assert_eq!(half, 3, "half of five, rounded up, is three");
}

#[test]
fn a_whole_line_hint_fills_one_whole_line_and_nothing_else() {
    let (p, pattern) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let hint = next(&p, &blank(&p), Budget::default(), Size::AWholeLine);
    let Hint::Fill { squares, because } = hint else {
        panic!("expected a line to be filled in");
    };
    match because {
        Because::Row(row) => {
            assert!(
                squares.iter().all(|square| square.row == row),
                "a row hint strayed outside row {row}"
            );
        }
        Because::Column(column) => {
            assert!(
                squares.iter().all(|square| square.column == column),
                "a column hint strayed outside column {column}"
            );
        }
        Because::TheWholePuzzle => panic!("a line hint should name its line"),
    }
    for square in &squares {
        assert_eq!(
            square.mark == Mark::Filled,
            answer_fills(&pattern, square.row, square.column)
        );
    }
}

#[test]
fn every_hint_along_a_whole_game_agrees_with_the_one_answer() {
    // Play the puzzle entirely by hints, at every size, checking each square against
    // the answer before marking it. A hint that was ever wrong stops this.
    for size in EVERY_SIZE {
        let (p, pattern) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
        let dims = p.clues.dims;
        let mut board = blank(&p);
        let mut given = 0;

        loop {
            let hint = next(&p, &board, Budget::default(), size);
            match hint {
                Hint::Fill { squares, .. } => {
                    for square in &squares {
                        assert_eq!(
                            square.mark == Mark::Filled,
                            answer_fills(&pattern, square.row, square.column),
                            "{size:?} hint {given} is wrong at row {}, column {}",
                            square.row,
                            square.column
                        );
                    }
                    mark_in(&mut board, dims, &squares);
                    given += 1;
                }
                Hint::NothingToSay => break,
                other => panic!("nothing is wrong on this board, so why {other:?}"),
            }
            assert!(
                given < 1000,
                "hints are going round in a circle at {size:?}"
            );
        }

        assert!(given > 0, "no hint was ever given at {size:?}");
        assert!(
            gs_core::ruleset::Ruleset::is_solved(&p, &board),
            "following every hint did not finish the puzzle at {size:?}"
        );
    }
}

#[test]
fn a_bigger_hint_finishes_the_puzzle_in_fewer_asks() {
    // The point of the bigger sizes, stated as a test: more help per hint.
    let (p, _) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let dims = p.clues.dims;
    let asks = |size| {
        let mut board = blank(&p);
        let mut asked = 0;
        while let Hint::Fill { squares, .. } = next(&p, &board, Budget::default(), size) {
            mark_in(&mut board, dims, &squares);
            asked += 1;
            assert!(asked < 1000, "hints are going round in a circle");
        }
        asked
    };
    let one = asks(Size::OneSquare);
    let whole = asks(Size::AWholeLine);
    println!("one square took {one} asks, a whole line took {whole}");
    assert!(
        whole < one,
        "a whole line ({whole} asks) should finish sooner than one square at a time ({one})"
    );
}

#[test]
fn a_row_that_cannot_match_its_numbers_is_said_so_at_every_size() {
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    // Row 1 reads "2": fill all three squares and it can never match.
    for column in 0..3 {
        board
            .set(index(dims, 0, column), FILLED)
            .expect("on the board");
    }
    for size in EVERY_SIZE {
        let hint = next(&p, &board, Budget::default(), size);
        assert_eq!(
            hint,
            Hint::LineCannotWork {
                because: Because::Row(0)
            },
            "at {size:?}"
        );
        assert!(hint.plainly().contains("row 1"), "said: {}", hint.plainly());
    }
}

#[test]
fn a_square_filled_wrongly_is_pointed_at_rather_than_worked_around() {
    // A mark that is fine for its own row and column on its own, but is not in the
    // answer. The hint has to find the mistake rather than fill more squares around it.
    let (p, pattern) = puzzle(&["#.#", ".#.", "#.#"]);
    let dims = p.clues.dims;
    let mut board = blank(&p);
    board.set(index(dims, 0, 1), FILLED).expect("on the board");
    assert!(
        !answer_fills(&pattern, 0, 1),
        "and that square is blank in the answer"
    );

    for size in EVERY_SIZE {
        let hint = next(&p, &board, Budget::default(), size);
        match hint {
            Hint::FilledWrong { row, column } => {
                assert!(
                    !answer_fills(&pattern, row, column),
                    "that square is in the answer"
                );
            }
            // Line reasoning sharp enough to refuse that mark outright is also right,
            // and better. Either is correct; more squares to fill is not.
            Hint::LineCannotWork { .. } => {}
            other => panic!("expected the mistake to be found at {size:?}, got {other:?}"),
        }
    }
}

#[test]
fn asking_for_a_hint_leaves_the_board_exactly_as_it_was() {
    // Marking the squares in is the caller's job. Nothing in here writes to a board.
    let (p, _) = puzzle(&["##.", ".#.", ".##"]);
    let mut board = blank(&p);
    board
        .set(index(p.clues.dims, 0, 0), FILLED)
        .expect("on the board");
    let before = board.canonical_bytes();
    for size in EVERY_SIZE {
        let _ = next(&p, &board, Budget::default(), size);
        let _ = next(&p, &board, Budget::default(), size);
    }
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
    for size in EVERY_SIZE {
        let first = next(&p, &board, Budget::default(), size);
        for _ in 0..5 {
            assert_eq!(
                first,
                next(&p, &board, Budget::default(), size),
                "at {size:?}"
            );
        }
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
    for size in EVERY_SIZE {
        assert_eq!(
            next(&p, &board, Budget::default(), size),
            Hint::NothingToSay,
            "at {size:?}"
        );
    }
}

#[test]
fn every_sentence_reads_as_words() {
    // The same guard the page has: no bare code names in front of a player, and no
    // sentence that is only half written. The habit this catches is a new variant
    // added with a `{:?}` in its sentence.
    let (p, _) = puzzle(&["#.##.", ".###.", "##..#", ".#.#.", "###.#"]);
    let mut said: Vec<String> = EVERY_SIZE
        .iter()
        .map(|size| next(&p, &blank(&p), Budget::default(), *size).plainly())
        .collect();
    said.push(Hint::NothingToSay.plainly());
    said.push(Hint::FilledWrong { row: 2, column: 3 }.plainly());
    said.push(
        Hint::LineCannotWork {
            because: Because::Column(4),
        }
        .plainly(),
    );
    for sentence in said {
        assert!(sentence.ends_with('.'), "not a sentence: {sentence}");
        assert!(
            sentence.chars().next().is_some_and(char::is_uppercase)
                || sentence.chars().next().is_some_and(char::is_numeric),
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
