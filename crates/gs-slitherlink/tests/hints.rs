//! Hints: are they true, are they derived, and does a bigger one stay true?
//!
//! The same three questions as `gs-nonogram/tests/hints.rs`, asked of the loop game.
//! Every line of every hint below is checked against the loop these tests drew by
//! hand, which the hint code never sees: it is given the numbers and the player's board
//! and nothing else. A hint that fills in a whole row of lines has a dozen chances to
//! be wrong instead of one, so this matters more now than it did when a hint was a
//! single line.

mod common;

use common::{board, rectangle, rectangle_loop};
use gs_core::state::State;
use gs_slitherlink::board::{EdgeId, Geometry};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::hint::{next, Because, Hint, Size, Where};
use gs_slitherlink::rules::{Slitherlink, LINE, NO_LINE};
use gs_slitherlink::solve::Budget;

const EVERY_SIZE: [Size; 3] = [Size::OneLine, Size::HalfALine, Size::AWholeLine];

/// A puzzle whose every square is numbered, and the loop that answers it.
fn puzzle(width: u16, height: u16, x0: u16, y0: u16, x1: u16, y1: u16) -> (Slitherlink, Vec<bool>) {
    let g = board(width, height);
    let answer = rectangle_loop(g, x0, y0, x1, y1);
    let p = Slitherlink::new(Clues::of(&answer));
    (p, rectangle(g, x0, y0, x1, y1))
}

fn blank(p: &Slitherlink) -> State {
    p.blank()
}

fn edge_of(g: Geometry, at: Where) -> EdgeId {
    let edge = if at.across {
        g.across(at.column, at.row)
    } else {
        g.down(at.column, at.row)
    };
    edge.expect("the hint points at a line that is on the board")
}

/// Write a hint's lines onto a board, the way a session does.
fn mark_in(state: &mut State, g: Geometry, lines: &[gs_slitherlink::hint::Line]) {
    for line in lines {
        let edge = edge_of(g, line.at);
        state
            .set(edge.0, if line.drawn { LINE } else { NO_LINE })
            .expect("on the board");
    }
}

#[test]
fn a_hint_of_every_size_is_true_on_a_blank_board() {
    let (p, answer) = puzzle(4, 4, 1, 1, 2, 2);
    let g = p.geometry();
    for size in EVERY_SIZE {
        let hint = next(&p, &blank(&p), Budget::default(), size);
        let Hint::Fill { lines, .. } = &hint else {
            panic!("expected lines on a blank board at {size:?}, got {hint:?}");
        };
        assert!(!lines.is_empty(), "a hint with nothing in it at {size:?}");
        for line in lines {
            let edge = edge_of(g, line.at);
            assert_eq!(
                line.drawn, answer[edge.0 as usize],
                "{size:?} disagrees with the one loop at {:?}",
                line.at
            );
        }
    }
}

#[test]
fn the_smallest_hint_comes_from_a_number_or_a_dot_and_settles_one_line() {
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let hint = next(&p, &blank(&p), Budget::default(), Size::OneLine);
    let Hint::Fill { lines, because } = hint else {
        panic!("expected a line to be forced on a blank board");
    };
    assert_eq!(lines.len(), 1, "the small hint is one line");
    assert!(
        matches!(because, Because::Number { .. } | Because::Dot { .. }),
        "a blank board should have something forced outright, said {because:?}"
    );
}

#[test]
fn the_sizes_really_do_get_bigger() {
    // The load-bearing test for what the project owner asked for, measured on one
    // board from one starting point so the three are comparable.
    let (p, _) = puzzle(5, 5, 1, 1, 3, 3);
    let state = blank(&p);
    let how_many = |size| match next(&p, &state, Budget::default(), size) {
        Hint::Fill { lines, .. } => lines.len(),
        other => panic!("expected lines at {size:?}, got {other:?}"),
    };
    let one = how_many(Size::OneLine);
    let half = how_many(Size::HalfALine);
    let whole = how_many(Size::AWholeLine);
    println!("one {one}, half {half}, whole {whole}");

    assert_eq!(one, 1);
    assert!(half > one, "half a row ({half}) is no bigger than one line");
    assert!(
        whole > half,
        "a whole row ({whole}) is no bigger than half of one ({half})"
    );
}

#[test]
fn a_whole_line_hint_stays_inside_the_row_or_column_it_names() {
    let (p, answer) = puzzle(5, 5, 1, 1, 3, 3);
    let g = p.geometry();
    let hint = next(&p, &blank(&p), Budget::default(), Size::AWholeLine);
    let Hint::Fill { lines, because } = hint else {
        panic!("expected a row or column of lines to be filled in");
    };
    match because {
        Because::AcrossRow(row) => {
            assert!(
                lines
                    .iter()
                    .all(|line| line.at.across && line.at.row == row),
                "an across-row hint strayed outside row {row}"
            );
        }
        Because::DownColumn(column) => {
            assert!(
                lines
                    .iter()
                    .all(|line| !line.at.across && line.at.column == column),
                "a down-column hint strayed outside column {column}"
            );
        }
        other => panic!("a whole-line hint should name its row or column, said {other:?}"),
    }
    for line in &lines {
        let edge = edge_of(g, line.at);
        assert_eq!(line.drawn, answer[edge.0 as usize]);
    }
}

#[test]
fn every_hint_along_a_whole_game_agrees_with_the_one_loop() {
    // Play the puzzle entirely by hints, at every size, checking each line against the
    // loop before marking it. A hint that was ever wrong stops this.
    for size in EVERY_SIZE {
        let (p, answer) = puzzle(5, 5, 1, 1, 3, 3);
        let g = p.geometry();
        let mut state = blank(&p);
        let mut given = 0;

        loop {
            let hint = next(&p, &state, Budget::default(), size);
            match hint {
                Hint::Fill { lines, .. } => {
                    for line in &lines {
                        let edge = edge_of(g, line.at);
                        assert_eq!(
                            line.drawn, answer[edge.0 as usize],
                            "{size:?} hint {given} is wrong at {:?}",
                            line.at
                        );
                    }
                    mark_in(&mut state, g, &lines);
                    given += 1;
                }
                Hint::NothingToSay => break,
                other => panic!("nothing is wrong on this board, so why {other:?}"),
            }
            assert!(
                given < 2000,
                "hints are going round in a circle at {size:?}"
            );
        }

        assert!(given > 0, "no hint was ever given at {size:?}");
        assert!(
            gs_core::ruleset::Ruleset::is_solved(&p, &state),
            "following every hint did not finish the puzzle at {size:?}"
        );
    }
}

#[test]
fn a_bigger_hint_finishes_the_puzzle_in_fewer_asks() {
    // The point of the bigger sizes, stated as a test: more help per hint.
    let (p, _) = puzzle(5, 5, 1, 1, 3, 3);
    let g = p.geometry();
    let asks = |size| {
        let mut state = blank(&p);
        let mut asked = 0;
        while let Hint::Fill { lines, .. } = next(&p, &state, Budget::default(), size) {
            mark_in(&mut state, g, &lines);
            asked += 1;
            assert!(asked < 2000, "hints are going round in a circle");
        }
        asked
    };
    let one = asks(Size::OneLine);
    let whole = asks(Size::AWholeLine);
    println!("one line took {one} asks, a whole row took {whole}");
    assert!(
        whole < one,
        "a whole row ({whole} asks) should finish sooner than one line at a time ({one})"
    );
}

#[test]
fn a_number_that_can_no_longer_be_met_is_said_so_at_every_size() {
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let g = p.geometry();
    // Find a square whose number is 0 and draw one of its sides anyway.
    let (cell, _) = p
        .clues
        .given()
        .find(|(_, clue)| *clue == 0)
        .expect("a rectangle loop leaves some square with no sides used");
    let sides = g.edges_of_cell(cell).expect("four sides");
    let mut state = blank(&p);
    state.set(sides[0].0, LINE).expect("on the board");
    let (column, row) = g.cell_at(cell).expect("on the board");

    for size in EVERY_SIZE {
        let hint = next(&p, &state, Budget::default(), size);
        assert_eq!(
            hint,
            Hint::NumberCannotWork { column, row },
            "at {size:?}, got {hint:?}"
        );
        assert!(
            hint.plainly().contains("can no longer be met"),
            "said: {}",
            hint.plainly()
        );
    }
}

#[test]
fn a_dot_with_three_lines_at_it_is_said_so() {
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let g = p.geometry();
    // A dot in the middle has four lines at it. Draw three.
    let dot = g.dot(2, 2).expect("a dot in the middle");
    let edges = g.edges_of_dot(dot);
    assert_eq!(edges.len(), 4, "a middle dot has four lines at it");
    let mut state = blank(&p);
    for edge in edges.iter().take(3) {
        state.set(edge.0, LINE).expect("on the board");
    }
    for size in EVERY_SIZE {
        let hint = next(&p, &state, Budget::default(), size);
        match hint {
            // Either answer is right: the three lines break a dot, and they usually
            // break one of the numbers around it too. Both say "that cannot work",
            // which is the point. What must not come back is more lines to draw.
            Hint::DotCannotWork { .. } | Hint::NumberCannotWork { .. } => {}
            other => {
                panic!("expected the board to be called impossible at {size:?}, got {other:?}")
            }
        }
    }
}

#[test]
fn a_line_drawn_wrongly_is_pointed_at_rather_than_worked_around() {
    // A board with hardly any numbers, so the easy deductions run out and the hint has
    // to work the whole puzzle out. The one line drawn is not in the loop.
    let g = board(4, 4);
    let answer = rectangle_loop(g, 1, 1, 2, 2);
    let drawn = rectangle(g, 1, 1, 2, 2);
    let p = Slitherlink::new(Clues::of(&answer));

    let wrong = g
        .edges()
        .find(|e| !drawn[e.0 as usize])
        .expect("some line is not in the loop");
    let mut state = blank(&p);
    state.set(wrong.0, LINE).expect("on the board");

    for size in EVERY_SIZE {
        let hint = next(&p, &state, Budget::default(), size);
        match hint {
            Hint::DrawnWrong { at } => {
                let edge = edge_of(g, at);
                assert!(
                    !drawn[edge.0 as usize],
                    "that line is in the loop after all"
                );
            }
            // Local reasoning sharp enough to call it impossible outright is also
            // right, and better. What must not come back is another line to draw.
            Hint::NumberCannotWork { .. } | Hint::DotCannotWork { .. } => {}
            other => panic!("expected the mistake to be found at {size:?}, got {other:?}"),
        }
    }
}

#[test]
fn asking_for_a_hint_leaves_the_board_exactly_as_it_was() {
    // Marking the lines in is the caller's job. Nothing in here writes to a board.
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let mut state = blank(&p);
    state
        .set(p.geometry().across(1, 1).expect("a line").0, LINE)
        .expect("on the board");
    let before = state.canonical_bytes();
    for size in EVERY_SIZE {
        let _ = next(&p, &state, Budget::default(), size);
        let _ = next(&p, &state, Budget::default(), size);
    }
    assert_eq!(
        before,
        state.canonical_bytes(),
        "a hint changed the board, which makes it a move and not a hint"
    );
}

#[test]
fn the_same_board_always_gets_the_same_hint() {
    let (p, _) = puzzle(5, 5, 1, 1, 3, 3);
    let state = blank(&p);
    for size in EVERY_SIZE {
        let first = next(&p, &state, Budget::default(), size);
        for _ in 0..5 {
            assert_eq!(
                first,
                next(&p, &state, Budget::default(), size),
                "at {size:?}"
            );
        }
    }
}

#[test]
fn a_finished_board_has_nothing_left_to_say() {
    let (p, answer) = puzzle(4, 4, 1, 1, 2, 2);
    let mut state = blank(&p);
    for edge in p.geometry().edges() {
        let value = if answer[edge.0 as usize] {
            LINE
        } else {
            NO_LINE
        };
        state.set(edge.0, value).expect("on the board");
    }
    assert!(gs_core::ruleset::Ruleset::is_solved(&p, &state));
    for size in EVERY_SIZE {
        assert_eq!(
            next(&p, &state, Budget::default(), size),
            Hint::NothingToSay,
            "at {size:?}"
        );
    }
}

#[test]
fn every_sentence_reads_as_words() {
    // The same guard the page has: no bare code names in front of a player, and no
    // sentence that is only half written.
    let (p, _) = puzzle(5, 5, 1, 1, 3, 3);
    let mut said: Vec<String> = EVERY_SIZE
        .iter()
        .map(|size| next(&p, &blank(&p), Budget::default(), *size).plainly())
        .collect();
    said.push(Hint::NothingToSay.plainly());
    said.push(Hint::NumberCannotWork { column: 1, row: 2 }.plainly());
    said.push(Hint::DotCannotWork { column: 1, row: 2 }.plainly());
    said.push(
        Hint::DrawnWrong {
            at: Where {
                across: false,
                column: 2,
                row: 3,
            },
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
