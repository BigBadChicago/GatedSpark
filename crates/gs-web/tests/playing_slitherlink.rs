//! Playing a Slitherlink in the page's own code.
//!
//! The same shape as `playing.rs`, and for the same reason. The test that matters most
//! is the dull one — that drawing the real loop makes the puzzle solved, and that
//! nothing short of it does. Everything else exists so that test cannot pass by
//! accident.
//!
//! What is different here, and worth the separate file: a Slitherlink is finished by two
//! conditions at once, one loop and every number met, and a page that checked only one
//! of them would be a page that said "well done" to a wrong board. So these tests take a
//! finished board apart in both directions.

use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_slitherlink::board::Along;
use gs_slitherlink::generate::{propose, Shape as BoardShape, REMOVAL_BUDGET};
use gs_slitherlink::loops::Shape;
use gs_slitherlink::solve::{solve, Budget};
use gs_web::play::slitherlink::{Line, Session, Which};
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
/// solvers in `gs-slitherlink`.
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

/// A real Slitherlink, loaded the way a browser loads one.
///
/// Going through `load` rather than building a `Playable` directly, because a session
/// can only be started on a puzzle whose signature was checked and the tests should use
/// the same door.
///
/// Five squares by five, which is big enough to have a loop that goes round corners and
/// small enough that these tests are instant.
fn a_loaded_puzzle() -> Playable {
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

/// The answer, taken from the engine's own solver rather than typed out here. A test
/// that drew its own loop would be checking the session against a puzzle of its own.
fn the_answer(puzzle: &Playable) -> Vec<bool> {
    let clues = puzzle
        .slitherlink()
        .expect("every puzzle in this file is a Slitherlink")
        .clone();
    let rules = gs_slitherlink::rules::Slitherlink::new(clues);
    let state = solve(&rules, Budget::default())
        .witness
        .expect("a uniquely solvable puzzle has an answer");
    rules
        .geometry()
        .edges()
        .map(|edge| gs_slitherlink::loops::drawn_in(&state, edge))
        .collect()
}

/// Draw the whole answer, and nothing else.
fn draw_the_answer(session: &mut Session, answer: &[bool]) {
    for (edge, drawn) in session.geometry().edges().zip(answer) {
        if *drawn {
            session
                .mark_edge(edge, Line::Drawn)
                .expect("drawing a line on a blank board is a legal move");
        }
    }
}

#[test]
fn a_new_game_is_blank_and_unsolved() {
    let puzzle = a_loaded_puzzle();
    let session = Session::start(puzzle).expect("a Slitherlink package starts a Slitherlink");

    assert!(!session.is_solved(), "a blank board is not a solved board");
    assert_eq!(session.moves_made(), 0);
    assert_eq!(
        session.shape(),
        Shape::Nothing,
        "a blank board has no lines on it"
    );
    for edge in session.geometry().edges() {
        assert_eq!(
            session.line_of(edge),
            Line::Unknown,
            "every line starts undecided"
        );
    }
}

#[test]
fn a_slitherlink_package_carries_no_level() {
    // Not an oversight, a decision: this game has no difficulty levels until there is
    // play data to set them from. A page that invented one would be telling a player
    // something nobody measured.
    let puzzle = a_loaded_puzzle();
    assert_eq!(puzzle.band, None);
    assert_eq!(puzzle.family(), "slitherlink");
}

#[test]
fn drawing_the_answer_solves_it_and_the_rules_say_so() {
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    draw_the_answer(&mut session, &answer);

    assert!(session.is_solved(), "the real answer is a solved board");
    assert_eq!(session.shape(), Shape::OneLoop);
    assert!(
        session.numbers_not_met().is_empty(),
        "a solved board meets every number"
    );
}

#[test]
fn leaving_the_blanks_undecided_is_still_solved() {
    // The player's half of the rule. Ruling lines out is how a person solves one of
    // these, but it is not part of being finished, and a page that demanded it would be
    // refusing to accept a correct answer.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    draw_the_answer(&mut session, &answer);
    assert!(session.is_solved());

    // Now rule out every line that is not in the answer. Still solved, and not solved
    // twice over: the same answer, said more fully.
    let edges: Vec<_> = session.geometry().edges().collect();
    for (edge, drawn) in edges.into_iter().zip(&answer) {
        if !*drawn {
            session
                .mark_edge(edge, Line::RuledOut)
                .expect("ruling out an undecided line is a legal move");
        }
    }
    assert!(session.is_solved(), "ruling out the blanks changes nothing");
}

#[test]
fn one_line_short_of_the_answer_is_not_solved() {
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    draw_the_answer(&mut session, &answer);
    assert!(
        session.is_solved(),
        "the answer first, so the rest means something"
    );

    // Take one line of the loop away. The loop is now broken, so this must not be
    // solved — and the reason is about the loop, not about the numbers.
    let first_drawn = session
        .geometry()
        .edges()
        .zip(&answer)
        .find_map(|(edge, drawn)| drawn.then_some(edge))
        .expect("the answer has lines in it");
    session
        .mark_edge(first_drawn, Line::Unknown)
        .expect("clearing a drawn line is a legal move");

    assert!(!session.is_solved(), "a broken loop is not a solved puzzle");
    assert_ne!(session.shape(), Shape::OneLoop);
}

#[test]
fn one_line_too_many_is_not_solved_either() {
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    draw_the_answer(&mut session, &answer);
    assert!(session.is_solved());

    let spare = session
        .geometry()
        .edges()
        .zip(&answer)
        .find_map(|(edge, drawn)| (!drawn).then_some(edge))
        .expect("the answer leaves lines out");
    session
        .mark_edge(spare, Line::Drawn)
        .expect("drawing an undecided line is a legal move");

    assert!(
        !session.is_solved(),
        "an extra line is not a solved puzzle, whatever the numbers say"
    );
}

#[test]
fn undo_takes_the_board_back_exactly() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    let blank = session.board_fingerprint();

    let first = session
        .geometry()
        .across(0, 0)
        .expect("a board has a line along the top");
    session
        .mark_edge(first, Line::Drawn)
        .expect("a legal first move");
    assert_eq!(session.line_of(first), Line::Drawn);
    assert_ne!(session.board_fingerprint(), blank);

    session.undo().expect("one move, so one undo");
    assert_eq!(session.line_of(first), Line::Unknown);
    assert_eq!(
        session.board_fingerprint(),
        blank,
        "undo must give back the board it started from, not one that looks like it"
    );

    session.redo().expect("one undo, so one redo");
    assert_eq!(session.line_of(first), Line::Drawn);
}

#[test]
fn being_solved_is_asked_again_after_an_undo() {
    // The cached-answer test. A session that remembered "solved" would still say so
    // here, which is the single most expensive mistake this module could make.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    draw_the_answer(&mut session, &answer);
    assert!(session.is_solved());

    session.undo().expect("there are moves to undo");
    assert!(
        !session.is_solved(),
        "an undone answer is not an answer any more"
    );

    session.redo().expect("there is a move to redo");
    assert!(session.is_solved(), "and redoing it makes it one again");
}

#[test]
fn a_line_off_the_board_is_refused_rather_than_wrapping() {
    let puzzle = a_loaded_puzzle();
    let geometry = puzzle.slitherlink().expect("a Slitherlink").geometry();
    let dots_across = geometry.dots_across();
    let dots_down = geometry.dots_down();
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    // A line running across from the last dot in a row starts on the board and ends off
    // it, so there is no such line. The bug this guards against is that index becoming
    // some other line at the start of the next row.
    assert_eq!(
        session.mark(Which::Across, dots_across - 1, 0, Line::Drawn),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.mark(Which::Down, 0, dots_down - 1, Line::Drawn),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.mark(Which::Across, u16::MAX, u16::MAX, Line::Drawn),
        Err(NoChange::NoSuchSquare)
    );
    assert_eq!(
        session.line_at(Which::Across, dots_across - 1, 0),
        Line::Unknown,
        "and it reads as undecided rather than as somebody else's line"
    );
    assert_eq!(session.moves_made(), 0, "none of that was a move");
}

#[test]
fn marking_a_line_the_way_it_already_is_does_nothing_and_says_so() {
    let puzzle = a_loaded_puzzle();
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    session
        .mark(Which::Across, 0, 0, Line::Drawn)
        .expect("the first move");
    assert_eq!(
        session.mark(Which::Across, 0, 0, Line::Drawn),
        Err(NoChange::AlreadyThatWay)
    );
    assert_eq!(session.moves_made(), 1, "the second tap was not a move");
}

#[test]
fn the_numbers_are_the_ones_in_the_package() {
    let puzzle = a_loaded_puzzle();
    let clues = puzzle.slitherlink().expect("a Slitherlink").clone();
    let geometry = clues.geometry();
    let session = Session::start(puzzle).expect("a Slitherlink");

    let mut given = 0;
    for row in 0..geometry.height() {
        for column in 0..geometry.width() {
            let cell = geometry.cell(column, row).expect("a square on the board");
            assert_eq!(
                session.number_at(column, row),
                clues.at(cell),
                "the number shown at {column},{row} is the one in the package"
            );
            if clues.at(cell).is_some() {
                given += 1;
            }
        }
    }
    assert_eq!(
        given,
        clues.given_count(),
        "and every number in the package is on the board somewhere"
    );
    assert_eq!(
        session.number_at(geometry.width(), 0),
        None,
        "a square off the board has no number, rather than borrowing one"
    );
}

#[test]
fn the_count_beside_a_number_comes_from_the_rules() {
    // A page that counted its own marks would be a second opinion about the board. This
    // checks that the count a player sees and the count the rules judge are one count.
    let puzzle = a_loaded_puzzle();
    let geometry = puzzle.slitherlink().expect("a Slitherlink").geometry();
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    let cell = geometry.cell(0, 0).expect("the top-left square");
    assert_eq!(session.sides_drawn(0, 0), Some(0), "nothing drawn yet");

    let sides = geometry.edges_of_cell(cell).expect("four sides");
    session
        .mark_edge(sides[0], Line::Drawn)
        .expect("a legal move");
    assert_eq!(session.sides_drawn(0, 0), Some(1));

    session
        .mark_edge(sides[1], Line::RuledOut)
        .expect("a legal move");
    assert_eq!(
        session.sides_drawn(0, 0),
        Some(1),
        "a line ruled out is not a line drawn"
    );

    assert_eq!(
        session.sides_drawn(geometry.width(), 0),
        None,
        "a square off the board has no count"
    );
}

#[test]
fn the_shape_of_the_lines_is_reported_in_words_a_player_can_read() {
    let puzzle = a_loaded_puzzle();
    let geometry = puzzle.slitherlink().expect("a Slitherlink").geometry();
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    assert_eq!(session.shape().plainly(), "no lines are drawn yet");

    // One line from a dot and nothing else is a loose end, which is the commonest thing
    // a half-finished board is.
    let edge = geometry.across(0, 0).expect("a line along the top");
    session.mark_edge(edge, Line::Drawn).expect("a legal move");
    let said = session.shape().plainly();
    assert!(
        said.contains("mid-air"),
        "a single line is a loose end, and the sentence should say so: {said}"
    );
    assert!(
        !session.shape().is_one_loop(),
        "one line is not a closed loop"
    );
}

#[test]
fn the_squares_whose_numbers_are_not_met_shrink_to_none() {
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let clues = puzzle.slitherlink().expect("a Slitherlink").clone();
    let geometry = clues.geometry();
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    // A zero is met by a blank board: no sides of that square are used, which is what
    // it asks for. So the unmet list starts as every number *except* the zeros, and
    // this seed's board has three of them. Counting them here rather than writing 6
    // down, because the number would be about this seed and the property is not.
    let zeros = geometry
        .cells()
        .filter(|cell| clues.at(*cell) == Some(0))
        .count();
    assert!(
        zeros > 0,
        "this test is about zeros, so a board with none would pass for the wrong reason"
    );
    let not_met = session.numbers_not_met();
    assert_eq!(
        not_met.len() + zeros,
        usize::try_from(clues.given_count()).expect("a small board"),
        "on a blank board every number given is unmet except the zeros"
    );
    for (column, row) in &not_met {
        let cell = geometry.cell(*column, *row).expect("a square on the board");
        assert_ne!(
            clues.at(cell),
            Some(0),
            "a zero is met by a blank board and must not be in this list"
        );
    }

    draw_the_answer(&mut session, &answer);
    assert!(
        session.numbers_not_met().is_empty(),
        "the answer meets every number"
    );
}

#[test]
fn a_page_cannot_make_a_nonogram_move_on_a_slitherlink() {
    // The whole point of two session types. A single `mark(index)` for both games would
    // have let this through, and the line it marked would have been whichever line
    // happened to share that number.
    let puzzle = a_loaded_puzzle();
    let mut session = gs_web::play::Session::start(puzzle);

    assert!(
        session.as_nonogram().is_none(),
        "a Slitherlink has no Nonogram moves"
    );
    assert!(
        session.as_slitherlink().is_some(),
        "and it does have its own"
    );
    assert_eq!(session.puzzle().family(), "slitherlink");
    assert!(
        gs_web::play::slitherlink::Session::start(session.puzzle().clone()).is_some(),
        "its own session starts on it"
    );
    assert!(
        gs_web::play::nonogram::Session::start(session.puzzle().clone()).is_none(),
        "and the other game's does not"
    );
}

#[test]
fn the_board_the_page_shows_and_the_board_the_engine_hashes_are_the_same_board() {
    // `board_bytes` is what a saved game is written from and `board_fingerprint` is what
    // a report binds to, so the two must describe the same board as `line_of` does.
    let puzzle = a_loaded_puzzle();
    let answer = the_answer(&puzzle);
    let mut session = Session::start(puzzle).expect("a Slitherlink");
    draw_the_answer(&mut session, &answer);

    let bytes = session.board_bytes();
    assert!(!bytes.is_empty(), "a played board is not nothing");

    let before = session.board_fingerprint();
    session.undo().expect("there are moves to undo");
    assert_ne!(
        session.board_fingerprint(),
        before,
        "a different board has a different fingerprint"
    );
    session.redo().expect("there is a move to redo");
    assert_eq!(
        session.board_fingerprint(),
        before,
        "and the same board has the same one"
    );
    assert_eq!(session.board_bytes(), bytes);
}

#[test]
fn a_line_is_the_same_line_whichever_way_the_page_asks_for_it() {
    // Two ways in — by where it is, and by its number — and they must mean the same
    // line. If they ever disagreed, a page would draw one line and mark another.
    let puzzle = a_loaded_puzzle();
    let geometry = puzzle.slitherlink().expect("a Slitherlink").geometry();
    let mut session = Session::start(puzzle).expect("a Slitherlink");

    for row in 0..geometry.dots_down() {
        for column in 0..geometry.dots_across() {
            for (which, along) in [(Which::Across, Along::Across), (Which::Down, Along::Down)] {
                let by_place = match along {
                    Along::Across => geometry.across(column, row),
                    Along::Down => geometry.down(column, row),
                };
                let Some(edge) = by_place else { continue };
                session
                    .mark(which, column, row, Line::Drawn)
                    .expect("a line on the board can be drawn");
                assert_eq!(
                    session.line_of(edge),
                    Line::Drawn,
                    "drawing by place must mark the line that number names"
                );
                assert_eq!(session.line_at(which, column, row), Line::Drawn);
                session.undo().expect("take it back again");
            }
        }
    }
}
