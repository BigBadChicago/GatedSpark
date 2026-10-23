//! Hints: are they true, are they derived, and do they leave the puzzle alone?
//!
//! The same three questions as `gs-nonogram/tests/hints.rs`, asked of the loop game.
//! Every hint below is checked against the loop these tests drew by hand, which the
//! hint code never sees: it is given the numbers and the player's board and nothing
//! else.

mod common;

use common::{board, rectangle, rectangle_loop};
use gs_core::state::State;
use gs_slitherlink::board::{EdgeId, Geometry};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::hint::{next, Because, Hint, Where};
use gs_slitherlink::rules::{Slitherlink, LINE, NO_LINE};
use gs_slitherlink::solve::Budget;

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

#[test]
fn a_hint_on_a_blank_board_is_true_and_comes_from_a_number_or_a_dot() {
    let (p, answer) = puzzle(4, 4, 1, 1, 2, 2);
    let hint = next(&p, &blank(&p), Budget::default());
    let Hint::Line { at, drawn, because } = hint else {
        panic!("expected a line to be forced on a blank board, got {hint:?}");
    };
    assert!(
        matches!(because, Because::Number { .. } | Because::Dot { .. }),
        "a blank board should have something forced outright, said {because:?}"
    );
    let edge = edge_of(p.geometry(), at);
    assert_eq!(
        drawn, answer[edge.0 as usize],
        "the hint disagrees with the one loop at {at:?}"
    );
}

#[test]
fn every_hint_along_a_whole_game_agrees_with_the_one_loop() {
    // The load-bearing test. Play the puzzle entirely by hints, checking each one
    // against the loop before marking it.
    let (p, answer) = puzzle(5, 5, 1, 1, 3, 3);
    let g = p.geometry();
    let mut state = blank(&p);
    let mut given = 0;

    loop {
        let hint = next(&p, &state, Budget::default());
        match hint {
            Hint::Line { at, drawn, .. } => {
                let edge = edge_of(g, at);
                assert_eq!(
                    drawn, answer[edge.0 as usize],
                    "hint {given} is wrong at {at:?}"
                );
                state
                    .set(edge.0, if drawn { LINE } else { NO_LINE })
                    .expect("on the board");
                given += 1;
            }
            Hint::NothingToSay => break,
            other => panic!("nothing is wrong on this board, so why {other:?}"),
        }
        assert!(given < 2000, "hints are going round in a circle");
    }

    assert!(given > 0, "no hint was ever given");
    assert!(
        gs_core::ruleset::Ruleset::is_solved(&p, &state),
        "following every hint did not finish the puzzle"
    );
}

#[test]
fn a_number_that_can_no_longer_be_met_is_said_so() {
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

    let hint = next(&p, &state, Budget::default());
    let (column, row) = g.cell_at(cell).expect("on the board");
    assert_eq!(hint, Hint::NumberCannotWork { column, row }, "got {hint:?}");
    assert!(
        hint.plainly().contains("can no longer be met"),
        "said: {}",
        hint.plainly()
    );
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
    let hint = next(&p, &state, Budget::default());
    match hint {
        // Either answer is right: the three lines break a dot, and they usually break
        // one of the numbers around it too. Both say "that cannot work", which is the
        // point.
        Hint::DotCannotWork { .. } | Hint::NumberCannotWork { .. } => {}
        other => panic!("expected the board to be called impossible, got {other:?}"),
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

    let hint = next(&p, &state, Budget::default());
    match hint {
        Hint::DrawnWrong { at } => {
            let edge = edge_of(g, at);
            assert!(
                !drawn[edge.0 as usize],
                "that line is in the loop after all"
            );
        }
        // Local reasoning sharp enough to call it impossible outright is also right,
        // and better. What must not come back is another line to draw.
        Hint::NumberCannotWork { .. } | Hint::DotCannotWork { .. } => {}
        other => panic!("expected the mistake to be found, got {other:?}"),
    }
}

#[test]
fn asking_for_a_hint_leaves_the_board_exactly_as_it_was() {
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let mut state = blank(&p);
    state
        .set(p.geometry().across(1, 1).expect("a line").0, LINE)
        .expect("on the board");
    let before = state.canonical_bytes();
    let _ = next(&p, &state, Budget::default());
    let _ = next(&p, &state, Budget::default());
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
    let first = next(&p, &state, Budget::default());
    for _ in 0..5 {
        assert_eq!(first, next(&p, &state, Budget::default()));
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
    assert_eq!(next(&p, &state, Budget::default()), Hint::NothingToSay);
}

#[test]
fn every_sentence_reads_as_words() {
    let (p, _) = puzzle(4, 4, 1, 1, 2, 2);
    let said = [
        next(&p, &blank(&p), Budget::default()).plainly(),
        Hint::NothingToSay.plainly(),
        Hint::NumberCannotWork { column: 1, row: 2 }.plainly(),
        Hint::DotCannotWork { column: 1, row: 2 }.plainly(),
        Hint::DrawnWrong {
            at: Where {
                across: false,
                column: 2,
                row: 3,
            },
        }
        .plainly(),
        Hint::Line {
            at: Where {
                across: true,
                column: 0,
                row: 0,
            },
            drawn: true,
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
