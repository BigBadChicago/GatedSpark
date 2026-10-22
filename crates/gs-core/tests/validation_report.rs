//! Can a validation report claim more than was actually proved?
//!
//! The report is what a later audit, a corpus re-verification, or a player's bug
//! report will be read against. A report that says "fine" more readily than the gate
//! does would make every one of those jobs unreliable, so these tests are mostly
//! attempts to get a "yes" out of a report that has not earned one.

use gs_core::content::{ContentId, Digest};
use gs_core::hash::StateHash;
use gs_core::registry::FamilyId;
use gs_core::report::{Measurement, NotPublishable, Replay, ValidationReport};
use gs_core::ruleset::RulesetId;
use gs_core::verification::{Outcome, Query, Refusal};

/// A stand-in digest, for tests only.
///
/// Deliberately not a hash: it copies the first bytes it is given. Using it anywhere
/// real would be a security hole, which is exactly why it lives in a test file and
/// why `content::Digest` still has no production implementation (ADR 0004). It is
/// here so the report's *structure* can be tested without waiting on that decision.
struct NotAHash;

impl Digest for NotAHash {
    fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (slot, byte) in out.iter_mut().zip(bytes) {
            *slot = *byte;
        }
        out
    }
}

fn ruleset() -> RulesetId {
    RulesetId {
        family: FamilyId("nonogram"),
        version: 1,
    }
}

fn solved_queries() -> Vec<Query> {
    vec![
        Query {
            name: "solvability",
            required: Outcome::Sat,
            returned: Outcome::Sat,
        },
        Query {
            name: "uniqueness",
            required: Outcome::Unsat,
            returned: Outcome::Unsat,
        },
    ]
}

fn good_report() -> ValidationReport {
    ValidationReport {
        content_id: Some(ContentId::of(&NotAHash, b"packaged bytes")),
        ruleset: ruleset(),
        generator_version: 1,
        seed: 1_058,
        queries: solved_queries(),
        encoding: "qf_uf one bool per cell, exact line disjunctions",
        measurements: vec![
            Measurement {
                name: "propagation_rounds",
                value: 4,
            },
            Measurement {
                name: "decision_points",
                value: 0,
            },
        ],
        band: Some("gentle"),
        replay: Replay::Reproduced(StateHash(0xab64_d17d_0a37_fa41)),
    }
}

#[test]
fn a_complete_report_is_publishable() {
    // The control. Without it, every test below could pass because nothing is ever
    // publishable, which would be a vacuous suite.
    assert_eq!(good_report().publishable(), Ok(()));
}

#[test]
fn a_report_with_no_queries_refuses_rather_than_defaulting_to_yes() {
    let mut report = good_report();
    report.queries.clear();
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Failed(
            "no_queries_asserted"
        )))
    );
}

#[test]
fn an_undecided_query_is_not_a_pass() {
    // The central claim of the whole platform, checked at the report layer too: a
    // timeout means nobody knows, which is not the same as a refutation and must
    // never read as a pass.
    let mut report = good_report();
    report.queries[1].returned = Outcome::Unknown;
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Undecided(
            "uniqueness"
        )))
    );
    assert!(report.has_undecided_query());
}

#[test]
fn an_undecided_query_is_still_findable_in_a_report_refused_for_another_reason() {
    // What an audit needs. A report refused for a missing witness must still show
    // that a query timed out, or the timeout disappears from the record.
    let mut report = good_report();
    report.queries[0].returned = Outcome::Unknown;
    report.replay = Replay::NotAttempted;
    assert!(report.has_undecided_query());
    assert!(report.publishable().is_err());
}

#[test]
fn a_puzzle_with_two_answers_is_not_publishable() {
    // The uniqueness re-query came back sat, meaning a second answer exists.
    let mut report = good_report();
    report.queries[1].returned = Outcome::Sat;
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Failed("uniqueness")))
    );
    assert!(
        !report.has_undecided_query(),
        "a decisive wrong answer is not an undecided one"
    );
}

#[test]
fn a_witness_the_engine_refused_is_not_publishable() {
    let mut report = good_report();
    report.replay = Replay::Refused("final hash disagreed");
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::WitnessNotReproduced)
    );
}

#[test]
fn a_witness_never_replayed_is_not_publishable_either() {
    // Not attempted and refused are different facts and both block publication.
    // Treating "not attempted" as acceptable is how an unchecked witness ships.
    let mut report = good_report();
    report.replay = Replay::NotAttempted;
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::WitnessNotReproduced)
    );
    assert!(!Replay::NotAttempted.reproduced());
}

#[test]
fn a_report_with_no_content_id_is_not_publishable() {
    // The current honest state of the world: no digest implementation, so no content
    // id, so nothing can be published. That is the right failure, not a stub id.
    let mut report = good_report();
    report.content_id = None;
    assert_eq!(report.publishable(), Err(NotPublishable::NoContentId));
}

#[test]
fn a_missing_measurement_is_not_a_zero() {
    let report = good_report();
    assert_eq!(report.measurement("decision_points"), Some(0));
    assert_eq!(report.measurement("max_search_depth"), None);
}

#[test]
fn an_ungraded_puzzle_is_recorded_as_ungraded() {
    // The game may grade a puzzle into no level. The report has to be able to say so
    // rather than inventing one.
    let mut report = good_report();
    report.band = None;
    assert_eq!(
        report.publishable(),
        Ok(()),
        "being ungraded is not a fault"
    );
    assert_eq!(report.band, None);
}

#[test]
fn the_encoding_a_uniqueness_claim_is_relative_to_is_recorded() {
    // Uniqueness is only ever true relative to an encoding. A report that did not
    // name one would be claiming more than was proved.
    assert!(!good_report().encoding.is_empty());
}

#[test]
fn two_reports_of_the_same_evidence_are_equal() {
    // So a corpus re-verification can compare a fresh report against a stored one and
    // find a difference, rather than needing a bespoke comparison per field.
    assert_eq!(good_report(), good_report());
}
