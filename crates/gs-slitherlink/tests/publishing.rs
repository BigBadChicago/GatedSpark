//! Building a validation report from a candidate, with a scripted solver.
//!
//! The solver here is a fake that answers whatever the test tells it to. The point is
//! not to check a solver — `solver_agreement` does that with real ones — but to check
//! what the pipeline does with each answer, including the answers a real solver gives
//! when something has gone wrong.

mod common;

use common::{board, rectangle_loop};
use gs_core::report::{NotPublishable, Replay};
use gs_core::state::State;
use gs_core::verification::{Outcome, Refusal};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::generate::{propose, Candidate, Shape, REMOVAL_BUDGET};
use gs_slitherlink::publish::{filter_and_solver_agree, replay_of, report_for, Verifier, ENCODING};
use gs_slitherlink::rules::{Slitherlink, LINE, NO_LINE};
use gs_slitherlink::solve::Budget;

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
    propose(0, Shape::middling(5, 5), REMOVAL_BUDGET).expect("a puzzle")
}

#[test]
fn a_report_records_the_seed_the_ruleset_and_the_encoding() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert_eq!(report.seed, candidate.seed);
    assert_eq!(report.ruleset.family.0, "slitherlink");
    assert_eq!(report.encoding, ENCODING);
    assert!(
        report.encoding.contains("one loop"),
        "the recorded encoding has to name the rule the claim depends on: {}",
        report.encoding
    );
    assert!(report.replay.reproduced());
}

#[test]
fn the_two_questions_are_asked_in_order_and_both_recorded() {
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");

    assert_eq!(report.queries.len(), 2);
    assert_eq!(report.queries[0].name, "solvability");
    assert_eq!(report.queries[0].required, Outcome::Sat);
    assert_eq!(report.queries[1].name, "uniqueness_blocking_the_witness");
    assert_eq!(report.queries[1].required, Outcome::Unsat);
}

#[test]
fn a_second_answer_refuses_publication() {
    // The solver found another board. Our own solver thought there was only one, so
    // this also records a disagreement, which is the more serious of the two.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Sat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Failed(
            "uniqueness_blocking_the_witness"
        )))
    );
    assert!(
        !filter_and_solver_agree(&candidate, &report),
        "our solver said one answer and the real one said two: that is a disagreement"
    );
}

#[test]
fn an_unanswered_question_is_recorded_as_undecided_and_refuses() {
    // The rule the whole platform turns on. A timeout is not a proof, so it must not
    // read as one, and it must stay distinguishable from a refutation in the record.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unknown]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert_eq!(
        report.publishable(),
        Err(NotPublishable::Verification(Refusal::Undecided(
            "uniqueness_blocking_the_witness"
        )))
    );
    assert_eq!(report.queries[1].returned, Outcome::Unknown);
}

#[test]
fn a_fresh_report_is_held_back_only_by_its_missing_fingerprint() {
    // What a correct report looks like before anything is sealed. Everything checked
    // out and the only objection left is the fingerprint of a package that does not
    // exist yet.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert_eq!(report.publishable(), Err(NotPublishable::NoContentId));
}

#[test]
fn the_measurements_carry_the_cost_of_making_the_puzzle() {
    // The number that decides how fast a pool can be filled travels with the puzzle,
    // so it can be read back later instead of re-measured.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    let named = |name: &str| {
        report
            .measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.value)
    };
    assert_eq!(
        named("uniqueness_questions_while_generating"),
        Some(candidate.questions_asked)
    );
    assert_eq!(named("clues_removed"), Some(candidate.clues_removed));
    assert_eq!(
        named("clues_given"),
        Some(candidate.puzzle.clues.given_count())
    );
    assert!(named("forced_steps").is_some());
    assert!(named("edges").is_some());
}

#[test]
fn no_difficulty_level_is_claimed_yet() {
    // Deliberate, and worth a test so it does not quietly become a guess later.
    // Levels are the game's to set, and this game has no measured data to set them
    // from. An invented threshold would sort puzzles into levels that mean nothing
    // and would fail silently.
    let candidate = a_candidate();
    let verifier = Scripted::new(&[Outcome::Sat, Outcome::Unsat]);
    let report = report_for(&candidate, &verifier, Budget::default()).expect("askable");
    assert_eq!(report.band, None);
}

#[test]
fn a_witness_that_leaves_an_edge_undecided_is_refused() {
    // A player has finished when the drawn lines are right, whatever they left
    // unmarked. A *stored answer* is a whole board. The two judgements are separate
    // on purpose, and this is the one that is stricter.
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    let puzzle = Slitherlink::new(Clues::of(&answer));

    let whole = answer.as_state();
    assert!(matches!(replay_of(&puzzle, &whole), Replay::Reproduced(_)));

    // The same board with one ruled-out edge rubbed back out. Still finished as far
    // as a player is concerned, and not a stored answer.
    let mut half_marked = whole.clone();
    let undecide = g
        .edges()
        .find(|e| !answer.is_drawn(*e))
        .expect("an edge not in the loop");
    half_marked
        .set(undecide.0, gs_core::state::Cell::UNKNOWN)
        .expect("on the board");
    assert!(
        gs_core::ruleset::Ruleset::is_solved(&puzzle, &half_marked),
        "a player would be finished"
    );
    assert_eq!(
        replay_of(&puzzle, &half_marked),
        Replay::Refused("the witness leaves an edge undecided"),
        "and a stored answer would not be accepted"
    );
}

#[test]
fn a_witness_of_the_wrong_shape_is_refused() {
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    let puzzle = Slitherlink::new(Clues::of(&answer));
    let wrong = State::blank(gs_core::state::Dims::new(4, 1));
    assert_eq!(
        replay_of(&puzzle, &wrong),
        Replay::Refused("the witness is not the shape of the board")
    );
}

#[test]
fn a_witness_the_engine_will_not_accept_is_refused() {
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    let puzzle = Slitherlink::new(Clues::of(&answer));
    // Every edge decided, and all of them ruled out: a complete board that is not an
    // answer. The refusal has to name the engine rather than the shape.
    let mut nothing_drawn = State::blank(g.state_dims());
    for edge in g.edges() {
        nothing_drawn.set(edge.0, NO_LINE).expect("on the board");
    }
    assert_eq!(
        replay_of(&puzzle, &nothing_drawn),
        Replay::Refused("the engine does not consider the witness solved")
    );
    // And the other direction, so the test is not just about blank boards: a board
    // with every edge drawn is not a loop either.
    let mut all_drawn = State::blank(g.state_dims());
    for edge in g.edges() {
        all_drawn.set(edge.0, LINE).expect("on the board");
    }
    assert_eq!(
        replay_of(&puzzle, &all_drawn),
        Replay::Refused("the engine does not consider the witness solved")
    );
}
