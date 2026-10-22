//! The generator's random numbers have to be reproducible, because a recorded seed
//! is how a published puzzle can be rebuilt from scratch and checked.
//!
//! These tests exist to stop the stream changing by accident. If a change to
//! `rng.rs` is deliberate, the pinned values below have to be updated by hand and
//! every seed recorded before that change stops reproducing its puzzle — which is
//! exactly the conversation the pin is here to force.

use gs_nonogram::rng::Rng;

/// The first four numbers from one fixed seed, written down.
///
/// Nothing derives these: they were read out of a run and pasted here. A test that
/// recomputed them from the same code it is testing would pass no matter what the
/// code did.
const PINNED_SEED: u64 = 0x5EED;
const PINNED_STREAM: [u64; 4] = [
    0x09f1_fd9d_03f0_a9b4,
    0x5532_7416_1bbf_8475,
    0x5d5b_ca46_96b3_43b3,
    0x70d2_9b6c_7d22_528d,
];

#[test]
fn the_stream_from_a_seed_is_pinned() {
    let mut rng = Rng::from_seed(PINNED_SEED);
    let got: Vec<u64> = (0..PINNED_STREAM.len()).map(|_| rng.next_u64()).collect();
    assert_eq!(
        got,
        PINNED_STREAM.to_vec(),
        "the generator's number stream changed. Every seed recorded before this \
         change now rebuilds a different puzzle. If that is intended, update the \
         pinned values and say so in the commit."
    );
}

#[test]
fn the_same_seed_twice_gives_the_same_numbers() {
    let a: Vec<u64> = {
        let mut rng = Rng::from_seed(12_345);
        (0..32).map(|_| rng.next_u64()).collect()
    };
    let b: Vec<u64> = {
        let mut rng = Rng::from_seed(12_345);
        (0..32).map(|_| rng.next_u64()).collect()
    };
    assert_eq!(a, b);
}

#[test]
fn different_seeds_give_different_numbers() {
    let mut one = Rng::from_seed(1);
    let mut two = Rng::from_seed(2);
    // Not a statistical claim, just the sanity check that the seed reaches the
    // stream at all. A generator that ignored its seed would pass every other test
    // in this file.
    let first: Vec<u64> = (0..8).map(|_| one.next_u64()).collect();
    let second: Vec<u64> = (0..8).map(|_| two.next_u64()).collect();
    assert_ne!(first, second);
}

#[test]
fn the_two_ends_of_chance_are_certain() {
    let mut rng = Rng::from_seed(7);
    for _ in 0..1_000 {
        assert!(!rng.chance_per_mille(0), "0 in 1000 must never happen");
    }
    for _ in 0..1_000 {
        assert!(
            rng.chance_per_mille(1000),
            "1000 in 1000 must always happen"
        );
    }
    // Out of range is clamped rather than wrapping into a small chance.
    for _ in 0..100 {
        assert!(rng.chance_per_mille(5_000));
    }
}

#[test]
fn chance_lands_somewhere_between_the_ends() {
    // Deliberately loose. This is not a randomness test; it only catches a
    // threshold that is off by a factor or inverted, which is the mistake that
    // would otherwise show up much later as a generator that makes only blank
    // boards.
    let mut rng = Rng::from_seed(99);
    let hits = (0..10_000).filter(|_| rng.chance_per_mille(300)).count();
    assert!(
        (2_000..4_000).contains(&hits),
        "asked for about 3000 in 10000 and got {hits}"
    );
}

#[test]
fn below_zero_has_no_answer_and_says_so() {
    let mut rng = Rng::from_seed(3);
    assert_eq!(rng.below(0), None, "there is no number below zero to pick");
}

#[test]
fn below_stays_in_range() {
    let mut rng = Rng::from_seed(4);
    for n in 1..64u32 {
        for _ in 0..64 {
            let picked = rng.below(n).expect("n is not zero");
            assert!(picked < n, "{picked} is not below {n}");
        }
    }
}
