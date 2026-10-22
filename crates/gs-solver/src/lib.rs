//! Running a real SMT solver, and refusing to guess what it said.
//!
//! Everything upstream of here produces SMT-LIB2 text and is checked against our own
//! code. This crate is where that text meets a program nobody here wrote, which makes
//! it the one place in the pipeline where an outside answer enters the trust chain.
//!
//! # The one rule this crate exists to enforce
//!
//! **Every failure is [`Outcome::Unknown`].** A solver that will not start, dies on a
//! signal, exceeds its time limit, prints nothing, prints something unparseable, or
//! prints two contradictory verdicts — all of those are "nobody knows", and none of
//! them is `Unsat`.
//!
//! That is not defensive politeness. `Unsat` on the uniqueness query is the single
//! claim this platform exists to make: it is how a puzzle is declared to have exactly
//! one answer. An implementation that returned `Unsat` because the solver binary was
//! missing would turn an unconfigured machine into a proof, and the puzzles it waved
//! through would be indistinguishable, in the stored record, from ones a solver had
//! actually examined.
//!
//! # Why there is no panic anywhere in here
//!
//! The test runner in `gs-nonogram/tests/solver_agreement.rs` panics when a solver
//! gives no verdict, and that is right for a test: a malformed encoding should stop
//! the build loudly. This crate is the opposite case. It runs unattended, inside a
//! batch worker that may be filling a pool for hours, and a panic there either takes
//! down the run or, worse, gets caught and turned into something. So failures come
//! back as `Unknown` with the reason recorded for a human, and the *gate* decides what
//! an `Unknown` means — which it already does, by refusing.
//!
//! # What this crate does not do
//!
//! It does not choose a solver, install one, or know anything about Nonogram. It takes
//! a path to a program that speaks SMT-LIB2 on a file and reports the verdict.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gs_core::verification::{Outcome, Verifier};

/// How long a single query may take before it is killed.
///
/// There is no default of "unlimited" and no way to spell one: a batch worker left
/// running overnight against a query the solver cannot finish would otherwise hang
/// forever, and a hang is harder to notice than a wrong answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeLimit(Duration);

impl TimeLimit {
    /// Build a limit. Zero is rejected, because a limit of zero would make every
    /// query `Unknown` while looking like a configuration rather than a mistake.
    #[must_use]
    pub fn of(duration: Duration) -> Option<Self> {
        if duration.is_zero() {
            None
        } else {
            Some(TimeLimit(duration))
        }
    }

    #[must_use]
    pub fn seconds(seconds: u32) -> Option<Self> {
        TimeLimit::of(Duration::from_secs(u64::from(seconds)))
    }

    #[must_use]
    pub fn as_duration(self) -> Duration {
        self.0
    }
}

/// Why a query did not produce a verdict.
///
/// Every variant maps to [`Outcome::Unknown`]. The distinction is kept for the human
/// reading a run's log, not for the gate: a missing solver and a timeout need very
/// different responses from whoever is operating the pipeline, and flattening them
/// into one "unknown" would hide a broken installation behind what looks like a hard
/// puzzle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoVerdict {
    /// The program could not be started at all — usually not installed, or not on the
    /// path. The single most likely cause of a silent pipeline, so it is named.
    CouldNotStart(String),
    /// The query could not be written to a file for the solver to read.
    CouldNotWriteQuery(String),
    /// The solver was still running when its time limit expired, and was killed.
    TimedOut(Duration),
    /// The solver exited without printing any of `sat`, `unsat` or `unknown`. This is
    /// what a malformed encoding looks like from out here.
    NoAnswer { status: String, stderr: String },
    /// The solver printed more than one different verdict. Treated as no verdict at
    /// all rather than taking the first or the last: output nobody can read one way is
    /// not evidence, and picking one of two answers is picking at random.
    ContradictoryAnswers,
    /// The solver said `unknown` itself. The honest case, and the only variant here
    /// that is not a malfunction.
    SolverSaidUnknown,
}

/// What one query produced: a verdict, and the reason if there was not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub outcome: Outcome,
    /// `None` when the solver gave a verdict. Always `Some` when the outcome is
    /// `Unknown`, so a caller can tell a timeout from a solver that is not installed.
    pub why_not: Option<NoVerdict>,
    /// How long the solver ran. Recorded so a pool-filling run can be told apart from
    /// a pipeline that is quietly failing fast on every query.
    pub elapsed: Duration,
}

impl Answer {
    fn decided(outcome: Outcome, elapsed: Duration) -> Self {
        Answer {
            outcome,
            why_not: None,
            elapsed,
        }
    }

    fn unknown(why: NoVerdict, elapsed: Duration) -> Self {
        Answer {
            outcome: Outcome::Unknown,
            why_not: Some(why),
            elapsed,
        }
    }
}

/// A solver that is run as a separate program.
///
/// # Why a file and not a pipe
///
/// The query is written to a file and the solver's output is collected into another
/// file, rather than either travelling through a pipe. Two reasons, and the second is
/// the one that matters:
///
/// * Some solvers choose their input language from the file extension, so the query
///   file is named `.smt2`.
/// * Polling for the time limit means not blocking on the child's output, and a child
///   writing to a pipe nobody is reading stops when the pipe fills. That deadlock
///   would look exactly like a solver taking a long time on a hard query, so it would
///   be diagnosed as a timeout and never found. A file cannot fill.
pub struct ExternalSolver {
    program: PathBuf,
    name: &'static str,
    limit: TimeLimit,
    scratch: PathBuf,
}

impl ExternalSolver {
    /// Configure a solver.
    ///
    /// `name` is what goes into the validation report, so it should identify the
    /// program and ideally its version. It is `&'static str` because that is what
    /// [`Verifier::name`] requires, and because a name read from somewhere at runtime
    /// has no business in a record that is supposed to be comparable years later.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, name: &'static str, limit: TimeLimit) -> Self {
        ExternalSolver {
            program: program.into(),
            name,
            limit,
            scratch: std::env::temp_dir(),
        }
    }

    /// Put the query and output files somewhere other than the system temporary
    /// directory.
    #[must_use]
    pub fn with_scratch_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.scratch = dir.into();
        self
    }

    /// Whether the program can be run at all.
    ///
    /// Worth calling once before a long run. It is **not** called automatically before
    /// every query: that would double the number of processes launched, and a solver
    /// that disappears mid-run is already handled, because every failure is `Unknown`.
    ///
    /// A pipeline that treats a `false` here as a reason to carry on anyway will
    /// publish nothing, which is the correct outcome and not a silent one.
    pub fn runnable(&self) -> bool {
        Command::new(&self.program)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Ask one query and report the verdict with its reason.
    ///
    /// [`Verifier::ask`] throws the reason away because the trait returns only an
    /// outcome. Callers that want to log why a run produced nothing should use this.
    pub fn ask_explained(&self, query: &str) -> Answer {
        let started = Instant::now();

        let paths = match self.scratch_paths() {
            Ok(paths) => paths,
            Err(e) => return Answer::unknown(NoVerdict::CouldNotWriteQuery(e), started.elapsed()),
        };
        let _cleanup = Cleanup(vec![paths.query.clone(), paths.output.clone()]);

        if let Err(e) = write_all(&paths.query, query.as_bytes()) {
            return Answer::unknown(NoVerdict::CouldNotWriteQuery(e), started.elapsed());
        }

        let output_file = match std::fs::File::create(&paths.output) {
            Ok(f) => f,
            Err(e) => {
                return Answer::unknown(
                    NoVerdict::CouldNotWriteQuery(format!("{}: {e}", paths.output.display())),
                    started.elapsed(),
                )
            }
        };

        let mut child = match Command::new(&self.program)
            .arg(&paths.query)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output_file))
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                return Answer::unknown(
                    NoVerdict::CouldNotStart(format!("{}: {e}", self.program.display())),
                    started.elapsed(),
                )
            }
        };

        // Poll rather than block, so the time limit is real. The interval starts short
        // and grows, because most queries finish in milliseconds and a long run should
        // not spend its time waking up to ask again.
        let deadline = started + self.limit.as_duration();
        let mut interval = Duration::from_millis(1);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Err(e) => {
                    let _ = child.kill();
                    return Answer::unknown(
                        NoVerdict::CouldNotStart(format!("waiting on the solver: {e}")),
                        started.elapsed(),
                    );
                }
                Ok(None) => {}
            }
            if Instant::now() >= deadline {
                // Killed, then reaped. Without the wait the process would stay a
                // zombie, and a worker filling a pool would leak one per timeout.
                let _ = child.kill();
                let _ = child.wait();
                return Answer::unknown(
                    NoVerdict::TimedOut(self.limit.as_duration()),
                    started.elapsed(),
                );
            }
            std::thread::sleep(interval);
            interval = (interval * 2).min(Duration::from_millis(50));
        };

        let stderr = child
            .stderr
            .take()
            .map(|mut pipe| {
                let mut text = String::new();
                let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
                text
            })
            .unwrap_or_default();
        let printed = std::fs::read_to_string(&paths.output).unwrap_or_default();
        let elapsed = started.elapsed();

        match verdict(&printed) {
            Verdict::Decided(outcome) => Answer::decided(outcome, elapsed),
            Verdict::SaidUnknown => Answer::unknown(NoVerdict::SolverSaidUnknown, elapsed),
            Verdict::Contradictory => Answer::unknown(NoVerdict::ContradictoryAnswers, elapsed),
            Verdict::Silent => Answer::unknown(
                NoVerdict::NoAnswer {
                    status: describe(&status),
                    stderr: first_lines(&stderr, 10),
                },
                elapsed,
            ),
        }
    }

    fn scratch_paths(&self) -> Result<ScratchPaths, String> {
        std::fs::create_dir_all(&self.scratch)
            .map_err(|e| format!("{}: {e}", self.scratch.display()))?;
        // Process id plus a counter: unique within a run, and unique between
        // concurrent runs on one machine, without reading a clock. Nothing in this
        // project may decide anything from a wall clock, and a temporary file name is
        // not a decision, but one source of names is simpler than two.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stem = format!(
            "gatedspark-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Ok(ScratchPaths {
            query: self.scratch.join(format!("{stem}.smt2")),
            output: self.scratch.join(format!("{stem}.out")),
        })
    }
}

impl Verifier for ExternalSolver {
    fn name(&self) -> &'static str {
        self.name
    }

    /// Ask one query, keeping only the outcome.
    ///
    /// Every failure arrives here as `Unknown`, which the gate refuses. Use
    /// [`ExternalSolver::ask_explained`] when the reason matters.
    fn ask(&self, query: &str) -> Outcome {
        self.ask_explained(query).outcome
    }
}

struct ScratchPaths {
    query: PathBuf,
    output: PathBuf,
}

/// Removes the scratch files when a query finishes, however it finishes.
///
/// A `Drop` guard rather than a tidy-up at the end, because `ask_explained` returns
/// early from a dozen places and one of them would eventually be forgotten. Failures
/// to remove are ignored: a leftover file in a temporary directory is not worth
/// turning a solver answer into an error.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_all(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.flush().map_err(|e| format!("{}: {e}", path.display()))
}

enum Verdict {
    Decided(Outcome),
    SaidUnknown,
    Contradictory,
    Silent,
}

/// Read a solver's output.
///
/// # Whole lines, never substrings
///
/// `unsat` contains `sat`. A substring search would read every refutation as a
/// confirmation, which is the one mistake in this crate that would invert the
/// platform's central claim while appearing to work perfectly: every puzzle would be
/// reported as having many answers, or worse, every uniqueness query would come back
/// the wrong way. So lines are trimmed and compared exactly.
///
/// # Two different verdicts is no verdict
///
/// A query that prints `sat` and then `unsat` is output nobody can read one way.
/// Taking the first or the last would be picking one of two answers at random, so it
/// is [`Verdict::Contradictory`] and becomes `Unknown`. Repeating the *same* verdict
/// is fine: some solvers echo it.
fn verdict(printed: &str) -> Verdict {
    let mut seen: Option<Verdict> = None;
    for line in printed.lines() {
        let found = match line.trim() {
            "sat" => Verdict::Decided(Outcome::Sat),
            "unsat" => Verdict::Decided(Outcome::Unsat),
            "unknown" => Verdict::SaidUnknown,
            _ => continue,
        };
        seen = match (seen, found) {
            (None, first) => Some(first),
            (Some(Verdict::Decided(a)), Verdict::Decided(b)) if a == b => Some(Verdict::Decided(a)),
            (Some(Verdict::SaidUnknown), Verdict::SaidUnknown) => Some(Verdict::SaidUnknown),
            _ => return Verdict::Contradictory,
        };
    }
    seen.unwrap_or(Verdict::Silent)
}

fn describe(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        // No code means a signal on Unix. Worth saying plainly: a solver killed by the
        // out-of-memory killer looks like this, and it is a very different problem from
        // a malformed query.
        None => format!("killed by a signal ({status})"),
    }
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}
