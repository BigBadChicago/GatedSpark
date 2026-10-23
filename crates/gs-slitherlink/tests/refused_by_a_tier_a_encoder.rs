//! What a compiler that cannot encode a loop does with these rules.
//!
//! `gs_nonogram::smtlib::reject_unsupported` has always refused global constraints by
//! name, and until now the only thing it could be tested against was a hand-written
//! stand-in — a global constraint built in a test, for a family that did not exist.
//! A guard checked against a stand-in is a guard whose real case has never happened.
//!
//! Now there is a real Tier B family, so the guard is checked against the thing it
//! was written for. The test lives here, with the family that produces the
//! constraint, and `gs-nonogram` is a test-only dependency of this crate: nothing in
//! this crate's own code knows the other family exists.
//!
//! The rule being held is the one from PuzzleGen: **a compiler that cannot encode a
//! constraint refuses the puzzle.** It never drops the constraint and verifies what
//! is left. Dropping it here would leave the per-cell counts, which every two-ring
//! board satisfies, and the refused puzzle would come out "proved".

mod common;

use common::{board, rectangle_loop};
use gs_nonogram::smtlib::{reject_unsupported, EncodeError};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::rules::Slitherlink;

fn a_real_slitherlink() -> Slitherlink {
    let g = board(3, 3);
    Slitherlink::new(Clues::of(&rectangle_loop(g, 0, 0, 1, 1)))
}

#[test]
fn the_nonogram_encoder_refuses_a_slitherlink_and_names_the_loop() {
    let rules = a_real_slitherlink().dsl_rules();
    match reject_unsupported(&rules) {
        Err(EncodeError::UnsupportedGlobal(named)) => {
            assert!(
                named.contains("SingleClosedLoop"),
                "the refusal has to say what it refused, and said {named:?}"
            );
        }
        other => panic!("a Tier A encoder must refuse a Tier B puzzle, and gave {other:?}"),
    }
}

#[test]
fn it_refuses_even_a_slitherlink_with_no_clues_at_all() {
    // The clues are the part a Nonogram encoder could half-understand. With none of
    // them, all that is left is the loop rule, and the refusal must still happen —
    // otherwise "nothing I recognise" would quietly mean "nothing to check".
    let g = board(3, 3);
    let rules = Slitherlink::new(Clues::none(g)).dsl_rules();
    assert!(matches!(
        reject_unsupported(&rules),
        Err(EncodeError::UnsupportedGlobal(_))
    ));
}

#[test]
fn the_nonogram_encoder_still_accepts_a_nonogram() {
    // The other half, so the test above cannot pass by refusing everything.
    let pattern =
        gs_nonogram::clues::Pattern::from_rows(&["##.", ".#.", "..#"]).expect("a small pattern");
    let puzzle = gs_nonogram::rules::Nonogram::new(gs_nonogram::clues::Clues::of(&pattern));
    assert_eq!(reject_unsupported(&puzzle.dsl_rules()), Ok(()));
}
