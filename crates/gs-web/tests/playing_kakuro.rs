//! Playing a Kakuro in the page's own code.
//!
//! The same shape as `playing.rs` and `playing_slitherlink.rs`, and for the same
//! reason: the test that matters most is the dull one, that writing the real answer
//! makes the puzzle solved and that nothing short of it does. Everything else exists so
//! that test cannot pass by accident.
//!
//! What is different here is the whole reason this family was built third. **A move
//! carries a value the player chose.** Nonogram has three marks, Slitherlink has three
//! marks, and every layer above them was written against that. A Kakuro square has ten
//! states. So this file spends most of its length on the places a value could go wrong
//! where a mark could not: a digit outside one to nine, a nought, an emptied square, and
//! the question of whether the move counter and undo still mean what they meant.

use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_kakuro::generate::{propose, Shape as BoardShape};
use gs_kakuro::solve::{solve, Budget};
use gs_web::play::kakuro::{Session, Square};
use gs_web::play::NoChange;
use gs_web::{load, Playable};

const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const KEY_ID: &str = "the-page-trusts-this-one";

/// A solver that agrees the puzzle has exactly one answer.
///
/// Stands in for Z3 so this file runs anywhere. It is not pretending to prove anything:
/// these tests are about playing a puzzle, and the proving is tested against real
/// solvers in `gs-kakuro`.
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

/// A real Kakuro, loaded the way a browser loads one.
///
/// Going through `load` rather than building a `Playable` directly, because a session
/// can only be started on a puzzle whose signature was checked and the tests should use
/// the same door.
fn a_loaded_puzzle() -> Playable {
    // The first seed that works, not a fixed one. Which seeds produce a puzzle is a fact
    // about the generator, and it changed when boards started being built rather than
    // drawn at random (ADR 0021), which broke three test files at once.
    let candidate = (0..40_u64)
        .find_map(|seed| propose(seed, BoardShape::middling(5, 5), Budget::default()).ok())
        .expect("some seed in forty makes a 5 by 5");
    let published = gs_kakuro::publish::publish(
        &candidate,
        &AgreesWithAUniquePuzzle {
            asked: std::cell::RefCell::new(0),
        },
        Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes");
    load(&published.package, &Sha256, &the_page_trusts_us())
        .expect("a package this build published is one this build accepts")
}

fn a_session() -> Session {
    Session::start(a_loaded_puzzle()).expect("a Kakuro board makes a Kakuro session")
}

/// The answer, taken from the engine's own solver rather than typed out here, as a
/// column, row and digit for every white square. A test that wrote its own digits
/// would be checking the session against a puzzle of its own.
fn the_answer(puzzle: &Playable) -> Vec<(u16, u16, i16)> {
    let rules = puzzle
        .kakuro()
        .expect("every puzzle in this file is a Kakuro")
        .clone();
    let state = solve(&rules, Budget::default())
        .witness
        .expect("a uniquely solvable puzzle has an answer");
    rules
        .layout
        .whites()
        .filter_map(|white| {
            let (column, row) = rules
                .layout
                .square_of(white)
                .and_then(|square| rules.layout.square_at(square))?;
            Some((column, row, rules.digit(&state, white)?))
        })
        .collect()
}

#[test]
fn a_fresh_board_is_empty_and_not_solved() {
    let session = a_session();
    assert!(!session.is_solved());
    assert_eq!(session.moves_made(), 0);
    for white in session.layout().whites() {
        let (column, row) = session
            .layout()
            .square_of(white)
            .and_then(|square| session.layout().square_at(square))
            .expect("a white square is on the board");
        assert_eq!(session.digit_at(column, row), None);
    }
    // Every run is still to go, which is the ordinary state of a blank board rather
    // than a mistake.
    assert_eq!(session.runs_not_met().len(), session.layout().run_count());
    assert!(session.runs_with_a_repeat().is_empty());
}

#[test]
fn writing_the_answer_solves_it_and_nothing_less_does() {
    // The test this file exists for.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a session");

    // Everything but the last square. Not solved: a Kakuro with one empty square has a
    // run that does not meet its total, which is the thing that makes this family's
    // finishing condition different from both others'.
    for (column, row, digit) in &answer[..answer.len() - 1] {
        session.write(*column, *row, *digit).expect("a legal digit");
        assert!(
            !session.is_solved(),
            "solved before the board was full, at {column},{row}"
        );
    }

    let (column, row, digit) = answer[answer.len() - 1];
    session.write(column, row, digit).expect("a legal digit");
    assert!(session.is_solved(), "the real answer should solve it");
    assert!(session.runs_not_met().is_empty());
    assert!(session.runs_with_a_repeat().is_empty());
}

#[test]
fn a_full_board_with_one_digit_changed_is_not_solved() {
    // The other direction. A board that is full is not a board that is right.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a session");
    for (column, row, digit) in &answer {
        session.write(*column, *row, *digit).expect("a legal digit");
    }
    assert!(session.is_solved());

    let (column, row, right) = answer[0];
    let wrong = if right == 9 { 1 } else { right + 1 };
    session.write(column, row, wrong).expect("a legal digit");
    assert!(
        !session.is_solved(),
        "a full board with a wrong digit in it must not be solved"
    );
}

#[test]
fn a_digit_outside_one_to_nine_is_refused() {
    // The case a mark could never have produced, and the reason `write` checks before
    // the rules see the value: a page reads a keystroke, and a keystroke is not a digit.
    let mut session = a_session();
    let (column, row) = first_white(&session);
    for not_a_digit in [-5_i16, -1, 0, 10, 99, i16::MAX, i16::MIN] {
        assert_eq!(
            session.write(column, row, not_a_digit),
            Err(NoChange::TheRulesRefused),
            "{not_a_digit} should be refused"
        );
    }
    assert_eq!(session.moves_made(), 0, "a refused move is not a move");
}

#[test]
fn zero_does_not_mean_empty() {
    // There is no number that means "empty". Nought is refused outright by this family
    // — a nought beside a nine would satisfy a total of nine and the no-repeats rule
    // would not catch it — so emptying a square is its own move.
    let mut session = a_session();
    let (column, row) = first_white(&session);
    session.write(column, row, 4).expect("a legal digit");
    assert_eq!(session.digit_at(column, row), Some(4));

    assert_eq!(
        session.write(column, row, 0),
        Err(NoChange::TheRulesRefused)
    );
    assert_eq!(session.digit_at(column, row), Some(4), "nothing changed");

    session.clear(column, row).expect("clearing is a move");
    assert_eq!(session.digit_at(column, row), None);
}

#[test]
fn emptying_a_square_is_a_move_and_not_an_undo() {
    let mut session = a_session();
    let (column, row) = first_white(&session);
    session.write(column, row, 7).expect("a legal digit");
    assert_eq!(session.moves_made(), 1);

    session.clear(column, row).expect("a legal move");
    assert_eq!(
        session.moves_made(),
        2,
        "clearing a square is a move the player made, not a step back"
    );

    // And undo takes that move back, which puts the digit back rather than leaving the
    // square empty.
    session.undo().expect("something to undo");
    assert_eq!(session.digit_at(column, row), Some(7));
}

#[test]
fn emptying_an_already_empty_square_does_nothing_and_says_so() {
    let mut session = a_session();
    let (column, row) = first_white(&session);
    assert_eq!(session.clear(column, row), Err(NoChange::AlreadyThatWay));
    assert_eq!(session.moves_made(), 0);
}

#[test]
fn writing_the_digit_already_there_does_nothing_and_says_so() {
    let mut session = a_session();
    let (column, row) = first_white(&session);
    session.write(column, row, 3).expect("a legal digit");
    assert_eq!(session.write(column, row, 3), Err(NoChange::AlreadyThatWay));
    assert_eq!(session.moves_made(), 1);
}

#[test]
fn a_black_square_is_not_a_square_a_player_can_write_in() {
    // The board's shape is part of the puzzle. There is no move that turns a square
    // black or white, so writing in a black one is refused rather than ignored.
    let session = a_session();
    let black = session
        .layout()
        .squares()
        .find(|square| session.layout().is_block(*square))
        .expect("a middling 5 by 5 has black squares");
    let (column, row) = session
        .layout()
        .square_at(black)
        .expect("a square on the board has a place");

    let mut session = session;
    assert_eq!(session.write(column, row, 5), Err(NoChange::NoSuchSquare));
    assert_eq!(session.clear(column, row), Err(NoChange::NoSuchSquare));
    assert_eq!(session.digit_at(column, row), None);
    assert!(matches!(session.square_at(column, row), Square::Black(_)));
}

#[test]
fn a_square_off_the_board_is_refused_rather_than_becoming_another_one() {
    let mut session = a_session();
    let width = session.layout().width();
    let height = session.layout().height();
    assert_eq!(session.write(width, 0, 5), Err(NoChange::NoSuchSquare));
    assert_eq!(session.write(0, height, 5), Err(NoChange::NoSuchSquare));
    assert_eq!(
        session.write(u16::MAX, u16::MAX, 5),
        Err(NoChange::NoSuchSquare)
    );
    assert!(matches!(session.square_at(width, 0), Square::OffTheBoard));
}

#[test]
fn undo_and_redo_walk_the_digits_back_and_forward() {
    let mut session = a_session();
    let whites = every_white(&session);
    let places: Vec<(u16, u16)> = whites.iter().take(4).copied().collect();
    assert!(places.len() >= 2, "the board should have a few squares");

    for (index, (column, row)) in places.iter().enumerate() {
        let digit = i16::try_from(index % 9 + 1).expect("a digit");
        session.write(*column, *row, digit).expect("a legal digit");
    }
    assert_eq!(session.moves_made(), places.len() as u32);

    for _ in 0..places.len() {
        session.undo().expect("something to undo");
    }
    assert_eq!(session.moves_made(), 0);
    for (column, row) in &places {
        assert_eq!(session.digit_at(*column, *row), None);
    }
    assert_eq!(session.undo(), Err(NoChange::AtTheStart));

    for _ in 0..places.len() {
        session.redo().expect("something to redo");
    }
    for (index, (column, row)) in places.iter().enumerate() {
        let digit = i16::try_from(index % 9 + 1).expect("a digit");
        assert_eq!(session.digit_at(*column, *row), Some(digit));
    }
    assert_eq!(session.redo(), Err(NoChange::NothingToRedo));
}

#[test]
fn a_repeat_inside_one_run_is_seen_and_the_same_digit_in_two_runs_is_not() {
    // The one Kakuro mistake a player can be shown at a glance. It has to be a fact
    // about runs rather than about rows: the same digit twice in one grid row is
    // perfectly legal when a black square sits between the two.
    let mut session = a_session();
    let run = session
        .layout()
        .runs()
        .find(|(_, run)| run.len() >= 2)
        .map(|(id, _)| id)
        .expect("a board has a run of two or more");
    let squares: Vec<(u16, u16)> = session
        .layout()
        .run(run)
        .expect("the run exists")
        .squares
        .iter()
        .filter_map(|white| {
            session
                .layout()
                .square_of(*white)
                .and_then(|square| session.layout().square_at(square))
        })
        .collect();

    session
        .write(squares[0].0, squares[0].1, 4)
        .expect("a legal digit");
    assert!(session.runs_with_a_repeat().is_empty());
    session
        .write(squares[1].0, squares[1].1, 4)
        .expect("a legal digit");
    assert!(
        session.runs_with_a_repeat().contains(&run),
        "a digit twice in one run should be seen"
    );

    session.undo().expect("something to undo");
    assert!(session.runs_with_a_repeat().is_empty());
}

#[test]
fn the_totals_a_page_draws_are_the_rules_own_totals() {
    // A page draws a total in the black square at the head of a run. This checks the
    // two halves agree: every total the layout holds is one `heading_at` or
    // `heading_for_rim` reports, and no total is reported twice.
    let session = a_session();
    let layout = session.layout();
    let mut drawn = 0_usize;
    for row in 0..layout.height() {
        for column in 0..layout.width() {
            match session.square_at(column, row) {
                Square::Black(heading) => {
                    drawn += usize::from(heading.across.is_some());
                    drawn += usize::from(heading.down.is_some());
                }
                Square::White { .. } => {
                    // A run starting hard against the rim has no black square in front
                    // of it, so its total is drawn in the margin.
                    if column == 0 && session.heading_for_rim(column, row, true).is_some() {
                        drawn += 1;
                    }
                    if row == 0 && session.heading_for_rim(column, row, false).is_some() {
                        drawn += 1;
                    }
                }
                Square::OffTheBoard => panic!("a square inside the board is on the board"),
            }
        }
    }
    assert_eq!(
        drawn,
        layout.run_count(),
        "every run's total must be drawn exactly once, somewhere a player can see it"
    );
}

#[test]
fn the_board_fingerprint_follows_the_digits() {
    let mut session = a_session();
    let blank = session.board_fingerprint();
    let (column, row) = first_white(&session);

    session.write(column, row, 6).expect("a legal digit");
    let written = session.board_fingerprint();
    assert_ne!(blank, written);

    session.undo().expect("something to undo");
    assert_eq!(
        session.board_fingerprint(),
        blank,
        "undoing back to the start should give the starting fingerprint"
    );
}

#[test]
fn the_session_only_starts_on_a_kakuro() {
    // There is no way to start a Kakuro session on another game's board, which is what
    // keeps the three sessions from ever holding each other's puzzles.
    let nonogram_or_slitherlink = {
        let candidate = gs_slitherlink::generate::propose(
            7,
            gs_slitherlink::generate::Shape {
                width: 4,
                height: 4,
                enclosed_per_mille: 400,
            },
            gs_slitherlink::generate::REMOVAL_BUDGET,
        )
        .expect("a puzzle");
        let published = gs_slitherlink::publish::publish(
            &candidate,
            &AgreesWithAUniquePuzzle {
                asked: std::cell::RefCell::new(0),
            },
            gs_slitherlink::solve::Budget::default(),
            &Sha256,
            &our_signer(),
        )
        .expect("it publishes");
        load(&published.package, &Sha256, &the_page_trusts_us()).expect("it loads")
    };
    assert!(Session::start(nonogram_or_slitherlink).is_none());
}

/// Every white square's place, in board order.
fn every_white(session: &Session) -> Vec<(u16, u16)> {
    session
        .layout()
        .whites()
        .filter_map(|white| {
            session
                .layout()
                .square_of(white)
                .and_then(|square| session.layout().square_at(square))
        })
        .collect()
}

/// The first white square's place, for a test that only needs one.
fn first_white(session: &Session) -> (u16, u16) {
    every_white(session)
        .first()
        .copied()
        .expect("a board has white squares")
}
