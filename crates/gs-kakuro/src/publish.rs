//! From a candidate to a validation report: the step that asks a real solver.
//!
//! The same shape as `gs_nonogram::publish` and `gs_slitherlink::publish`,
//! deliberately, because the pipeline belongs to the platform and not to the family.
//! Everything up to here produced a [`Candidate`], meaning *our own* fast solver
//! believes the puzzle has one answer. This asks a real solver the two questions that
//! matter and writes down what it said, whatever that was.
//!
//! # Why that distinction is sharper in this family than in either other
//!
//! Our own Kakuro solver prunes with a **relaxation**: when it asks whether some
//! squares could still make up a remaining total, it pools their candidate digits
//! instead of enumerating the combinations. That is sound in the one direction that
//! matters — it never drops a digit that an answer could have used — but it is weaker
//! than the truth, so "our solver found one answer" is a weaker claim here than the
//! same sentence about Nonogram. Nothing in this module leans on it. The report
//! records what Z3 and CVC5 said about the text in `smtlib`, and
//! [`filter_and_solver_agree`] is where our own count is held against theirs.
//!
//! # The solver is passed in, not reached for
//!
//! [`Verifier`] is a trait defined in the core, and this crate implements none of it.
//! Running a solver means launching a program, which belongs to whatever is driving
//! the pipeline rather than to a puzzle family. Two things follow, both deliberate:
//! this crate cannot quietly answer its own questions, since the only way to obtain
//! an `Outcome` is to be handed one; and a caller can pass in two independent solvers
//! and compare, which is the ADR 0002 cross-check.
//!
//! # Nothing here decides to publish
//!
//! [`report_for`] builds a report and returns it. Whether it may be published is
//! [`ValidationReport::publishable`], which is [`gs_core::verification::decide`] over
//! the stored queries. A report of a refused puzzle is a useful record rather than an
//! error, so the refusal appears inside the report instead of replacing it.

use gs_core::content::Digest;
use gs_core::package::{seal, EncodeError as PackageEncodeError, Manifest, Sealed, Signer};
use gs_core::report::{Measurement, NotPublishable, Replay, ValidationReport};
use gs_core::ruleset::Ruleset;
use gs_core::state::State;
use gs_core::verification::{Outcome, Query};

/// Re-exported from the core, where it lives. The rule it carries: a solver that
/// could not answer must say [`Outcome::Unknown`] and never `Unsat`.
pub use gs_core::verification::Verifier;

use crate::generate::{Candidate, GENERATOR_VERSION};
use crate::rules::Kakuro;
use crate::smtlib::{solvability_query, uniqueness_query, EncodeError};
use crate::solve::{solve, Budget, Solved};

/// What the encoding's uniqueness claim is relative to.
///
/// Recorded in every report, because uniqueness is only ever true relative to an
/// encoding. One definition, so a report cannot describe an encoding other than the
/// one `smtlib` emits.
///
/// `QF_LIA` and not `QF_UFLIA`: this family declares no helper variables at all, so
/// the digits are the whole model. That is also why its blocking constraint is over
/// the digits and nothing else — see [`crate::smtlib`].
pub const ENCODING: &str = "QF_LIA, one Int per white square held to one through nine, \
                            one adding-up rule per run, all-different per run of two or more";

/// Why a report could not be built at all. Distinct from a report that says "do not
/// publish": this is the pipeline failing to ask the question, not an answer to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unaskable {
    /// The puzzle could not be encoded. Carries the encoder's reason.
    Encoding(EncodeError),
    /// Our own solver found no answer to block for the uniqueness re-query. A
    /// candidate should always have one, so this is a fault upstream rather than a
    /// property of the puzzle.
    NoWitness,
}

/// Ask a solver about a candidate and write down what it said.
///
/// Two queries, in this order:
///
/// 1. **Solvability.** Are there digits satisfying every total? Must be `sat`.
/// 2. **Uniqueness.** With our answer ruled out, is there *another* set of digits?
///    Must be `unsat`. This is the question the platform exists to ask, and it is why
///    a witness is needed before it can be asked at all.
///
/// The report carries whatever the solver actually said. Where that disagrees with
/// our own solver, the report records the disagreement and refuses publication. It
/// does not retry, reinterpret, or prefer our own answer.
pub fn report_for(
    candidate: &Candidate,
    verifier: &dyn Verifier,
    budget: Budget,
) -> Result<ValidationReport, Unaskable> {
    let found = solve(&candidate.puzzle, budget);
    let witness = found.witness.ok_or(Unaskable::NoWitness)?;

    let solvability = solvability_query(&candidate.puzzle).map_err(Unaskable::Encoding)?;
    let uniqueness = uniqueness_query(&candidate.puzzle, &witness).map_err(Unaskable::Encoding)?;

    let queries = vec![
        Query {
            name: "solvability",
            required: Outcome::Sat,
            returned: verifier.ask(&solvability),
        },
        Query {
            name: "uniqueness_blocking_the_witness",
            required: Outcome::Unsat,
            returned: verifier.ask(&uniqueness),
        },
    ];

    Ok(ValidationReport {
        // No content id yet, which is the correct state of a report at this point
        // rather than a gap: a package's fingerprint is the fingerprint of the
        // package, which does not exist until this report has been turned into one.
        content_id: None,
        ruleset: Ruleset::id(&candidate.puzzle),
        generator_version: GENERATOR_VERSION,
        seed: candidate.seed,
        queries,
        encoding: ENCODING,
        measurements: measurements_of(candidate),
        // **No difficulty level, on purpose**, for the third time and the same reason
        // (settled 2026-10-22: levels are each game's to decide). The reason to wait has
        // changed rather than gone away: `given_away_squares` is now zero by
        // construction, and the number most likely to move is
        // `digits_at_the_ends_per_mille`, which ADR 0021 leaves at about 480 and names as
        // the next thing to improve. A level set from today's boards would describe
        // boards we still intend to change. `examples/sweep.rs` produces the numbers; the
        // levels come after them.
        band: None,
        replay: replay_of(&candidate.puzzle, &witness),
    })
}

/// The difficulty numbers, with the names this game gives them.
///
/// `given_away_squares` is in here rather than left out, although it was the number this
/// family was least proud of. A measurement nobody can see does not get fixed, and a
/// report is where an audit would look for it. The built boards of ADR 0021 hold it at
/// zero, which is why `digits_at_the_ends_per_mille` is now in here beside it: that is
/// the number the fix cost, and it is this family's next one to bring down.
fn measurements_of(candidate: &Candidate) -> Vec<Measurement> {
    let f = &candidate.features;
    vec![
        Measurement {
            name: "forced_steps",
            value: f.forced_steps,
        },
        Measurement {
            name: "propagation_only_squares",
            value: f.propagation_only_squares,
        },
        Measurement {
            name: "decision_points",
            value: f.decision_points,
        },
        Measurement {
            name: "max_search_depth",
            value: f.max_search_depth,
        },
        Measurement {
            name: "white_squares",
            value: f.white_squares,
        },
        Measurement {
            name: "runs",
            value: f.runs,
        },
        Measurement {
            name: "longest_run",
            value: f.longest_run,
        },
        Measurement {
            name: "given_away_squares",
            value: f.given_away_squares,
        },
        Measurement {
            name: "single_square_runs",
            value: f.single_square_runs,
        },
        Measurement {
            name: "swap_rectangles",
            value: f.swap_rectangles,
        },
        Measurement {
            name: "digits_at_the_ends_per_mille",
            value: f.digits_at_the_ends_per_mille,
        },
        Measurement {
            name: "rounds_of_repair_while_generating",
            value: candidate.repairs,
        },
    ]
}

/// Replay the witness through the real engine.
///
/// A board a solver is happy with is not yet a board the engine accepts, and the
/// engine is what a player runs. This checks the thing that ships, through
/// [`Ruleset::is_solved`] rather than by re-adding the totals here.
///
/// Unlike both other families there is **no second, stricter condition** to add.
/// Nonogram's and Slitherlink's `is_solved` deliberately permit an unfinished board —
/// a Nonogram is done when the filled squares match the clues whether or not the
/// blanks were crossed out, and a Slitherlink when the drawn lines make the loop — so
/// each of those had to require separately that a stored answer decides every cell.
/// A Kakuro's `is_solved` already requires every white square to hold a digit, because
/// a run with an empty square in it has not met its total. So the two questions
/// coincide here, and this says so rather than adding a check that could only ever
/// pass.
///
/// Public so the judgement can be tested directly. `report_for` finds its own witness
/// by solving, so there is no way to hand it a half-filled one from outside.
#[must_use]
pub fn replay_of(puzzle: &Kakuro, witness: &State) -> Replay {
    if witness.dims() != puzzle.layout.state_dims() {
        return Replay::Refused("the witness is not the shape of the board");
    }
    if !puzzle.is_solved(witness) {
        return Replay::Refused("the engine does not consider the witness solved");
    }
    Replay::Reproduced(gs_core::hash::StateHash::of(witness))
}

/// Whether the real solver agreed with our own.
///
/// Separate from the report so a caller can tell two failures apart: a puzzle the
/// solver refuses, and a puzzle where our own solver said something different. The
/// second is a fault in us and far more serious — and in this family it is the check
/// that holds the sum relaxation honest, since a relaxation that had the direction
/// backwards would show up here as our `One` against the solver's `Sat`.
#[must_use]
pub fn filter_and_solver_agree(candidate: &Candidate, report: &ValidationReport) -> bool {
    let ours = solve(&candidate.puzzle, Budget::default()).solved;
    let solvability = report.queries.first().map(|q| q.returned);
    let uniqueness = report.queries.get(1).map(|q| q.returned);
    match ours {
        Solved::One => solvability == Some(Outcome::Sat) && uniqueness == Some(Outcome::Unsat),
        Solved::Several => solvability == Some(Outcome::Sat) && uniqueness == Some(Outcome::Sat),
        Solved::None => solvability == Some(Outcome::Unsat),
        // Our own search gave up, so there is nothing to agree or disagree with. Not
        // counted as agreement: an exhausted budget is not an answer.
        Solved::Undecided => false,
    }
}

/// Turn a candidate and its report into the body of a publishable package.
///
/// The payload is the board's canonical bytes — the black squares and the totals —
/// and nothing else. The digits the generator filled in are not carried: the pipeline
/// throws them away on purpose, and a package shipping them alongside the totals
/// would be shipping the answer.
///
/// The witness is not carried either. It belongs to the validation report, which
/// stays on the server, because a package is what a player receives.
#[must_use]
pub fn manifest_for(candidate: &Candidate, report: &ValidationReport) -> Manifest {
    Manifest::from_report(report, candidate.puzzle.canonical_bytes())
}

/// A puzzle that passed every check, the evidence for it, and the file a player gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub report: ValidationReport,
    pub package: Vec<u8>,
}

/// Why a candidate did not become a published package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The question could not be asked at all.
    Unaskable(Unaskable),
    /// The question was asked and the answer was no. Carries the gate's own refusal
    /// and the report, because that record is what a later audit reads to find out
    /// why a puzzle was dropped.
    Refused {
        reason: NotPublishable,
        report: Box<ValidationReport>,
    },
    /// Some field is longer than the file format can express.
    Encoding(PackageEncodeError),
}

/// The whole loop: ask the solver, write the evidence, and seal a package only if the
/// evidence says yes.
///
/// The order is the point, and it is the same order both other families publish in:
///
/// 1. Build the report, which asks the solver and replays the witness.
/// 2. Put it to the one gate, and go on **only** if its single remaining objection is
///    the missing content id.
/// 3. Seal the package. This is the first point at which anything is signed, so
///    nothing is ever signed before the gate has agreed to it.
/// 4. Bind the report to the package just sealed, which is where the content id comes
///    from.
/// 5. Ask the gate again. It must now say yes; if it does not, that is a fault here
///    rather than a property of the puzzle, so it is an assertion and not an error.
///
/// Step 2 treats [`NotPublishable::NoContentId`] as the go-ahead rather than
/// re-checking the queries by hand, because a second copy of the gate's logic is how
/// two copies of one judgement come to disagree.
pub fn publish<D: Digest, S: Signer>(
    candidate: &Candidate,
    verifier: &dyn Verifier,
    budget: Budget,
    digest: &D,
    signer: &S,
) -> Result<Published, PublishError> {
    let mut report = report_for(candidate, verifier, budget).map_err(PublishError::Unaskable)?;

    match report.publishable() {
        Err(NotPublishable::NoContentId) | Ok(()) => {}
        Err(reason) => {
            return Err(PublishError::Refused {
                reason,
                report: Box::new(report),
            })
        }
    }

    let manifest = manifest_for(candidate, &report);
    let sealed: Sealed = seal(&manifest, digest, signer).map_err(PublishError::Encoding)?;
    report.bind_to(&sealed);

    debug_assert_eq!(
        report.publishable(),
        Ok(()),
        "a report that only lacked a content id must be publishable once it has one"
    );

    Ok(Published {
        report,
        package: sealed.bytes,
    })
}
