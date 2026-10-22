//! Does a real solver agree with us?
//!
//! Every other test in this repository checks our work against our own work. The
//! `encoding` tests check that the SMT-LIB2 text *looks* right; nothing checks that a
//! solver reading it reaches the answer we expect. A malformed encoding would pass
//! all of them.
//!
//! This test closes that gap by running an actual solver over the fixtures and
//! comparing its verdict to our line solver's.
//!
//! # Finding a solver, and why absence is loud
//!
//! A test that quietly skips when its dependency is missing is the vacuous pass this
//! whole repository is arranged against — it would report green forever while
//! checking nothing. So:
//!
//! * With `GATEDSPARK_REQUIRE_SOLVER=1` set, a missing solver **fails** the test.
//!   CI sets it and installs a solver, so skipping is impossible there.
//! * Without it, the test reports loudly that it did not verify anything and
//!   returns. That is for a contributor's laptop, never for CI.
//!
//! The enforcement table in `docs/adr/0003-engine-game-boundary.md` records this as
//! enforced *in CI*, not enforced everywhere, because that is the truth.
//!
//! # Configuration
//!
//! * `GATEDSPARK_SMT_SOLVER` — path to the primary solver. Defaults to `z3`.
//! * `GATEDSPARK_SMT_SOLVER_2` — optional second, independent solver. When set, its
//!   answers must match the first's. This is the cross-check from ADR 0002: two
//!   solvers disagreeing about a uniqueness result is a build failure rather than
//!   something nobody would ever notice.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use gs_core::state::Dims;
use gs_nonogram::clues::{Clues, Pattern};
use gs_nonogram::rules::Nonogram;
use gs_nonogram::smtlib::{solvability_query, uniqueness_query};
use gs_nonogram::solve::{solve, Budget, Solved};

/// What a solver said. Mirrors `gs_core::verification::Outcome`, kept separate here
/// because this runner is test scaffolding and must not become the engine's verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Sat,
    Unsat,
    Unknown,
}

struct Solver {
    name: String,
    program: PathBuf,
}

impl Solver {
    /// Run one query and return the solver's verdict.
    ///
    /// Parses whole lines: `unsat` contains `sat`, so a substring search would read
    /// every refutation as a confirmation — which is the one mistake here that would
    /// invert the project's central claim while looking like it worked.
    fn ask(&self, query: &str, label: &str) -> Answer {
        let path = write_query(query, label);
        let output = Command::new(&self.program)
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("{} could not run on {label}: {e}", self.name));

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut answer = None;
        for line in stdout.lines() {
            match line.trim() {
                "sat" => answer = Some(Answer::Sat),
                "unsat" => answer = Some(Answer::Unsat),
                "unknown" => answer = Some(Answer::Unknown),
                _ => {}
            }
        }

        answer.unwrap_or_else(|| {
            panic!(
                "{} gave no verdict on {label}.\n\
                 This is what a malformed encoding looks like, so the query is below.\n\
                 stdout: {stdout}\nstderr: {stderr}\n\
                 --- query ---\n{}",
                self.name,
                first_lines(query, 25)
            )
        })
    }
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

/// Write a query where the solver can read it. The `.smt2` extension matters: some
/// solvers pick their input language from it.
fn write_query(query: &str, label: &str) -> PathBuf {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut path = std::env::temp_dir();
    path.push(format!("gatedspark_{}_{safe}.smt2", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("a temporary file for the query");
    file.write_all(query.as_bytes()).expect("writing the query");
    path
}

/// The solvers configured for this run, or an explanation of why there are none.
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
                program: PathBuf::from(program),
            });
        } else if var == "GATEDSPARK_SMT_SOLVER_2" {
            // An explicitly configured second solver that cannot run is always an
            // error: it was asked for, so silently dropping it would quietly remove
            // the cross-check ADR 0002 exists to provide.
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

fn runnable(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A fixture, and what our own line solver concluded about it.
struct Fixture {
    label: &'static str,
    puzzle: Nonogram,
}

fn fixtures() -> Vec<Fixture> {
    let cross =
        Pattern::from_rows(&["..#..", "..#..", "#####", "..#..", "..#.."]).expect("rectangular");
    let picture = Pattern::from_rows(&["##.", ".#.", ".##"]).expect("rectangular");

    vec![
        Fixture {
            label: "cross 5x5, uniquely solvable",
            puzzle: Nonogram::new(Clues::of(&cross)),
        },
        Fixture {
            label: "small picture 3x3",
            puzzle: Nonogram::new(Clues::of(&picture)),
        },
        Fixture {
            // The control. Both diagonals fit these clues, so there are two answers.
            // Without this case, an encoding that was accidentally too strict would
            // answer "unsat" to every uniqueness query and look like success.
            label: "ambiguous 2x2, two answers",
            puzzle: Nonogram::new(Clues {
                dims: Dims::new(2, 2),
                rows: vec![vec![1], vec![1]],
                columns: vec![vec![1], vec![1]],
            }),
        },
        Fixture {
            // Rows claim three filled cells, columns claim none.
            label: "contradictory clues, no answer",
            puzzle: Nonogram::new(Clues {
                dims: Dims::new(3, 1),
                rows: vec![vec![3]],
                columns: vec![vec![], vec![], vec![]],
            }),
        },
    ]
}

#[test]
fn a_real_solver_agrees_with_our_line_solver() {
    let solvers = match solvers() {
        Ok(s) => s,
        Err(why) => {
            // Loud on purpose. Run with `-- --nocapture` to see it.
            println!(
                "\n*** SOLVER AGREEMENT NOT VERIFIED: {why}.\n\
                 *** Nothing in this test ran. CI sets GATEDSPARK_REQUIRE_SOLVER=1,\n\
                 *** under which this is a failure rather than this message.\n"
            );
            return;
        }
    };
    println!("verifying against {} solver(s)", solvers.len());

    for fixture in fixtures() {
        let ours = solve(&fixture.puzzle, Budget::default()).solved;
        let label = fixture.label;

        // What our line solver concluded, expressed as the two answers a real solver
        // must give. `Undecided` would mean our own search gave up, which none of
        // these fixtures should do; it is asserted rather than silently allowed.
        let (want_solvable, want_unique) = match ours {
            Solved::One => (Answer::Sat, Answer::Unsat),
            Solved::Several => (Answer::Sat, Answer::Sat),
            Solved::None => (Answer::Unsat, Answer::Unsat),
            Solved::Undecided => panic!("our line solver gave up on {label}; fixture is too hard"),
        };

        let solvability = solvability_query(&fixture.puzzle).expect("encodable");
        for solver in &solvers {
            let got = solver.ask(&solvability, &format!("{label} solvability"));
            assert_eq!(
                got, want_solvable,
                "{}: on '{label}' our line solver said {ours:?}, so solvability should be \
                 {want_solvable:?}, but the solver said {got:?}",
                solver.name
            );
        }

        // Uniqueness needs a witness to block, so it only applies where one exists.
        if let Some(witness) = solve(&fixture.puzzle, Budget::default()).witness {
            let unique = uniqueness_query(&fixture.puzzle, &witness).expect("encodable");
            for solver in &solvers {
                let got = solver.ask(&unique, &format!("{label} uniqueness"));
                assert_eq!(
                    got, want_unique,
                    "{}: on '{label}' our line solver said {ours:?}, so the uniqueness \
                     re-query should be {want_unique:?}, but the solver said {got:?}",
                    solver.name
                );
            }
        } else {
            assert_eq!(ours, Solved::None, "{label}: no witness but not unsolvable");
        }
    }
}

#[test]
fn an_unrunnable_second_solver_is_an_error_not_a_quiet_single_check() {
    // The cross-check from ADR 0002 is worth nothing if a typo in its configuration
    // turns it off silently. Child process, because the check panics.
    let exe = std::env::current_exe().expect("this test binary");
    let output = Command::new(exe)
        .args([
            "an_unrunnable_second_solver_is_an_error_helper",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "GATEDSPARK_SMT_SOLVER_2",
            "definitely-not-a-real-solver-binary",
        )
        .output()
        .expect("re-running this test binary");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("GATEDSPARK_SMT_SOLVER_2 is set to") && combined.contains("will not run"),
        "a misconfigured second solver must fail loudly; got:\n{combined}"
    );
}

#[test]
#[ignore = "helper for an_unrunnable_second_solver_is_an_error_not_a_quiet_single_check"]
fn an_unrunnable_second_solver_is_an_error_helper() {
    let _ = solvers();
}
