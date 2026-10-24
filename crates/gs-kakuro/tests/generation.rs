//! Making puzzles: that it works, and that it keeps the promises it makes.
//!
//! The headline of this file is `repair_is_what_makes_this_work`, which measures the
//! obvious method against the one in use. It is a test rather than a note in a
//! document because the gap between them is the whole reason `generate.rs` is shaped
//! the way it is, and a note would quietly stop being true.

mod common;

use gs_core::ruleset::Ruleset;
use gs_kakuro::construct::Weave;
use gs_kakuro::generate::{
    batch, given_away_per_mille, propose, starting_pattern, Rejected, Shape, GENERATOR_VERSION,
};
use gs_kakuro::layout::WhiteId;
use gs_kakuro::rules::Kakuro;
use gs_kakuro::smtlib::{solvability_query, uniqueness_query};
use gs_kakuro::solve::{fill, solve, Budget, Solved};
use gs_kakuro::totals::Totals;

#[test]
fn a_seed_produces_a_puzzle_whose_witness_is_an_answer() {
    let candidate = propose(0, Shape::middling(5, 5), Budget::default()).expect("a puzzle");
    assert!(
        candidate.puzzle.is_solved(&candidate.witness),
        "the witness must be an answer to the puzzle it came with"
    );
    assert_eq!(candidate.generator_version, GENERATOR_VERSION);
}

#[test]
fn the_puzzle_a_seed_produces_has_exactly_one_answer_by_our_own_solver() {
    for seed in 0..6_u64 {
        let Ok(candidate) = propose(seed, Shape::middling(5, 5), Budget::default()) else {
            continue;
        };
        assert_eq!(
            solve(&candidate.puzzle, Budget::default()).solved,
            Solved::One,
            "seed {seed}: the generator kept it, so it must have one answer"
        );
    }
}

#[test]
fn one_seed_always_gives_the_same_puzzle() {
    // Determinism, which the whole platform rests on: a validation report records a
    // seed, and a seed that produced a different puzzle tomorrow would make the
    // report a record of nothing. Both of the repair's tie-breaks are fixed for this
    // reason, so this test is what holds them fixed.
    for seed in [0_u64, 3, 11] {
        let once = propose(seed, Shape::middling(5, 5), Budget::default());
        let twice = propose(seed, Shape::middling(5, 5), Budget::default());
        assert_eq!(once, twice, "seed {seed} gave two different results");
    }
}

#[test]
fn different_seeds_give_different_puzzles() {
    let kept: Vec<_> = (0..10_u64)
        .filter_map(|seed| propose(seed, Shape::middling(5, 5), Budget::default()).ok())
        .map(|c| c.puzzle)
        .collect();
    assert!(kept.len() >= 5, "too few puzzles to judge: {}", kept.len());
    let all_the_same = kept.windows(2).all(|pair| pair[0] == pair[1]);
    assert!(
        !all_the_same,
        "every seed gave the same puzzle, so the seed is doing nothing"
    );
}

#[test]
fn repair_is_what_makes_this_work() {
    // The measurement that justifies this module's whole design, kept as a test so it
    // cannot quietly stop being true.
    //
    // The obvious method is the Nonogram one: propose a random pattern, fill it, read
    // the totals off it, keep it if it has one answer. Here it almost never keeps
    // anything, because a random sparse layout has roughly half as many runs as it has
    // white squares, and that is half as many adding-up equations as unknowns.
    //
    // The method in use fixes the layout instead, using the second answer to say which
    // square is carrying the slack. This asserts the gap is large, not a particular
    // pair of numbers, so an improvement to either side does not fail the build.
    let shape = Shape::middling(6, 6);
    let seeds = 0..25_u64;

    let mut naive_kept = 0_u32;
    for seed in seeds.clone() {
        let Some(layout) = starting_pattern(seed, shape) else {
            continue;
        };
        let Some(digits) = fill(&layout, seed, Budget::default()) else {
            continue;
        };
        let Some(totals) = Totals::of(&layout, &|white: WhiteId| {
            digits.get(usize::try_from(white.0).ok()?).copied()
        }) else {
            continue;
        };
        let Ok(puzzle) = Kakuro::new(layout, totals) else {
            continue;
        };
        if solve(&puzzle, Budget::default()).solved == Solved::One {
            naive_kept += 1;
        }
    }

    let (_, report) = batch(seeds, shape, Budget::default());
    assert!(report.is_consistent(), "{report:?}");

    assert!(
        report.kept > naive_kept * 3,
        "repair kept {} of 25 and the obvious method kept {naive_kept}; if those are \
         close, the reasoning in this module's notes no longer holds and the extra \
         machinery is not earning its place",
        report.kept
    );
    assert!(
        report.kept_per_mille() > 500,
        "fewer than half the seeds produced a puzzle: {report:?}"
    );
}

#[test]
fn a_batch_report_adds_up() {
    let (kept, report) = batch(0..12_u64, Shape::middling(5, 5), Budget::default());
    assert!(report.is_consistent(), "{report:?}");
    assert_eq!(report.seeds, 12);
    assert_eq!(u32::try_from(kept.len()).expect("fits"), report.kept);
}

#[test]
fn an_empty_batch_reports_nothing_rather_than_dividing_by_nothing() {
    let (kept, report) = batch(0..0_u64, Shape::middling(5, 5), Budget::default());
    assert!(kept.is_empty());
    assert_eq!(report.kept_per_mille(), 0);
    assert!(report.is_consistent());
}

#[test]
fn the_giveaway_cap_refuses_a_board_that_hands_too_much_over() {
    // The quality gate, tested against the **random** method, which is the one it was
    // built for. A cap of nothing must refuse every board that method makes, because it
    // always ends up giving some squares away.
    //
    // Deliberately not tested against the built method, where a cap of nothing refuses
    // nothing at all, since a built board gives nothing away. That is not the gate
    // failing; it is the gate never firing because the thing it guards against has been
    // fixed. The built method's version of this is
    // `built_boards.rs::the_cap_on_giving_squares_away_is_zero_for_the_built_method`.
    let shape = Shape {
        max_given_away_per_mille: 0,
        ..Shape::random_pattern(5, 5)
    };
    let mut refused_for_quality = 0;
    for seed in 0..8_u64 {
        if let Err(Rejected::TooMuchGivenAway { cap: 0, .. }) =
            propose(seed, shape, Budget::default())
        {
            refused_for_quality += 1;
        }
    }
    assert!(
        refused_for_quality > 0,
        "a cap of nothing refused nothing, so the gate is not wired up"
    );
}

#[test]
fn a_kept_board_is_within_its_own_giveaway_cap() {
    let shape = Shape::middling(6, 6);
    for seed in 0..8_u64 {
        let Ok(candidate) = propose(seed, shape, Budget::default()) else {
            continue;
        };
        let share = given_away_per_mille(&candidate.puzzle.layout);
        assert!(
            share <= shape.max_given_away_per_mille,
            "seed {seed}: kept a board giving away {share} per mille, over the cap of {}",
            shape.max_given_away_per_mille
        );
    }
}

#[test]
fn the_feature_vector_records_what_was_given_away() {
    // `solve` on its own cannot fill this in, because only the generator knows the
    // finished layout. So the number must be put there on the way out, and this is
    // what checks it was.
    let mut checked = 0;
    for seed in 0..10_u64 {
        let Ok(candidate) = propose(seed, Shape::middling(6, 6), Budget::default()) else {
            continue;
        };
        checked += 1;
        assert_eq!(
            candidate.features.given_away_squares,
            gs_kakuro::generate::given_away(&candidate.puzzle.layout),
            "seed {seed}: the feature vector and the layout must agree about the giveaways"
        );
        assert_eq!(
            candidate.features.swap_rectangles,
            gs_kakuro::construct::swap_rectangles(&candidate.puzzle.layout),
            "seed {seed}: and about the swap rectangles"
        );
    }
    assert!(checked > 0, "nothing was made, so nothing was checked");
}

#[test]
fn every_generated_puzzle_can_be_turned_into_a_query() {
    // The join between part three and part two. A puzzle the generator keeps but the
    // encoder refuses would be a puzzle that can never be published, and nothing else
    // in the repository would notice.
    for seed in 0..10_u64 {
        let Ok(candidate) = propose(seed, Shape::middling(6, 6), Budget::default()) else {
            continue;
        };
        solvability_query(&candidate.puzzle)
            .unwrap_or_else(|e| panic!("seed {seed}: cannot be asked about: {e:?}"));
        uniqueness_query(&candidate.puzzle, &candidate.witness)
            .unwrap_or_else(|e| panic!("seed {seed}: no uniqueness query: {e:?}"));
    }
}

#[test]
fn repair_only_ever_adds_blocks() {
    // What the **random** method's repair is allowed to do. Each round turns one white
    // square black, so the white count must fall by exactly the number of repairs and
    // never rise. A repair that moved a block instead would be a different and much
    // harder thing to reason about, and this is what keeps it from drifting into one.
    //
    // The built method has two levers rather than one, so its rounds do not count
    // blacked-out squares; its version of this is
    // `the_built_method_never_adds_a_white_square` below.
    let shape = Shape::random_pattern(6, 6);
    for seed in 0..8_u64 {
        let Ok(candidate) = propose(seed, shape, Budget::default()) else {
            continue;
        };
        let started = starting_pattern(seed, shape).expect("a starting pattern");
        assert_eq!(
            started.white_count() - candidate.puzzle.layout.white_count(),
            usize::try_from(candidate.repairs).expect("fits"),
            "seed {seed}: the white count must fall by exactly one per repair"
        );
    }
}

#[test]
fn the_built_method_never_adds_a_white_square() {
    // The built method's version of `repair_only_ever_adds_blocks`. Its rounds are not
    // all blackings — most of them change a digit and leave the board alone — so the
    // white count cannot be counted off against the rounds. What still has to hold is
    // the direction: repair only ever takes squares away, never puts them back, because
    // a repair that could re-whiten a square could undo the thing that made the board
    // fair.
    let shape = Shape::middling(6, 6);
    let mut checked = 0;
    for seed in 0..8_u64 {
        let Ok(candidate) = propose(seed, shape, Budget::default()) else {
            continue;
        };
        // The board it started from. Which rung of the escalation it came from is not
        // knowable from here, and the top rung is the whitest, so this is the bound that
        // holds whichever rung it was.
        let started = gs_kakuro::construct::weave(6, 6, seed, Weave::middling())
            .expect("the first rung builds");
        checked += 1;
        assert!(
            candidate.puzzle.layout.white_count() <= started.white_count(),
            "seed {seed}: ended with more white squares than the board it started from"
        );
    }
    assert!(checked > 0, "nothing was made, so nothing was checked");
}
