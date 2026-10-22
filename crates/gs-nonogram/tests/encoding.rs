//! The SMT-LIB2 encoding.
//!
//! No solver runs here: this environment's network policy blocks the crate registry,
//! so no solver binding is vendored yet (ADR 0004). What can still be checked, and
//! is, is that the encoding is exhaustive, faithful to the clue, reproducible, and
//! that it refuses what it cannot express instead of quietly dropping it.

use gs_core::dsl::{Constraint, Domain, Global, Region, Rules};
use gs_core::state::Dims;
use gs_nonogram::clues::{Clues, Pattern};
use gs_nonogram::line::{feasible, Mark};
use gs_nonogram::rules::Nonogram;
use gs_nonogram::smtlib::{
    placements, reject_unsupported, solvability_query, uniqueness_query, EncodeError,
};
use gs_nonogram::solve::{solve, Budget};

#[test]
fn a_global_constraint_is_refused_by_name() {
    // Slitherlink's rule, handed to Nonogram's encoder. The refusal must say what it
    // refused: an encoder that dropped this and verified the rest would certify a
    // puzzle against rules it never encoded.
    let rules = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![
            Constraint::RunLengths {
                region: Region::Row(0),
                value: 1,
                runs: vec![2],
            },
            Constraint::Global(Global::SingleClosedLoop { value: 1 }),
        ],
    };
    match reject_unsupported(&rules) {
        Err(EncodeError::UnsupportedGlobal(named)) => {
            assert!(named.contains("SingleClosedLoop"), "got {named}");
        }
        other => panic!("a global constraint must be refused by name, got {other:?}"),
    }
}

#[test]
fn a_local_constraint_with_no_encoder_is_also_refused() {
    // The subtler version of the same failure: the constraint is local, so a tier
    // check alone would wave it through, but this encoder has no clause for it.
    // Unencodable is unencodable, whatever its tier.
    let rules = Rules {
        domain: Domain { min: 0, max: 8 },
        constraints: vec![Constraint::Distinct {
            region: Region::Row(0),
        }],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedGlobal(_))
    ));
}

#[test]
fn a_nonogram_is_always_encodable() {
    let pattern = Pattern::from_rows(&["##.", ".#.", ".##"]).expect("rectangular");
    let puzzle = Nonogram::new(Clues::of(&pattern));
    assert_eq!(reject_unsupported(&puzzle.dsl_rules()), Ok(()));
    assert!(solvability_query(&puzzle).is_ok());
}

#[test]
fn placements_agree_with_the_line_solver() {
    // Two independent implementations of the same question. `placements` enumerates
    // every layout of a clue; `feasible` decides whether a clue can still be placed
    // over given marks. A board is a placement if and only if `feasible` accepts it
    // fully decided. A disagreement means one of them is wrong, and this is the kind
    // of check that catches an off-by-one in a mandatory gap.
    for runs in [
        vec![],
        vec![1u16],
        vec![5],
        vec![1, 1],
        vec![2, 1],
        vec![1, 1, 1],
        vec![3, 1],
    ] {
        let length = 5u16;
        let Ok(options) = placements(&runs, length) else {
            continue;
        };

        // Every enumerated placement is accepted.
        for option in &options {
            let marks: Vec<Mark> = option
                .iter()
                .map(|f| if *f { Mark::Filled } else { Mark::Empty })
                .collect();
            assert!(
                feasible(&marks, &runs),
                "placements produced a layout the line solver rejects: {runs:?} {option:?}"
            );
        }

        // And nothing else is. Brute force over all 2^5 lines.
        for bits in 0u32..(1 << length) {
            let line: Vec<bool> = (0..length).map(|i| bits >> i & 1 == 1).collect();
            let marks: Vec<Mark> = line
                .iter()
                .map(|f| if *f { Mark::Filled } else { Mark::Empty })
                .collect();
            assert_eq!(
                options.contains(&line),
                feasible(&marks, &runs),
                "disagreement on {runs:?} / {line:?}"
            );
        }
    }
}

#[test]
fn a_clue_longer_than_its_line_is_refused() {
    assert_eq!(placements(&[3, 3], 5), Err(7));
    let clues = Clues {
        dims: Dims::new(5, 1),
        rows: vec![vec![3, 3]],
        columns: (0..5).map(|_| vec![]).collect(),
    };
    assert!(matches!(
        solvability_query(&Nonogram::new(clues)),
        Err(EncodeError::ClueLongerThanLine { .. })
    ));
}

#[test]
fn clues_that_do_not_match_the_board_are_refused() {
    let clues = Clues {
        dims: Dims::new(3, 3),
        rows: vec![vec![1]],
        columns: vec![vec![1], vec![1], vec![1]],
    };
    assert_eq!(
        solvability_query(&Nonogram::new(clues)),
        Err(EncodeError::ClueCountMismatch {
            expected: 3,
            found: 1
        })
    );
}

#[test]
fn the_query_is_byte_identical_for_the_same_clues() {
    // A reproducible release needs the query to be reproducible: the same puzzle must
    // produce the same text, or the validation report cannot name the encoding its
    // uniqueness claim was relative to.
    let pattern = Pattern::from_rows(&["#.#", ".#.", "#.#"]).expect("rectangular");
    let a = solvability_query(&Nonogram::new(Clues::of(&pattern))).expect("encodable");
    let b = solvability_query(&Nonogram::new(Clues::of(&pattern))).expect("encodable");
    assert_eq!(a, b);
}

#[test]
fn the_query_declares_every_cell_and_asserts_every_line() {
    let pattern = Pattern::from_rows(&["##.", ".#.", ".##"]).expect("rectangular");
    let query = solvability_query(&Nonogram::new(Clues::of(&pattern))).expect("encodable");

    assert_eq!(query.matches("(declare-const ").count(), 9, "one per cell");
    assert_eq!(
        query.matches("\n(assert ").count(),
        6,
        "three rows, three columns"
    );
    assert_eq!(query.matches("(check-sat)").count(), 1);
    for y in 0..3 {
        for x in 0..3 {
            assert!(query.contains(&format!("(declare-const c_{x}_{y} Bool)")));
        }
    }
}

#[test]
fn the_uniqueness_query_blocks_the_witness_and_nothing_else() {
    let pattern = Pattern::from_rows(&["##.", ".#.", ".##"]).expect("rectangular");
    let puzzle = Nonogram::new(Clues::of(&pattern));
    let witness = solve(&puzzle, Budget::default())
        .witness
        .expect("a solution");

    let base = solvability_query(&puzzle).expect("encodable");
    let unique = uniqueness_query(&puzzle, &witness).expect("encodable");

    // The uniqueness query is the solvability query plus exactly one more assertion.
    assert_eq!(
        unique.matches("\n(assert ").count(),
        base.matches("\n(assert ").count() + 1
    );
    assert_eq!(
        unique.matches("(check-sat)").count(),
        1,
        "one question, asked once"
    );
    assert!(unique.contains("at least one cell differs from the witness"));

    // The blocking clause names every cell, negated exactly where the witness is
    // filled. A clause that omitted a cell would permit a "different" board that is
    // in fact the same one.
    let clause = unique
        .lines()
        .find(|l| l.starts_with("(assert (or c_") || l.starts_with("(assert (or (not c_"))
        .expect("the blocking clause");
    for y in 0..3u16 {
        for x in 0..3u16 {
            let expected = if pattern.is_filled(x, y) {
                format!("(not c_{x}_{y})")
            } else {
                format!("c_{x}_{y}")
            };
            assert!(clause.contains(&expected), "missing {expected} in {clause}");
        }
    }
}
