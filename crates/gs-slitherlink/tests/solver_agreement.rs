//! Does a real solver agree with us about loops?
//!
//! Every other test in this crate checks our work against our own work.
//! `tests/encoding.rs` checks that the SMT-LIB2 text *looks* right; nothing there
//! would notice if a solver reading it reached the wrong verdict, and a malformed
//! encoding passes all of it.
//!
//! This is the job that closes that gap for Slitherlink, and the headline case is
//! `two_rings_are_refused_by_the_solver`. The whole reason this family is second is
//! that its central rule has no local encoding, so the one thing that must be proved
//! here is that the rule really reaches the solver. If the one-piece assertions were
//! dropped, every local rule would still be satisfied by a board of two separate
//! rings and the solver would answer `sat` — and the puzzle would be published with
//! an "answer" that is not one.
//!
//! # Finding a solver, and why absence is loud
//!
//! The same arrangement as `gs-nonogram`'s job of this name, deliberately repeated
//! rather than shared: this is test scaffolding, and a shared helper crate holding
//! the only code that can run a solver during a build is the sort of thing that ends
//! up in a release.
//!
//! * With `GATEDSPARK_REQUIRE_SOLVER=1` set, a missing solver **fails** the test. CI
//!   sets it and installs a solver, so skipping is impossible there.
//! * Without it, the test says loudly that it verified nothing and returns. That is
//!   for a contributor's laptop, never for CI.
//!
//! # Configuration
//!
//! * `GATEDSPARK_SMT_SOLVER` — path to the primary solver. Defaults to `z3`.
//! * `GATEDSPARK_SMT_SOLVER_2` — an optional second, independent solver, whose
//!   answers must match the first's. That is the ADR 0002 cross-check: two solvers
//!   disagreeing about a uniqueness result is a build failure.

mod common;

use std::process::Command;
use std::time::Duration;

use common::{add_rectangle, board, nothing_drawn, rectangle_loop};
use gs_core::report::NotPublishable;
use gs_core::ruleset::Ruleset;
use gs_core::state::State;
use gs_core::verification::Outcome;
use gs_slitherlink::board::Geometry;
use gs_slitherlink::clues::Clues;
use gs_slitherlink::generate::{batch, Shape, REMOVAL_BUDGET};
use gs_slitherlink::loops::Loop;
use gs_slitherlink::publish::{filter_and_solver_agree, report_for};
use gs_slitherlink::rules::Slitherlink;
use gs_slitherlink::smtlib::{solvability_query, uniqueness_query};
use gs_slitherlink::solve::Budget;
use gs_solver::{ExternalSolver, NoVerdict, TimeLimit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Sat,
    Unsat,
    Unknown,
}

struct Solver {
    name: String,
    runner: ExternalSolver,
}

impl Solver {
    /// Run one query and return the solver's verdict.
    ///
    /// Uses the production runner in `gs-solver` rather than a copy, so the code that
    /// ships is the code this job proves. It panics where the runner does not: the
    /// runner turns every failure into `Unknown`, which is right for an unattended
    /// worker, but here a solver giving no verdict on our own encoding means the
    /// encoding is malformed and must stop the build with the query in the log. The
    /// solver's own `unknown` passes through, because that is a real answer about a
    /// hard query.
    fn ask(&self, query: &str, label: &str) -> Answer {
        let answer = self.runner.ask_explained(query);
        match (answer.outcome, &answer.why_not) {
            (Outcome::Sat, _) => Answer::Sat,
            (Outcome::Unsat, _) => Answer::Unsat,
            (Outcome::Unknown, Some(NoVerdict::SolverSaidUnknown)) => Answer::Unknown,
            (Outcome::Unknown, why) => panic!(
                "{} gave no verdict on {label}: {why:?}\n\
                 This is what a malformed encoding looks like, so the query is below.\n\
                 --- query ---\n{}",
                self.name,
                first_lines(query, 40)
            ),
        }
    }
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

fn runnable(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn solvers() -> Result<Vec<Solver>, String> {
    let required = std::env::var("GATEDSPARK_REQUIRE_SOLVER").is_ok_and(|v| v == "1");
    let primary = std::env::var("GATEDSPARK_SMT_SOLVER").unwrap_or_else(|_| "z3".to_string());

    let mut found = Vec::new();
    for (var, program) in [("GATEDSPARK_SMT_SOLVER", primary.clone())]
        .into_iter()
        .chain(
            std::env::var("GATEDSPARK_SMT_SOLVER_2")
                .ok()
                .map(|p| ("GATEDSPARK_SMT_SOLVER_2", p)),
        )
    {
        if runnable(&program) {
            found.push(Solver {
                name: format!("{program} (via {var})"),
                runner: ExternalSolver::new(
                    &program,
                    "solver under test",
                    TimeLimit::of(Duration::from_secs(120)).expect("non-zero"),
                ),
            });
        } else if var == "GATEDSPARK_SMT_SOLVER_2" {
            // An explicitly configured second solver that cannot run is always an
            // error: dropping it silently would quietly remove the cross-check.
            panic!("GATEDSPARK_SMT_SOLVER_2 is set to {program}, which will not run");
        }
    }

    if found.is_empty() {
        let message =
            format!("no SMT solver found (tried {primary}; set GATEDSPARK_SMT_SOLVER to a path)");
        if required {
            panic!("GATEDSPARK_REQUIRE_SOLVER=1 but {message}");
        }
        return Err(message);
    }
    Ok(found)
}

/// Run `body` with every configured solver, or say loudly that nothing was checked.
fn with_solvers(what: &str, body: impl Fn(&Solver)) {
    match solvers() {
        Ok(solvers) => {
            println!("{what}: {} solver(s)", solvers.len());
            for solver in &solvers {
                println!("  asking {}", solver.name);
                body(solver);
            }
        }
        Err(why) => println!("VERIFIED NOTHING: {why}"),
    }
}

/// The clue grid a board of drawn edges implies, built without going through
/// [`Loop`] so that a board which is *not* a loop can still be numbered.
fn numbers_for(g: Geometry, drawn: &[bool]) -> Clues {
    let numbers = g
        .cells()
        .map(|cell| {
            let sides = g.edges_of_cell(cell).expect("four sides");
            let used = sides
                .iter()
                .filter(|e| drawn.get(e.0 as usize).copied().unwrap_or(false))
                .count();
            Some(u8::try_from(used).expect("at most four"))
        })
        .collect();
    Clues::new(g, numbers).expect("a grid")
}

/// A 4 by 4 board whose answer is the loop round the four middle cells.
fn middle_loop() -> (Geometry, Loop) {
    let g = board(4, 4);
    (g, rectangle_loop(g, 1, 1, 2, 2))
}

#[test]
fn a_fully_numbered_board_has_exactly_one_answer() {
    let (_, answer) = middle_loop();
    let p = Slitherlink::new(Clues::of(&answer));
    let witness: State = answer.as_state();
    with_solvers("a fully numbered board", |solver| {
        assert_eq!(
            solver.ask(&solvability_query(&p).expect("encodable"), "solvability"),
            Answer::Sat,
            "the board we drew is an answer, so there is at least one"
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&p, &witness).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat,
            "and no second answer"
        );
    });
}

#[test]
fn taking_half_the_numbers_away_leaves_exactly_one_answer() {
    // The first evidence for the plan's part three: numbers can come off a board
    // without a second answer appearing. Here it is a fixed half rather than a
    // search, because part three is where the searching goes.
    let (g, answer) = middle_loop();
    let mut clues = Clues::of(&answer);
    for cell in g.cells().filter(|c| c.0 % 2 == 0) {
        clues = clues.without(cell);
    }
    assert_eq!(clues.given_count(), 8, "half of sixteen");
    let p = Slitherlink::new(clues);
    let witness = answer.as_state();
    with_solvers("a board with half its numbers", |solver| {
        assert_eq!(
            solver.ask(&solvability_query(&p).expect("encodable"), "solvability"),
            Answer::Sat
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&p, &witness).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat
        );
    });
}

#[test]
fn a_board_with_no_numbers_has_many_answers() {
    // The control. Without it, an encoding that was accidentally too strict would
    // answer `unsat` to every uniqueness query and look like success.
    let (g, answer) = middle_loop();
    let p = Slitherlink::new(Clues::none(g));
    let witness = answer.as_state();
    with_solvers("a board with no numbers", |solver| {
        assert_eq!(
            solver.ask(&solvability_query(&p).expect("encodable"), "solvability"),
            Answer::Sat,
            "any loop will do"
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&p, &witness).expect("encodable"),
                "uniqueness"
            ),
            Answer::Sat,
            "so there are plenty of other answers"
        );
    });
}

#[test]
fn two_rings_are_refused_by_the_solver() {
    // The test this file exists for. The numbers are read off a board of two separate
    // rings, so that board satisfies every one of them exactly. It is the only board
    // that does, and the loop rule forbids it, so the right answer is `unsat`.
    //
    // If the one-piece assertions were ever dropped from the encoding, this query
    // would answer `sat` — and the test below proves that is exactly what happens.
    let g = board(4, 4);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 0, 0);
    add_rectangle(g, &mut drawn, 2, 2, 3, 3);
    assert!(
        Loop::of(g, drawn.clone()).is_err(),
        "two rings, by our own check"
    );
    let p = Slitherlink::new(numbers_for(g, &drawn));
    with_solvers("two rings", |solver| {
        assert_eq!(
            solver.ask(&solvability_query(&p).expect("encodable"), "solvability"),
            Answer::Unsat,
            "a board of two rings is not an answer, however well the numbers fit"
        );
    });
}

#[test]
fn without_the_one_piece_rule_the_solver_would_accept_two_rings() {
    // The anti-vacuity half of the test above, and the reason the whole-board rule is
    // not optional. The same numbers are asked about with the one-piece assertions
    // cut off the end of our own query text, leaving the corner rule and the numbers.
    // The solver then says `sat`: the two-ring board satisfies everything that is
    // left.
    //
    // The cut is made on a marker our own encoder writes, and the test asserts the
    // marker was found, so a change to the text makes this fail loudly rather than
    // pass by cutting nothing.
    const MARKER: &str = "\n; all in one piece";
    let g = board(4, 4);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 0, 0);
    add_rectangle(g, &mut drawn, 2, 2, 3, 3);
    let p = Slitherlink::new(numbers_for(g, &drawn));
    let whole = solvability_query(&p).expect("encodable");
    let (local_only, cut) = whole.split_once(MARKER).expect("the one-piece marker");
    assert!(
        cut.contains("declare-const root Int"),
        "the part cut off must be the one-piece rule"
    );
    let local_only = format!("{local_only}\n(check-sat)\n");
    with_solvers("two rings, local rules only", |solver| {
        assert_eq!(
            solver.ask(&local_only, "solvability without the one-piece rule"),
            Answer::Sat,
            "every local rule is happy with two separate rings, which is the whole point"
        );
    });
}

#[test]
fn a_board_where_every_square_says_nought_has_no_answer() {
    // A loop must exist, and a board of noughts says it touches nothing. There is no
    // such board, and the solver has to say so rather than quietly accept the empty
    // board — which would be the one place "no lines at all" could sneak through as
    // an answer.
    let g = board(3, 3);
    let p = Slitherlink::new(Clues::new(g, vec![Some(0); g.cell_count()]).expect("a grid"));
    with_solvers("every square nought", |solver| {
        assert_eq!(
            solver.ask(&solvability_query(&p).expect("encodable"), "solvability"),
            Answer::Unsat
        );
    });
}

#[test]
fn relabelling_the_same_loop_is_not_a_second_answer() {
    // The `root` and distance labels are working-out, and one board carries many sets
    // of them. If the uniqueness query blocked whole models instead of edges, the
    // solver could answer `sat` by relabelling the very board it was given, and every
    // puzzle would look ambiguous. A long thin loop has the most labellings to choose
    // from, so it is the fixture most likely to show the fault.
    let g = board(6, 2);
    let answer = rectangle_loop(g, 0, 0, 5, 1);
    let p = Slitherlink::new(Clues::of(&answer));
    with_solvers("a long loop", |solver| {
        assert_eq!(
            solver.ask(
                &uniqueness_query(&p, &answer.as_state()).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat,
            "the same board labelled differently is the same board"
        );
    });
}

#[test]
fn the_whole_pipeline_produces_reports_a_real_solver_agrees_with() {
    // The exit test for making puzzles: generated puzzles, not hand-written ones, all
    // the way through to a validation report, with a real solver answering both
    // questions.
    //
    // It matters more here than the fixtures above, because the generator decides what
    // to keep using **our** solver. If our solver and a real one disagreed about
    // uniqueness, every puzzle this family makes would be suspect, and nothing else in
    // the repository would notice.
    let shape = Shape::middling(5, 5);
    let (kept, report) = batch(0..6, shape, REMOVAL_BUDGET);
    assert!(report.is_consistent(), "{report:?}");
    assert!(!kept.is_empty(), "six seeds produced no puzzle at all");

    with_solvers(&format!("the pipeline: {} puzzles", kept.len()), |solver| {
        for candidate in &kept {
            let report =
                report_for(candidate, &AsVerifier(solver), Budget::default()).expect("askable");
            assert_eq!(
                report.publishable(),
                Err(NotPublishable::NoContentId),
                "seed {}: a generated puzzle must pass every check but the \
                     fingerprint it does not have yet",
                candidate.seed
            );
            assert!(
                filter_and_solver_agree(candidate, &report),
                "seed {}: our own solver and {} disagree, which is a fault in us",
                candidate.seed,
                solver.name
            );
        }
    });
}

/// Lets the pipeline ask the solver under test.
///
/// Panics where `gs-solver` would answer `Unknown` for any reason but the solver's
/// own `unknown`, for the reason [`Solver::ask`] does: in this job, no verdict on our
/// own encoding means the encoding is malformed and must stop the build.
struct AsVerifier<'a>(&'a Solver);

impl gs_slitherlink::publish::Verifier for AsVerifier<'_> {
    fn name(&self) -> &'static str {
        "solver under test"
    }
    fn ask(&self, query: &str) -> Outcome {
        match self.0.ask(query, "a query from the pipeline") {
            Answer::Sat => Outcome::Sat,
            Answer::Unsat => Outcome::Unsat,
            Answer::Unknown => Outcome::Unknown,
        }
    }
}

#[test]
fn a_real_loop_is_accepted_whatever_its_length() {
    // The other direction of the one-piece rule: it must accept every genuine loop,
    // not only convenient ones. A loop can be labelled with step-counts from any dot
    // on it, so this should always hold — and a bound set too tight, or a step
    // written the wrong way round, would show up as a real loop being refused.
    let shapes: [(u16, u16, u16, u16, u16, u16); 4] = [
        (2, 2, 0, 0, 0, 0),
        (5, 5, 0, 0, 4, 4),
        (8, 2, 1, 0, 6, 1),
        (3, 7, 0, 1, 2, 5),
    ];
    with_solvers("loops of several shapes", |solver| {
        for (w, h, x0, y0, x1, y1) in shapes {
            let g = board(w, h);
            let answer = rectangle_loop(g, x0, y0, x1, y1);
            let p = Slitherlink::new(Clues::of(&answer));
            let label = format!(
                "a {w}x{h} board with a loop of {} lines",
                answer.edges_drawn()
            );
            assert_eq!(
                solver.ask(&solvability_query(&p).expect("encodable"), &label),
                Answer::Sat,
                "{label} must be accepted"
            );
            assert!(
                p.is_solved(&answer.as_state()),
                "{label}: and our own engine agrees it is finished"
            );
        }
    });
}
