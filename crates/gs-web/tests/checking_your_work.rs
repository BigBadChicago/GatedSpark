//! Checking your work, in the page's own layer, on all three games.
//!
//! Whether each finding is *true* is tested in each game's crate, next to the counting.
//! This file is about the four things the page layer is responsible for:
//!
//! 1. **A check reaches all three games**, with a sentence and a list of places.
//! 2. **A check changes nothing**: no move is made, and no hint is spent.
//! 3. **A fresh board is clear**, so a player who checks before starting is not told
//!    they have already gone wrong.
//! 4. **A player's own mistake is found**, and the places point at the board.
//!
//! All three puzzles are loaded the same way a browser loads one, so nothing here can
//! check a puzzle that was not signed.

use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::solve::Budget;
use gs_slitherlink::generate::{propose as propose_loop, Shape as LoopShape, REMOVAL_BUDGET};
use gs_web::play::{Place, Session};
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
    let candidate = propose_loop(
        7,
        LoopShape {
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

fn a_loaded_kakuro() -> Playable {
    let candidate = (0..40_u64)
        .find_map(|seed| {
            gs_kakuro::generate::propose(
                seed,
                gs_kakuro::generate::Shape::middling(5, 5),
                gs_kakuro::solve::Budget::default(),
            )
            .ok()
        })
        .expect("some seed in forty makes a 5 by 5");
    let published = gs_kakuro::publish::publish(
        &candidate,
        &agreeable(),
        gs_kakuro::solve::Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes");
    load(&published.package, &Sha256, &the_page_trusts_us()).expect("a real puzzle loads")
}

fn every_game() -> Vec<(&'static str, Session)> {
    vec![
        ("nonogram", Session::start(a_loaded_nonogram())),
        ("slitherlink", Session::start(a_loaded_slitherlink())),
        ("kakuro", Session::start(a_loaded_kakuro())),
    ]
}

#[test]
fn every_game_can_be_checked_and_says_something() {
    for (game, session) in every_game() {
        let found = session.check();
        assert!(
            !found.says.is_empty(),
            "{game} checked and said nothing at all"
        );
        // A sentence, not a word. The point of the feature is being told how you are
        // doing, and "clear" on its own is not that.
        assert!(
            found.says.ends_with('.'),
            "{game} said something that is not a sentence: {}",
            found.says
        );
    }
}

#[test]
fn a_fresh_board_is_clear() {
    for (game, session) in every_game() {
        let found = session.check();
        assert!(
            found.clear,
            "{game} told a player who has not moved yet that something is wrong: {}",
            found.says
        );
        assert!(
            found.places.is_empty(),
            "{game} marked squares on a board nobody has touched"
        );
    }
}

#[test]
fn checking_changes_nothing_and_costs_nothing() {
    for (game, mut session) in every_game() {
        let moves = session.moves_made();
        let hints = session.hints_left();
        let board = session.board_fingerprint();
        for _ in 0..5 {
            let _ = session.check();
        }
        assert_eq!(session.moves_made(), moves, "{game}: a check made a move");
        assert_eq!(
            session.hints_left(),
            hints,
            "{game}: a check spent part of the hint allowance"
        );
        assert_eq!(
            session.board_fingerprint(),
            board,
            "{game}: a check changed the board"
        );
        // And undo has nothing to take back, because nothing happened.
        assert!(
            session.undo().is_err(),
            "{game}: a check left something on the timeline to undo"
        );
    }
}

#[test]
fn a_nonogram_line_that_cannot_work_is_found_and_marked() {
    let mut session = gs_web::play::nonogram::Session::start(a_loaded_nonogram())
        .expect("a Nonogram board makes a Nonogram session");
    let width = session.puzzle().nonogram().expect("a Nonogram").dims.width;

    // Fill the whole of row 0. A gentle puzzle's first row is not entirely filled, so
    // this cannot match its numbers however the rest of the board goes.
    for column in 0..width {
        let _ = session.mark_at(0, column, gs_web::play::nonogram::Mark::Filled);
    }

    let found = session.check();
    assert!(
        !found.clear,
        "a row filled right across was called fine: {}",
        found.says
    );
    assert!(
        found.says.contains("row 1"),
        "the sentence does not name the row it means: {}",
        found.says
    );
    // The whole line is marked, not one square in it: which square is wrong is the
    // player's to decide.
    let marked: Vec<&Place> = found
        .places
        .iter()
        .filter(|place| matches!(place, Place::Square { row: 0, .. }))
        .collect();
    assert_eq!(
        marked.len(),
        usize::from(width),
        "the whole of row 1 should be marked, not part of it"
    );
}

#[test]
fn crossing_squares_out_is_never_called_a_mistake() {
    let mut session = gs_web::play::nonogram::Session::start(a_loaded_nonogram())
        .expect("a Nonogram board makes a Nonogram session");
    let clues = session.puzzle().nonogram().expect("a Nonogram").clone();
    // Every square crossed out. Wrong as an answer, and deliberately not reported: the
    // owner settled that crossing out is a note the player keeps for themselves and
    // nothing checks it.
    for row in 0..clues.dims.height {
        for column in 0..clues.dims.width {
            let _ = session.mark_at(row, column, gs_web::play::nonogram::Mark::Empty);
        }
    }
    let found = session.check();
    assert!(
        found.clear,
        "crossing squares out was treated as a mistake: {}",
        found.says
    );
}
