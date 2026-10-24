//! Moves: what a player may do, and what the platform refuses.

mod common;

use common::{from_answer, layout};
use gs_core::registry::{Family, FamilyId, Registry};
use gs_core::ruleset::{Illegal, Ruleset, RulesetId};
use gs_core::state::Cell;
use gs_kakuro::layout::WhiteId;
use gs_kakuro::rules::{is_digit, Action, Kakuro, RULESET_VERSION};
use gs_kakuro::totals::Totals;

fn puzzle() -> Kakuro {
    from_answer(&["...", "#.."], &[1, 2, 3, 4, 5]).0
}

#[test]
fn writing_a_digit_changes_one_square() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    let delta = puzzle
        .transition(
            &board,
            &Action::Write {
                square: WhiteId(0),
                value: Cell(7),
            },
        )
        .expect("writing a seven is a legal move");
    assert_eq!(delta.changes.len(), 1);
    assert_eq!(delta.changes[0].index, 0);
    assert_eq!(delta.changes[0].from, Cell::UNKNOWN);
    assert_eq!(delta.changes[0].to, Cell(7));
}

#[test]
fn rubbing_a_digit_out_is_a_legal_move() {
    let puzzle = puzzle();
    let mut board = puzzle.blank();
    board.set(0, Cell(7)).expect("square exists");
    puzzle
        .transition(
            &board,
            &Action::Write {
                square: WhiteId(0),
                value: Cell::UNKNOWN,
            },
        )
        .expect("clearing a square is a move, not the absence of one");
}

#[test]
fn a_zero_is_refused() {
    // Worth its own test. Zero is a perfectly good number and the other two families
    // both use it as a meaningful value, so nothing but this check stops a zero
    // being written into a Kakuro and satisfying a total of nine with 9 + 0.
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert!(!is_digit(Cell(0)), "zero is not a Kakuro digit");
    assert!(matches!(
        puzzle.transition(
            &board,
            &Action::Write {
                square: WhiteId(0),
                value: Cell(0),
            },
        ),
        Err(Illegal::NotPermitted(_))
    ));
}

#[test]
fn a_digit_above_nine_is_refused() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert!(matches!(
        puzzle.transition(
            &board,
            &Action::Write {
                square: WhiteId(0),
                value: Cell(10),
            },
        ),
        Err(Illegal::NotPermitted(_))
    ));
}

#[test]
fn a_negative_digit_is_refused() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert!(matches!(
        puzzle.transition(
            &board,
            &Action::Write {
                square: WhiteId(0),
                value: Cell(-3),
            },
        ),
        Err(Illegal::NotPermitted(_))
    ));
}

#[test]
fn writing_the_digit_already_there_is_refused_as_a_no_op() {
    // A no-op is not a move: letting one into the timeline makes undo counts
    // disagree with what the player did.
    let puzzle = puzzle();
    let mut board = puzzle.blank();
    board.set(2, Cell(6)).expect("square exists");
    assert_eq!(
        puzzle.transition(
            &board,
            &Action::Write {
                square: WhiteId(2),
                value: Cell(6),
            },
        ),
        Err(Illegal::NoOp)
    );
}

#[test]
fn a_square_off_the_board_is_refused() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert_eq!(
        puzzle.transition(
            &board,
            &Action::Write {
                square: WhiteId(99),
                value: Cell(1),
            },
        ),
        Err(Illegal::OutOfBounds)
    );
}

#[test]
fn a_blank_board_is_not_finished() {
    let puzzle = puzzle();
    assert!(!puzzle.is_solved(&puzzle.blank()));
}

#[test]
fn every_square_empty_is_reported_on_a_blank_board() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert_eq!(
        puzzle.empty_squares(&board).count(),
        puzzle.layout.white_count()
    );
}

#[test]
fn no_run_is_met_on_a_blank_board() {
    let puzzle = puzzle();
    let board = puzzle.blank();
    assert_eq!(
        puzzle.runs_not_met(&board).count(),
        puzzle.layout.run_count(),
        "a run with empty squares has no sum, so it is not met"
    );
}

#[test]
fn the_family_identifies_itself_and_its_ruleset_version() {
    let puzzle = puzzle();
    assert_eq!(Family::id(&puzzle), FamilyId("kakuro"));
    assert_eq!(
        Ruleset::id(&puzzle),
        RulesetId {
            family: FamilyId("kakuro"),
            version: RULESET_VERSION,
        }
    );
}

#[test]
fn the_platform_does_not_invent_this_family() {
    // The engine/game boundary: an empty registry refuses by name rather than
    // falling back to a default, which is what makes the no-family CI run pass for
    // the right reason.
    let registry = Registry::empty();
    assert!(registry.resolve(FamilyId("kakuro")).is_err());
}

#[test]
fn totals_that_do_not_match_the_run_count_are_refused() {
    let layout = layout(&["..", ".."]);
    let runs = layout.run_count();
    let totals = Totals::new(&layout, vec![3; runs]).expect("legal totals");
    let smaller = layout.clone();
    // Drop one run's worth by building against a layout with fewer runs, then
    // pairing those totals with the bigger one.
    let fewer = Totals::new(
        &gs_kakuro::layout::Layout::open(2, 1).expect("one row"),
        vec![3, 1, 2],
    )
    .expect("legal totals for the smaller layout");
    assert!(Kakuro::new(smaller, totals).is_ok());
    assert!(
        Kakuro::new(layout, fewer).is_err(),
        "a puzzle needs one total per run"
    );
}
