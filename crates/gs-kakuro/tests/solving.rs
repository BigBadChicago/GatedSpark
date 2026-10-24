//! Our own fast solver: what it decides, and what it refuses to decide.
//!
//! This solver is a generation filter and the source of the difficulty numbers. It
//! proves nothing, so none of these tests is evidence that a puzzle may be published;
//! `tests/solver_agreement.rs` is where a real solver is asked.

mod common;

use common::{from_answer, layout};
use gs_kakuro::layout::WhiteId;
use gs_kakuro::rules::Kakuro;
use gs_kakuro::solve::{fill, solve, Budget, Solved};
use gs_kakuro::totals::Totals;

#[test]
fn a_pinned_board_has_exactly_one_answer() {
    // A single row. Each square's own down run of one names its digit outright, so
    // there is nothing to search and propagation alone should finish it.
    let (puzzle, answer) = from_answer(&["..."], &[4, 7, 2]);
    let found = solve(&puzzle, Budget::default());
    assert_eq!(found.solved, Solved::One);
    assert_eq!(found.witness.as_ref(), Some(&answer));
    assert!(
        found.features.needs_no_guessing(),
        "a board where every square is named needs no guess: {:?}",
        found.features
    );
    assert_eq!(
        found.features.propagation_only_squares, 3,
        "all three settled before any guess"
    );
}

#[test]
fn a_loose_board_has_several_answers() {
    // The 2 by 2 whose totals two different boards reach, from the agreement tests.
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    assert_eq!(solve(&puzzle, Budget::default()).solved, Solved::Several);
}

#[test]
fn a_tight_board_has_exactly_one_answer() {
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    let found = solve(&puzzle, Budget::default());
    assert_eq!(found.solved, Solved::One);
    assert_eq!(found.witness.as_ref(), Some(&answer));
}

#[test]
fn an_impossible_board_has_no_answer() {
    // The across run of two totals 3, forcing 1 and 2. The down run over the first
    // square alone totals 9, forcing a 9 there. Both cannot hold.
    let layout = layout(&[".."]);
    let mut numbers = vec![0_u16; layout.run_count()];
    for (id, run) in layout.runs() {
        numbers[usize::try_from(id.0).expect("fits")] = if run.len() == 2 { 3 } else { 9 };
    }
    let totals = Totals::new(&layout, numbers).expect("each is reachable alone");
    let puzzle = Kakuro::new(layout, totals).expect("one per run");
    let found = solve(&puzzle, Budget::default());
    assert_eq!(found.solved, Solved::None);
    assert_eq!(found.witness, None, "and it offers no witness");
}

#[test]
fn a_run_of_nine_is_solved_without_guessing_when_its_columns_name_the_digits() {
    let (puzzle, answer) = from_answer(&["........."], &[9, 1, 8, 2, 7, 3, 6, 4, 5]);
    let found = solve(&puzzle, Budget::default());
    assert_eq!(found.solved, Solved::One);
    assert_eq!(found.witness.as_ref(), Some(&answer));
    assert_eq!(found.features.longest_run, 9);
    assert_eq!(
        found.features.single_square_runs, 9,
        "one per column, and they are what makes this easy"
    );
}

#[test]
fn an_exhausted_budget_is_undecided_and_never_unique() {
    // The distinction that matters most in this file. A budget of almost nothing must
    // report `Undecided`, never `One` and never `None`, because an exhausted budget is
    // not an answer and a filter that read it as uniqueness would publish an
    // ambiguous puzzle.
    let (puzzle, _) = from_answer(
        &["....", "....", "....", "...."],
        &[1, 2, 3, 4, 5, 6, 7, 8, 2, 1, 4, 3, 6, 5, 8, 7],
    );
    let found = solve(
        &puzzle,
        Budget {
            max_nodes: 1,
            max_depth: 1,
        },
    );
    assert_eq!(found.solved, Solved::Undecided);
}

#[test]
fn the_feature_vector_describes_the_board_it_was_measured_on() {
    let (puzzle, _) = from_answer(&["..#", "...", "#.."], &[1, 2, 3, 4, 5, 6, 7]);
    let found = solve(&puzzle, Budget::default());
    assert_eq!(found.features.white_squares, 7);
    assert_eq!(
        found.features.runs,
        u32::try_from(puzzle.layout.run_count()).expect("fits")
    );
    assert!(found.features.longest_run >= 2);
}

#[test]
fn filling_a_layout_breaks_no_no_repeats_rule() {
    // The first step of making a puzzle. There are no totals yet, so the only rule is
    // that no run repeats a digit.
    for seed in 0..20_u64 {
        let layout = layout(&["..#..", ".....", "..#..", "....."]);
        let digits = fill(&layout, seed, Budget::default())
            .unwrap_or_else(|| panic!("seed {seed}: nine colours always suffice here"));
        assert_eq!(digits.len(), layout.white_count());
        for digit in &digits {
            assert!(
                (1..=9).contains(digit),
                "seed {seed}: {digit} is not a digit"
            );
        }
        for (id, run) in layout.runs() {
            let mut seen = Vec::new();
            for white in &run.squares {
                let d = digits[usize::try_from(white.0).expect("fits")];
                assert!(
                    !seen.contains(&d),
                    "seed {seed}: run {id:?} holds {d} twice"
                );
                seen.push(d);
            }
        }
    }
}

#[test]
fn filling_is_the_same_every_time_for_one_seed() {
    // Determinism, which the whole platform rests on: a validation report records a
    // seed, and a seed that produced a different board tomorrow would make the report
    // a record of nothing.
    let layout = layout(&["...", "...", "..."]);
    let once = fill(&layout, 7, Budget::default()).expect("fillable");
    let twice = fill(&layout, 7, Budget::default()).expect("fillable");
    assert_eq!(once, twice);
}

#[test]
fn different_seeds_give_different_boards() {
    let layout = layout(&["...", "...", "..."]);
    let boards: Vec<Vec<i16>> = (0..8)
        .filter_map(|seed| fill(&layout, seed, Budget::default()))
        .collect();
    assert_eq!(boards.len(), 8);
    let distinct = boards
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(
        distinct > 1,
        "eight seeds all produced the same board, so the seed is doing nothing"
    );
}

#[test]
fn a_filled_layout_and_its_totals_make_a_board_our_rules_accept() {
    // The round trip the generator depends on: fill a layout, read the totals off it,
    // and the board you started from must be an answer to the puzzle you end up with.
    let layout = layout(&["..#.", "....", ".#..", "...."]);
    for seed in 0..10_u64 {
        let digits = fill(&layout, seed, Budget::default()).expect("fillable");
        let totals = Totals::of(&layout, &|white: WhiteId| {
            digits.get(usize::try_from(white.0).ok()?).copied()
        })
        .expect("a full board implies totals");
        let puzzle = Kakuro::new(layout.clone(), totals).expect("one per run");
        let found = solve(&puzzle, Budget::default());
        assert_ne!(
            found.solved,
            Solved::None,
            "seed {seed}: the board it was built from is an answer, so there is one"
        );
    }
}
