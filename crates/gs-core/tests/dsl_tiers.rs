//! The DSL's tier classification.
//!
//! Nonogram needs only Tier A. Slitherlink, family two, needs Tier B. The reason
//! these tests exist on day one is the options review's warning: letting the DSL
//! assume every constraint is local is the one decision here that would be expensive
//! to reverse.

use gs_core::dsl::{Cmp, Constraint, Domain, Global, Region, Rules, Tier};

fn nonogram_line(row: u16, runs: Vec<u16>) -> Constraint {
    Constraint::RunLengths {
        region: Region::Row(row),
        value: 1,
        runs,
    }
}

#[test]
fn a_nonogram_shaped_ruleset_is_tier_a() {
    let rules = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![
            nonogram_line(0, vec![3]),
            nonogram_line(1, vec![1, 1]),
            Constraint::Count {
                region: Region::Column(0),
                value: 1,
                cmp: Cmp::Eq,
                count: 2,
            },
        ],
    };
    assert_eq!(rules.tier(), Tier::A);
    assert!(rules.constraints.iter().all(|c| c.globals().is_empty()));
}

#[test]
fn a_slitherlink_shaped_ruleset_is_tier_b() {
    let rules = Rules {
        domain: Domain { min: 0, max: 1 },
        constraints: vec![
            Constraint::Count {
                region: Region::Cells(vec![0, 1, 2, 3]),
                value: 1,
                cmp: Cmp::Eq,
                count: 3,
            },
            Constraint::Global(Global::SingleClosedLoop { value: 1 }),
        ],
    };
    assert_eq!(rules.tier(), Tier::B);
}

#[test]
fn a_global_constraint_nested_in_a_conjunction_is_not_lost() {
    // The failure this guards against: a compiler that checks only the top level,
    // sees local constraints, encodes them, and verifies a puzzle whose real rule it
    // never encoded. Dropping a constraint and verifying what is left is how
    // PuzzleGen published a board with a fabricated answer.
    let nested = Constraint::All(vec![
        nonogram_line(0, vec![2]),
        Constraint::All(vec![
            nonogram_line(1, vec![1]),
            Constraint::Global(Global::Connected { value: 1 }),
        ]),
    ]);

    assert_eq!(nested.tier(), Tier::B);
    assert_eq!(nested.globals(), vec![&Global::Connected { value: 1 }]);
}

#[test]
fn every_global_in_a_tree_is_reported_so_a_refusal_can_name_them() {
    let both = Constraint::All(vec![
        Constraint::Global(Global::Connected { value: 0 }),
        Constraint::Global(Global::NoSolidSquare { value: 0 }),
        nonogram_line(0, vec![1]),
    ]);
    assert_eq!(both.globals().len(), 2);
}

#[test]
fn an_empty_conjunction_is_local_and_says_so() {
    // Not an accident: `All([])` is vacuously true and local. It must not be
    // classified as Tier B, which would make an empty rule set look like one that
    // needs a connectivity encoder.
    assert_eq!(Constraint::All(Vec::new()).tier(), Tier::A);
    assert_eq!(
        Rules {
            domain: Domain { min: 0, max: 1 },
            constraints: Vec::new(),
        }
        .tier(),
        Tier::A
    );
}
