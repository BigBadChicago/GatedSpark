//! Building a validation report from a candidate, with a scripted solver.
//!
//! These tests use a fake verifier that returns whatever the test tells it to. The
//! point is not to check a solver — `solver_agreement` does that with a real one —
//! but to check what the pipeline does with each answer, including the answers a real
//! solver gives when something has gone wrong.

use gs_core::report::{NotPublishable, Replay};
use gs_core::verification::{Outcome, Refusal};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::generate::Candidate;
use gs_nonogram::publish::{filter_and_solver_agree, report_for, Verifier, ENCODING};
use gs_nonogram::solve::Budget;

/// Answers whatever it was told to, in order.
struct Scripted {
    answers: Vec<Outcome>,
    asked: std::cell::RefCell<usize>,
}

impl Scripted {
    fn new(answers: &[Outcome]) -> Self {
        Scripted {
            answers: answers.to_vec(),
            asked: std::cell::RefCell::new(0),
        }
    }
}

impl Verifier for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn ask(&self, query: &str) -> Outcome {
        assert!(
            query.contains("(check-sat)"),
            "the pipeline handed the verifier something that is not a query"
        );
        let mut asked = self.asked.borrow_mut();
        let answer = self.answers[*asked];
        *asked += 1;
        answer
    }
}

fn a_candidate() -> Candidate {
    (0..400u64)
        .find_map(|seed| propose_in_band(&BANDS[0], seed, Budget::default()))
        .expect("the easiest level produces something in 400 seeds")
}

#[test]
fn a_report_records_the_seed_the_ruleset_and_the_encoding() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert_eq!(report.seed, candidate.seed);
    assert_eq!(report.ruleset.family.0, "nonogram");
    assert_eq!(report.encoding, ENCODING);
    assert_eq!(report.queries.len(), 2);
    assert_eq!(report.band, Some("gentle"));
    assert!(report.measurement("propagation_rounds").is_some());
}

#[test]
fn the_witness_is_replayed_through_the_engine() {
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&a_candidate(), &verifier, Budget::default()).expect("askable");
    assert!(
        matches!(report.replay, Replay::Reproduced(_)),
        "got {:?}",
        report.replay
    );
}

#[test]
fn the_only_thing_standing_between_a_good_report_and_publication_is_the_missing_digest() {
    // The precise current state of the world, asserted rather than described. When a
    // digest exists, this test is what says the rest of the pipeline was already
    // complete.
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&a_candidate(), &verifier, Budget::default()).expect("askable");
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::NoContentId),
        "something other than the digest is blocking publication"
    );
}

#[test]
fn a_solver_timeout_is_recorded_and_refused_not_retried() {
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unknown]);
    let report = report_for(&a_candidate(), &verifier, Budget::default()).expect("askable");

    assert!(report.has_undecided_query());
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Undecided(
            "uniqueness_blocking_the_witness"
        )))
    );
}

#[test]
fn a_solver_that_disagrees_with_the_filter_is_recorded_as_disagreeing() {
    // The serious case. Our filter said one answer; the solver says there is a
    // second. The report must carry the solver's answer, not ours.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Sat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert_eq!(report.queries[1].returned, Outcome::Sat);
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Failed(
            "uniqueness_blocking_the_witness"
        )))
    );
    assert!(
        !filter_and_solver_agree(&candidate, &report),
        "a disagreement was reported as agreement"
    );
}

#[test]
fn agreement_is_only_claimed_when_both_answers_match() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert!(filter_and_solver_agree(&candidate, &report));
}

#[test]
fn an_undecided_solver_answer_is_not_agreement() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Unknown, Outcome::Unknown]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert!(
        !filter_and_solver_agree(&candidate, &report),
        "nobody knowing was treated as agreeing"
    );
}

#[test]
fn the_uniqueness_query_is_the_second_one_asked() {
    // Order matters, because the uniqueness query is the one that needs the witness.
    // A pipeline that asked them the other way round would be blocking a witness it
    // had not yet established.
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&a_candidate(), &verifier, Budget::default()).expect("askable");
    assert_eq!(report.queries[0].name, "solvability");
    assert_eq!(report.queries[0].required, Outcome::Sat);
    assert_eq!(report.queries[1].name, "uniqueness_blocking_the_witness");
    assert_eq!(report.queries[1].required, Outcome::Unsat);
}

#[test]
fn two_reports_for_the_same_candidate_are_identical() {
    // So a corpus re-verification can compare a fresh report with a stored one.
    let candidate = a_candidate();
    let first = report_for(
        &candidate,
        &Scripted::new(&[Outcome::Sat, Outcome::Unsat]),
        Budget::default(),
    )
    .expect("askable");
    let second = report_for(
        &candidate,
        &Scripted::new(&[Outcome::Sat, Outcome::Unsat]),
        Budget::default(),
    )
    .expect("askable");
    assert_eq!(first, second);
}
