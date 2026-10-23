//! From a candidate to a validation report: the step that asks a real solver.
//!
//! The same shape as `gs_nonogram::publish`, deliberately, because the pipeline is
//! the platform's and not the family's. Everything up to here produced a
//! [`Candidate`], meaning *our own* fast solver believes the puzzle has one answer.
//! This asks a real solver the two questions that matter and writes down what it
//! said, whatever that was.
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
use crate::rules::Slitherlink;
use crate::smtlib::{solvability_query, uniqueness_query, EncodeError};
use crate::solve::{solve, Budget, Solved};

/// What the encoding's uniqueness claim is relative to.
///
/// Recorded in every report, because uniqueness is only ever true relative to an
/// encoding. One definition, so a report cannot describe an encoding other than the
/// one `smtlib` emits. The distance labels are named because they are the whole of
/// the one-loop rule; see ADR 0013.
pub const ENCODING: &str =
    "QF_UFLIA, one Bool per edge, exact per-cell side counts, nought-or-two per dot, \
     one loop by root-and-distance labels";

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
/// 1. **Solvability.** Is there any board satisfying this puzzle? Must be `sat`.
/// 2. **Uniqueness.** With our answer ruled out, is there *another* board? Must be
///    `unsat`. This is the question the platform exists to ask, and it is why a
///    witness is needed before it can be asked at all.
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
        // **No difficulty level, on purpose.** Levels are the game's to decide
        // (settled 2026-10-22), and this game has no measured data to set them from
        // yet. A threshold guessed now would sort puzzles into levels that mean
        // nothing, and would fail silently rather than loudly. `examples/sweep.rs`
        // is the tool that will produce the numbers; the levels come after them.
        band: None,
        replay: replay_of(&candidate.puzzle, &witness),
    })
}

/// The difficulty numbers, with the names this game gives them.
fn measurements_of(candidate: &Candidate) -> Vec<Measurement> {
    let f = &candidate.features;
    vec![
        Measurement {
            name: "forced_steps",
            value: f.forced_steps,
        },
        Measurement {
            name: "propagation_only_edges",
            value: f.propagation_only_edges,
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
            name: "edges",
            value: f.edges,
        },
        Measurement {
            name: "clues_given",
            value: f.clues_given,
        },
        Measurement {
            name: "clues_removed",
            value: candidate.clues_removed,
        },
        Measurement {
            name: "uniqueness_questions_while_generating",
            value: candidate.questions_asked,
        },
    ]
}

/// Replay the witness through the real engine.
///
/// A board a solver is happy with is not yet a board the engine accepts, and the
/// engine is what a player runs. This checks the thing that ships, through
/// [`Ruleset::is_solved`] rather than by re-reading the clues here.
///
/// It also requires **every edge to be decided**, which `is_solved` deliberately does
/// not. The two are different questions and are asked separately, exactly as in
/// Nonogram: a player has finished when the drawn lines are right, while a stored
/// answer is a whole board. One with undecided edges in it would be a worse record of
/// what the solver found and would hash to a different value, so the store's copy
/// would no longer be the answer anyone checked.
///
/// Public so the judgement can be tested directly. `report_for` finds its own witness
/// by solving, so there is no way to hand it a half-marked one from outside.
#[must_use]
pub fn replay_of(puzzle: &Slitherlink, witness: &State) -> Replay {
    let geometry = puzzle.geometry();
    if witness.dims() != geometry.state_dims() {
        return Replay::Refused("the witness is not the shape of the board");
    }
    let every_edge_decided = geometry
        .edges()
        .all(|e| witness.get(e.0).map(|c| !c.is_unknown()) == Ok(true));
    if !every_edge_decided {
        return Replay::Refused("the witness leaves an edge undecided");
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
/// second is a fault in us and far more serious.
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
/// The payload is the clue grid's canonical bytes and nothing else. The loop the
/// generator drew is not carried: the pipeline throws it away on purpose, and a
/// package shipping the loop alongside the numbers would be shipping the answer.
///
/// The witness is not carried either. It belongs to the validation report, which
/// stays on the server, because a package is what a player receives.
#[must_use]
pub fn manifest_for(candidate: &Candidate, report: &ValidationReport) -> Manifest {
    Manifest::from_report(report, candidate.puzzle.clues.canonical_bytes())
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
/// The order is the point, and it is the same order Nonogram publishes in:
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
