//! The shape of the SMT-LIB2 text, without a solver.
//!
//! These tests check that the text says what it should. They cannot check that a
//! solver reading it reaches the right verdict — a malformed encoding passes every
//! test in this file. That gap is what `tests/solver_agreement.rs` is for, and it is
//! the only job here that puts our work in front of a program nobody in this project
//! wrote.

mod common;

use common::{board, rectangle_loop};
use gs_core::dsl::{Cmp, Constraint, Domain, Global, Region, Rules};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::rules::{Slitherlink, LINE};
use gs_slitherlink::smtlib::{
    encoded_clues, reject_unsupported, solvability_query, uniqueness_query, EncodeError,
};

fn small() -> (Slitherlink, gs_slitherlink::loops::Loop) {
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    (Slitherlink::new(Clues::of(&answer)), answer)
}

#[test]
fn every_edge_is_declared_once() {
    let (p, _) = small();
    let text = solvability_query(&p).expect("encodable");
    for edge in p.geometry().edges() {
        let declaration = format!("(declare-const e_{} Bool)\n", edge.0);
        assert_eq!(
            text.matches(&declaration).count(),
            1,
            "edge {} is declared exactly once",
            edge.0
        );
    }
}

#[test]
fn there_is_one_assertion_per_clue_and_it_names_the_clue() {
    let (p, _) = small();
    let text = solvability_query(&p).expect("encodable");
    let g = p.geometry();
    for (cell, clue) in p.clues.given() {
        let (x, y) = g.cell_at(cell).expect("a place");
        assert!(
            text.contains(&format!("; cell ({x}, {y}) = {clue}\n")),
            "the text has to say which clue each assertion is, and is missing ({x}, {y})"
        );
    }
}

#[test]
fn every_dot_gets_the_nought_or_two_rule() {
    let (p, _) = small();
    let text = solvability_query(&p).expect("encodable");
    assert_eq!(
        text.matches("(or (= n 0) (= n 2))").count(),
        p.geometry().dot_count(),
        "one corner rule per dot, and no dot left out"
    );
}

#[test]
fn every_dot_gets_a_distance_and_a_step_towards_the_root() {
    let (p, _) = small();
    let text = solvability_query(&p).expect("encodable");
    let g = p.geometry();
    for dot in g.dots() {
        assert!(
            text.contains(&format!("(declare-const d_{} Int)", dot.0)),
            "dot {} has no distance label",
            dot.0
        );
        assert!(
            text.contains(&format!("(= root {})", dot.0)),
            "dot {} is never considered as the root",
            dot.0
        );
    }
    assert_eq!(text.matches("(declare-const root Int)").count(), 1);
}

#[test]
fn a_step_only_ever_goes_to_a_real_neighbour() {
    // The encoding's own version of the geometry check: a step from a dot must name
    // an edge at that dot and the distance label of the dot at its other end. A
    // wrong neighbour here would let the solver walk across the board and would make
    // the whole-board rule accept two rings.
    let (p, _) = small();
    let text = solvability_query(&p).expect("encodable");
    let g = p.geometry();
    for dot in g.dots() {
        for edge in g.edges_of_dot(dot) {
            let (a, b) = g.ends_of(edge).expect("two ends");
            let other = if a == dot { b } else { a };
            let step = format!("(and e_{} (< d_{} d_{}))", edge.0, other.0, dot.0);
            assert!(
                text.contains(&step),
                "dot {} should be able to step along edge {} to dot {}",
                dot.0,
                edge.0,
                other.0
            );
        }
    }
}

#[test]
fn the_uniqueness_query_blocks_the_witness_over_edges_only() {
    let (p, answer) = small();
    let text = uniqueness_query(&p, &answer.as_state()).expect("encodable");
    assert!(text.contains("; uniqueness: at least one edge differs from the witness"));
    // One literal per edge, each the opposite of what the witness says. No literal
    // mentions `root` or a distance label, which is what keeps a relabelling of the
    // same board from reading as a second answer.
    let blocking = text
        .split("; uniqueness: at least one edge differs from the witness\n")
        .nth(1)
        .expect("the blocking constraint");
    assert!(!blocking.contains("root"), "{blocking}");
    assert!(!blocking.contains("d_"), "{blocking}");
    for edge in p.geometry().edges() {
        let expected = if answer.is_drawn(edge) {
            format!("(not e_{})", edge.0)
        } else {
            format!("e_{}", edge.0)
        };
        assert!(
            blocking.contains(&expected),
            "edge {} is blocked the wrong way round",
            edge.0
        );
    }
}

#[test]
fn the_uniqueness_query_is_the_solvability_query_plus_the_blocking() {
    let (p, answer) = small();
    let solvable = solvability_query(&p).expect("encodable");
    let unique = uniqueness_query(&p, &answer.as_state()).expect("encodable");
    let shared = solvable
        .trim_end()
        .trim_end_matches("(check-sat)")
        .to_string();
    assert!(
        unique.starts_with(&shared),
        "the uniqueness query must ask the same thing plus one constraint"
    );
    assert_eq!(unique.matches("(check-sat)").count(), 1);
}

#[test]
fn the_same_puzzle_gives_the_same_text_every_time() {
    // A reproducible release needs the query to be reproducible, since what the
    // report records is the identity of the text.
    let (p, answer) = small();
    assert_eq!(solvability_query(&p), solvability_query(&p));
    assert_eq!(
        uniqueness_query(&p, &answer.as_state()),
        uniqueness_query(&p, &answer.as_state())
    );
}

#[test]
fn a_puzzle_with_no_clues_still_states_both_halves_of_the_loop_rule() {
    let g = board(3, 3);
    let p = Slitherlink::new(Clues::none(g));
    let text = solvability_query(&p).expect("a clueless puzzle is still encodable");
    assert_eq!(
        text.matches("(or (= n 0) (= n 2))").count(),
        g.dot_count(),
        "the corner rule"
    );
    assert!(
        text.contains("(declare-const root Int)"),
        "the one-piece rule"
    );
}

#[test]
fn rules_without_the_loop_rule_are_refused_rather_than_patched() {
    // An encoder that supplied the missing rule itself would be deciding what the
    // puzzle is, which belongs to the family. So this is a refusal.
    let only_a_count = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![Constraint::Count {
            region: Region::Cells(vec![0, 1, 2, 3]),
            value: LINE.0,
            cmp: Cmp::Eq,
            count: 2,
        }],
    };
    assert_eq!(
        reject_unsupported(&only_a_count),
        Err(EncodeError::NoLoopRule)
    );
}

#[test]
fn the_dsls_other_global_rules_are_refused_by_name() {
    // This encoder implements one global rule. The DSL has three, and the other two
    // belong to families that do not exist yet. A future family carrying one must not
    // be able to ride in on this encoder, which would verify its local rules and
    // silently ignore the rule that makes it that game.
    for other in [
        Global::Connected { value: 1 },
        Global::NoSolidSquare { value: 1 },
    ] {
        let rules = Rules {
            domain: Domain { min: 0, max: 1 },
            constraints: vec![
                Constraint::Global(other.clone()),
                Constraint::Global(Global::SingleClosedLoop { value: 1 }),
            ],
        };
        match reject_unsupported(&rules) {
            Err(EncodeError::UnsupportedGlobal(named)) => {
                assert!(named.contains(match other {
                    Global::Connected { .. } => "Connected",
                    Global::NoSolidSquare { .. } => "NoSolidSquare",
                    Global::SingleClosedLoop { .. } => unreachable!(),
                }));
            }
            result => panic!("{other:?} must be refused by name, and gave {result:?}"),
        }
    }
}

#[test]
fn a_local_rule_with_no_translation_is_refused_too() {
    // Just as dangerous as an unencoded global one. A `Distinct` over edges means
    // nothing in this game, and encoding the rest of the puzzle while ignoring it
    // would be verifying a different puzzle.
    let rules = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![
            Constraint::Distinct {
                region: Region::Cells(vec![0, 1]),
            },
            Constraint::Global(Global::SingleClosedLoop { value: LINE.0 }),
        ],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedConstraint(_))
    ));
}

#[test]
fn a_count_that_is_not_an_exact_count_is_refused() {
    // "At most two sides" is a sensible rule and not one this encoder writes. It is
    // refused rather than read as "exactly two", which would be verifying a puzzle
    // nobody set.
    let rules = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![
            Constraint::Count {
                region: Region::Cells(vec![0, 1, 2, 3]),
                value: LINE.0,
                cmp: Cmp::AtMost,
                count: 2,
            },
            Constraint::Global(Global::SingleClosedLoop { value: LINE.0 }),
        ],
    };
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedConstraint(_))
    ));
}

#[test]
fn a_real_slitherlink_is_accepted() {
    // The other half, so the refusal tests cannot pass by refusing everything.
    let (p, _) = small();
    assert_eq!(reject_unsupported(&p.dsl_rules()), Ok(()));
}

#[test]
fn the_recorded_encoding_names_the_clues_it_was_built_from() {
    let (p, _) = small();
    let recorded = encoded_clues(&p.clues);
    assert!(recorded.starts_with("3x3 "), "{recorded}");
    assert!(recorded.contains("(0,0)="), "{recorded}");
    // Two different puzzles must not record the same encoding, or the report would
    // describe an encoding other than the one that was asked about.
    let fewer = p.clues.without(gs_slitherlink::board::CellId(0));
    assert_ne!(recorded, encoded_clues(&fewer));
}
