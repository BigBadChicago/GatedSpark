//! Playing a puzzle: what a move does, and what "solved" means.
//!
//! The test that matters most here is the dull one — that playing the real answer makes
//! the puzzle solved, and that nothing short of it does. Everything else in this file
//! exists so that test cannot pass by accident.
//!
//! The thing being guarded is a single mistake: a page working out for itself whether
//! the board is finished. There would then be two answers to that question, and the one
//! the player saw would eventually be the wrong one. So these tests check that the
//! answer comes from the rules, including after an undo, where a cached answer would
//! show up.

use gs_core::hash::StateHash;
use gs_core::ruleset::Ruleset;
use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::rules::{mark_of, Nonogram};
use gs_nonogram::solve::{solve, Budget};
use gs_web::play::nonogram::{Mark, Session};
use gs_web::play::NoChange;
use gs_web::{load, Playable};

const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const KEY_ID: &str = "the-page-trusts-this-one";

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

/// The clue numbers of a puzzle these tests know is a Nonogram.
fn clues_of(puzzle: &Playable) -> &gs_nonogram::clues::Clues {
    puzzle
        .nonogram()
        .expect("every puzzle in this file is a Nonogram")
}

/// A real puzzle, loaded the way a browser loads one.
///
/// Going through `load` rather than building a `Playable` directly, because a session
/// can only be started on a puzzle whose signature was checked and the tests should use
/// the same door.
fn a_loaded_puzzle() -> Playable {
    let candidate = (1u64..3_000)
        .find_map(|seed| propose_in_band(&BANDS[0], seed, Budget::default()))
        .expect("some seed in the first few thousand produces a gentle candidate");
    let signer = Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY);
    let published = gs_nonogram::publish::publish(
        &candidate,
        &AgreesWithAUniquePuzzle {
            asked: std::cell::RefCell::new(0),
        },
        Budget::default(),
        &Sha256,
        &signer,
    )
    .expect("a uniquely solvable candidate publishes");

    let mut keys = gs_crypto::TrustedKeys::none();
    keys.trust(KEY_ID, &signer.public_key())
        .expect("the test key is valid");
    load(&published.package, &Sha256, &keys).expect("a real puzzle loads")
}

/// The answer to a puzzle, as a list of marks per square.
///
/// Taken from the line solver, which is the engine's own answer rather than one invented
/// here. A test that typed out a board would be checking the session against a puzzle of
/// its own.
fn the_answer(puzzle: &Playable) -> Vec<Mark> {
    let clues = clues_of(puzzle);
    let rules = Nonogram::new(clues.clone());
    let state = solve(&rules, Budget::default())
        .witness
        .expect("a uniquely solvable puzzle has an answer");
    (0..clues.dims.cell_count())
        .map(|i| {
            let cell = state
                .get(u32::try_from(i).expect("a board this size fits"))
                .expect("every square of the answer is on the board");
            match mark_of(cell) {
                gs_nonogram::line::Mark::Filled => Mark::Filled,
                gs_nonogram::line::Mark::Empty => Mark::Empty,
                gs_nonogram::line::Mark::Unknown => Mark::Unknown,
            }
        })
        .collect()
}

/// Play the whole answer into a session.
fn play_the_answer(session: &mut Session, answer: &[Mark]) {
    for (i, &mark) in answer.iter().enumerate() {
        let index = u32::try_from(i).expect("a board this size fits");
        match session.mark(index, mark) {
            Ok(()) => {}
            // A square the answer leaves undecided is already undecided, so marking it
            // so is no change. That is correct rather than a failure.
            Err(NoChange::AlreadyThatWay) => {}
            Err(other) => panic!("playing square {index} as {mark:?} failed: {other}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Where a game starts
// ---------------------------------------------------------------------------

#[test]
fn a_new_game_is_blank_and_unsolved() {
    let puzzle = a_loaded_puzzle();
    let dims = clues_of(&puzzle).dims;
    let session = Session::start(puzzle).expect("these tests load Nonograms");

    assert!(!session.is_solved(), "a blank board is not a solved board");
    assert_eq!(session.moves_made(), 0);
    for i in 0..u32::try_from(dims.cell_count()).expect("fits") {
        assert_eq!(
            session.mark_of(i),
            Mark::Unknown,
            "square {i} should start undecided"
        );
    }
}

// ---------------------------------------------------------------------------
// The test this file exists for
// ---------------------------------------------------------------------------

#[test]
fn playing_the_real_answer_solves_the_puzzle() {
    // Everything else here is scaffolding around this.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    assert!(!session.is_solved());
    play_the_answer(&mut session, &answer);
    assert!(
        session.is_solved(),
        "playing the puzzle's own answer must solve it"
    );
}

#[test]
fn a_board_one_filled_square_short_is_not_solved() {
    // The anti-vacuity half. If `is_solved` returned true for anything, the test above
    // would pass while meaning nothing.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    play_the_answer(&mut session, &answer);
    assert!(session.is_solved());

    // Clear one *filled* square, which is a move rather than an undo.
    let a_filled_square = answer
        .iter()
        .position(|&m| m == Mark::Filled)
        .expect("any real answer fills at least one square");
    session
        .mark(u32::try_from(a_filled_square).expect("fits"), Mark::Unknown)
        .expect("clearing a filled square is a legal move");
    assert!(
        !session.is_solved(),
        "a board missing one of its filled squares is not solved"
    );
}

#[test]
fn clearing_a_crossed_out_square_leaves_the_puzzle_solved() {
    // The other side of the rule the owner chose on 2026-10-23: crossing a square out
    // is a note to the player and nothing more, so taking the note away cannot unfinish
    // a finished puzzle. Paired with the test above, which is what stops this one
    // turning into "nothing ever unfinishes a puzzle".
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let blank_squares = answer.iter().filter(|&&m| m == Mark::Empty).count();
    assert!(
        blank_squares > 0,
        "this test needs a puzzle whose answer leaves some squares blank"
    );
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    play_the_answer(&mut session, &answer);
    assert!(session.is_solved());

    for (i, &mark) in answer.iter().enumerate() {
        if mark != Mark::Empty {
            continue;
        }
        session
            .mark(u32::try_from(i).expect("fits"), Mark::Unknown)
            .expect("clearing a crossed-out square is a legal move");
    }
    assert!(
        session.is_solved(),
        "taking every cross off a finished board must leave it finished"
    );
}

#[test]
fn a_board_with_only_its_filled_squares_marked_is_solved() {
    // The rule stated directly, from a blank board rather than by taking marks off a
    // finished one: a player who fills the right squares and crosses out nothing has
    // finished. This is the whole of the change made on 2026-10-23.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    for (i, &mark) in answer.iter().enumerate() {
        if mark == Mark::Filled {
            session
                .mark(u32::try_from(i).expect("fits"), Mark::Filled)
                .expect("filling a square is a legal move");
        }
    }

    assert_eq!(
        session.moves_made(),
        u32::try_from(answer.iter().filter(|&&m| m == Mark::Filled).count()).expect("fits"),
        "only the filled squares should have been touched"
    );
    assert!(
        session.is_solved(),
        "filling the right squares and nothing else must finish the puzzle"
    );
}

#[test]
fn a_full_board_with_a_wrong_square_is_not_solved() {
    // Every square decided and still wrong. This is the case a check that only counted
    // filled-in squares would get wrong, and it is the reason `is_solved` asks the
    // rules about the clues instead.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    play_the_answer(&mut session, &answer);

    let a_square = answer
        .iter()
        .position(|&m| m == Mark::Filled)
        .expect("any real answer fills at least one square");
    session
        .mark(u32::try_from(a_square).expect("fits"), Mark::Empty)
        .expect("changing a square is a legal move");

    assert!(
        !session.is_solved(),
        "a completely decided but incorrect board is not solved"
    );
}

#[test]
fn filling_every_square_does_not_solve_it() {
    // The laziest possible wrong answer, and a check that accepted it would be the
    // worst bug available here. (Guarded against a puzzle whose answer really is every
    // square filled, which would make the assertion wrong rather than the code.)
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    if answer.iter().all(|&m| m == Mark::Filled) {
        return;
    }
    let count = u32::try_from(clues_of(&puzzle).dims.cell_count()).expect("fits");
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    for i in 0..count {
        let _ = session.mark(i, Mark::Filled);
    }
    assert!(
        !session.is_solved(),
        "filling in everything must not count as solving it"
    );
}

#[test]
fn solving_then_undoing_one_move_is_no_longer_solved() {
    // Where a cached "solved" flag would show itself. The answer has to be asked of the
    // rules every time, and this is the test that would catch somebody optimising that
    // away.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    play_the_answer(&mut session, &answer);
    // End on a filled square, so that the move being undone is one the rules care
    // about. Since 2026-10-23 undoing a cross-out does not unfinish a board, so an
    // answer whose last square happens to be blank would make this test pass for the
    // wrong reason.
    let a_filled_square = u32::try_from(
        answer
            .iter()
            .position(|&m| m == Mark::Filled)
            .expect("fills"),
    )
    .expect("fits");
    session
        .mark(a_filled_square, Mark::Unknown)
        .expect("clearing a filled square is a legal move");
    session
        .mark(a_filled_square, Mark::Filled)
        .expect("filling it back in is a legal move");
    assert!(session.is_solved());

    session.undo().expect("there are moves to undo");
    assert!(
        !session.is_solved(),
        "undoing the last move must un-solve the puzzle"
    );

    session.redo().expect("the undone move can be redone");
    assert!(
        session.is_solved(),
        "redoing it must solve the puzzle again"
    );
}

// ---------------------------------------------------------------------------
// Moves, undo and redo
// ---------------------------------------------------------------------------

#[test]
fn a_mark_shows_up_and_undo_takes_it_back_exactly() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    session.mark(0, Mark::Filled).expect("a legal first move");
    assert_eq!(session.mark_of(0), Mark::Filled);
    assert_eq!(session.moves_made(), 1);

    session.mark(1, Mark::Empty).expect("a legal second move");
    assert_eq!(session.mark_of(1), Mark::Empty);
    assert_eq!(session.moves_made(), 2);

    session.undo().expect("undo");
    assert_eq!(session.mark_of(1), Mark::Unknown, "the second move is gone");
    assert_eq!(session.mark_of(0), Mark::Filled, "the first one is not");
    assert_eq!(session.moves_made(), 1);

    session.undo().expect("undo again");
    assert_eq!(session.mark_of(0), Mark::Unknown);
    assert_eq!(session.moves_made(), 0);
}

#[test]
fn clearing_a_square_is_a_move_and_not_an_undo() {
    // A player retracting a guess has not undone it, they have made a new move. The
    // difference is visible in whether redo has anything to do.
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    session.mark(0, Mark::Filled).expect("mark it");
    session.mark(0, Mark::Unknown).expect("clear it");

    assert_eq!(session.mark_of(0), Mark::Unknown);
    assert_eq!(
        session.moves_made(),
        2,
        "clearing is the second move, not the undoing of the first"
    );
    assert_eq!(
        session.redo(),
        Err(NoChange::NothingToRedo),
        "nothing was undone, so there is nothing to redo"
    );
}

#[test]
fn undo_at_the_start_and_redo_at_the_end_say_so() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    assert_eq!(session.undo(), Err(NoChange::AtTheStart));
    assert_eq!(session.redo(), Err(NoChange::NothingToRedo));

    session.mark(0, Mark::Filled).expect("a move");
    assert_eq!(
        session.redo(),
        Err(NoChange::NothingToRedo),
        "a fresh move leaves nothing to redo"
    );
}

#[test]
fn marking_a_square_the_way_it_already_is_changes_nothing() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    session.mark(0, Mark::Filled).expect("the first move");
    assert_eq!(session.mark(0, Mark::Filled), Err(NoChange::AlreadyThatWay));
    assert_eq!(
        session.moves_made(),
        1,
        "a move that changed nothing is not a move"
    );
}

#[test]
fn a_square_that_is_not_on_the_board_is_refused() {
    let puzzle = a_loaded_puzzle();
    let count = u32::try_from(clues_of(&puzzle).dims.cell_count()).expect("fits");
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    assert_eq!(
        session.mark(count, Mark::Filled),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.mark(u32::MAX, Mark::Filled),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(session.mark_of(count), Mark::Unknown, "and reads as blank");
    assert_eq!(session.moves_made(), 0);
}

#[test]
fn a_column_past_the_edge_does_not_wrap_onto_the_next_row() {
    // The bug this is here to not have. Working out a square's position from a row and
    // a column by arithmetic alone turns (row 0, one past the last column) into the
    // first square of row 1, and a player tapping just outside the board would mark a
    // square somewhere else entirely.
    let puzzle = a_loaded_puzzle();
    let dims = clues_of(&puzzle).dims;
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    assert_eq!(
        session.mark_at(0, dims.width, Mark::Filled),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.mark_at(dims.height, 0, Mark::Filled),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.mark_at_of(0, dims.width),
        Mark::Unknown,
        "and reading past the edge is blank, not the next row's first square"
    );
    assert_eq!(
        session.moves_made(),
        0,
        "no square anywhere on the board was marked"
    );
}

#[test]
fn rows_and_columns_land_where_a_page_expects() {
    let puzzle = a_loaded_puzzle();
    let dims = clues_of(&puzzle).dims;
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    session
        .mark_at(1, 0, Mark::Filled)
        .expect("the first square of the second row");
    assert_eq!(session.mark_at_of(1, 0), Mark::Filled);
    assert_eq!(
        session.mark_of(u32::from(dims.width)),
        Mark::Filled,
        "row 1, column 0 is one full row in"
    );
    assert_eq!(
        session.mark_at_of(0, 0),
        Mark::Unknown,
        "and the top-left square was not touched"
    );
}

// ---------------------------------------------------------------------------
// What a session can hand back
// ---------------------------------------------------------------------------

#[test]
fn the_board_fingerprint_matches_the_board_bytes() {
    // The property that lets a saved game be checked later against something signed:
    // hashing the state and hashing its canonical bytes have to agree. Without it, a
    // stored board could not be compared to the number in a published report.
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    for i in 0..3u32 {
        let _ = session.mark(i, Mark::Filled);
    }
    assert_eq!(
        session.board_fingerprint(),
        StateHash::of_bytes(&session.board_bytes())
    );
}

#[test]
fn the_fingerprint_changes_with_the_board_and_comes_back_on_undo() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    let blank = session.board_fingerprint();

    session.mark(0, Mark::Filled).expect("a move");
    let after = session.board_fingerprint();
    assert_ne!(
        blank, after,
        "a different board has a different fingerprint"
    );

    session.undo().expect("undo");
    assert_eq!(
        session.board_fingerprint(),
        blank,
        "undo must return the board to exactly what it was, not nearly"
    );
}

#[test]
fn a_session_still_knows_which_puzzle_it_is() {
    let puzzle = a_loaded_puzzle();
    let fingerprint = puzzle.content_id;
    let signed_by = puzzle.signed_by.clone();
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");
    session.mark(0, Mark::Filled).expect("a move");

    // Playing does not detach a board from the puzzle it belongs to, which is what lets
    // a page show "signed by, level, one answer only" while someone plays.
    assert_eq!(session.puzzle().content_id, fingerprint);
    assert_eq!(session.puzzle().signed_by, signed_by);
}

#[test]
fn the_session_agrees_with_the_rules_about_being_solved() {
    // Direct check that nothing is being decided here: the session's answer and the
    // rules' answer, asked separately, over a board that changes under both.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let rules = Nonogram::new(clues_of(&puzzle).clone());
    let mut session = Session::start(puzzle).expect("these tests load Nonograms");

    // A second board, kept in step by hand through the rules themselves rather than
    // through the session. Two paths to the same board, and the rules asked about each.
    let mut board = gs_core::state::State::blank(rules.clues.dims);

    for (i, &mark) in answer.iter().enumerate() {
        let index = u32::try_from(i).expect("fits");
        let played = session.mark(index, mark);
        let value = match mark {
            Mark::Filled => gs_nonogram::rules::FILLED,
            Mark::Empty => gs_nonogram::rules::EMPTY,
            Mark::Unknown => gs_core::state::Cell::UNKNOWN,
        };
        match rules.transition(&board, &gs_nonogram::rules::Action::Set { index, value }) {
            Ok(delta) => {
                assert!(
                    played.is_ok(),
                    "the rules allowed square {i} and the session did not"
                );
                delta
                    .apply(&mut board)
                    .expect("a delta the rules produced applies");
            }
            Err(_) => assert!(
                played.is_err(),
                "the rules refused square {i} and the session did not"
            ),
        }
        assert_eq!(
            session.is_solved(),
            rules.is_solved(&board),
            "the session and the rules disagreed after square {i}"
        );
        assert_eq!(
            session.board_bytes(),
            board.canonical_bytes(),
            "the two boards drifted apart at square {i}"
        );
    }
    assert!(session.is_solved(), "and the answer does solve it");
}
