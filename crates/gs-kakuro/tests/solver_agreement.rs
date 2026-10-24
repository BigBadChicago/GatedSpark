//! Does a real solver agree with us about Kakuro?
//!
//! Every other test in this crate checks our work against our own work.
//! `tests/encoding.rs` checks that the SMT-LIB2 text *looks* right; nothing there
//! would notice if a solver reading it reached the wrong verdict, and a malformed
//! encoding passes all of it.
//!
//! This is the job that closes that gap, and it has two headline cases rather than
//! one.
//!
//! * `a_repeated_digit_board_is_refused_by_the_solver` — the numbers are read off a
//!   board that satisfies every total exactly and repeats a digit. The totals alone
//!   accept it, and the no-repeats rule must reach the solver and refuse it.
//! * `without_the_no_repeats_rule_the_solver_would_accept_a_repeat` — the same board
//!   with the no-repeats assertions cut off our own query text, which the solver then
//!   accepts. That is the anti-vacuity half: it shows the rule is doing work rather
//!   than being satisfied by accident.
//!
//! Together they are this family's version of Slitherlink's two-separate-rings pair,
//! and they exist for the same reason. ADR 0018 emits the adding-up rule and the
//! no-repeats rule separately, so the specific thing that could go wrong here is one
//! of the two going missing while the puzzle still looks complete.
//!
//! # Finding a solver, and why absence is loud
//!
//! The same arrangement as the other two families' jobs of this name, deliberately
//! repeated rather than shared: this is test scaffolding, and a shared helper crate
//! holding the only code that can run a solver during a build is the sort of thing
//! that ends up in a release.
//!
//! * With `GATEDSPARK_REQUIRE_SOLVER=1` set, a missing solver **fails** the test. CI
//!   sets it and installs a solver, so skipping is impossible there.
//! * Without it, the test says loudly that it verified nothing and returns. That is
//!   for a contributor's laptop, never for CI.
//!
//! # Configuration
//!
//! * `GATEDSPARK_SMT_SOLVER` — path to the primary solver. Defaults to `z3`.
//! * `GATEDSPARK_SMT_SOLVER_2` — an optional second, independent solver, whose answers
//!   must match the first's. That is the ADR 0002 cross-check: two solvers disagreeing
//!   about a uniqueness result is a build failure.

mod common;

use std::process::Command;
use std::time::Duration;

use common::{from_answer, layout};
use gs_core::ruleset::Ruleset;
use gs_core::state::{Cell, State};
use gs_core::verification::Outcome;
use gs_kakuro::layout::{Layout, WhiteId};
use gs_kakuro::rules::Kakuro;
use gs_kakuro::smtlib::{solvability_query, uniqueness_query};
use gs_kakuro::totals::Totals;
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

/// A puzzle whose totals are read off the digits given, **without** checking that
/// those digits are a legal answer.
///
/// The fixture the headline tests need: a board that repeats a digit has no legal
/// answer, so it cannot be built through the ordinary helper, and the totals still
/// have to be the ones that board implies or the test would be about a different
/// puzzle.
fn totals_read_off(rows: &[&str], digits: &[i16]) -> (Kakuro, State) {
    let layout: Layout = layout(rows);
    assert_eq!(digits.len(), layout.white_count(), "one digit per square");
    let mut numbers = Vec::with_capacity(layout.run_count());
    for (_, run) in layout.runs() {
        let sum: i16 = run
            .squares
            .iter()
            .map(|w| digits[usize::try_from(w.0).expect("fits")])
            .sum();
        numbers.push(u16::try_from(sum).expect("a sum of digits fits"));
    }
    let totals = Totals::new(&layout, numbers).expect("reachable totals");
    let mut state = State::blank(layout.state_dims());
    for (index, digit) in digits.iter().enumerate() {
        state
            .set(u32::try_from(index).expect("fits"), Cell(*digit))
            .expect("square exists");
    }
    let puzzle = Kakuro::new(layout, totals).expect("one total per run");
    (puzzle, state)
}

#[test]
fn a_board_we_filled_in_ourselves_has_at_least_one_answer() {
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    assert!(puzzle.is_solved(&answer), "by our own rules first");
    with_solvers("a 2 by 2 board", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Sat,
            "the board we filled in is an answer, so there is at least one"
        );
    });
}

#[test]
fn a_tightly_constrained_board_has_exactly_one_answer() {
    // A 2 by 2 whose four totals leave no room: 1+2=3 and 3+4=7 across, 1+3=4 and
    // 2+4=6 down. Any other arrangement breaks one of them.
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 2, 3, 4]);
    with_solvers("a tightly constrained board", |solver| {
        assert_eq!(
            solver.ask(
                &uniqueness_query(&puzzle, &answer).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat,
            "and no second answer"
        );
    });
}

#[test]
fn a_loose_board_has_more_than_one_answer() {
    // The control. Without it, an encoding that was accidentally too strict would
    // answer `unsat` to every uniqueness query and look like success.
    //
    // Finding a loose board takes more care than it looks, and the reason is worth
    // recording because it caught this test out once. **A board one square tall or
    // one square wide is never loose.** Every white square sits in an across run and
    // a down run, so on a single row each square has its own down run of one, and a
    // run of one names its digit outright. The whole board is then pinned and the
    // answer is forced.
    //
    // So this fixture is 2 by 2. Its four totals are 6 and 8 across, 3 and 11 down,
    // and two different boards reach them: 1 5 / 2 6, and 2 4 / 1 7. Both have four
    // runs of two distinct digits adding to the right numbers, so the solver must
    // find the second one.
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    assert_eq!(puzzle.totals.as_slice(), &[6, 8, 3, 11]);
    with_solvers("a loose board", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Sat
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&puzzle, &answer).expect("encodable"),
                "uniqueness"
            ),
            Answer::Sat,
            "1 5 / 2 6 is not the only board with these totals"
        );
    });
}

#[test]
fn a_repeated_digit_board_is_refused_by_the_solver() {
    // One of the two tests this file exists for. The totals are read off a board that
    // repeats a digit, so that board satisfies every total exactly. It is the only
    // board that does — a run of two squares totalling eight has 4+4, 3+5 and 5+3,
    // and the down runs of one pin each square to its own digit — and the no-repeats
    // rule forbids it, so the right answer is `unsat`.
    let (puzzle, repeated) = totals_read_off(&[".."], &[4, 4]);
    assert!(
        !puzzle.is_solved(&repeated),
        "our own rules refuse it, which is tests/the_two_halves.rs's job"
    );
    assert_eq!(
        puzzle.totals.as_slice(),
        &[8, 4, 4],
        "across totalling eight, then each square's own down run"
    );
    with_solvers("a board that repeats a digit", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Unsat,
            "4 + 4 makes eight and is not an answer, however well the totals fit"
        );
    });
}

#[test]
fn without_the_no_repeats_rule_the_solver_would_accept_a_repeat() {
    // The anti-vacuity half of the test above, and the reason the no-repeats rule is
    // not optional. The same totals are asked about with the no-repeats assertions cut
    // off the end of our own query text, leaving the digit bounds and the totals. The
    // solver then says `sat`: the repeated-digit board satisfies everything left.
    //
    // The cut is made on a marker our own encoder writes, and the test asserts the
    // marker was found, so a change to the text makes this fail loudly rather than
    // pass by cutting nothing.
    const MARKER: &str = "\n; no repeats";
    let (puzzle, _) = totals_read_off(&[".."], &[4, 4]);
    let whole = solvability_query(&puzzle).expect("encodable");
    let (totals_only, cut) = whole.split_once(MARKER).expect("the no-repeats marker");
    assert!(
        cut.contains("(assert (distinct"),
        "the part cut off must be the no-repeats rule"
    );
    let totals_only = format!("{totals_only}\n(check-sat)\n");
    with_solvers("a repeat, totals only", |solver| {
        assert_eq!(
            solver.ask(&totals_only, "solvability without the no-repeats rule"),
            Answer::Sat,
            "the totals alone are perfectly happy with 4 + 4, which is the whole point"
        );
    });
}

#[test]
fn without_the_digit_bounds_the_solver_would_find_an_answer_outside_one_to_nine() {
    // The second rule that could go missing, and the reason ADR 0018 made the digit
    // range a rule rather than a hint.
    //
    // The case that motivates it: a run of two totalling nine is satisfied by 9 + 0,
    // and the no-repeats rule does not catch that, because one nought does not
    // repeat. Nor would it catch a negative number. So the bounds are the only thing
    // keeping a Kakuro's squares to the digits a player can write.
    //
    // This checks the rule is load-bearing by cutting the bounds off our own query
    // text, on a marker the encoder writes, and asking for a second answer. The board
    // 1 2 / 3 4 is the only answer within one to nine; without the bounds it is not,
    // and the solver finds another. The test asserts the marker was found, so a change
    // to the text fails loudly rather than passing by cutting nothing.
    const MARKER: &str = "\n; every white square holds a digit";
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 2, 3, 4]);

    // With the bounds in place there is exactly one answer. Asserted here so the two
    // halves of the claim sit side by side rather than in separate tests that could
    // drift apart.
    let bounded = uniqueness_query(&puzzle, &answer).expect("encodable");
    let (head, rest) = bounded.split_once(MARKER).expect("the digit-bounds marker");
    let after = rest
        .split_once("\n; the totals")
        .expect("the totals follow the bounds")
        .1;
    let unbounded = format!("{head}\n; the totals{after}");
    assert!(
        !unbounded.contains("(<= 1 w_0)"),
        "the bounds must really be gone"
    );
    assert!(
        unbounded.contains("(assert (distinct"),
        "and the no-repeats rule must still be there, or this proves nothing"
    );

    with_solvers("the digit bounds", |solver| {
        assert_eq!(
            solver.ask(&bounded, "uniqueness with the digit bounds"),
            Answer::Unsat,
            "within one to nine, 1 2 / 3 4 is the only board with these totals"
        );
        assert_eq!(
            solver.ask(&unbounded, "uniqueness with no digit bounds"),
            Answer::Sat,
            "without them there is another, using a number no player could write, \
             and the no-repeats rule does not catch it"
        );
    });
}

#[test]
fn a_total_only_one_arrangement_can_reach_is_unique() {
    // A run of nine squares must hold all nine digits, so its total is forced to 45
    // and the digits are a permutation. Adding a down run of one square per column
    // pins each digit, leaving exactly one answer. The widest single run the rules
    // allow, so it is also the largest query this family will ever produce for one
    // run.
    let (puzzle, answer) = from_answer(&["........."], &[9, 1, 8, 2, 7, 3, 6, 4, 5]);
    with_solvers("a run of all nine digits", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Sat
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&puzzle, &answer).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat,
            "each column's own total names its digit, so there is one answer"
        );
    });
}

#[test]
fn a_board_whose_totals_cannot_all_hold_has_no_answer() {
    // Two runs that cross and disagree. The across run over squares 0 and 1 totals 3,
    // which forces 1 and 2 in some order. The down run over square 0 alone totals 9,
    // which forces a 9 there. Both cannot hold, and the solver has to say so rather
    // than find some way to satisfy one of them.
    let layout = layout(&[".."]);
    let mut numbers = vec![0_u16; layout.run_count()];
    for (id, run) in layout.runs() {
        let index = usize::try_from(id.0).expect("fits");
        numbers[index] = if run.len() == 2 { 3 } else { 9 };
    }
    let totals = Totals::new(&layout, numbers).expect("each total is reachable alone");
    let puzzle = Kakuro::new(layout, totals).expect("one per run");
    with_solvers("totals that cannot all hold", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Unsat,
            "every total is reachable on its own, and not together"
        );
    });
}

#[test]
fn a_board_with_a_lone_white_square_is_handled() {
    // The run of one is the only case the encoder skips a rule for, so it gets a
    // solver's opinion rather than only our own. A lone square whose two runs both
    // say five has exactly one answer, and asking a real solver is the only way to
    // know the skipped `distinct` did not take the total with it.
    let (puzzle, answer) = from_answer(&["#.#"], &[5]);
    with_solvers("a lone white square", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Sat
        );
        assert_eq!(
            solver.ask(
                &uniqueness_query(&puzzle, &answer).expect("encodable"),
                "uniqueness"
            ),
            Answer::Unsat,
            "one square, one total, one answer"
        );
    });
}

#[test]
fn an_awkward_shape_is_handled() {
    // Runs of one, two and three, crossing each other, with blocks at a rim and in
    // the middle. Nothing special about it except that it is not a rectangle, which
    // is where the run finding is most likely to be wrong in a way our own tests
    // share the assumption with.
    let rows = &["..#", "...", "#.."];
    let (puzzle, answer) = from_answer(rows, &[1, 2, 3, 4, 5, 6, 7]);
    assert!(puzzle.is_solved(&answer), "by our own rules first");
    with_solvers("an awkward shape", |solver| {
        assert_eq!(
            solver.ask(
                &solvability_query(&puzzle).expect("encodable"),
                "solvability"
            ),
            Answer::Sat,
            "the board we filled in is an answer"
        );
    });
}

#[test]
fn our_rules_and_the_solver_agree_on_every_filling_of_a_small_board() {
    // The broadest check in the file, and the one most likely to catch an encoding
    // that is subtly off. Every way of filling a 2 by 2 board with digits one to
    // nine, which is 6561 boards, judged by our own `is_solved`. For each board our
    // rules accept, the solver is asked whether it is an answer to the puzzle whose
    // totals that board implies, and must agree.
    //
    // Kept to a 2 by 2 so the whole space fits in a test. A sweep over larger boards
    // belongs in part three, where generation can produce them.
    let layout = layout(&["..", ".."]);
    let mut accepted = 0_u32;
    let mut cases: Vec<(Kakuro, State)> = Vec::new();
    for code in 0..6561_u32 {
        let digits: Vec<i16> = (0..4)
            .map(|i| i16::try_from((code / 9_u32.pow(i)) % 9 + 1).expect("one to nine"))
            .collect();
        let totals = Totals::of(&layout, &|white: WhiteId| {
            digits.get(usize::try_from(white.0).ok()?).copied()
        })
        .expect("a full board");
        let puzzle = Kakuro::new(layout.clone(), totals).expect("one per run");
        let mut state = State::blank(layout.state_dims());
        for (index, digit) in digits.iter().enumerate() {
            state
                .set(u32::try_from(index).expect("fits"), Cell(*digit))
                .expect("square exists");
        }
        if puzzle.is_solved(&state) {
            accepted += 1;
            // One in sixteen, so the job stays a few seconds rather than minutes. The
            // count below is over all of them, so a change in what our rules accept
            // still shows up here.
            if accepted.is_multiple_of(16) {
                cases.push((puzzle, state));
            }
        }
    }
    // Worth arriving at rather than trusting. A 2 by 2 has four runs of two, so it is
    // finished exactly when the two in each row differ and the two in each column
    // differ: a != b, c != d, a != c, b != d. The two **diagonal** pairs share no run,
    // so a == d and b == c are both allowed, and the count is not 9 x 8 x 7 x 6.
    // Counting it properly: 9 choices of a, 8 for b, 8 for c, and then d must avoid b
    // and c — which is 8 values when b == c and 7 when they differ. Of the 64 (b, c)
    // pairs, 8 have b == c. So 9 x (8 x 8 + 56 x 7) = 9 x 456.
    assert_eq!(
        accepted,
        9 * (8 * 8 + 56 * 7),
        "the two digits in each row and each column must differ, while the \
         diagonals share no run and may repeat"
    );
    assert!(!cases.is_empty(), "the sample must not be empty");

    with_solvers(
        &format!("{} sampled boards of {accepted}", cases.len()),
        |solver| {
            for (puzzle, state) in &cases {
                let named = gs_kakuro::smtlib::encoded_puzzle(puzzle);
                assert_eq!(
                    solver.ask(
                        &solvability_query(puzzle).expect("encodable"),
                        "solvability of a swept board"
                    ),
                    Answer::Sat,
                    "our rules call this board finished, so the solver must agree \
                     there is an answer: {named}"
                );
                // And the second question reaches a verdict. Whether this particular
                // board is the only answer depends on its totals and is not asserted
                // either way; what is asserted is that no query this family produces
                // comes back `unknown`. An `unknown` is a rejection at the gate, so a
                // family whose queries were sometimes undecidable would quietly
                // throw away good puzzles, and this is where that would show up.
                assert_ne!(
                    solver.ask(
                        &uniqueness_query(puzzle, state).expect("encodable"),
                        "uniqueness of a swept board"
                    ),
                    Answer::Unknown,
                    "every question this family asks must get a real answer: {named}"
                );
            }
        },
    );
}
