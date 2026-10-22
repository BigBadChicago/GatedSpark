//! Nonogram's difficulty levels: are they real, distinct, and reachable?
//!
//! The failure this file exists to prevent is a level that nothing can produce. A
//! band with no production behind it is a label, and a menu offering a level the
//! generator never fills is worse than a menu with fewer levels on it.

use gs_core::state::Dims;
use gs_nonogram::difficulty::{band_of, propose_in_band, Band, BANDS};
use gs_nonogram::generate::{batch, Shape};
use gs_nonogram::solve::{Budget, Features};

#[test]
fn there_is_at_least_one_band() {
    // The same guard the platform suite uses on its own check list. An empty list
    // would make every other test in this file pass by iterating nothing.
    assert!(
        !BANDS.is_empty(),
        "no bands declared, so nothing below tests anything"
    );
}

#[test]
fn every_band_is_reachable_by_generating_at_its_own_shape() {
    // The load-bearing test. Each band says what to generate to fill it; this checks
    // that doing so actually fills it.
    for band in BANDS {
        let (kept, report) = batch(0..400, band.shape, Budget::default());
        let in_band = kept
            .iter()
            .filter(|c| band.claims(band.shape.dims, &c.features))
            .count();
        println!(
            "{}: {}/400 seeds kept, {} of those in band",
            band.name, report.kept, in_band
        );
        assert!(
            in_band > 0,
            "band '{}' is unreachable: 400 seeds at its own shape produced {} puzzles \
             and none of them landed in it. Either the window is wrong or the shape is.",
            band.name,
            report.kept
        );
    }
}

#[test]
fn no_two_bands_claim_the_same_puzzle() {
    // Bands may overlap in round count across different board sizes, which is fine
    // because a band is a size and a window together. What must not happen is two
    // bands claiming one puzzle, which would make `band_of` depend on list order.
    for band in BANDS {
        let (kept, _) = batch(0..200, band.shape, Budget::default());
        for candidate in &kept {
            let claimants: Vec<&str> = BANDS
                .iter()
                .filter(|b| b.claims(band.shape.dims, &candidate.features))
                .map(|b| b.name)
                .collect();
            assert!(
                claimants.len() <= 1,
                "seed {} is claimed by {:?}",
                candidate.seed,
                claimants
            );
        }
    }
}

#[test]
fn bands_are_ordered_from_easier_to_harder() {
    // Not an arbitrary tidiness rule: the list order is what a menu would show, and
    // a level list that is not sorted by difficulty misleads a player. Compared by
    // board size first, since a bigger board is more work at the same round count.
    let mut previous: Option<&Band> = None;
    for band in BANDS {
        if let Some(earlier) = previous {
            let earlier_cells = earlier.shape.dims.cell_count();
            let cells = band.shape.dims.cell_count();
            assert!(
                cells > earlier_cells
                    || (cells == earlier_cells && band.min_rounds >= earlier.min_rounds),
                "'{}' is not harder than '{}'",
                band.name,
                earlier.name
            );
        }
        previous = Some(band);
    }
}

#[test]
fn a_puzzle_needing_a_guess_is_in_no_band() {
    // The deliberate exclusion. Written as a direct test on the features rather than
    // by hunting for a guessing puzzle, so it holds for every size and window.
    let needs_a_guess = Features {
        propagation_only_cells: 10,
        propagation_rounds: 4,
        decision_points: 1,
        max_search_depth: 1,
        cells: 64,
    };
    assert!(!needs_a_guess.line_solvable());
    for band in BANDS {
        assert!(
            !band.claims(band.shape.dims, &needs_a_guess),
            "band '{}' claimed a puzzle that needs a guess",
            band.name
        );
    }
    assert_eq!(band_of(BANDS[0].shape.dims, &needs_a_guess), None);
}

#[test]
fn a_puzzle_outside_every_window_is_ungraded_rather_than_squeezed_in() {
    let band = &BANDS[0];
    let too_quick = Features {
        propagation_only_cells: 64,
        propagation_rounds: band.min_rounds.saturating_sub(1),
        decision_points: 0,
        max_search_depth: 0,
        cells: 64,
    };
    assert_eq!(
        band_of(band.shape.dims, &too_quick),
        None,
        "a puzzle below the easiest window was given a band anyway"
    );
}

#[test]
fn a_puzzle_of_the_wrong_size_is_not_claimed() {
    // A band is a shape and a window together. Features alone must not be enough.
    let band = &BANDS[0];
    let features = Features {
        propagation_only_cells: 64,
        propagation_rounds: band.min_rounds,
        decision_points: 0,
        max_search_depth: 0,
        cells: 64,
    };
    assert!(band.claims(band.shape.dims, &features));
    let wrong_size = Dims {
        width: band.shape.dims.width + 1,
        height: band.shape.dims.height,
    };
    assert!(!band.claims(wrong_size, &features));
}

#[test]
fn asking_for_one_band_never_returns_another() {
    // What a batch worker depends on: it names a level and gets that level or
    // nothing. If this leaked, a worker filling one band would quietly fill another
    // and the pool would be mislabelled.
    for band in BANDS {
        let mut found = 0;
        for seed in 0..200u64 {
            if let Some(candidate) = propose_in_band(band, seed, Budget::default()) {
                found += 1;
                assert_eq!(
                    band_of(band.shape.dims, &candidate.features).map(|b| b.name),
                    Some(band.name),
                    "asked for '{}' and got a puzzle that belongs elsewhere",
                    band.name
                );
            }
        }
        assert!(found > 0, "'{}' produced nothing in 200 seeds", band.name);
    }
}

#[test]
fn every_bands_shape_is_one_the_generator_accepts() {
    // A shape whose boards are nearly all thrown out as lopsided would make a band
    // technically reachable and practically empty. This is a yield floor, loose on
    // purpose: it catches a shape that is broken, not one that is merely expensive.
    for band in BANDS {
        let (_, report) = batch(0..200, band.shape, Budget::default());
        println!("{}: kept {} per mille", band.name, report.kept_per_mille());
        assert!(
            report.kept_per_mille() > 50,
            "band '{}' keeps only {} per mille of seeds, which is a broken shape \
             rather than a hard level",
            band.name,
            report.kept_per_mille()
        );
    }
}

#[test]
fn the_shapes_are_distinct_so_each_band_has_its_own_production() {
    let mut shapes: Vec<Shape> = BANDS.iter().map(|b| b.shape).collect();
    let before = shapes.len();
    shapes.sort_by_key(|s| {
        (
            s.dims.width,
            s.dims.height,
            s.fill_per_mille,
            s.smoothing_rounds,
        )
    });
    shapes.dedup();
    assert_eq!(
        shapes.len(),
        before,
        "two bands generate from the same shape, so one is competing with the other"
    );
}
