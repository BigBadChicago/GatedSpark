//! The rule that makes this family different, and the specific wrong boards it has
//! to reject.
//!
//! The one this file exists for is `two_separate_loops_are_refused`. Every local
//! rule a person would write down is satisfied by two separate rings, so a checker
//! that only counts lines at each corner accepts them — and a published puzzle
//! "proved" against such a checker would have an answer that is not an answer. This
//! is the whole reason the core reserved a global constraint before writing its
//! first encoder.

mod common;

use common::{add_rectangle, board, nothing_drawn, rectangle, rectangle_loop};
use gs_slitherlink::board::{EdgeId, Geometry};
use gs_slitherlink::loops::{shape_of, Loop, Shape};

fn shape(g: Geometry, drawn: &[bool]) -> Shape {
    shape_of(g, &|edge: EdgeId| {
        drawn.get(edge.0 as usize).copied().unwrap_or(false)
    })
}

#[test]
fn one_rectangle_is_one_loop() {
    let g = board(4, 4);
    assert_eq!(shape(g, &rectangle(g, 1, 1, 2, 2)), Shape::OneLoop);
}

#[test]
fn the_smallest_loop_is_one_square() {
    let g = board(2, 2);
    let drawn = rectangle(g, 0, 0, 0, 0);
    assert_eq!(shape(g, &drawn), Shape::OneLoop);
    assert_eq!(
        Loop::of(g, drawn).expect("one loop").edges_drawn(),
        4,
        "a loop round one cell uses four edges"
    );
}

#[test]
fn a_loop_round_the_whole_board_is_one_loop() {
    let g = board(5, 3);
    assert_eq!(shape(g, &rectangle(g, 0, 0, 4, 2)), Shape::OneLoop);
}

#[test]
fn a_blank_board_is_nothing_not_a_mistake() {
    // Distinct on purpose: this is where every player starts, and a checker that
    // called it wrong would put an error on screen before anyone had done anything.
    let g = board(3, 3);
    assert_eq!(shape(g, &nothing_drawn(g)), Shape::Nothing);
}

#[test]
fn two_separate_loops_are_refused() {
    // The test this whole module is for. Both rings are perfectly good loops, every
    // corner has nought or two lines, and the board is still wrong.
    let g = board(5, 5);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 1, 1);
    add_rectangle(g, &mut drawn, 3, 3, 4, 4);
    assert_eq!(shape(g, &drawn), Shape::SeveralLoops(2));
    assert!(Loop::of(g, drawn).is_err(), "two rings are not a loop");
}

#[test]
fn two_loops_pass_the_corner_count_on_its_own() {
    // The same board, checked against the local half of the rule only. This is what
    // a checker without the whole-board rule would see, and it says yes. Keeping it
    // as a test records *why* the global rule is not optional.
    let g = board(5, 5);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 1, 1);
    add_rectangle(g, &mut drawn, 3, 3, 4, 4);
    let degrees = gs_slitherlink::loops::degrees(g, &|edge: EdgeId| {
        drawn.get(edge.0 as usize).copied().unwrap_or(false)
    });
    assert!(
        degrees.iter().all(|d| *d == 0 || *d == 2),
        "every corner has nought or two lines, and the board is still wrong"
    );
}

#[test]
fn a_line_that_stops_in_mid_air_is_refused() {
    let g = board(3, 3);
    let mut drawn = nothing_drawn(g);
    drawn[g.across(0, 0).expect("an edge").0 as usize] = true;
    match shape(g, &drawn) {
        Shape::LooseEnd(dot) => {
            let (x, y) = g.dot_at(dot).expect("a place");
            assert_eq!((x, y), (0, 0), "names the first loose end it finds");
        }
        other => panic!("expected a loose end, found {other:?}"),
    }
}

#[test]
fn three_lines_at_one_corner_are_refused() {
    let g = board(3, 3);
    let mut drawn = nothing_drawn(g);
    // Three edges meeting at dot (1, 1). Their far ends are loose too, because three
    // lines in the middle of a blank board cannot help leaving ends dangling, so the
    // check reports whichever fault it meets first in corner order. The test asserts
    // the refusal, not which fault gets named: a board with several faults has no one
    // right thing to say about it, and pinning the order would be pinning a detail of
    // the scan rather than the rule.
    drawn[g.across(0, 1).expect("an edge").0 as usize] = true;
    drawn[g.across(1, 1).expect("an edge").0 as usize] = true;
    drawn[g.down(1, 1).expect("an edge").0 as usize] = true;
    let found = shape(g, &drawn);
    assert!(!found.is_one_loop(), "found {found:?}");
    assert!(
        matches!(found, Shape::LooseEnd(_) | Shape::Crossing(_)),
        "found {found:?}"
    );
}

#[test]
fn a_figure_of_eight_is_refused() {
    // Two rings sharing one corner. In one piece, so the flood fill is happy; the
    // shared corner has four lines, so the corner count catches it. Worth its own
    // test because it is the case where the two halves of the rule swap roles.
    let g = board(4, 4);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 1, 1);
    add_rectangle(g, &mut drawn, 2, 2, 3, 3);
    match shape(g, &drawn) {
        // The shared corner is the bottom-right of the first ring and the top-left of
        // the second: dot (2, 2). Four lines meet there, so the corner count catches
        // it even though the flood fill sees one piece.
        Shape::Crossing(dot) => assert_eq!(g.dot_at(dot), Some((2, 2))),
        other => panic!("two rings touching at a corner are not one loop: {other:?}"),
    }
}

#[test]
fn a_loop_is_the_only_way_to_hold_a_loop() {
    // The type carries the check: a function taking a `Loop` does not have to
    // re-check and cannot forget to.
    let g = board(4, 4);
    let mut nearly = rectangle(g, 1, 1, 2, 2);
    nearly[g.across(1, 1).expect("an edge").0 as usize] = false;
    assert!(Loop::of(g, nearly).is_err());
    assert!(Loop::of(g, vec![false; 3]).is_err(), "wrong length refused");
}

#[test]
fn a_loops_answer_decides_every_edge() {
    // A stored answer is a whole board. Every edge is either drawn or ruled out, and
    // none is left undecided, which is what `publish` requires separately of a
    // witness.
    let g = board(3, 3);
    let state = rectangle_loop(g, 0, 0, 1, 1).as_state();
    assert!(
        g.edges()
            .all(|e| state.get(e.0).map(|c| !c.is_unknown()).unwrap_or(false)),
        "every edge is decided"
    );
}

#[test]
fn reading_a_board_back_finds_the_same_loop() {
    let g = board(4, 3);
    let drawn = rectangle_loop(g, 1, 0, 2, 2);
    let read_back = gs_slitherlink::loops::loop_in(g, &drawn.as_state()).expect("the same loop");
    assert_eq!(read_back, drawn);
}

#[test]
fn an_undecided_edge_counts_as_not_drawn() {
    // A blank board has every edge undecided, and reads as nothing drawn rather than
    // as a board full of lines. The decision from 2026-10-23, in the one place where
    // getting it backwards would be hardest to notice.
    let g = board(3, 3);
    let blank = gs_core::state::State::blank(g.state_dims());
    assert!(g
        .edges()
        .all(|e| !gs_slitherlink::loops::drawn_in(&blank, e)));
    assert_eq!(
        shape_of(g, &|e| gs_slitherlink::loops::drawn_in(&blank, e)),
        Shape::Nothing
    );
}

#[test]
fn every_refusal_reads_as_words() {
    // The lesson from the withdrawal list: a reason shown to a player has to be a
    // sentence, and the way that was found to be broken was by looking at the page
    // rather than by a test passing. So the test checks the shape of the words.
    for shape in [
        Shape::OneLoop,
        Shape::Nothing,
        Shape::LooseEnd(gs_slitherlink::board::DotId(0)),
        Shape::Crossing(gs_slitherlink::board::DotId(0)),
        Shape::SeveralLoops(2),
    ] {
        let words = shape.plainly();
        assert!(!words.is_empty());
        assert!(
            words.chars().all(|c| !c.is_ascii_uppercase()),
            "{words:?} looks like a name from the code, not a sentence"
        );
        assert!(words.contains(' '), "{words:?} is not a sentence");
    }
}
