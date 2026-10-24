//! Boards built on purpose, and the one promise they make.
//!
//! The promise is small enough to state in a line: **no run of one square, anywhere.**
//! A run of one square has its digit named by its own total, so a board containing one
//! has been partly handed to the player. ADR 0019 measured the older method at about 38
//! in every 100 white squares given away like that, and ADR 0021 is this.
//!
//! Most of this file is about the promise surviving things, rather than about it holding
//! once. It is easy to build a fair board and then lose the property in the repair, and
//! the repair is where the earlier discarded attempt went wrong.

mod common;

use common::layout;
use gs_kakuro::construct::{
    gives_nothing_away, rules_per_unknown_per_mille, weave, NotWoven, Weave, CHOICES,
    SHORTEST_FAIR_RUN,
};
use gs_kakuro::generate::{given_away, given_away_per_mille, propose, LayoutFrom, Shape};
use gs_kakuro::layout::LONGEST_RUN;
use gs_kakuro::solve::{solve, Budget, Solved};

/// Every size worth building, including the oblong ones. A square board is the easy
/// case, and three families in, the oblong case is where the index arithmetic goes
/// wrong.
const SIZES: [(u16, u16); 8] = [
    (2, 2),
    (4, 4),
    (5, 5),
    (6, 6),
    (7, 7),
    (9, 9),
    (4, 7),
    (9, 4),
];

#[test]
fn a_built_board_has_no_run_of_one_square_at_any_size() {
    // The whole point of the module, over every size and every run limit it offers.
    for (width, height) in SIZES {
        for longest_run in SHORTEST_FAIR_RUN..=LONGEST_RUN {
            let asked = Weave {
                longest_run,
                block_first_per_mille: 350,
                avoid_swaps: true,
            };
            for seed in 0..5_u64 {
                let Ok(built) = weave(width, height, seed, asked) else {
                    continue;
                };
                assert!(
                    gives_nothing_away(&built),
                    "{width}x{height}, longest {longest_run}, seed {seed}: a built board \
                     must have no run of one square"
                );
                for (_, run) in built.runs() {
                    assert!(
                        run.len() >= SHORTEST_FAIR_RUN && run.len() <= longest_run,
                        "{width}x{height}, longest {longest_run}, seed {seed}: a run of \
                         {} squares is outside the limits asked for",
                        run.len()
                    );
                }
            }
        }
    }
}

#[test]
fn the_search_finds_a_board_at_every_size_rather_than_only_the_easy_ones() {
    // A builder that worked at 5 by 5 and gave up at 9 by 9 would be no use, and this is
    // the test that would have caught the discarded row-then-column repair, which did
    // not converge at any size.
    for (width, height) in SIZES {
        let built = weave(width, height, 0, Weave::middling());
        assert!(
            built.is_ok(),
            "{width}x{height}: no board built, got {built:?}"
        );
    }
}

#[test]
fn one_seed_always_builds_the_same_board() {
    // Determinism, which the whole platform rests on: a validation report records a
    // seed. The search makes random choices, so this is what holds them repeatable.
    for seed in [0_u64, 7, 4321] {
        let once = weave(7, 7, seed, Weave::middling());
        let twice = weave(7, 7, seed, Weave::middling());
        assert_eq!(once, twice, "seed {seed} built two different boards");
    }
}

#[test]
fn different_seeds_build_different_boards() {
    let boards: Vec<_> = (0..8_u64)
        .filter_map(|seed| weave(7, 7, seed, Weave::middling()).ok())
        .collect();
    assert!(boards.len() >= 6, "most seeds should build something");
    let first = boards.first().expect("at least one");
    assert!(
        boards.iter().any(|board| board != first),
        "eight seeds gave one board, so the seed is not reaching the search"
    );
}

#[test]
fn a_shorter_longest_run_really_does_mean_more_rules_per_unknown() {
    // The reasoning the escalation rests on, as a measurement rather than an assertion
    // in a comment. Each run is one adding-up rule, so cutting the board into shorter
    // pieces has to buy more rules over the same squares. If this stopped being true,
    // escalating to shorter runs would be a pointless loop.
    //
    // Each rung is asserted not to go *down*, and the whole ladder to go up. Not every
    // rung goes up on its own, and the reason is worth knowing: the search already
    // prefers boards with fewer swap rectangles, which already pushes it towards shorter
    // runs, so a limit of nine and a limit of five can land on much the same boards. The
    // claim that matters is the one the escalation uses, which is about the ends.
    let mut previous = 0_u32;
    let mut first = None;
    for longest_run in [9_usize, 5, 4, 3, 2] {
        let asked = Weave {
            longest_run,
            block_first_per_mille: 350,
            avoid_swaps: true,
        };
        let mut total = 0_u32;
        let mut boards = 0_u32;
        for seed in 0..8_u64 {
            let Ok(built) = weave(9, 9, seed, asked) else {
                continue;
            };
            total += rules_per_unknown_per_mille(&built);
            boards += 1;
        }
        assert!(boards > 0, "longest {longest_run}: nothing built");
        let average = total / boards;
        assert!(
            average >= previous,
            "longest {longest_run} averaged {average} rules per 1000 unknowns, which is \
             fewer than the {previous} the longer runs gave, so shortening the runs is \
             taking rules away and the escalation is pointing the wrong way"
        );
        first = first.or(Some(average));
        previous = average;
    }
    let longest_runs_gave = first.expect("the ladder ran");
    assert!(
        previous > longest_runs_gave,
        "the whole ladder went from {longest_runs_gave} to {previous} rules per 1000 \
         unknowns, so shortening the runs buys nothing and the escalation is a pointless \
         loop"
    );
    // And the end of that ladder is the number the arithmetic predicts: every run two
    // squares long, every square in two runs, so exactly one rule per unknown.
    let all_twos = weave(
        9,
        9,
        0,
        Weave {
            longest_run: 2,
            block_first_per_mille: 350,
            avoid_swaps: true,
        },
    )
    .expect("a board of nothing but runs of two");
    assert_eq!(rules_per_unknown_per_mille(&all_twos), 1000);
}

#[test]
fn a_board_too_narrow_for_a_fair_run_is_refused_with_a_reason() {
    // One square wide cannot hold a fair board: any white square in it is its own across
    // run of one. Answered by arithmetic at the door rather than by an exhausted search,
    // so the caller gets a sentence instead of a timeout.
    for (width, height) in [(1_u16, 9_u16), (9, 1), (1, 1)] {
        assert!(
            matches!(
                weave(width, height, 0, Weave::middling()),
                Err(NotWoven::NoSuchBoard { .. })
            ),
            "{width}x{height} should be refused as having no fair board"
        );
    }
}

#[test]
fn a_run_length_no_board_could_have_is_refused_before_searching() {
    for longest_run in [0_usize, 1, LONGEST_RUN + 1, 100] {
        let asked = Weave {
            longest_run,
            block_first_per_mille: 350,
            avoid_swaps: true,
        };
        assert!(
            matches!(
                weave(7, 7, 0, asked),
                Err(NotWoven::ImpossibleRunLength { .. })
            ),
            "a longest run of {longest_run} should be refused outright"
        );
    }
}

#[test]
fn the_search_has_a_limit_and_it_is_not_reached_in_normal_use() {
    // Every search in this project has a limit. This one checks that the limit is a
    // safety net and not something normal boards are living against, because a builder
    // that routinely ran out of choices would be returning the boards it happened to
    // find first rather than the boards asked for.
    const { assert!(CHOICES > 10_000, "the limit should be generous") };
    for (width, height) in SIZES {
        assert!(
            weave(width, height, 1, Weave::middling()).is_ok(),
            "{width}x{height}: a middling board should be found well inside the limit"
        );
    }
}

#[test]
fn the_density_setting_changes_the_look_and_not_the_promise() {
    // Taste must not be able to break correctness. Whatever the density, the run rules
    // hold; what changes is how black the board is.
    let mut blackest = 0_usize;
    let mut whitest = usize::MAX;
    for block_first_per_mille in [50_u16, 350, 900] {
        let asked = Weave {
            longest_run: 5,
            block_first_per_mille,
            avoid_swaps: true,
        };
        let built = weave(9, 9, 3, asked).expect("a board at every density");
        assert!(
            gives_nothing_away(&built),
            "density {block_first_per_mille} broke the promise"
        );
        let blacks = built.square_count() - built.white_count();
        blackest = blackest.max(blacks);
        whitest = whitest.min(blacks);
    }
    assert!(
        blackest > whitest,
        "the density setting changed nothing, so it is not reaching the search"
    );
}

#[test]
fn shortening_the_runs_stops_rather_than_going_below_a_fair_run() {
    // The escalation ladder has to end. Each step shortens the longest run by one, and
    // the step below two squares does not exist, because a run of one is the thing this
    // whole module is for.
    let mut asked = Weave::middling();
    let mut steps = 0;
    while let Some(shorter) = asked.with_shorter_runs() {
        assert!(shorter.longest_run < asked.longest_run);
        assert!(shorter.longest_run >= SHORTEST_FAIR_RUN);
        asked = shorter;
        steps += 1;
        assert!(steps < 20, "the ladder is not terminating");
    }
    assert_eq!(asked.longest_run, SHORTEST_FAIR_RUN);
}

#[test]
fn gives_nothing_away_notices_a_board_that_does() {
    // The anti-vacuity check for this file's main assertion. A property every board
    // passes is not a property, so here is a board that fails it: the middle column is
    // cut so that the square at the top right is alone in its row.
    assert!(gives_nothing_away(&layout(&["...", "...", "..."])));
    // The blocks either side leave one white square alone in its row, which is a run of
    // one square, which is a digit the total names outright.
    let handed_over = layout(&["#.#", "...", "..."]);
    assert!(
        !gives_nothing_away(&handed_over),
        "a board with a run of one square must not pass"
    );
    assert!(given_away(&handed_over) > 0);
}

#[test]
fn a_puzzle_from_the_built_method_gives_nothing_away() {
    // The promise surviving the whole of generation, not just the building. This is the
    // one that matters: the board is built fair, then filled in, then repaired round
    // after round, and the repair is the step that could lose the property.
    let mut kept = 0;
    for (width, height) in [(5_u16, 5_u16), (6, 6), (7, 7)] {
        for seed in 0..6_u64 {
            let Ok(candidate) = propose(seed, Shape::middling(width, height), Budget::default())
            else {
                continue;
            };
            kept += 1;
            assert_eq!(
                given_away_per_mille(&candidate.puzzle.layout),
                0,
                "{width}x{height} seed {seed}: the built method gave squares away"
            );
            assert!(gives_nothing_away(&candidate.puzzle.layout));
            assert_eq!(
                solve(&candidate.puzzle, Budget::default()).solved,
                Solved::One,
                "{width}x{height} seed {seed}: kept, so it must have one answer"
            );
        }
    }
    assert!(kept >= 6, "only {kept} puzzles made, which proves little");
}

#[test]
fn the_cap_on_giving_squares_away_is_zero_for_the_built_method() {
    // Not a tightened guess. Zero is the property the construction has, so the cap is
    // the cheapest possible check that the construction and the repair both still do
    // what they say. The older method keeps the looser cap it was measured against.
    assert_eq!(Shape::middling(7, 7).max_given_away_per_mille, 0);
    assert!(Shape::random_pattern(7, 7).max_given_away_per_mille > 0);
    assert!(matches!(
        Shape::middling(7, 7).layout_from,
        LayoutFrom::Construction(_)
    ));
    assert!(matches!(
        Shape::random_pattern(7, 7).layout_from,
        LayoutFrom::RandomPattern
    ));
}

#[test]
fn building_the_board_is_what_takes_the_giveaway_to_nothing() {
    // The headline measurement of this change, kept as a test for the same reason
    // `generation.rs::repair_is_what_makes_this_work` is kept: the gap between the two
    // methods is the whole reason the extra machinery exists, and a number in a document
    // quietly stops being true.
    //
    // Asserts the gap is large rather than a particular pair of numbers, so an
    // improvement to either side does not fail the build.
    let seeds = 0..12_u64;
    let budget = Budget::default();

    let mut random_given_away = 0_u32;
    let mut random_kept = 0_u32;
    for seed in seeds.clone() {
        if let Ok(candidate) = propose(seed, Shape::random_pattern(6, 6), budget) {
            random_kept += 1;
            random_given_away += given_away_per_mille(&candidate.puzzle.layout);
        }
    }
    assert!(random_kept > 0, "the older method made nothing to compare");
    let random_average = random_given_away / random_kept;

    let mut built_kept = 0_u32;
    for seed in seeds {
        if let Ok(candidate) = propose(seed, Shape::middling(6, 6), budget) {
            built_kept += 1;
            assert_eq!(given_away_per_mille(&candidate.puzzle.layout), 0);
        }
    }

    assert!(
        built_kept > 0,
        "the built method made nothing, so it is not a method"
    );
    assert!(
        random_average > 100,
        "the older method gave away {random_average} per 1000, which is low enough that \
         this whole change would not be worth its code"
    );
}
