//! The batch worker: does it stop, does it tell the truth, and does it repeat?

use gs_nonogram::clues::Clues;
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::solve::{solve, Budget, Solved};
use gs_nonogram::worker::{run, Limits, Quota};

fn limits(max_seeds_per_band: u64) -> Limits {
    Limits {
        max_seeds_per_band,
        budget: Budget::default(),
    }
}

fn quota(index: usize, wanted: u32) -> Quota {
    Quota {
        band: &BANDS[index],
        wanted,
    }
}

#[test]
fn a_run_fills_every_level_it_is_asked_for() {
    let quotas: Vec<Quota> = (0..BANDS.len()).map(|i| quota(i, 3)).collect();
    let result = run(1_000, &quotas, limits(2_000));

    for outcome in &result.outcomes {
        println!(
            "{}: produced {}/{} from {} seeds ({} seeds per puzzle), {} duplicates dropped",
            outcome.band_name,
            outcome.produced,
            outcome.wanted,
            outcome.seeds_tried,
            outcome.seeds_per_puzzle(),
            outcome.duplicates_dropped,
        );
    }
    assert!(
        result.complete(),
        "levels came up short: {:?}",
        result.shortfalls()
    );
    assert_eq!(result.candidates.len(), 3 * BANDS.len());
}

#[test]
fn the_same_request_produces_the_same_puzzles() {
    // The property the whole pool rests on. A worker that produced different output
    // from the same request could not be re-run to rebuild a pool, and a recorded
    // seed would mean nothing.
    let quotas = [quota(0, 4), quota(2, 2)];
    let first = run(77, &quotas, limits(1_500));
    let second = run(77, &quotas, limits(1_500));

    assert_eq!(first.outcomes, second.outcomes);
    assert_eq!(first.candidates, second.candidates);
}

#[test]
fn a_different_start_seed_produces_different_puzzles() {
    let quotas = [quota(0, 4)];
    let first = run(1, &quotas, limits(1_500));
    let second = run(500_000, &quotas, limits(1_500));
    assert_ne!(first.candidates, second.candidates);
}

#[test]
fn no_two_puzzles_in_a_run_are_the_same() {
    // Two seeds can produce the same clues, and at the smaller board sizes they do.
    // A pool with duplicates hands a player the same puzzle twice.
    let quotas: Vec<Quota> = (0..BANDS.len()).map(|i| quota(i, 5)).collect();
    let result = run(0, &quotas, limits(3_000));

    let mut keys: Vec<Vec<u8>> = result
        .candidates
        .iter()
        .map(|c| c.puzzle.clues.canonical_bytes())
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), before, "the run contains duplicate puzzles");
}

#[test]
fn an_impossible_quota_stops_and_says_so_rather_than_running_forever() {
    // The test that makes this safe to leave unattended. Ten seeds cannot fill a
    // quota of a thousand, and the worker must come back and say so.
    let quotas = [quota(BANDS.len() - 1, 1_000)];
    let result = run(0, &quotas, limits(10));

    let outcome = &result.outcomes[0];
    assert_eq!(outcome.seeds_tried, 10, "the seed limit was not respected");
    assert!(outcome.hit_seed_limit);
    assert!(!outcome.filled());
    assert!(outcome.shortfall() > 0);
    assert!(!result.complete());
    assert_eq!(result.shortfalls().len(), 1);
}

#[test]
fn the_duplicate_check_is_not_dead_code() {
    // Without this, `duplicates_dropped` would be a counter nothing ever increments
    // and the dedup would be untested code that merely looks reassuring.
    //
    // Measured on 2026-10-22: on the 8x8 level, seed 2119 produces the same clues as
    // seed 1058, and 20,000 seeds there contain 30 such collisions. The larger levels
    // showed none, which makes sense — there are fewer distinct small boards. So the
    // dedup earns its place because of the easiest level, not in general.
    //
    // The two seeds are named so this test stays cheap. If the generator changes, the
    // pair will stop colliding and this test will fail; the fix is to find the new
    // first colliding pair, not to delete the test.
    let band = &BANDS[0];
    let budget = Budget::default();
    let earlier = propose_in_band(band, 1_058, budget).expect("seed 1058 produces a puzzle");
    let later = propose_in_band(band, 2_119, budget).expect("seed 2119 produces a puzzle");
    assert_eq!(
        earlier.puzzle.clues.canonical_bytes(),
        later.puzzle.clues.canonical_bytes(),
        "seeds 1058 and 2119 no longer collide, so the pair in this test is stale"
    );
    assert_ne!(earlier.seed, later.seed);

    // Now the worker over a range covering both: it must count one and keep one.
    let result = run(
        1_058,
        &[Quota {
            band,
            wanted: 100_000,
        }],
        Limits {
            max_seeds_per_band: 1_100,
            budget,
        },
    );
    let outcome = &result.outcomes[0];
    println!(
        "{}: {} produced, {} duplicates dropped over {} seeds",
        outcome.band_name, outcome.produced, outcome.duplicates_dropped, outcome.seeds_tried
    );
    assert!(
        outcome.duplicates_dropped > 0,
        "the worker saw both colliding seeds and counted no duplicate"
    );
}

#[test]
fn a_zero_quota_does_no_work() {
    let quotas = [quota(0, 0)];
    let result = run(0, &quotas, limits(1_000));
    assert_eq!(result.candidates.len(), 0);
    assert_eq!(result.outcomes[0].seeds_tried, 0);
    assert!(
        result.outcomes[0].filled(),
        "nothing asked for is nothing owed"
    );
    assert!(result.complete());
}

#[test]
fn an_empty_request_is_a_complete_run_and_not_an_error() {
    let result = run(0, &[], limits(1_000));
    assert!(result.candidates.is_empty());
    assert!(result.outcomes.is_empty());
    assert!(result.complete());
}

#[test]
fn every_puzzle_produced_is_in_the_level_it_was_asked_for() {
    // A worker that filled the wrong level would mislabel the pool, and a player
    // would meet a "gentle" puzzle that is not.
    for (index, band) in BANDS.iter().enumerate() {
        let result = run(10_000, &[quota(index, 2)], limits(2_000));
        for candidate in &result.candidates {
            assert_eq!(candidate.shape, band.shape);
            assert!(
                band.claims(band.shape.dims, &candidate.features),
                "'{}' was handed a puzzle it does not claim",
                band.name
            );
        }
    }
}

#[test]
fn every_puzzle_produced_still_has_exactly_one_answer() {
    // End to end, against the filter directly rather than through the worker's own
    // bookkeeping. This is the claim a player experiences.
    let quotas: Vec<Quota> = (0..BANDS.len()).map(|i| quota(i, 2)).collect();
    let result = run(4_242, &quotas, limits(2_000));
    assert!(!result.candidates.is_empty());
    for candidate in &result.candidates {
        let analysis = solve(&candidate.puzzle, Budget::default());
        assert_eq!(
            analysis.solved,
            Solved::One,
            "seed {} was delivered but is {:?}",
            candidate.seed,
            analysis.solved
        );
    }
}

#[test]
fn a_candidate_can_be_rebuilt_from_its_recorded_seed() {
    // The validation report will carry a seed. If the seed does not rebuild the
    // puzzle, the report is decoration.
    let result = run(555, &[quota(1, 3)], limits(2_000));
    assert!(!result.candidates.is_empty());
    for candidate in &result.candidates {
        let rebuilt = gs_nonogram::generate::pattern(candidate.seed, candidate.shape);
        assert_eq!(
            Clues::of(&rebuilt),
            candidate.puzzle.clues,
            "seed {} does not rebuild its own puzzle",
            candidate.seed
        );
    }
}
