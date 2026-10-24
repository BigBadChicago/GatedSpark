//! Hints in the page's own code: the allowance, the growing sizes, and what a hint
//! does to the board.
//!
//! Whether a hint is *true* is tested in each game's crate, against that game's one
//! answer. This file is about the four things the page layer is responsible for:
//!
//! 1. **Three hints per puzzle, and then no more.**
//! 2. **The hints grow:** one square or line, then half a row or column, then a whole
//!    one.
//! 3. **A hint fills the board in**, through the ordinary rules, so every mark it makes
//!    is one the player could have made and can take back with undo.
//! 4. **A hint reaches both games**, with its sentence and the places it filled.
//!
//! Both games are loaded the same way a browser loads one, so nothing here can get a
//! hint on a puzzle that was not signed and checked.

use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::solve::Budget;
use gs_slitherlink::generate::{propose, Shape as BoardShape, REMOVAL_BUDGET};
use gs_web::play::{NoHint, Place, Session, HINTS_PER_PUZZLE, HINT_SIZES};
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
fn the_sizes_and_the_allowance_are_the_same_length() {
    // Three hints and three sizes. If the two ever disagreed, the last hint would get
    // whichever size happened to be last in the list rather than the one meant.
    assert_eq!(HINT_SIZES.len(), usize::from(HINTS_PER_PUZZLE));
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
            assert!(
                !advice.filled.is_empty(),
                "{game} hint {spent} filled nothing in"
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
        assert_eq!(session.hint(), Err(NoHint::NoneLeft));
        assert_eq!(session.hints_left(), 0);
    }
}

#[test]
fn the_hints_get_bigger_as_they_are_used() {
    // What the project owner asked for, as seen from the page: the second hint helps
    // more than the first, and the third more than the second.
    for (game, mut session) in both_games() {
        let mut sizes = Vec::new();
        for spent in 1..=3 {
            let advice = session
                .hint()
                .unwrap_or_else(|why| panic!("{game} hint {spent} was refused: {why}"));
            sizes.push(advice.filled.len());
        }
        println!("{game} filled in {sizes:?}");
        assert_eq!(sizes[0], 1, "{game} first hint is one square or line");
        assert!(
            sizes[1] > sizes[0],
            "{game} second hint ({}) is no bigger than the first ({})",
            sizes[1],
            sizes[0]
        );
        assert!(
            sizes[2] > sizes[1],
            "{game} third hint ({}) is no bigger than the second ({})",
            sizes[2],
            sizes[1]
        );
    }
}

#[test]
fn a_hint_fills_the_board_in_and_undo_takes_it_back() {
    // The change the project owner asked for: a hint marks the squares rather than
    // only pointing at them. It does it through the ordinary rules, which is what
    // makes undo work, and is also what stops a hint writing something a player could
    // not have written.
    for (game, mut session) in both_games() {
        let before = session.board_bytes();
        let fingerprint = session.board_fingerprint();

        let advice = session.hint().expect("a hint on a blank board");
        assert!(!advice.filled.is_empty(), "{game} filled nothing in");
        assert_eq!(
            session.moves_made(),
            advice.filled.len() as u32,
            "{game} made a different number of moves than it reported filling"
        );
        assert_ne!(
            session.board_bytes(),
            before,
            "{game} said it filled squares in and the board is unchanged"
        );

        // Every mark is undoable, one at a time, back to exactly where the player was.
        for _ in 0..advice.filled.len() {
            session
                .undo()
                .expect("a hint's marks undo like any other move");
        }
        assert_eq!(
            session.board_bytes(),
            before,
            "{game} did not come back to where it started"
        );
        assert_eq!(
            session.board_fingerprint(),
            fingerprint,
            "{game} board fingerprint did not come back"
        );
        assert_eq!(session.moves_made(), 0);
    }
}

#[test]
fn a_hint_never_finishes_the_puzzle_by_itself() {
    // Three hints on a real puzzle help; they do not play it. This is the line between
    // help and giving the answer away, and the only thing holding it is the allowance.
    for (game, mut session) in both_games() {
        for _ in 0..HINTS_PER_PUZZLE {
            let _ = session.hint();
        }
        assert!(
            !session.is_solved(),
            "{game} was finished by its three hints"
        );
    }
}

#[test]
fn a_hint_that_marks_nothing_costs_nothing_and_is_free_to_repeat() {
    // A hint saying a row cannot work marks nothing: which square in it is wrong is the
    // player's to decide. A player who taps Hint twice on such a board has not been
    // helped twice, so they are not charged twice.
    for (game, mut session) in both_games() {
        let first = session.hint().expect("a hint on a blank board");
        let left = first.hints_left;
        for _ in 0..5 {
            let again = session.hint();
            match again {
                Ok(more) if more.says == first.says => assert_eq!(
                    more.hints_left, left,
                    "{game} charged again for a hint it had already given"
                ),
                // The usual case now: the first hint filled squares in, so the board
                // moved on and the next hint is a new one that costs.
                Ok(more) => assert!(more.hints_left < left, "{game} gave a new hint for nothing"),
                Err(NoHint::NoneLeft) => break,
                Err(why) => panic!("{game}: {why}"),
            }
        }
    }
}

#[test]
fn a_hint_fills_the_right_sort_of_place() {
    // A Nonogram hint fills squares, a Slitherlink hint fills lines. What is not
    // allowed is one game being handed the other game's kind of place.
    let mut nonogram = Session::start(a_loaded_nonogram());
    let advice = nonogram.hint().expect("a hint on a blank Nonogram");
    assert!(
        advice
            .filled
            .iter()
            .all(|place| matches!(place, Place::Square { .. })),
        "a Nonogram hint filled {:?}",
        advice.filled
    );

    let mut slitherlink = Session::start(a_loaded_slitherlink());
    let advice = slitherlink.hint().expect("a hint on a blank Slitherlink");
    assert!(
        advice
            .filled
            .iter()
            .all(|place| matches!(place, Place::Line { .. })),
        "a Slitherlink hint filled {:?}",
        advice.filled
    );
}

#[test]
fn every_square_a_hint_fills_is_on_the_board() {
    let puzzle = a_loaded_nonogram();
    let dims = puzzle.dims();
    let mut session = Session::start(puzzle);
    for _ in 0..HINTS_PER_PUZZLE {
        if let Ok(advice) = session.hint() {
            for place in advice.filled {
                let Place::Square { row, column } = place else {
                    panic!("a Nonogram hint filled a line");
                };
                assert!(
                    row < dims.height && column < dims.width,
                    "a hint filled a square off the board at row {row}, column {column}"
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
