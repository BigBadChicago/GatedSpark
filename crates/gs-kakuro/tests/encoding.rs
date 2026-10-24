//! What the SMT-LIB2 text says, checked without running a solver.
//!
//! These tests check that the text *looks* right. Nothing here would notice if a
//! solver reading it reached the wrong verdict, which is what
//! `tests/solver_agreement.rs` is for. The division is deliberate: this file runs
//! everywhere with no solver installed, and that one runs where one is.

mod common;

use common::from_answer;
use gs_core::dsl::{Cmp, Constraint, Domain, Global, Region, Rules};
use gs_core::state::Cell;
use gs_kakuro::smtlib::{
    encoded_puzzle, reject_unsupported, solvability_query, uniqueness_query, EncodeError,
};

#[test]
fn the_query_declares_one_integer_per_white_square_and_no_others() {
    let (puzzle, _) = from_answer(&["..#", "..."], &[1, 2, 3, 4, 5]);
    let query = solvability_query(&puzzle).expect("encodable");
    for n in 0..5 {
        assert!(
            query.contains(&format!("(declare-const w_{n} Int)")),
            "white square {n} must be declared"
        );
    }
    assert!(
        !query.contains("w_5"),
        "and nothing beyond the white squares: a block is not a variable"
    );
    assert_eq!(
        query.matches("declare-const").count(),
        5,
        "this family declares no helper variables at all, which is why its \
         uniqueness query can block the digits and the model at once"
    );
}

#[test]
fn every_square_is_bounded_to_one_through_nine() {
    let (puzzle, _) = from_answer(&[".."], &[4, 5]);
    let query = solvability_query(&puzzle).expect("encodable");
    assert!(query.contains("(assert (and (<= 1 w_0) (<= w_0 9)))"));
    assert!(query.contains("(assert (and (<= 1 w_1) (<= w_1 9)))"));
    assert!(
        !query.contains("(<= 0 w_0)"),
        "nought is excluded by the rule, not merely unlikely: 9 + 0 would satisfy \
         a total of nine and no-repeats would not catch it"
    );
}

#[test]
fn each_run_gets_its_own_total() {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let query = solvability_query(&puzzle).expect("encodable");
    // Across: 1+2=3 over squares 0 and 1, then 3+4=7 over squares 2 and 3.
    assert!(query.contains("(assert (= 3 (+ w_0 w_1)))"));
    assert!(query.contains("(assert (= 7 (+ w_2 w_3)))"));
    // Down: 1+3=4 over squares 0 and 2, then 2+4=6 over squares 1 and 3.
    assert!(query.contains("(assert (= 4 (+ w_0 w_2)))"));
    assert!(query.contains("(assert (= 6 (+ w_1 w_3)))"));
}

#[test]
fn every_run_of_two_or_more_gets_a_distinct() {
    // The test guarding the one skip in the whole encoder. `distinct` needs two
    // arguments to be well-formed, so a run of one square gets none. That is sound,
    // and it is also the same shape as the mistake this project most wants to avoid,
    // so the count is checked on a board with runs of one, two and three.
    let board = &["..#", "..#", "#.."];
    let (puzzle, _) = from_answer(board, &[1, 2, 3, 4, 5, 6]);
    let query = solvability_query(&puzzle).expect("encodable");

    let long_runs = puzzle
        .layout
        .runs()
        .filter(|(_, run)| run.len() >= 2)
        .count();
    let short_runs = puzzle
        .layout
        .runs()
        .filter(|(_, run)| run.len() == 1)
        .count();
    assert!(short_runs > 0, "the fixture must contain a run of one");
    assert!(long_runs > 0, "and runs of more than one");
    assert_eq!(
        query.matches("(assert (distinct").count(),
        long_runs,
        "one no-repeats rule per run of two or more, and none for a run of one"
    );
}

#[test]
fn a_run_of_one_still_gets_its_total() {
    // The other half of the skip. A single square is skipped for `distinct` only, and
    // its total is still asserted — otherwise the square would be unconstrained and
    // the puzzle would have nine answers where it should have one.
    let (puzzle, _) = from_answer(&["#.#"], &[5]);
    let query = solvability_query(&puzzle).expect("encodable");
    assert!(
        !query.contains("distinct"),
        "no run here has two squares, so there is nothing to keep apart"
    );
    assert_eq!(
        query.matches("(assert (= 5 w_0))").count(),
        2,
        "its across run and its down run both say five"
    );
    assert!(
        !query.contains("(+ w_0)"),
        "a one-square run must not be written as a unary `+`: SMT-LIB2's `+` takes \
         two or more arguments, Z3 accepts the malformed form and CVC5 refuses the \
         whole query, so this spelling made every such board unverifiable"
    );
}

#[test]
fn the_uniqueness_query_blocks_the_witness_square_by_square() {
    let (puzzle, answer) = from_answer(&[".."], &[4, 5]);
    let query = uniqueness_query(&puzzle, &answer).expect("encodable");
    assert!(query.contains("(assert (or (not (= w_0 4)) (not (= w_1 5))))"));
    assert!(
        query.contains("(check-sat)"),
        "and it still asks the question"
    );
}

#[test]
fn the_uniqueness_query_contains_the_whole_solvability_query() {
    // The second question must be the first question plus one constraint, not a
    // separately built query that might drift from it.
    let (puzzle, answer) = from_answer(&["...", "#.."], &[1, 2, 3, 4, 5]);
    let solvability = solvability_query(&puzzle).expect("encodable");
    let uniqueness = uniqueness_query(&puzzle, &answer).expect("encodable");
    let shared = solvability
        .strip_suffix("\n(check-sat)\n")
        .expect("the query ends by asking");
    assert!(
        uniqueness.starts_with(shared),
        "the uniqueness query must be the solvability query plus a blocking rule"
    );
}

#[test]
fn an_incomplete_witness_is_refused() {
    // "At least one square differs from the witness" has no meaning at a square the
    // witness never filled in. Leaving that square out of the blocking rule would
    // accept a second answer differing only there; inventing a digit for it would ask
    // about a board nobody produced. So it is refused.
    let (puzzle, answer) = from_answer(&[".."], &[4, 5]);
    let mut partial = answer;
    partial.set(1, Cell::UNKNOWN).expect("square exists");
    assert!(uniqueness_query(&puzzle, &partial).is_err());
}

#[test]
fn a_global_constraint_is_refused_by_name() {
    // This encoder implements no whole-board rule. A puzzle carrying one is refused
    // rather than verified without it, which is the rule that stops a future family's
    // connectivity requirement riding in on this encoder and being silently dropped.
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![
            Constraint::Sum {
                region: Region::Cells(vec![0, 1]),
                cmp: Cmp::Eq,
                total: 9,
            },
            Constraint::Distinct {
                region: Region::Cells(vec![0, 1]),
            },
            Constraint::Global(Global::Connected { value: 1 }),
        ],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedGlobal(_))
    ));
}

#[test]
fn a_constraint_this_encoder_cannot_translate_is_refused() {
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![Constraint::RunLengths {
            region: Region::Row(0),
            value: 1,
            runs: vec![3],
        }],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedConstraint(_))
    ));
}

#[test]
fn an_at_most_total_is_refused_rather_than_read_as_equals() {
    // The DSL's `Sum` carries a comparison, and this encoder only knows `Eq`. An
    // `AtMost` quietly encoded as `Eq` would be a stricter puzzle than the one asked
    // about, and an `unsat` from it would be a claim about a different board.
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![Constraint::Sum {
            region: Region::Cells(vec![0, 1]),
            cmp: Cmp::AtMost,
            total: 9,
        }],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedConstraint(_))
    ));
}

#[test]
fn rules_with_no_total_at_all_are_refused() {
    // Refused rather than added. A Kakuro without its totals is a different puzzle,
    // and an encoder that supplied the missing rule would be deciding what the puzzle
    // is, which is the family's job.
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![Constraint::Distinct {
            region: Region::Cells(vec![0, 1]),
        }],
    };
    assert_eq!(reject_unsupported(&rules), Err(EncodeError::NoTotalRule));
}

#[test]
fn a_sum_without_its_matching_distinct_is_refused() {
    // The failure this family is most exposed to, because ADR 0018 emits the two
    // rules separately. A board verified with only its sums would have `4 + 4`
    // accepted as an answer to a total of eight, so a run with one rule and not the
    // other is refused at the door.
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![Constraint::Sum {
            region: Region::Cells(vec![0, 1]),
            cmp: Cmp::Eq,
            total: 8,
        }],
    };
    assert_eq!(
        reject_unsupported(&rules),
        Err(EncodeError::SumWithoutDistinct {
            squares: vec![0, 1]
        })
    );
}

#[test]
fn a_one_square_run_needs_no_distinct_to_be_accepted() {
    // The mirror of the test above: the missing-distinct check must not fire on the
    // case the encoder legitimately skips, or no board with a lone white square could
    // ever be encoded.
    let rules = Rules {
        domain: Domain { min: 1, max: 9 },
        constraints: vec![Constraint::Sum {
            region: Region::Cells(vec![0]),
            cmp: Cmp::Eq,
            total: 5,
        }],
    };
    assert_eq!(reject_unsupported(&rules), Ok(()));
}

#[test]
fn a_real_puzzle_is_accepted_by_the_rule_check() {
    let (puzzle, _) = from_answer(&["..#", "...", "#.."], &[1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(reject_unsupported(&puzzle.dsl_rules()), Ok(()));
}

#[test]
fn the_recorded_identity_names_the_blocks_and_the_totals() {
    // What a validation report records its uniqueness claim as being relative to. It
    // has to pin the layout as well as the totals, because the same totals on
    // different blocks are a different puzzle.
    let (puzzle, _) = from_answer(&[".#", ".."], &[1, 2, 3]);
    let recorded = encoded_puzzle(&puzzle);
    assert!(recorded.contains("kakuro 2x2"));
    assert!(
        recorded.contains("blocks=.#.."),
        "the block pattern is part of the puzzle's identity: {recorded}"
    );
    assert!(recorded.contains("totals="));
}

#[test]
fn the_recorded_identity_distinguishes_two_boards_of_the_same_size() {
    // Two 2 by 2 boards, same size and the same number of runs, differing only in
    // where the block is. The recorded identity has to tell them apart, because the
    // same totals on different blocks are a different puzzle and a report that could
    // not distinguish them would be pinning nothing.
    let (open, _) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let (blocked, _) = from_answer(&[".#", ".."], &[1, 2, 3]);
    assert_eq!(
        open.layout.run_count(),
        blocked.layout.run_count(),
        "the fixture is only interesting while the run counts match"
    );
    assert!(encoded_puzzle(&open).contains("blocks=...."));
    assert!(encoded_puzzle(&blocked).contains("blocks=.#.."));
    assert_ne!(encoded_puzzle(&open), encoded_puzzle(&blocked));
}
