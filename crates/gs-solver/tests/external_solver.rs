//! What a solver runner must never do.
//!
//! Almost every test here is about a *failure*, because the failures are where the
//! danger is. The one answer this crate must never invent is `unsat`: it is how a
//! puzzle is declared to have exactly one answer, so a runner that produced it from a
//! missing program, a timeout, or output it could not read would turn a broken machine
//! into a proof, and the puzzles it waved through would look, in the stored record,
//! exactly like ones a solver had examined.
//!
//! The solver used here is `fake-solver`, a program in this crate that solves nothing
//! and does whatever the query file tells it to. Using a real solver would make these
//! tests depend on one being installed, and the failures being checked — a program
//! that will not start, a hang, contradictory output — are ones a real solver cannot
//! be asked to produce on demand.

use std::time::Duration;

use gs_core::verification::{Outcome, Verifier};
use gs_solver::{ExternalSolver, NoVerdict, TimeLimit};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-solver");

fn solver() -> ExternalSolver {
    ExternalSolver::new(
        FAKE,
        "fake-solver",
        TimeLimit::seconds(30).expect("non-zero"),
    )
}

/// A query that tells the fake solver what to do. The directive is a comment, so the
/// text is still a legal SMT-LIB2 fragment.
fn query(mode: &str) -> String {
    format!("; FAKE: {mode}\n(check-sat)\n")
}

#[test]
fn the_three_verdicts_come_back_as_themselves() {
    // The control. Without it every refusal below could pass because nothing is ever
    // decided.
    let s = solver();
    assert_eq!(s.ask(&query("sat")), Outcome::Sat);
    assert_eq!(s.ask(&query("unsat")), Outcome::Unsat);
    assert_eq!(s.ask(&query("unknown")), Outcome::Unknown);
}

#[test]
fn a_solver_that_says_unknown_is_recorded_as_having_said_so() {
    // `Unknown` has two very different causes — the solver decided it could not
    // decide, or something here went wrong — and an operator needs to tell them apart.
    let answer = solver().ask_explained(&query("unknown"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    assert_eq!(answer.why_not, Some(NoVerdict::SolverSaidUnknown));
}

#[test]
fn the_word_unsat_inside_a_sentence_is_not_a_refutation() {
    // `unsat` contains `sat`, and a verdict is a whole line. This is the single
    // mistake in this crate that would invert the platform's central claim while
    // looking like working code.
    let answer = solver().ask_explained(&query("prose"));
    assert_eq!(
        answer.outcome,
        Outcome::Unknown,
        "a verdict found inside prose is not a verdict"
    );
    assert!(matches!(answer.why_not, Some(NoVerdict::NoAnswer { .. })));
}

#[test]
fn two_different_verdicts_are_no_verdict() {
    // Taking the first or the last would be choosing one of two answers at random.
    let answer = solver().ask_explained(&query("contradictory"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    assert_eq!(answer.why_not, Some(NoVerdict::ContradictoryAnswers));
}

#[test]
fn the_same_verdict_twice_is_still_that_verdict() {
    // Some solvers echo their answer. That is not a contradiction, and treating it as
    // one would reject perfectly good results.
    assert_eq!(solver().ask(&query("repeat")), Outcome::Unsat);
}

#[test]
fn output_with_no_verdict_in_it_is_unknown_and_says_what_the_solver_printed() {
    let answer = solver().ask_explained(&query("chatty"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    match answer.why_not {
        Some(NoVerdict::NoAnswer { status, .. }) => {
            assert!(
                status.contains("exit status 0"),
                "the exit status belongs in the record, got {status}"
            );
        }
        other => panic!("expected NoAnswer, got {other:?}"),
    }
}

#[test]
fn a_failing_solver_is_unknown_and_its_complaint_is_kept() {
    let answer = solver().ask_explained(&query("stderr"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    match answer.why_not {
        Some(NoVerdict::NoAnswer { stderr, .. }) => assert!(
            stderr.contains("line 7"),
            "what the solver complained about is the only clue an operator has, got {stderr:?}"
        ),
        other => panic!("expected NoAnswer, got {other:?}"),
    }
}

#[test]
fn a_verdict_still_counts_when_the_solver_exits_non_zero() {
    // Real solvers exit non-zero for reasons unrelated to the answer. Throwing away a
    // verdict the solver printed would make the pipeline fail on things that worked.
    assert_eq!(solver().ask(&query("unsat-then-fail")), Outcome::Unsat);
}

#[test]
fn a_solver_that_will_not_start_is_unknown_and_never_unsat() {
    // The most likely cause of a silent pipeline: the solver is not installed. If this
    // returned `Unsat`, an unconfigured machine would publish puzzles nothing had
    // checked.
    let missing = ExternalSolver::new(
        "gatedspark-no-such-solver-exists",
        "missing",
        TimeLimit::seconds(5).expect("non-zero"),
    );
    let answer = missing.ask_explained(&query("sat"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    assert!(matches!(answer.why_not, Some(NoVerdict::CouldNotStart(_))));
    assert!(!missing.runnable());
}

#[test]
fn a_solver_that_never_finishes_is_killed_and_reported_as_a_timeout() {
    let limit = TimeLimit::of(Duration::from_millis(250)).expect("non-zero");
    let s = ExternalSolver::new(FAKE, "fake-solver", limit);
    let answer = s.ask_explained(&query("hang"));
    assert_eq!(answer.outcome, Outcome::Unknown);
    assert_eq!(
        answer.why_not,
        Some(NoVerdict::TimedOut(Duration::from_millis(250)))
    );
    // The limit is real, not advisory. A generous ceiling, because a loaded CI machine
    // is slow, but far below "it ran to completion".
    assert!(
        answer.elapsed < Duration::from_secs(20),
        "the solver should have been killed near its limit, took {:?}",
        answer.elapsed
    );
}

#[test]
fn a_very_talkative_solver_does_not_deadlock() {
    // The reason output goes to a file rather than a pipe. A child writing to a pipe
    // nobody is reading stops when the pipe fills, and because this runner polls for
    // its time limit rather than blocking on output, that deadlock would be diagnosed
    // as a timeout and never found. 20,000 lines is far more than any pipe buffer.
    let s = ExternalSolver::new(
        FAKE,
        "fake-solver",
        TimeLimit::seconds(30).expect("non-zero"),
    );
    let answer = s.ask_explained(&query("noisy"));
    assert_eq!(
        answer.outcome,
        Outcome::Unsat,
        "a verdict after a lot of output must still arrive, not time out"
    );
}

#[test]
fn a_time_limit_of_zero_cannot_be_built() {
    // A zero limit would make every query `Unknown` while looking like a setting
    // rather than a mistake, so there is no way to spell it.
    assert!(TimeLimit::of(Duration::ZERO).is_none());
    assert!(TimeLimit::seconds(0).is_none());
    assert!(TimeLimit::seconds(1).is_some());
}

#[test]
fn a_runnable_solver_says_so() {
    assert!(solver().runnable());
}

#[test]
fn the_name_in_the_report_is_the_name_it_was_given() {
    // The validation report records which solver answered, and a report that named the
    // wrong one would be worse than a report that named none.
    assert_eq!(Verifier::name(&solver()), "fake-solver");
}

#[test]
fn every_failure_in_this_crate_is_unknown_and_none_is_unsat() {
    // The crate's whole rule, asserted in one place over every failure mode it has, so
    // a new mode added without thinking about this shows up here.
    for mode in ["prose", "contradictory", "chatty", "stderr", "hang"] {
        let limit = TimeLimit::of(Duration::from_millis(250)).expect("non-zero");
        let s = ExternalSolver::new(FAKE, "fake-solver", limit);
        let answer = s.ask_explained(&query(mode));
        assert_eq!(
            answer.outcome,
            Outcome::Unknown,
            "mode {mode} must be Unknown"
        );
        assert!(
            answer.why_not.is_some(),
            "mode {mode} must record why there was no verdict"
        );
    }

    let missing = ExternalSolver::new(
        "gatedspark-no-such-solver",
        "missing",
        TimeLimit::seconds(2).expect("non-zero"),
    );
    assert_eq!(missing.ask(&query("sat")), Outcome::Unknown);
}

#[test]
fn the_scratch_directory_can_be_chosen_and_is_left_clean() {
    // A pool-filling run makes two files per query. If they were not removed, a long
    // run would fill the disk, and the failure would appear as every query suddenly
    // becoming `Unknown`.
    let dir = std::env::temp_dir().join(format!("gatedspark-scratch-{}", std::process::id()));
    let s = ExternalSolver::new(
        FAKE,
        "fake-solver",
        TimeLimit::seconds(30).expect("non-zero"),
    )
    .with_scratch_dir(&dir);
    assert_eq!(s.ask(&query("sat")), Outcome::Sat);
    assert_eq!(s.ask(&query("unsat")), Outcome::Unsat);

    let left: Vec<_> = std::fs::read_dir(&dir)
        .expect("the directory was created")
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert!(
        left.is_empty(),
        "the query and output files must be removed, found {left:?}"
    );
    let _ = std::fs::remove_dir(&dir);
}
