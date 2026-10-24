//! Totals: deriving them from an answer, and refusing ones no digits could reach.

mod common;

use common::{from_answer, layout};
use gs_kakuro::layout::{Layout, WhiteId};
use gs_kakuro::totals::{Totals, TotalsError};

#[test]
fn totals_derived_from_an_answer_match_the_answer() {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    // Across runs come first: row one is 1 + 2 = 3, row two is 3 + 4 = 7.
    // Then the down runs: column one is 1 + 3 = 4, column two is 2 + 4 = 6.
    assert_eq!(puzzle.totals.as_slice(), &[3, 7, 4, 6]);
}

#[test]
fn a_partly_filled_board_implies_no_totals() {
    // Guessing a total from a half-filled board would invent a puzzle nobody drew.
    let layout = Layout::open(2, 1).expect("one row");
    let derived = Totals::of(&layout, &|white: WhiteId| {
        if white.0 == 0 {
            Some(4)
        } else {
            None
        }
    });
    assert_eq!(derived, None);
}

#[test]
fn a_board_holding_a_zero_implies_no_totals() {
    let layout = Layout::open(2, 1).expect("one row");
    assert_eq!(Totals::of(&layout, &|_| Some(0)), None);
}

#[test]
fn a_total_too_small_for_its_run_is_refused() {
    // Three squares hold three different digits, so the least they can add up to is
    // 1 + 2 + 3 = 6. A total of five is impossible by arithmetic, and a refusal with
    // a reason is more use than asking a solver and getting UNSAT with none.
    let layout = layout(&["..."]);
    let runs = layout.run_count();
    let mut numbers = vec![6_u16; runs];
    numbers[0] = 5;
    let error = Totals::new(&layout, numbers).expect_err("five is below the floor");
    assert!(
        matches!(
            error,
            TotalsError::Unreachable {
                total: 5,
                least: 6,
                ..
            }
        ),
        "expected an unreachable refusal naming the floor, got {error:?}"
    );
}

#[test]
fn a_total_too_large_for_its_run_is_refused() {
    // Two squares, so the most they can reach is 9 + 8 = 17.
    let layout = layout(&[".."]);
    let runs = layout.run_count();
    let mut numbers = vec![5_u16; runs];
    numbers[0] = 18;
    let error = Totals::new(&layout, numbers).expect_err("eighteen is above the ceiling");
    assert!(
        matches!(
            error,
            TotalsError::Unreachable {
                total: 18,
                most: 17,
                ..
            }
        ),
        "expected an unreachable refusal naming the ceiling, got {error:?}"
    );
}

#[test]
fn the_wrong_number_of_totals_is_refused() {
    let layout = layout(&[".."]);
    let error = Totals::new(&layout, vec![3]).expect_err("one total is not three");
    assert_eq!(
        error,
        TotalsError::WrongLength {
            expected: layout.run_count(),
            found: 1
        }
    );
}

#[test]
fn a_run_of_nine_has_only_one_possible_total() {
    // All nine digits once each, so the total is forced to 45 and every other number
    // is refused. A nice check that the bounds are exact rather than roughly right.
    let layout = Layout::open(9, 1).expect("nine squares");
    let runs = layout.run_count();
    let mut good = vec![5_u16; runs];
    good[0] = 45;
    assert!(Totals::new(&layout, good).is_ok());

    let mut bad = vec![5_u16; runs];
    bad[0] = 44;
    assert!(Totals::new(&layout, bad).is_err());
}
