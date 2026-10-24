//! What this family hands the rule language.
//!
//! The first family to produce a [`Constraint::Sum`] or a [`Constraint::Distinct`].
//! Both were written into the core on the slice's first day and have never been used,
//! so these tests are the first evidence either reservation was worth anything — the
//! same role Slitherlink played for the single-loop rule.

mod common;

use common::from_answer;
use gs_core::dsl::{Cmp, Constraint, Region, Tier};

#[test]
fn the_rules_are_tier_a() {
    // Every Kakuro rule is about one run of squares and nothing else, so this family
    // is local — Tier A — and still needs arithmetic. That combination is new here:
    // Nonogram is Tier A with no arithmetic, Slitherlink is Tier B.
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let rules = puzzle.dsl_rules();
    assert_eq!(rules.tier(), Tier::A);
    for constraint in &rules.constraints {
        assert!(
            constraint.globals().is_empty(),
            "no Kakuro rule is about the whole board at once: {constraint:?}"
        );
    }
}

#[test]
fn the_domain_is_one_to_nine_not_nought_to_nine() {
    let (puzzle, _) = from_answer(&[".."], &[4, 5]);
    let rules = puzzle.dsl_rules();
    assert_eq!(rules.domain.min, 1, "zero is not a Kakuro digit");
    assert_eq!(rules.domain.max, 9);
}

#[test]
fn every_run_gets_a_sum_and_a_distinct() {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let rules = puzzle.dsl_rules();
    let runs = puzzle.layout.run_count();

    let sums = rules
        .constraints
        .iter()
        .filter(|c| matches!(c, Constraint::Sum { .. }))
        .count();
    let distincts = rules
        .constraints
        .iter()
        .filter(|c| matches!(c, Constraint::Distinct { .. }))
        .count();
    assert_eq!(sums, runs, "one adding-up rule per run");
    assert_eq!(distincts, runs, "one no-repeats rule per run");
    assert_eq!(
        rules.constraints.len(),
        runs * 2,
        "and nothing else, so no rule is smuggled in"
    );
}

#[test]
fn a_sum_names_exactly_its_own_run_and_its_own_total() {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let rules = puzzle.dsl_rules();

    // The first across run is the top row, white squares 0 and 1, adding to 3.
    let first = rules
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Sum {
                region: Region::Cells(cells),
                cmp,
                total,
            } if cells == &vec![0, 1] => Some((*cmp, *total)),
            _ => None,
        })
        .expect("a rule over the top row");
    assert_eq!(first, (Cmp::Eq, 3), "exactly three, not at most three");
}

#[test]
fn the_sum_and_the_distinct_for_a_run_cover_the_same_squares() {
    // They are two rules rather than one, so this is what keeps them talking about
    // the same thing. Welding them into one would hide which of the two a solver
    // disagreed about, and knowing that is most of the value of running two solvers.
    let (puzzle, _) = from_answer(&["..#", "...", "#.."], &[1, 2, 3, 4, 5, 6, 7]);
    let rules = puzzle.dsl_rules();

    for (id, run) in puzzle.layout.runs() {
        let cells: Vec<u32> = run.squares.iter().map(|w| w.0).collect();
        let has_sum = rules.constraints.iter().any(|c| {
            matches!(c, Constraint::Sum { region: Region::Cells(r), total, .. }
                if r == &cells && *total == i64::from(puzzle.totals.get(id).unwrap_or(0)))
        });
        let has_distinct = rules
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Distinct { region: Region::Cells(r) } if r == &cells));
        assert!(has_sum, "run {id:?} has no adding-up rule over its squares");
        assert!(
            has_distinct,
            "run {id:?} has no no-repeats rule over its squares"
        );
    }
}

#[test]
fn a_one_square_run_still_gets_both_rules() {
    // A run of one names its own digit, so its no-repeats rule can never be broken.
    // It is emitted anyway: leaving it out would make the rule count depend on run
    // length, and a rule set whose shape varies is one a reader has to reason about
    // instead of check.
    let (puzzle, _) = from_answer(&["#.#"], &[5]);
    let rules = puzzle.dsl_rules();
    assert_eq!(puzzle.layout.run_count(), 2, "one across, one down");
    assert_eq!(rules.constraints.len(), 4);
}
