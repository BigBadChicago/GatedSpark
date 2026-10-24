//! The layout: white numbering, run finding, and the two index spaces.
//!
//! This is the module with the most chances to be off by one, because a run ends at
//! a block *or* at the rim and those two cases have to behave identically. Most of
//! what follows is checking that they do.

mod common;

use common::layout;
use gs_kakuro::layout::{Along, Layout, LayoutError, Run};

fn across_runs(layout: &Layout) -> Vec<Vec<u32>> {
    layout
        .runs()
        .filter(|(_, run)| run.along == Along::Across)
        .map(|(_, run)| run.squares.iter().map(|w| w.0).collect())
        .collect()
}

fn down_runs(layout: &Layout) -> Vec<Vec<u32>> {
    layout
        .runs()
        .filter(|(_, run)| run.along == Along::Down)
        .map(|(_, run)| run.squares.iter().map(|w| w.0).collect())
        .collect()
}

#[test]
fn an_open_board_has_one_run_per_row_and_column() {
    let layout = Layout::open(3, 2).expect("3 by 2 is a legal board");
    assert_eq!(layout.white_count(), 6);
    assert_eq!(across_runs(&layout), vec![vec![0, 1, 2], vec![3, 4, 5]]);
    assert_eq!(down_runs(&layout), vec![vec![0, 3], vec![1, 4], vec![2, 5]]);
}

#[test]
fn a_block_splits_a_row_into_two_runs() {
    let layout = layout(&[".#.."]);
    assert_eq!(layout.white_count(), 3);
    assert_eq!(across_runs(&layout), vec![vec![0], vec![1, 2]]);
}

#[test]
fn a_block_at_the_rim_makes_no_empty_run() {
    // The bug this guards against: closing a run when the walk reaches a block, and
    // closing one again when it reaches the rim, producing a run of nothing. A run
    // of nothing would get a total, and a total on no squares is a rule no digit can
    // break, which is exactly the kind of check that passes for the wrong reason.
    let layout = layout(&["#..#"]);
    assert_eq!(across_runs(&layout), vec![vec![0, 1]]);
    for (_, run) in layout.runs() {
        assert!(!run.is_empty(), "no run may be empty: {run:?}");
    }
}

#[test]
fn a_single_white_square_is_a_run_both_ways() {
    // One white square alone is an across run of one and a down run of one. Both
    // exist, because every white square sits in exactly one of each, and that is
    // what makes a total mean the same thing everywhere.
    let layout = layout(&["#.#"]);
    assert_eq!(layout.white_count(), 1);
    assert_eq!(layout.run_count(), 2);
    let white = layout.whites().next().expect("the one white square");
    let (across, down) = layout.runs_of(white).expect("both runs");
    assert_ne!(across, down, "the two runs are different runs");
    assert_eq!(layout.run(across).map(Run::len), Some(1));
    assert_eq!(layout.run(down).map(Run::len), Some(1));
}

#[test]
fn every_white_square_is_in_exactly_one_run_each_way() {
    let layout = layout(&["..#..", ".#..#", "...#."]);
    for white in layout.whites() {
        let (across, down) = layout.runs_of(white).expect("both runs");
        let across_run = layout.run(across).expect("the across run exists");
        let down_run = layout.run(down).expect("the down run exists");
        assert_eq!(across_run.along, Along::Across);
        assert_eq!(down_run.along, Along::Down);
        assert!(
            across_run.squares.contains(&white),
            "a square's across run must contain it"
        );
        assert!(
            down_run.squares.contains(&white),
            "a square's down run must contain it"
        );
    }
}

#[test]
fn the_runs_cover_every_white_square_exactly_once_each_way() {
    let layout = layout(&["..#..", ".#..#", "...#."]);
    let mut across: Vec<u32> = across_runs(&layout).into_iter().flatten().collect();
    let mut down: Vec<u32> = down_runs(&layout).into_iter().flatten().collect();
    across.sort_unstable();
    down.sort_unstable();
    let all: Vec<u32> = layout.whites().map(|w| w.0).collect();
    assert_eq!(across, all, "the across runs partition the white squares");
    assert_eq!(down, all, "so do the down runs");
}

#[test]
fn white_numbering_skips_blocks_and_round_trips() {
    let layout = layout(&[".#.", "...", "#.."]);
    assert_eq!(layout.white_count(), 7);
    for white in layout.whites() {
        let square = layout.square_of(white).expect("a white sits somewhere");
        assert!(!layout.is_block(square));
        assert_eq!(layout.white_at(square), Some(white), "the two ways agree");
    }
    for square in layout.squares() {
        if layout.is_block(square) {
            assert_eq!(
                layout.white_at(square),
                None,
                "a block is not a white square"
            );
        }
    }
}

#[test]
fn square_coordinates_round_trip() {
    let layout = layout(&["..#", "..."]);
    for y in 0..layout.height() {
        for x in 0..layout.width() {
            let square = layout.square(x, y).expect("on the board");
            assert_eq!(layout.square_at(square), Some((x, y)));
        }
    }
    assert_eq!(layout.square(3, 0), None, "off the right edge");
    assert_eq!(layout.square(0, 2), None, "off the bottom");
}

#[test]
fn state_is_white_shaped_not_square_shaped() {
    // The point of the white index space. A square-shaped state would have somewhere
    // to put a digit in a block, and making that unrepresentable is cheaper than
    // checking for it everywhere.
    let layout = layout(&["#..", "..#"]);
    assert_eq!(layout.square_count(), 6);
    assert_eq!(layout.white_count(), 4);
    assert_eq!(usize::from(layout.state_dims().width), 4);
    assert_eq!(layout.state_dims().height, 1);
}

#[test]
fn a_run_of_ten_is_refused() {
    // Ten squares in a row cannot hold ten different digits from one to nine. That
    // is arithmetic, not a search result, so it is refused here and the solver is
    // never asked a question whose answer was decided by counting.
    let error = Layout::open(10, 1).expect_err("a run of ten is impossible");
    assert!(
        matches!(error, LayoutError::RunTooLong { len: 10, .. }),
        "expected a run-too-long refusal naming the length, got {error:?}"
    );
}

#[test]
fn a_wide_board_is_legal_when_blocks_keep_the_runs_short() {
    // The limit is on run length, not on board width, and this is the test that
    // keeps those two from being confused. Twelve columns is fine as long as no run
    // is longer than nine.
    let layout = layout(&["....#....#.."]);
    assert_eq!(layout.width(), 12);
    assert_eq!(across_runs(&layout).len(), 3);
}

#[test]
fn an_all_block_board_is_refused() {
    let error = Layout::new(2, 2, vec![true; 4]).expect_err("nothing to fill in");
    assert_eq!(error, LayoutError::NoWhiteSquares);
}

#[test]
fn a_board_with_no_squares_is_refused() {
    assert_eq!(
        Layout::open(0, 3).expect_err("no width"),
        LayoutError::TooSmall {
            width: 0,
            height: 3
        }
    );
    assert_eq!(
        Layout::open(3, 0).expect_err("no height"),
        LayoutError::TooSmall {
            width: 3,
            height: 0
        }
    );
}

#[test]
fn a_block_pattern_of_the_wrong_size_is_refused() {
    assert_eq!(
        Layout::new(3, 2, vec![false; 5]).expect_err("five is not six"),
        LayoutError::WrongLength {
            expected: 6,
            found: 5
        }
    );
}

#[test]
fn total_bounds_are_the_smallest_and_largest_a_run_could_add_up_to() {
    let layout = Layout::open(1, 1).expect("one square");
    let (_, run) = layout.runs().next().expect("a run");
    assert_eq!(run.total_bounds(), (1, 9), "one digit, so one to nine");

    let layout = Layout::open(2, 1).expect("two squares");
    let (_, run) = layout
        .runs()
        .find(|(_, r)| r.len() == 2)
        .expect("the across run");
    assert_eq!(run.total_bounds(), (3, 17), "1+2 up to 9+8");

    let layout = Layout::open(9, 1).expect("nine squares");
    let (_, run) = layout
        .runs()
        .find(|(_, r)| r.len() == 9)
        .expect("the across run");
    assert_eq!(
        run.total_bounds(),
        (45, 45),
        "all nine digits, so the total is forced"
    );
}
