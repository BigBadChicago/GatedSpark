//! Hints in the page's own code: the allowance, and what a hint does not do.
//!
//! Whether a hint is *true* is tested in each game's crate, against that game's one
//! answer. This file is about the three things the page layer is responsible for:
//!
//! 1. **Three hints per puzzle, and then no more.**
//! 2. **A hint is not a move.** The board, the move count and whether the puzzle is
//!    solved are untouched by asking.
//! 3. **A hint reaches both games**, with its sentence and somewhere to point.
//!
//! Both games are loaded the same way a browser loads one, so nothing here can get a
//! hint on a puzzle that was not signed and checked.

use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::solve::Budget;
use gs_slitherlink::generate::{propose, Shape as BoardShape, REMOVAL_BUDGET};
use gs_web::play::{NoHint, PointsAt, Session, HINTS_PER_PUZZLE};
use gs_web::{load, Playable};

const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const KEY_ID: &str = "the-page-trusts-this-one";

/// A solver that agrees the puzzle has exactly one answer. Stands in for Z3 so this
/// file runs anywhere; the proving is tested against real solvers in the game crates.
struct AgreesWithAUniquePuzzle {
    asked: std::cell::RefCell<usize>,
}

impl Verifier for AgreesWithAUniquePuzzle {
    fn name(&self) -> &'static str {
        "a solver that agrees the puzzle is unique"
    }

    fn ask(&self, _query: &str) -> Outcome {
        let mut asked = self.asked.borrow_mut();
        let answer = if *asked == 0 {
            Outcome::Sat
        } else {
            Outcome::Unsat
        };
        *asked += 1;
        answer
    }
}

fn our_signer() -> Ed25519Signer {
    Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY)
}

fn the_page_trusts_us() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &our_signer().public_key())
        .expect("a valid key into an empty list");
    keys
}

fn agreeable() -> AgreesWithAUniquePuzzle {
    AgreesWithAUniquePuzzle {
        asked: std::cell::RefCell::new(0),
    }
}

fn a_loaded_nonogram() -> Playable {
    let candidate = (1u64..3_000)
        .find_map(|seed| propose_in_band(&BANDS[0], seed, Budget::default()))
        .expect("some seed in the first few thousand produces a gentle candidate");
    let published = gs_nonogram::publish::publish(
        &candidate,
        &agreeable(),
        Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes");
    load(&published.package, &Sha256, &the_page_trusts_us()).expect("a real puzzle loads")
}

fn a_loaded_slitherlink() -> Playable {
    let candidate = propose(
        7,
        BoardShape {
            width: 5,
            height: 5,
            enclosed_per_mille: 400,
        },
        REMOVAL_BUDGET,
    )
    .expect("this seed makes a puzzle");
    let published = gs_slitherlink::publish::publish(
        &candidate,
        &agreeable(),
        gs_slitherlink::solve::Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes");
    load(&published.package, &Sha256, &the_page_trusts_us()).expect("a real puzzle loads")
}

fn both_games() -> Vec<(&'static str, Session)> {
    vec![
        ("nonogram", Session::start(a_loaded_nonogram())),
        ("slitherlink", Session::start(a_loaded_slitherlink())),
    ]
}

#[test]
fn a_puzzle_starts_with_three_hints() {
    assert_eq!(HINTS_PER_PUZZLE, 3, "the owner asked for three");
    for (game, session) in both_games() {
        assert_eq!(session.hints_left(), 3, "{game} starts with three hints");
    }
}

#[test]
fn three_hints_arrive_and_the_fourth_is_refused() {
    for (game, mut session) in both_games() {
        for spent in 1..=3 {
            let advice = session
                .hint()
                .unwrap_or_else(|why| panic!("{game} hint {spent} was refused: {why}"));
            assert!(
                !advice.says.is_empty(),
                "{game} hint {spent} said nothing at all"
            );
            assert_eq!(
                advice.hints_left,
                3 - spent,
                "{game} hint {spent} counted wrongly"
            );
            assert_eq!(session.hints_left(), 3 - spent);
        }
        assert_eq!(
            session.hint(),
            Err(NoHint::NoneLeft),
            "{game} gave away a fourth hint"
        );
        // And it keeps saying no, rather than coming back round.
        assert_eq!(session.hint(), Err(NoHint::NoneLeft));
        assert_eq!(session.hints_left(), 0);
    }
}

#[test]
fn asking_for_a_hint_is_not_a_move() {
    for (game, mut session) in both_games() {
        let board = session.board_bytes();
        let fingerprint = session.board_fingerprint();
        let solved = session.is_solved();
        for _ in 0..3 {
            let _ = session.hint();
        }
        assert_eq!(session.moves_made(), 0, "{game} counted a hint as a move");
        assert_eq!(session.board_bytes(), board, "{game} board changed");
        assert_eq!(
            session.board_fingerprint(),
            fingerprint,
            "{game} board fingerprint changed"
        );
        assert_eq!(session.is_solved(), solved, "{game} solved answer changed");
        // Undo is about moves, and a hint was not one, so there is still nothing to
        // undo. This is the check that would catch a hint quietly marking the board.
        assert!(
            session.undo().is_err(),
            "{game} had something to undo after only hints"
        );
    }
}

#[test]
fn a_hint_points_at_the_right_sort_of_thing() {
    // A Nonogram hint points at a square, a Slitherlink hint at a line. A hint about a
    // whole row or a whole number points nowhere, which is also allowed — what is not
    // allowed is one game being handed the other game's kind of place.
    let mut nonogram = Session::start(a_loaded_nonogram());
    let advice = nonogram.hint().expect("a hint on a blank Nonogram");
    assert!(
        matches!(
            advice.points_at,
            PointsAt::Square { .. } | PointsAt::Nowhere
        ),
        "a Nonogram hint pointed at {:?}",
        advice.points_at
    );

    let mut slitherlink = Session::start(a_loaded_slitherlink());
    let advice = slitherlink.hint().expect("a hint on a blank Slitherlink");
    assert!(
        matches!(advice.points_at, PointsAt::Line { .. } | PointsAt::Nowhere),
        "a Slitherlink hint pointed at {:?}",
        advice.points_at
    );
}

#[test]
fn a_square_a_hint_points_at_is_on_the_board() {
    let puzzle = a_loaded_nonogram();
    let dims = puzzle.dims();
    let mut session = Session::start(puzzle);
    for _ in 0..3 {
        if let Ok(advice) = session.hint() {
            if let PointsAt::Square { row, column } = advice.points_at {
                assert!(
                    row < dims.height && column < dims.width,
                    "a hint pointed off the board at row {row}, column {column}"
                );
            }
        }
    }
}

#[test]
fn the_refusals_read_as_words() {
    // These two sentences are shown to a player, so they get the same guard as the
    // rest of the page: no code names, and a whole sentence.
    for why in [NoHint::NoneLeft, NoHint::NothingToSay] {
        let said = why.to_string();
        assert!(said.ends_with('.'), "not a sentence: {said}");
        assert!(
            said.chars().next().is_some_and(char::is_uppercase),
            "does not start a sentence: {said}"
        );
        assert!(!said.contains('_'), "that is code, not words: {said}");
        for pair in said.chars().collect::<Vec<_>>().windows(2) {
            assert!(
                !(pair[0].is_lowercase() && pair[1].is_uppercase()),
                "a run-together code name in: {said}"
            );
        }
    }
}
