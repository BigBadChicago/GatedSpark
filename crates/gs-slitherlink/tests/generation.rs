//! Making puzzles: what comes out, and what the generator refuses to do.

mod common;

use gs_core::ruleset::Ruleset;
use gs_slitherlink::clues::Clues;
use gs_slitherlink::generate::{batch, grow_loop, propose, Rejected, Shape, REMOVAL_BUDGET};
use gs_slitherlink::loops::Loop;
use gs_slitherlink::solve::{solve, Budget, Solved};

#[test]
fn a_grown_shape_is_always_one_loop() {
    // The type carries this, since `grow_loop` can only return a `Loop` and `Loop`
    // can only be built by the checker. The test is that it returns one at all, over
    // many seeds and sizes: a generator that silently produced nothing would leave
    // the pipeline looking fine and empty.
    for (w, h) in [(2u16, 2u16), (4, 4), (7, 5), (10, 10)] {
        for seed in 0..12u64 {
            let shape = Shape::middling(w, h);
            let grown = grow_loop(seed, shape);
            assert!(
                grown.is_some(),
                "a {w}x{h} board grew nothing from seed {seed}"
            );
        }
    }
}

#[test]
fn a_bigger_enclosed_share_grows_a_bigger_loop() {
    // Not a claim about any single seed — a long thin loop round few cells can be
    // longer than a fat one round many. Over a batch the trend has to hold, or the
    // dial does nothing and the shape argument is decoration.
    let total = |per_mille: u32| -> u32 {
        (0..16u64)
            .filter_map(|seed| {
                grow_loop(
                    seed,
                    Shape {
                        width: 8,
                        height: 8,
                        enclosed_per_mille: per_mille,
                    },
                )
            })
            .map(|l| l.edges_drawn())
            .sum()
    };
    assert!(
        total(600) > total(100),
        "asking for more enclosed cells must grow longer loops"
    );
}

#[test]
fn a_proposed_puzzle_has_exactly_one_answer_and_is_finishable() {
    for seed in 0..6u64 {
        let shape = Shape::middling(5, 5);
        let candidate = propose(seed, shape, REMOVAL_BUDGET).expect("a puzzle");
        let found = solve(&candidate.puzzle, Budget::default());
        assert_eq!(found.solved, Solved::One, "seed {seed}");
        let witness = found.witness.expect("an answer");
        assert!(
            candidate.puzzle.is_solved(&witness),
            "seed {seed}: the engine agrees the answer finishes it"
        );
    }
}

#[test]
fn numbers_really_do_come_off() {
    // The whole point of the removal loop. A puzzle that kept every number would be
    // correct and not worth playing.
    let candidate = propose(3, Shape::middling(6, 6), REMOVAL_BUDGET).expect("a puzzle");
    let cells = 36;
    assert!(candidate.clues_removed > 0, "nothing was removed");
    assert_eq!(
        candidate.clues_removed + candidate.puzzle.clues.given_count(),
        cells,
        "every cell either kept its number or lost it"
    );
    assert!(
        candidate.puzzle.clues.given_count() < cells / 2,
        "a 6 by 6 should end up under half numbered, and kept {}",
        candidate.puzzle.clues.given_count()
    );
}

#[test]
fn the_cost_is_recorded_and_is_about_one_question_per_cell() {
    // The number that decides how fast a pool can be filled, so it is carried in the
    // candidate and into the validation report rather than left to be guessed at.
    let candidate = propose(1, Shape::middling(5, 5), REMOVAL_BUDGET).expect("a puzzle");
    assert_eq!(
        candidate.questions_asked, 26,
        "one question for the full board and one per cell"
    );
}

#[test]
fn the_same_seed_gives_the_same_puzzle() {
    // A validation report records a seed, so the seed has to be enough to rebuild the
    // puzzle. Otherwise the report describes a puzzle nobody can reproduce.
    let shape = Shape::middling(6, 6);
    for seed in [0u64, 7, 99] {
        let once = propose(seed, shape, REMOVAL_BUDGET).expect("a puzzle");
        let twice = propose(seed, shape, REMOVAL_BUDGET).expect("a puzzle");
        assert_eq!(once, twice, "seed {seed}");
    }
}

#[test]
fn different_seeds_give_different_puzzles() {
    // The anti-vacuity half: a generator ignoring its seed would pass the test above
    // perfectly.
    let shape = Shape::middling(6, 6);
    let first = propose(0, shape, REMOVAL_BUDGET).expect("a puzzle");
    let second = propose(1, shape, REMOVAL_BUDGET).expect("a puzzle");
    assert_ne!(first.puzzle, second.puzzle);
}

#[test]
fn a_candidate_does_not_carry_the_loop_it_came_from() {
    // The loop is thrown away on purpose: a package carrying it would ship the
    // answer, and a candidate remembering it invites every later step to compare
    // against it instead of checking the numbers.
    //
    // Checked by what the type can hold rather than by poking at it: a candidate is
    // its puzzle, its seed, its shape and some measurements. The puzzle is its clue
    // grid, and a clue grid has no loop in it.
    let candidate = propose(2, Shape::middling(4, 4), REMOVAL_BUDGET).expect("a puzzle");
    let bytes = candidate.puzzle.clues.canonical_bytes();
    let read_back = Clues::from_canonical_bytes(&bytes).expect("round trip");
    assert_eq!(
        read_back, candidate.puzzle.clues,
        "a candidate is exactly its clue grid, and nothing else is carried"
    );
}

#[test]
fn an_impossible_board_size_is_refused_rather_than_fudged() {
    let shape = Shape {
        width: 0,
        height: 4,
        enclosed_per_mille: 400,
    };
    assert!(matches!(
        propose(0, shape, REMOVAL_BUDGET),
        Err(Rejected::Size(_))
    ));
}

#[test]
fn a_batch_report_adds_up() {
    let (kept, report) = batch(0..8, Shape::middling(5, 5), REMOVAL_BUDGET);
    assert!(report.is_consistent(), "{report:?}");
    assert_eq!(report.tried, 8);
    assert_eq!(u32::try_from(kept.len()), Ok(report.kept));
    assert!(report.kept > 0, "nothing at all came out of eight seeds");
    assert_eq!(report.kept_per_mille(), report.kept * 1000 / 8);
    assert!(
        report.questions_asked >= report.kept * 26,
        "the cost of each kept puzzle is counted into the batch"
    );
}

#[test]
fn a_tighter_cap_removes_less_and_never_removes_something_unsafe() {
    // The rule that makes a cap safe, and the measurement that makes it worth having.
    //
    // A tiny cap does not mean nothing can be removed. Where the forced deductions
    // alone settle the board, uniqueness is proved with no guessing at all and the
    // number comes off legitimately — which is exactly what should happen. What must
    // never happen is a number coming off because the question was not answered.
    //
    // So this checks both halves: a tighter cap removes no more than a generous one,
    // and whatever it does remove leaves a puzzle that still has exactly one answer
    // when checked generously.
    let shape = Shape::middling(5, 5);
    let tiny = Budget {
        max_nodes: 1,
        max_depth: 1,
    };
    let mut any_removed_under_the_tiny_cap = false;
    for seed in 0..5u64 {
        let capped = propose(seed, shape, tiny);
        let normal = propose(seed, shape, REMOVAL_BUDGET).expect("a puzzle");
        let Ok(capped) = capped else {
            // Refusing outright is a legitimate outcome of a cap this tight.
            continue;
        };
        assert!(
            capped.clues_removed <= normal.clues_removed,
            "seed {seed}: a tighter cap removed more ({} vs {})",
            capped.clues_removed,
            normal.clues_removed
        );
        any_removed_under_the_tiny_cap |= capped.clues_removed > 0;
        assert_eq!(
            solve(&capped.puzzle, Budget::default()).solved,
            Solved::One,
            "seed {seed}: a capped run still must leave exactly one answer"
        );
    }
    assert!(
        any_removed_under_the_tiny_cap,
        "if a one-node cap removed nothing anywhere, this test is not checking the \
         interesting case"
    );
}

#[test]
fn every_removal_keeps_the_puzzle_answerable_by_the_same_loop() {
    // Removing numbers must never change what the answer is, only how much help the
    // player gets finding it. So the loop the generator drew is still an answer to
    // the finished puzzle — which is also why the witness can be found by solving
    // rather than remembered.
    let shape = Shape::middling(5, 5);
    for seed in 0..5u64 {
        let drawn: Loop = grow_loop(seed, shape).expect("a loop");
        let candidate = propose(seed, shape, REMOVAL_BUDGET).expect("a puzzle");
        assert!(
            candidate.puzzle.is_solved(&drawn.as_state()),
            "seed {seed}: the loop it was built from must still finish it"
        );
    }
}
