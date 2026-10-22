//! Determinism: the same inputs produce the same logical state, and a replay that
//! does not reproduce the recorded hashes is refused rather than accepted.

mod common;

use common::{paint, PaintRules};
use gs_core::hash::StateHash;
use gs_core::registry::FamilyId;
use gs_core::ruleset::{Ruleset, RulesetId};
use gs_core::state::{Cell, Dims, State};
use gs_core::timeline::{replay, Timeline, TimelineError};

/// The hash of a known board, pinned.
///
/// CI runs this on x86_64 and aarch64, which is the point: a pinned value is how a
/// platform-dependent encoding or a non-stable hash gets caught, and it is why
/// `hash` writes out FNV-1a instead of using `DefaultHasher`, which is documented as
/// unstable across versions and platforms.
const PINNED_3X2_WITH_TWO_CELLS_SET: u64 = 0xab64_d17d_0a37_fa41;

fn fixture() -> State {
    let mut s = State::blank(Dims::new(3, 2));
    s.set(0, Cell(1)).expect("in bounds");
    s.set(4, Cell(2)).expect("in bounds");
    s
}

#[test]
fn the_state_hash_is_pinned_across_architectures() {
    assert_eq!(
        StateHash::of(&fixture()),
        StateHash(PINNED_3X2_WITH_TWO_CELLS_SET)
    );
}

#[test]
fn the_hash_goes_through_the_canonical_encoding_and_nothing_else() {
    let s = fixture();
    assert_eq!(StateHash::of(&s), StateHash::of_bytes(&s.canonical_bytes()));
}

#[test]
fn dimensions_are_part_of_identity() {
    // The same cell values in a different shape are a different state. Encoding only
    // the cells would make a 3x2 and a 2x3 board hash alike.
    let wide = State::blank(Dims::new(3, 2));
    let tall = State::blank(Dims::new(2, 3));
    assert_ne!(StateHash::of(&wide), StateHash::of(&tall));
}

#[test]
fn a_replay_reproduces_every_recorded_hash() {
    let rules = PaintRules::default();
    let mut t = Timeline::new(&rules, State::blank(Dims::new(4, 4)));
    for (i, v) in [(0u32, 1i16), (5, 2), (9, 1), (5, 3)] {
        t.play(&rules, paint(i, v)).expect("legal");
    }
    let log = t.action_log().expect("log");

    let replayed = replay(&rules, &log).expect("replay");
    assert_eq!(replayed.hash(), t.hash());
    assert_eq!(replayed.state(), t.state());
    assert_eq!(replayed.depth(), 4);
}

#[test]
fn a_replay_whose_hashes_disagree_is_refused() {
    let rules = PaintRules::default();
    let mut t = Timeline::new(&rules, State::blank(Dims::new(3, 3)));
    t.play(&rules, paint(0, 1)).expect("legal");
    t.play(&rules, paint(1, 1)).expect("legal");
    let mut log = t.action_log().expect("log");

    // A record that claims a state the actions do not produce. This is the shape of
    // the PuzzleGen failure: sound-looking content with a report that lies.
    log.hashes[1] = StateHash(0xdead_beef);

    match replay(&rules, &log) {
        Err(TimelineError::HashMismatch { recorded, .. }) => {
            assert_eq!(recorded, StateHash(0xdead_beef));
        }
        other => panic!("a lying record must be refused, got {other:?}"),
    }
}

#[test]
fn a_record_from_different_semantics_is_refused_not_reinterpreted() {
    let old = PaintRules { version: 1 };
    let new = PaintRules { version: 2 };
    let mut t = Timeline::new(&old, State::blank(Dims::new(2, 2)));
    t.play(&old, paint(0, 1)).expect("legal");
    let log = t.action_log().expect("log");

    match replay(&new, &log) {
        Err(TimelineError::RulesetMismatch { record, engine }) => {
            assert_eq!(record, old.id());
            assert_eq!(
                engine,
                RulesetId {
                    family: FamilyId("test.paint"),
                    version: 2
                }
            );
        }
        other => panic!("a record from another ruleset version must be refused, got {other:?}"),
    }
}
