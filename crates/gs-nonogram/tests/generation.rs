//! What the generator must and must not do.

use gs_core::state::Dims;
use gs_nonogram::clues::{runs_of, Clues};
use gs_nonogram::generate::{batch, pattern, propose, Rejected, Report, Shape};
use gs_nonogram::solve::{solve, Budget, Solved};

fn dims(w: u16, h: u16) -> Dims {
    Dims {
        width: w,
        height: h,
    }
}

#[test]
fn a_seed_rebuilds_the_same_pattern() {
    let shape = Shape::blobby(dims(8, 8));
    let a = pattern(4242, shape);
    let b = pattern(4242, shape);
    assert_eq!(a, b, "a recorded seed has to rebuild its board");
}

#[test]
fn a_different_seed_gives_a_different_pattern() {
    let shape = Shape::blobby(dims(8, 8));
    assert_ne!(pattern(1, shape), pattern(2, shape));
}

#[test]
fn smoothing_is_not_order_dependent() {
    // The automaton is double buffered, so one round is a pure function of the
    // previous grid. If it ever updated in place, this is where it would show: the
    // result would depend on the scan order and would not match a second run.
    let shape = Shape {
        dims: dims(9, 7),
        fill_per_mille: 500,
        smoothing_rounds: 1,
    };
    let once = pattern(77, shape);
    let again = pattern(77, shape);
    assert_eq!(once, again);
}

#[test]
fn a_candidates_clues_describe_its_own_board() {
    // The clues are derived from a pattern and then the pattern is thrown away. The
    // thing that must hold is that the clues, read on their own, are satisfied by
    // the witness the solver found, with no reference to the original board.
    let shape = Shape::blobby(dims(7, 7));
    let candidate = (0..400u64)
        .find_map(|seed| propose(seed, shape, Budget::default()).ok())
        .expect("400 seeds produced no candidate at all, which is a bug not bad luck");

    let analysis = solve(&candidate.puzzle, Budget::default());
    assert_eq!(analysis.solved, Solved::One);
    let witness = analysis.witness.expect("a unique answer has a witness");

    let derived = Clues::of(
        &gs_nonogram::clues::Pattern::new(
            witness.dims(),
            (0..u32::try_from(witness.dims().cell_count()).unwrap())
                .map(|i| witness.get(i) == Ok(gs_nonogram::rules::FILLED))
                .collect(),
        )
        .unwrap(),
    );
    assert_eq!(
        derived, candidate.puzzle.clues,
        "the answer the solver found does not produce the clues it was given"
    );
}

#[test]
fn nothing_accepted_is_ambiguous_or_undecided() {
    // The whole purpose of the filter. A candidate whose clues have two answers, or
    // whose question ran out of budget, must never come back as a candidate.
    let shape = Shape::blobby(dims(6, 6));
    let (kept, report) = batch(0..250, shape, Budget::default());
    assert!(report.is_consistent(), "a seed went missing: {report:?}");
    for candidate in &kept {
        let analysis = solve(&candidate.puzzle, Budget::default());
        assert_eq!(
            analysis.solved,
            Solved::One,
            "seed {} came back as a candidate but is {:?}",
            candidate.seed,
            analysis.solved
        );
    }
}

#[test]
fn a_lopsided_board_is_discarded_before_the_solver_is_asked() {
    // 1 in 1000 filled, no smoothing: effectively blank. It must be rejected for
    // being lopsided rather than reaching the solver at all.
    let shape = Shape {
        dims: dims(6, 6),
        fill_per_mille: 1,
        smoothing_rounds: 0,
    };
    match propose(7, shape, Budget::default()) {
        Err(Rejected::Lopsided { filled_per_mille }) => {
            assert!(filled_per_mille < 200, "got {filled_per_mille} per mille");
        }
        other => panic!("expected a lopsided rejection, got {other:?}"),
    }
}

#[test]
fn an_empty_line_is_the_empty_clue_not_a_zero() {
    // Belongs here because generated boards routinely contain blank lines, and a
    // `[0]` clue would be a different puzzle that no solver would agree with.
    assert_eq!(runs_of(&[false, false, false]), Vec::<u16>::new());
}

#[test]
fn the_same_block_of_seeds_gives_the_same_candidates() {
    let shape = Shape::blobby(dims(6, 6));
    let (first, report_one) = batch(100..180, shape, Budget::default());
    let (second, report_two) = batch(100..180, shape, Budget::default());
    assert_eq!(first, second, "a batch is not reproducible");
    assert_eq!(report_one, report_two);
}

#[test]
fn an_empty_batch_reports_nothing_rather_than_dividing_by_zero() {
    let shape = Shape::blobby(dims(6, 6));
    let (kept, report) = batch(0..0, shape, Budget::default());
    assert!(kept.is_empty());
    assert_eq!(report, Report::default());
    assert_eq!(report.kept_per_mille(), 0);
    assert!(report.is_consistent());
}

/// The measured yield. Printed, not asserted against a stored number.
///
/// Lesson from the last project: no number in a document unless CI regenerates it.
/// So this test prints what a run actually produced and asserts only the two things
/// that are properties rather than measurements — that the counts add up, and that
/// the generator produces *something*. A yield quoted anywhere has to come from this
/// output.
#[test]
fn the_yield_is_measured_and_printed() {
    for (w, h) in [(5u16, 5u16), (7, 7), (10, 10)] {
        let shape = Shape::blobby(dims(w, h));
        let (kept, report) = batch(0..300, shape, Budget::default());
        println!(
            "{w}x{h}: proposed {}, kept {} ({} per mille), lopsided {}, ambiguous {}, \
             no answer {}, undecided {}",
            report.proposed,
            report.kept,
            report.kept_per_mille(),
            report.lopsided,
            report.ambiguous,
            report.no_answer,
            report.undecided,
        );
        let line_solvable = kept.iter().filter(|c| c.features.line_solvable()).count();
        println!("        of those, {line_solvable} need no guessing");
        assert!(report.is_consistent(), "counts do not add up: {report:?}");
        assert_eq!(report.kept as usize, kept.len());
        assert!(
            report.kept > 0,
            "300 seeds at {w}x{h} produced no candidate at all. That is a broken \
             generator, not an unlucky run."
        );
        assert_eq!(
            report.no_answer, 0,
            "clues derived from a real board had no answer, which means derivation is wrong"
        );
    }
}
