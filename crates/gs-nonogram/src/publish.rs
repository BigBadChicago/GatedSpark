//! From a candidate to a validation report: the step that asks the solver.
//!
//! This is where the cheap filter stops being the authority. Everything up to here
//! produced a [`Candidate`], meaning the line solver believes the puzzle has one
//! answer. This module asks a real solver the two questions that matter and writes
//! down what it said, whatever that was.
//!
//! # The solver is passed in, not reached for
//!
//! [`Verifier`] is a trait, defined in the core, and this crate implements none of it.
//! Running a solver means launching a program, which belongs to whatever is driving
//! the pipeline rather than to a puzzle family; the real implementation is in
//! `gs-solver`. Two things follow and both are deliberate:
//!
//! * This crate cannot accidentally become a solver, or quietly answer its own
//!   questions. It has no way to produce an [`Outcome`] except by being handed one.
//! * A caller can hand in two independent solvers and compare, which is the ADR 0002
//!   cross-check.
//!
//! # Nothing here decides to publish
//!
//! [`report_for`] builds a report and returns it. Whether it may be published is
//! [`ValidationReport::publishable`], which is
//! [`gs_core::verification::decide`] over the queries. A report of a refused puzzle is
//! a useful record, not an error, so the refusal appears inside the report rather
//! than replacing it.

use gs_core::content::Digest;
use gs_core::package::{seal, EncodeError as PackageEncodeError, Manifest, Sealed, Signer};
use gs_core::report::{Measurement, NotPublishable, Replay, ValidationReport};
use gs_core::ruleset::Ruleset;
use gs_core::state::State;
use gs_core::verification::{Outcome, Query};

/// Re-exported from the core, where it now lives: an SMT-LIB2 query is a string and
/// the trait says nothing about Nonogram. See [`gs_core::verification::Verifier`] for
/// the rule it carries, which is that a solver that could not answer must say
/// [`Outcome::Unknown`] and never `Unsat`.
pub use gs_core::verification::Verifier;

use crate::generate::Candidate;
use crate::rules::Nonogram;
use crate::smtlib::{solvability_query, uniqueness_query, EncodeError};
use crate::solve::{solve, Budget, Solved};

/// What the encoding's uniqueness claim is relative to.
///
/// Recorded in every report, because uniqueness is only ever true relative to an
/// encoding. One definition, so a report cannot describe an encoding other than the
/// one `smtlib` emits.
pub const ENCODING: &str = "QF_UF, one Bool per cell, exact per-line placement disjunctions";

/// Why a report could not be built at all.
///
/// Distinct from a report that says "do not publish". This is the pipeline failing to
/// ask the question, not an answer to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unaskable {
    /// The puzzle could not be encoded. Carries the encoder's reason.
    Encoding(EncodeError),
    /// The filter found no witness, so there is nothing to block for the uniqueness
    /// re-query. A candidate should always have one, so this would be a bug upstream
    /// rather than a property of the puzzle.
    NoWitness,
}

/// Ask a solver about a candidate and write down what it said.
///
/// Two queries, in this order:
///
/// 1. **Solvability.** Is there any board matching these clues? Must be `sat`.
/// 2. **Uniqueness.** With the witness blocked, is there *another* board? Must be
///    `unsat`. This is the question the whole platform exists to ask, and it is why
///    a witness is needed before it can be asked at all.
///
/// The returned report carries whatever the solver actually said. If it disagrees
/// with the filter, the report records the disagreement and refuses publication; it
/// does not retry, reinterpret, or prefer our own answer.
pub fn report_for(
    candidate: &Candidate,
    verifier: &dyn Verifier,
    budget: Budget,
) -> Result<ValidationReport, Unaskable> {
    let analysis = solve(&candidate.puzzle, budget);
    let witness = analysis.witness.ok_or(Unaskable::NoWitness)?;

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
        // No content id yet, and that is the correct state for a report at this point
        // rather than a gap. A package's fingerprint is the fingerprint of the package,
        // which does not exist until the report has been turned into one; `publish`
        // below seals it and binds the id back. So this report is deliberately not
        // publishable, with `NoContentId` as its only remaining objection.
        content_id: None,
        ruleset: Ruleset::id(&candidate.puzzle),
        generator_version: GENERATOR_VERSION,
        seed: candidate.seed,
        queries,
        encoding: ENCODING,
        measurements: measurements_of(candidate),
        band: crate::difficulty::band_of(candidate.shape.dims, &candidate.features)
            .map(|band| band.name),
        replay: replay_of(&candidate.puzzle, &witness),
    })
}

/// Bumped when a change to generation would produce different puzzles from the same
/// seed. A report records it so an old report is never mistaken for a current one.
pub const GENERATOR_VERSION: u32 = 1;

/// The difficulty numbers, with the names the game gives them.
fn measurements_of(candidate: &Candidate) -> Vec<Measurement> {
    let f = &candidate.features;
    vec![
        Measurement {
            name: "propagation_rounds",
            value: f.propagation_rounds,
        },
        Measurement {
            name: "propagation_only_cells",
            value: f.propagation_only_cells,
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
            name: "cells",
            value: f.cells,
        },
    ]
}

/// Replay the witness through the real engine.
///
/// A board a solver is happy with is not yet a board the engine accepts, and the
/// engine is what a player will run. This checks the thing that ships: the witness
/// satisfies the ruleset's own goal test, reached through `Ruleset::is_solved` rather
/// than by re-reading the clues here.
fn replay_of(puzzle: &Nonogram, witness: &State) -> Replay {
    if witness.dims() != puzzle.clues.dims {
        return Replay::Refused("witness is not the shape of the board");
    }
    if !puzzle.is_solved(witness) {
        return Replay::Refused("the engine does not consider the witness solved");
    }
    Replay::Reproduced(gs_core::hash::StateHash::of(witness))
}

/// Whether the solver agreed with the filter.
///
/// Separate from the report so a caller can tell the two failures apart: a puzzle the
/// solver refuses, and a puzzle where our own filter said something different. The
/// second is a bug in us and much more serious.
#[must_use]
pub fn filter_and_solver_agree(candidate: &Candidate, report: &ValidationReport) -> bool {
    let filter = solve(&candidate.puzzle, Budget::default()).solved;
    let solvability = report.queries.first().map(|q| q.returned);
    let uniqueness = report.queries.get(1).map(|q| q.returned);
    match filter {
        Solved::One => solvability == Some(Outcome::Sat) && uniqueness == Some(Outcome::Unsat),
        Solved::Several => solvability == Some(Outcome::Sat) && uniqueness == Some(Outcome::Sat),
        Solved::None => solvability == Some(Outcome::Unsat),
        // Our own search gave up, so there is nothing to agree or disagree with. Not
        // treated as agreement: an exhausted budget is not an answer.
        Solved::Undecided => false,
    }
}

/// Turn a candidate and its report into the body of a publishable package.
///
/// The payload is [`Clues::canonical_bytes`] and nothing else. In particular the
/// *pattern* the generator started from is not carried: the pipeline throws it away
/// deliberately (see [`crate::generate`]), and a package that shipped the intended
/// picture alongside the clues would be shipping the answer.
///
/// The witness is not carried either. It lives in the validation report, which stays
/// on the server, because a package is what a player receives.
///
/// This function does not seal anything. Sealing needs a digest and a signing key,
/// neither of which this crate has or should have; see [`publish`] and
/// [`gs_core::package::seal`].
#[must_use]
pub fn manifest_for(candidate: &Candidate, report: &ValidationReport) -> Manifest {
    Manifest::from_report(report, candidate.puzzle.clues.canonical_bytes())
}

/// A puzzle that passed every check, the evidence for it, and the file a player gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The evidence, now carrying the fingerprint of the package below.
    pub report: ValidationReport,
    /// The package file, signed.
    pub package: Vec<u8>,
}

/// Why a candidate did not become a published package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The question could not be asked at all.
    Unaskable(Unaskable),
    /// The question was asked and the answer was no. Carries the gate's own refusal,
    /// so a caller can tell a solver that refuted the puzzle from one that timed out.
    ///
    /// A refused candidate is an ordinary outcome of generation, not a malfunction. The
    /// report that explains it is carried along, because that record is worth keeping:
    /// it is what a later audit reads to find out why a puzzle was dropped.
    Refused {
        reason: NotPublishable,
        report: Box<ValidationReport>,
    },
    /// The manifest could not be written — some field is longer than the format can
    /// express.
    Encoding(PackageEncodeError),
}

/// The whole loop: ask the solver, write the evidence, and seal a package only if the
/// evidence says yes.
///
/// # The order is the point
///
/// 1. Build the report, which asks the solver and replays the witness.
/// 2. Put it to [`ValidationReport::publishable`] — the one gate — and go on **only**
///    if its single remaining objection is the missing content id. Any other refusal
///    ends here, with the report kept as the record of why.
/// 3. Seal the package. This is the first point at which anything is signed, so
///    nothing is ever signed before the gate has agreed to it.
/// 4. Bind the report to the package that was just sealed, which is where the content
///    id comes from.
/// 5. Ask the gate again. It must now say yes, and if it does not, that is a bug here
///    rather than a property of the puzzle, so it is an assertion and not an error.
///
/// Step 2 deliberately treats [`NotPublishable::NoContentId`] as the go-ahead rather
/// than re-checking the queries and the replay by hand. Re-checking them would be a
/// second copy of the gate's logic, and two copies of a judgement is how the two come
/// to disagree.
pub fn publish<D: Digest, S: Signer>(
    candidate: &Candidate,
    verifier: &dyn Verifier,
    budget: Budget,
    digest: &D,
    signer: &S,
) -> Result<Published, PublishError> {
    let mut report = report_for(candidate, verifier, budget).map_err(PublishError::Unaskable)?;

    match report.publishable() {
        // The expected state of a fresh report: everything checked out and the only
        // thing missing is the fingerprint of a package that does not exist yet.
        Err(NotPublishable::NoContentId) => {}
        // Already bound. Not an error — sealing the same report again gives the same
        // bytes, because nothing in the manifest comes from a clock or a counter.
        Ok(()) => {}
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
