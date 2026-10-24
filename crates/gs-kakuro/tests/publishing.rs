//! Building a validation report from a candidate, with a scripted solver.
//!
//! The solver here is a fake that answers whatever the test tells it to. The point is
//! not to check a solver — `solver_agreement` does that with real ones — but to check
//! what the pipeline does with each answer, including the answers a real solver gives
//! when something has gone wrong.
//!
//! One thing is worth saying for this family in particular. Our own solver prunes with
//! a relaxation, so "our solver says one answer" is a weaker claim here than it is for
//! Nonogram. Nothing in `publish` leans on it: the report carries what the real solver
//! said, and `filter_and_solver_agree` is where our count is held against theirs. The
//! tests below check that the pipeline refuses on the solver's word rather than ours,
//! including when ours is the more confident of the two.

mod common;

use common::from_answer;
use gs_core::report::{NotPublishable, Replay};
use gs_core::state::{Cell, State};
use gs_core::verification::{Outcome, Refusal};
use gs_kakuro::generate::{propose, Candidate, Shape};
use gs_kakuro::publish::{
    filter_and_solver_agree, replay_of, report_for, Unaskable, Verifier, ENCODING,
};
use gs_kakuro::solve::Budget;

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
    propose(1, Shape::middling(5, 5), Budget::default()).expect("a puzzle")
}

#[test]
fn a_report_records_the_seed_the_ruleset_and_the_encoding() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert_eq!(report.seed, candidate.seed);
    assert_eq!(report.ruleset.family.0, "kakuro");
    assert_eq!(report.encoding, ENCODING);
    assert!(
        report.encoding.contains("all-different"),
        "the recorded encoding has to name the rule the claim depends on, and for this \
         family that is the no-repeats rule: {}",
        report.encoding
    );
    // Not QF_UFLIA. This family declares no helper variables, which is why its
    // blocking constraint can be over the digits alone.
    assert!(report.encoding.contains("QF_LIA"), "{}", report.encoding);
    assert!(report.replay.reproduced());
}

#[test]
fn the_report_carries_the_number_this_family_is_least_proud_of() {
    // `given_away_squares` is in the measurements rather than left out. A number nobody
    // can see does not get fixed, and a report is where an audit would look.
    //
    // It worked: built boards hold it at zero (ADR 0021). So the list now also carries
    // the number that fix cost, `digits_at_the_ends_per_mille`, which is this family's
    // next one to bring down, and `swap_rectangles`, which is what its uniqueness
    // actually turns on.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    let named: Vec<&str> = report.measurements.iter().map(|m| m.name).collect();
    for wanted in [
        "given_away_squares",
        "single_square_runs",
        "white_squares",
        "runs",
        "rounds_of_repair_while_generating",
        "swap_rectangles",
        "digits_at_the_ends_per_mille",
    ] {
        assert!(
            named.contains(&wanted),
            "{wanted} is missing from {named:?}"
        );
    }
}

#[test]
fn no_difficulty_level_is_assigned() {
    // On purpose, for the third family in a row: levels are each game's to decide, and
    // this one has no measured play data. A guessed threshold would sort puzzles into
    // levels that mean nothing and would fail quietly rather than loudly.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert_eq!(report.band, None);
}

#[test]
fn a_report_the_solver_vouched_for_is_publishable_once_it_has_a_fingerprint() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    // Its only remaining objection is the missing content id, which is what the
    // publish loop treats as the go-ahead.
    assert_eq!(report.publishable(), Err(NotPublishable::NoContentId));
}

#[test]
fn a_second_answer_is_refused_however_sure_our_own_solver_was() {
    // Our own solver believes this candidate has one answer — that is what being a
    // candidate means. The real solver says there is another. The gate takes the
    // solver's word.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Sat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    match report.publishable() {
        Err(NotPublishable::Verification(Refusal::Failed(name))) => {
            assert_eq!(name, "uniqueness_blocking_the_witness");
        }
        other => panic!("a second answer must be refused, got {other:?}"),
    }
    assert!(
        !filter_and_solver_agree(&candidate, &report),
        "our solver said one answer and the real one said two; that is a disagreement"
    );
}

#[test]
fn dont_know_is_never_read_as_proof() {
    // The rule the whole platform rests on: a solver that could not answer has not
    // said no. An undecided uniqueness query must not publish.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unknown]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert!(report.has_undecided_query());
    match report.publishable() {
        Err(NotPublishable::Verification(Refusal::Undecided(name))) => {
            // Undecided, not Failed. The distinction is the whole platform: a solver
            // that could not answer has not said no.
            assert_eq!(name, "uniqueness_blocking_the_witness");
        }
        other => panic!("an undecided query must be refused as undecided, got {other:?}"),
    }
}

#[test]
fn an_unsolvable_puzzle_is_refused_at_the_first_question() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Unsat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert!(report.publishable().is_err());
}

#[test]
fn a_witness_of_the_wrong_shape_is_refused() {
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    let wrong_shape = State::blank(gs_core::state::Dims::new(99, 1));
    assert_eq!(
        replay_of(&puzzle, &wrong_shape),
        Replay::Refused("the witness is not the shape of the board")
    );
}

#[test]
fn a_witness_the_engine_does_not_accept_is_refused() {
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    assert!(replay_of(&puzzle, &answer).reproduced());

    // Change one digit. Still the right shape, still every square filled, and the
    // totals no longer work.
    let mut spoiled = answer.clone();
    spoiled.set(0, Cell(9)).expect("square exists");
    assert_eq!(
        replay_of(&puzzle, &spoiled),
        Replay::Refused("the engine does not consider the witness solved")
    );
}

#[test]
fn a_half_filled_witness_is_refused_by_the_rules_themselves() {
    // The thing that is different about this family, written down. Nonogram and
    // Slitherlink both had to add a separate check that a stored answer decides every
    // cell, because their `is_solved` deliberately permits an unfinished board. A
    // Kakuro with an empty square has a run that does not meet its total, so the
    // ordinary rule already refuses it and there is no second check to add.
    let (puzzle, answer) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    let mut gap = answer.clone();
    gap.set(0, Cell::UNKNOWN).expect("square exists");
    assert_eq!(
        replay_of(&puzzle, &gap),
        Replay::Refused("the engine does not consider the witness solved"),
        "an empty square must be refused by `is_solved`, not by an extra check"
    );
}

#[test]
fn a_puzzle_that_cannot_be_encoded_is_unaskable_rather_than_unpublishable() {
    // There is no way to build an unencodable Kakuro through the public API — every
    // rule this family emits is one `smtlib` supports — so this checks the shape of
    // the distinction rather than reaching it: a report that could not be *built* is a
    // different thing from a report that says no, and `Unaskable` is how the pipeline
    // says which happened.
    //
    // The case that can be reached is a missing witness, and it is reached by handing
    // the pipeline a budget too small to solve anything.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let no_room = Budget {
        max_nodes: 0,
        max_depth: 0,
    };
    match report_for(&candidate, &verifier, no_room) {
        Err(Unaskable::NoWitness) => {}
        // A board whose propagation alone finishes it needs no search, so an exhausted
        // budget still yields a witness. That is a fair outcome and not a failure of
        // the distinction being checked.
        Ok(report) => assert!(report.replay.reproduced()),
        other => panic!("expected no witness or a sound report, got {other:?}"),
    }
}

#[test]
fn the_payload_is_the_board_and_not_the_answer() {
    // A package is what a player receives. Shipping the digits the generator filled in
    // would be shipping the answer, which is why the payload is the black squares and
    // the totals and nothing else.
    let candidate = a_candidate();
    let payload = candidate.puzzle.canonical_bytes();
    let answer = candidate.witness.canonical_bytes();

    assert!(!payload.is_empty());
    assert!(
        !contains(&payload, &answer),
        "the answer's bytes appear inside the payload"
    );
    // Read back and compared, so this is a claim about the puzzle rather than about
    // byte lengths: what the payload decodes to is the puzzle, with no digits in it.
    let back = gs_kakuro::clues::from_canonical_bytes(&payload).expect("the payload decodes");
    assert_eq!(back, candidate.puzzle);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}
