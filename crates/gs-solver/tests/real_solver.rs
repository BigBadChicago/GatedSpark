//! The runner against a real solver.
//!
//! Every other test in this crate uses `fake-solver`, which proves the runner handles
//! a program that behaves in a particular way. It cannot prove that a *real* solver
//! behaves in any of those ways. In particular, the thing most worth knowing is what
//! this runner does with the error output a real solver produces for a query it cannot
//! read — because that output contains words like `sat`, and reading it as a verdict is
//! the failure that would quietly invert the platform's central claim.
//!
//! # This test skips itself when no solver is installed, and says so
//!
//! With `GATEDSPARK_REQUIRE_SOLVER=1` a missing solver is a failure instead. CI sets
//! it, which is the whole point: a test that skips when its dependency is absent
//! reports green while checking nothing. Rust hides a passing test's output, so the
//! notice below is invisible unless the run uses `--nocapture`.

use std::time::Duration;

use gs_core::verification::{Outcome, Verifier};
use gs_solver::{ExternalSolver, NoVerdict, TimeLimit};

/// The solver to use, or why there is none.
fn solver() -> Result<ExternalSolver, String> {
    let program = std::env::var("GATEDSPARK_SMT_SOLVER").unwrap_or_else(|_| "z3".to_string());
    let s = ExternalSolver::new(
        &program,
        "a real solver",
        TimeLimit::seconds(30).expect("non-zero"),
    );
    if s.runnable() {
        return Ok(s);
    }
    let message = format!("no SMT solver found (tried {program}; set GATEDSPARK_SMT_SOLVER)");
    if std::env::var("GATEDSPARK_REQUIRE_SOLVER").is_ok_and(|v| v == "1") {
        panic!("GATEDSPARK_REQUIRE_SOLVER=1 but {message}");
    }
    Err(message)
}

/// Run `body` with a solver, or print a notice naming what was not checked.
fn with_solver(what: &str, body: impl FnOnce(&ExternalSolver)) {
    match solver() {
        Ok(s) => body(&s),
        Err(why) => println!(
            "\n*** NOT VERIFIED AGAINST A REAL SOLVER: {why}.\n\
             *** Unchecked: {what}\n"
        ),
    }
}

#[test]
fn a_real_solver_says_sat_and_unsat_and_this_runner_reads_both() {
    with_solver("that a real solver's verdicts are read correctly", |s| {
        assert_eq!(
            s.ask("(declare-const a Bool)\n(assert a)\n(check-sat)\n"),
            Outcome::Sat
        );
        assert_eq!(
            s.ask("(declare-const a Bool)\n(assert a)\n(assert (not a))\n(check-sat)\n"),
            Outcome::Unsat
        );
    });
}

#[test]
fn a_real_solvers_error_output_is_never_mistaken_for_a_verdict() {
    // The test this file exists for. Z3's reply to the query below, checked by hand
    // against Z3 4.8.12, is printed on **stdout** rather than stderr and reads:
    //
    //     unsupported
    //     (error "line 2 column 19: invalid check-sat-assuming command, ...")
    //
    // The echoed command name contains the text `sat`. So a runner that searched its
    // output for a substring would read a query no solver could parse as a verdict,
    // and which verdict it read would depend on the wording of an error message. That
    // is the failure this crate's whole-line parsing exists to prevent, and this is
    // the only test that shows a real solver really does produce such output.
    with_solver(
        "that a real solver's error output is not read as a verdict",
        |s| {
            let answer = s.ask_explained("(this is not SMT-LIB2 at all)\n(check-sat-assuming)\n");
            assert_eq!(
                answer.outcome,
                Outcome::Unknown,
                "an unreadable query must produce no verdict, got {answer:?}"
            );
            assert!(
                matches!(answer.why_not, Some(NoVerdict::NoAnswer { .. })),
                "expected NoAnswer, got {:?}",
                answer.why_not
            );
        },
    );
}

#[test]
fn an_empty_query_gets_no_verdict_rather_than_a_default() {
    // Nothing was asked, so nothing was answered. A runner with a default would be
    // answering a question nobody put.
    with_solver("that an empty query produces no verdict", |s| {
        assert_eq!(s.ask(""), Outcome::Unknown);
    });
}

#[test]
fn a_real_solver_that_says_unknown_is_recorded_as_having_said_it() {
    // A genuine `unknown` from a real solver, told apart from this runner giving up.
    // The two are easy to conflate and an operator needs the difference: a solver that
    // reported it could not decide is working correctly, while a solver this runner
    // killed may just need a longer limit.
    //
    // Produced by giving the solver its own short internal limit on a problem outside
    // a decidable fragment, so it stops and says so. Our own limit is left generous,
    // because a timeout here would be the other case and would prove nothing.
    with_solver("that a real solver's own `unknown` is distinguished", |s| {
        let answer = s.ask_explained(
            "(set-option :timeout 200)\n\
             (set-logic QF_NIA)\n\
             (declare-const x Int)\n(declare-const y Int)\n(declare-const z Int)\n\
             (assert (> x 1))(assert (> y 1))(assert (> z 1))\n\
             (assert (= (+ (* x x x) (* y y y)) (* z z z)))\n(check-sat)\n",
        );
        match answer.outcome {
            Outcome::Unknown => assert_eq!(
                answer.why_not,
                Some(NoVerdict::SolverSaidUnknown),
                "the solver was given its own short limit, so it should have reported \
                 `unknown` itself rather than being killed by this runner"
            ),
            // A solver that genuinely decides it is welcome to; the point of the test
            // is that nothing here invents an answer. `:timeout` is also a Z3 spelling,
            // so another solver may ignore it and decide the problem.
            Outcome::Unsat | Outcome::Sat => {
                assert_eq!(
                    answer.why_not, None,
                    "a decided answer carries no reason for not deciding"
                );
            }
        }
        assert!(
            answer.elapsed < Duration::from_secs(20),
            "this query is meant to come back quickly, took {:?}",
            answer.elapsed
        );
    });
}

#[test]
fn a_real_solver_is_killed_when_it_runs_past_its_limit() {
    // Deliberately a problem no solver finishes quickly, with a short limit. The
    // assertion is only that the runner stopped waiting and said so: a solver that
    // somehow answers is not a failure of this runner, so that case is allowed.
    with_solver(
        "that a real solver is actually killed at its time limit",
        |s| {
            let limit = TimeLimit::of(Duration::from_millis(400)).expect("non-zero");
            let program =
                std::env::var("GATEDSPARK_SMT_SOLVER").unwrap_or_else(|_| "z3".to_string());
            let impatient = ExternalSolver::new(&program, "a real solver", limit);
            let _ = s;

            // Factoring a large semiprime over the integers. Hard for an SMT solver.
            let answer = impatient.ask_explained(
                "(set-logic QF_NIA)\n\
             (declare-const p Int)\n(declare-const q Int)\n\
             (assert (> p 1))(assert (> q 1))\n\
             (assert (= (* p q) 1000000000000000000000000000057))\n(check-sat)\n",
            );
            if answer.outcome == Outcome::Unknown {
                assert!(
                    matches!(
                        answer.why_not,
                        Some(NoVerdict::TimedOut(_) | NoVerdict::SolverSaidUnknown)
                    ),
                    "expected a timeout or the solver's own unknown, got {:?}",
                    answer.why_not
                );
            }
            assert!(
                answer.elapsed < Duration::from_secs(20),
                "the limit must bound the wait, took {:?}",
                answer.elapsed
            );
        },
    );
}
