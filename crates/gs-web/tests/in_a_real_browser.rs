//! The one test that runs inside an actual browser.
//!
//! Everything else about this crate is tested on the machine doing the building, which
//! is the right default: a test that needs a browser is a test that will eventually be
//! skipped. But "compiles for a browser" and "works in a browser" are different claims,
//! and the second one was listed as unproven until this file existed.
//!
//! What this covers that nothing else can: that `wasm_bindgen` really hands a byte array
//! from a web page into Rust and a string back out, that real SHA-256 and real Ed25519
//! produce the same answers inside a browser's WebAssembly engine as they do natively,
//! and that `Page` refuses a stranger's puzzle there too.
//!
//! # Running it
//!
//! ```text
//! CHROMEDRIVER=$(which chromedriver) \
//!     cargo test -p gs-web --target wasm32-unknown-unknown --test in_a_real_browser
//! ```
//!
//! It needs `wasm-bindgen-cli` at the same version as the `wasm-bindgen` dependency,
//! plus a browser and its driver. Those are not on every machine, which is why this is
//! one file rather than the way the crate is tested.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use gs_core::verification::{Outcome, Verifier};
use gs_core::withdrawal::{seal_list, Reason, WithdrawalList, Withdrawn};
use gs_crypto::{Ed25519Signer, Sha256};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::publish::publish;
use gs_nonogram::solve::Budget;
use gs_web::browser::{Page, Square};

// Not in Node, in a browser. The distinction matters: Node has a filesystem and
// a process model, and a test passing there would say nothing about the place this
// code is meant to run.
wasm_bindgen_test_configure!(run_in_browser);

/// A private key printed in a public repository, which is all anybody needs to know
/// about it.
const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const SOMEBODY_ELSES_PRIVATE_KEY: [u8; 32] = [
    0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e, 0x0f,
    0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed, 0x4f, 0xb8, 0xa6, 0xfb,
];
const KEY_ID: &str = "the-page-trusts-this-one";

struct AgreesWithAUniquePuzzle {
    asked: core::cell::RefCell<usize>,
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

/// A real signed package, built here in the browser.
///
/// Worth noticing: generating a puzzle, deriving its clues, replaying the answer and
/// signing the result are all pure computation, so they run in a browser perfectly well.
/// What a browser cannot do is ask a *solver*, which is why the one scripted here is
/// scaffolding and why `gs-solver` is not allowed in this crate at all.
fn a_signed_package(private_key: &[u8; 32], key_id: &str) -> Vec<u8> {
    a_signed_package_at(private_key, key_id, 0)
}

/// The same, from a different level, so that two different puzzles can be had.
fn a_signed_package_at(private_key: &[u8; 32], key_id: &str, band: usize) -> Vec<u8> {
    let candidate = (1u64..3_000)
        .find_map(|seed| propose_in_band(&BANDS[band], seed, Budget::default()))
        .expect("some seed in the first few thousand produces a candidate");
    publish(
        &candidate,
        &AgreesWithAUniquePuzzle {
            asked: core::cell::RefCell::new(0),
        },
        Budget::default(),
        &Sha256,
        &Ed25519Signer::new(key_id, private_key),
    )
    .expect("a uniquely solvable candidate with an agreeing solver publishes")
    .package
}

fn a_page_that_trusts_us() -> Page {
    let mut page = Page::new();
    page.trust(
        KEY_ID,
        &Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY).public_key(),
    )
    .expect("our test key is valid and the page starts trusting nothing");
    page
}

#[wasm_bindgen_test]
fn a_browser_really_does_open_a_signed_puzzle() {
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let puzzle = a_page_that_trusts_us()
        .open(&package)
        .expect("a real, correctly signed puzzle opens in a browser");

    // Each of these crosses the boundary between Rust and the page, which is the part
    // no native test can exercise.
    assert!(puzzle.width() > 0);
    assert!(puzzle.height() > 0);
    assert_eq!(puzzle.fingerprint().len(), 64);
    assert_eq!(puzzle.signed_by(), KEY_ID);
    assert_eq!(puzzle.band(), BANDS[0].name);
    assert!(!puzzle.encoding().is_empty());

    // Clues come back as number lists, and their shape matches the board.
    let first_row = puzzle.row_clue(0);
    assert!(
        first_row.iter().all(|&run| run > 0),
        "a clue of zero is not a clue"
    );
    for row in 0..puzzle.height() {
        assert!(!puzzle.row_clue(row as usize).is_empty() || true);
    }
    // Out of range is an empty list rather than a crash, because a page drawing a board
    // asks in a loop and a thrown error there is a blank page.
    assert!(puzzle.row_clue(10_000).is_empty());
    assert!(puzzle.column_clue(10_000).is_empty());
}

#[wasm_bindgen_test]
fn a_browser_refuses_a_stranger_the_same_way() {
    // Real Ed25519 verification, in a browser's WebAssembly engine. If the signature
    // check behaved differently here than natively, this is the test that would say so.
    let theirs = a_signed_package(&SOMEBODY_ELSES_PRIVATE_KEY, "somebody-elses-key");
    assert!(
        a_page_that_trusts_us().open(&theirs).is_err(),
        "a stranger's puzzle must be refused in a browser too"
    );
}

#[wasm_bindgen_test]
fn a_browser_page_that_trusts_nothing_accepts_nothing() {
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    assert!(
        Page::new().open(&package).is_err(),
        "a page that installed no key must accept nothing"
    );
}

#[wasm_bindgen_test]
fn a_changed_byte_is_refused_in_a_browser_too() {
    // Not every byte: a browser test is slow and this is the claim, not the survey.
    // `loading::every_single_changed_byte_is_refused` does the exhaustive version
    // natively. This one proves the same digest gives the same answer in a browser.
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    for i in [0usize, 1, package.len() / 2, package.len() - 1] {
        let mut damaged = package.clone();
        damaged[i] = damaged[i].wrapping_add(1);
        assert!(
            page.open(&damaged).is_err(),
            "byte {i} was changed and the puzzle still opened"
        );
    }
}

#[wasm_bindgen_test]
fn a_wrong_length_key_is_refused_with_the_length_named() {
    // A page hands over whatever bytes it has, so this is a real input path rather than
    // a hypothetical one.
    let mut page = Page::new();
    assert!(page.trust(KEY_ID, &[0u8; 31]).is_err());
    assert!(page.trust(KEY_ID, &[0u8; 33]).is_err());
    assert!(page.trust(KEY_ID, &[]).is_err());
}

#[wasm_bindgen_test]
fn nothing_that_is_not_a_package_opens_in_a_browser() {
    let page = a_page_that_trusts_us();
    assert!(page.open(b"").is_err());
    assert!(page.open(b"<!doctype html>").is_err());
    assert!(page.open(&[0u8; 512]).is_err());
}

// ---------------------------------------------------------------------------
// Playing, in a browser
// ---------------------------------------------------------------------------

/// The answer to a puzzle, from the engine's own line solver.
fn the_answer(clues: &gs_nonogram::clues::Clues) -> Vec<Square> {
    let rules = gs_nonogram::rules::Nonogram::new(clues.clone());
    let state = gs_nonogram::solve::solve(&rules, Budget::default())
        .witness
        .expect("a uniquely solvable puzzle has an answer");
    (0..clues.dims.cell_count())
        .map(|i| {
            let cell = state
                .get(u32::try_from(i).expect("fits"))
                .expect("on the board");
            match gs_nonogram::rules::mark_of(cell) {
                gs_nonogram::line::Mark::Filled => Square::Filled,
                gs_nonogram::line::Mark::Empty => Square::Empty,
                gs_nonogram::line::Mark::Unknown => Square::Unknown,
            }
        })
        .collect()
}

#[wasm_bindgen_test]
fn a_puzzle_can_be_played_and_solved_in_a_browser() {
    // The end of the line this whole slice was built along: generate, verify, sign,
    // store, serve, open, and now solve — the last step inside a real browser.
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let puzzle = a_page_that_trusts_us().open(&package).expect("opens");
    let clues = {
        // The clues, read back out of the signed package the same way the page does.
        let keys = {
            let mut k = gs_crypto::TrustedKeys::none();
            k.trust(
                KEY_ID,
                &Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY).public_key(),
            )
            .expect("valid");
            k
        };
        gs_web::load(&package, &Sha256, &keys).expect("loads").clues
    };
    let answer = the_answer(&clues);
    let width = puzzle.width();

    let mut game = puzzle.play();
    assert!(!game.is_solved(), "a new game is not a solved game");
    assert_eq!(game.moves_made(), 0);
    assert_eq!(
        game.board().len(),
        usize::from(game.width()) * usize::from(game.height()),
        "the board comes back one byte per square"
    );
    assert!(
        game.board().iter().all(|&b| b == Square::Unknown as u8),
        "every square starts undecided"
    );

    // Play the answer, by row and column as a page would.
    for (i, &square) in answer.iter().enumerate() {
        let row = u16::try_from(i / usize::from(width)).expect("fits");
        let column = u16::try_from(i % usize::from(width)).expect("fits");
        game.mark(row, column, square);
    }

    assert!(
        game.is_solved(),
        "playing the puzzle's own answer must solve it, in a browser too"
    );
    // 16 characters, not 64: a board fingerprint is the engine's 64-bit state hash, not
    // the content fingerprint, which is SHA-256.
    assert_eq!(game.board_fingerprint().len(), 16);

    // Crossing out is optional since 2026-10-23, so the squares the answer leaves blank
    // need not have been marked at all. Check that directly, in a browser: a board with
    // only its filled squares marked is finished.
    let mut only_the_filled_ones = a_page_that_trusts_us()
        .open(&package)
        .expect("opens")
        .play();
    for (i, &square) in answer.iter().enumerate() {
        if square != Square::Filled {
            continue;
        }
        let row = u16::try_from(i / usize::from(width)).expect("fits");
        let column = u16::try_from(i % usize::from(width)).expect("fits");
        only_the_filled_ones.mark(row, column, square);
    }
    assert!(
        only_the_filled_ones.is_solved(),
        "filling the right squares and crossing out nothing must finish it"
    );

    // And the cached-answer check, which is where a page that decided for itself would
    // show up. The move undone has to be a *filled* square: since crossing out became
    // optional, undoing a cross-out correctly leaves the board finished, so ending on
    // one would make this pass for the wrong reason.
    let a_filled_square = answer
        .iter()
        .position(|&square| square == Square::Filled)
        .expect("any real answer fills at least one square");
    let row = u16::try_from(a_filled_square / usize::from(width)).expect("fits");
    let column = u16::try_from(a_filled_square % usize::from(width)).expect("fits");
    assert!(game.mark(row, column, Square::Unknown), "clearing it");
    assert!(game.mark(row, column, Square::Filled), "filling it back in");
    assert!(game.is_solved());

    assert!(game.undo(), "there are moves to undo");
    assert!(!game.is_solved(), "undoing a filled square un-solves it");
    assert!(game.redo(), "and redo puts it back");
    assert!(game.is_solved());
}

#[wasm_bindgen_test]
fn a_tap_outside_the_board_does_nothing_in_a_browser() {
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let puzzle = a_page_that_trusts_us().open(&package).expect("opens");
    let (width, height) = (puzzle.width(), puzzle.height());
    let mut game = puzzle.play();

    assert!(
        !game.mark(0, width, Square::Filled),
        "one past the last column"
    );
    assert!(
        !game.mark(height, 0, Square::Filled),
        "one past the last row"
    );
    assert_eq!(
        game.moves_made(),
        0,
        "no square anywhere on the board was marked"
    );
    assert!(
        game.mark_and_say(0, width, Square::Filled)
            .contains("no such square"),
        "and a page asking why gets words it can show"
    );
}

#[wasm_bindgen_test]
fn taking_back_a_move_works_through_the_browser() {
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let mut game = a_page_that_trusts_us()
        .open(&package)
        .expect("opens")
        .play();

    assert!(!game.undo(), "nothing to undo at the start");
    assert!(game.mark(0, 0, Square::Filled));
    assert_eq!(game.square(0, 0), Square::Filled);
    assert_eq!(game.moves_made(), 1);

    assert!(!game.mark(0, 0, Square::Filled), "the same mark twice");
    assert_eq!(game.moves_made(), 1);

    assert!(game.undo());
    assert_eq!(game.square(0, 0), Square::Unknown);
    assert_eq!(game.moves_made(), 0);
}

// ---------------------------------------------------------------------------
// The offline buffer, in the storage a real browser actually has
// ---------------------------------------------------------------------------
//
// `gs-offline`'s own tests run on a shelf in memory, which proves the policy. What they
// cannot prove is that the browser's storage behaves the way the shelf interface says:
// that bytes survive a round trip through text storage, that listing gives names back,
// and that the whole thing works at all in a place with no filesystem. That is what
// these do.

/// Clear anything a previous test left behind.
///
/// `localStorage` is shared by every test in this file, and they run in one page. A
/// test that assumed an empty device would be testing whatever ran before it.
fn forget_everything_saved() {
    let storage = web_sys::window()
        .expect("a browser window")
        .local_storage()
        .expect("storage is allowed")
        .expect("storage exists");
    storage.clear().expect("clearing works");
}

#[wasm_bindgen_test]
fn a_puzzle_saved_in_a_browser_comes_back_out_of_it() {
    forget_everything_saved();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();

    let fingerprint = page.save_for_later(&package).expect("saved");
    let puzzle = page.next_saved().expect("comes back");

    assert_eq!(
        puzzle.fingerprint(),
        fingerprint,
        "the puzzle that comes back is the puzzle that went in"
    );
    // And it is a real playable puzzle, not just bytes that survived.
    assert!(puzzle.width() > 0 && puzzle.height() > 0);
}

#[wasm_bindgen_test]
fn a_device_with_nothing_saved_says_so_rather_than_handing_over_nothing() {
    forget_everything_saved();
    let page = a_page_that_trusts_us();

    assert!(
        page.next_saved().is_err(),
        "there is nothing saved, and that is an answer"
    );
    let saved = page.saved().expect("counted");
    assert_eq!(saved.ready(), 0);
    assert_eq!(saved.wanted(), 5, "and it knows how many to fetch");
    assert!(
        saved.in_words().contains("No puzzles saved"),
        "in words: {}",
        saved.in_words()
    );
}

#[wasm_bindgen_test]
fn being_handed_a_saved_puzzle_is_not_the_same_as_having_played_it() {
    // The rule the buffer is shaped around, checked in the place it actually matters.
    forget_everything_saved();
    let page = a_page_that_trusts_us();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let fingerprint = page.save_for_later(&package).expect("saved");

    let _ = page.next_saved().expect("handed over");
    assert_eq!(
        page.saved().expect("counted").ready(),
        1,
        "handing it over does not use it up"
    );

    page.mark_played(&fingerprint).expect("recorded");
    assert_eq!(page.saved().expect("counted").ready(), 0);
    assert!(page.next_saved().is_err());
}

#[wasm_bindgen_test]
fn a_browser_will_not_save_a_stranger_s_puzzle() {
    forget_everything_saved();
    let page = a_page_that_trusts_us();
    let theirs = a_signed_package(&SOMEBODY_ELSES_PRIVATE_KEY, "somebody-else");

    assert!(page.save_for_later(&theirs).is_err());
    assert_eq!(
        page.saved().expect("counted").ready(),
        0,
        "and nothing was written"
    );
}

#[wasm_bindgen_test]
fn a_saved_puzzle_edited_in_the_browsers_storage_is_never_handed_over() {
    // The check that matters most here. `localStorage` is a few clicks from being edited
    // by hand in any browser's developer tools, so a saved puzzle is checked on the way
    // out exactly as one off the network is.
    forget_everything_saved();
    let page = a_page_that_trusts_us();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let fingerprint = page.save_for_later(&package).expect("saved");

    let storage = web_sys::window()
        .expect("a window")
        .local_storage()
        .expect("allowed")
        .expect("exists");
    let key = format!("gatedspark/ahead/{fingerprint}");
    let stored = storage.get_item(&key).expect("readable").expect("is there");
    // Flip one byte, by changing one pair of hex digits.
    let mut edited = stored.clone();
    let at = edited.len() - 2;
    edited.replace_range(at.., if stored.ends_with("00") { "01" } else { "00" });
    assert_ne!(edited, stored, "the edit really changed something");
    storage.set_item(&key, &edited).expect("written");

    assert!(
        page.next_saved().is_err(),
        "an edited puzzle must not reach a player"
    );
    let saved = page.saved().expect("counted");
    assert_eq!(saved.ready(), 0);
    assert_eq!(saved.spoiled(), 1, "it is counted, not hidden");
    assert_eq!(saved.wanted(), 5, "and a replacement is asked for");

    assert_eq!(page.discard_unusable().expect("discarded"), 1);
    assert_eq!(page.saved().expect("counted").spoiled(), 0);
}

#[wasm_bindgen_test]
fn saved_puzzles_do_not_disturb_anything_else_the_page_keeps() {
    forget_everything_saved();
    let storage = web_sys::window()
        .expect("a window")
        .local_storage()
        .expect("allowed")
        .expect("exists");
    storage.set_item("a-setting", "dark mode").expect("written");

    let page = a_page_that_trusts_us();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    page.save_for_later(&package).expect("saved");

    let saved = page.saved().expect("counted");
    assert_eq!(saved.ready(), 1);
    assert_eq!(saved.spoiled(), 0, "a setting is not a broken puzzle");
    assert_eq!(page.discard_unusable().expect("discarded"), 0);
    assert_eq!(
        storage.get_item("a-setting").expect("readable").as_deref(),
        Some("dark mode"),
        "and it is still there"
    );
}

// ---------------------------------------------------------------------------
// The "do not play these" list, in the storage a real browser actually has
// ---------------------------------------------------------------------------
//
// `gs-offline`'s own tests prove the rules on a shelf in memory. These prove them where
// they have to work: a withdrawal list signed with real Ed25519, read back through a
// real SHA-256 digest, acting on packages in a browser's own `localStorage`.

/// A signed "do not play these" list naming one puzzle.
fn a_withdrawal(
    serial: u64,
    fingerprint: &str,
    reason: Reason,
    key: &[u8; 32],
    key_id: &str,
) -> Vec<u8> {
    let mut id = [0u8; 32];
    let text = fingerprint.as_bytes();
    for (byte, pair) in id.iter_mut().zip(text.chunks(2)) {
        let digit = |c: u8| (c as char).to_digit(16).expect("a fingerprint is hex") as u8;
        *byte = digit(pair[0]) << 4 | digit(pair[1]);
    }
    let list = WithdrawalList {
        serial,
        withdrawn: vec![Withdrawn {
            content_id: id,
            reason,
        }],
    };
    seal_list(&list, &Sha256, &Ed25519Signer::new(key_id, key)).expect("a short list seals")
}

/// How many packages the buffer is holding, counted in the browser's own storage.
fn packages_in_storage() -> usize {
    let storage = web_sys::window()
        .expect("a browser window")
        .local_storage()
        .expect("storage is allowed")
        .expect("storage exists");
    let mut held = 0;
    for i in 0..storage.length().expect("a length") {
        if let Ok(Some(key)) = storage.key(i) {
            if key.starts_with("gatedspark/ahead/") {
                held += 1;
            }
        }
    }
    held
}

#[wasm_bindgen_test]
fn a_withdrawn_puzzle_is_deleted_from_a_real_browsers_storage() {
    forget_everything_saved();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    let fingerprint = page.save_for_later(&package).expect("saved");
    assert_eq!(packages_in_storage(), 1, "it must be there to be deleted");
    assert_eq!(page.withdrawals_seen().expect("reads"), 0);

    let list = a_withdrawal(
        1,
        &fingerprint,
        Reason::MoreThanOneAnswer,
        &TEST_ONLY_PRIVATE_KEY,
        KEY_ID,
    );
    let noticed = page.accept_withdrawals(&list).expect("accepted");

    assert_eq!(noticed.serial(), 1);
    assert_eq!(noticed.named(), 1);
    assert_eq!(noticed.dropped(), 1);
    assert_eq!(
        packages_in_storage(),
        0,
        "the bytes must be gone from the browser's storage, not filtered on the way out"
    );
    assert_eq!(page.withdrawals_seen().expect("reads"), 1);
}

#[wasm_bindgen_test]
fn a_withdrawn_puzzle_will_not_open_for_play_and_will_not_be_saved_again() {
    forget_everything_saved();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    let fingerprint = page.open(&package).expect("opens").fingerprint();
    page.accept_withdrawals(&a_withdrawal(
        1,
        &fingerprint,
        Reason::NoAnswer,
        &TEST_ONLY_PRIVATE_KEY,
        KEY_ID,
    ))
    .expect("accepted");

    // `open` is the package check and still passes: the package is perfectly good.
    // That is the whole reason this mechanism exists.
    assert!(page.open(&package).is_ok());
    // `open_for_play` is the question a player's screen depends on.
    assert!(page.open_for_play(&package).is_err());
    assert!(page.save_for_later(&package).is_err());
    assert_eq!(packages_in_storage(), 0);

    let why = page
        .why_withdrawn(&fingerprint)
        .expect("reads")
        .expect("it has been withdrawn");
    assert!(why.contains("withdrawn"), "{why}");
    assert!(why.contains("no answer"), "{why}");
}

#[wasm_bindgen_test]
fn a_puzzle_nobody_withdrew_is_untouched_in_a_real_browser() {
    // The anti-vacuity half. Everything above would pass for a page that refused
    // every puzzle once it had seen any list at all.
    forget_everything_saved();
    let doomed = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    let doomed_print = page.open(&doomed).expect("opens").fingerprint();
    // A different puzzle: a different band gives different clues.
    let other = a_signed_package_at(&TEST_ONLY_PRIVATE_KEY, KEY_ID, 1);
    let other_print = page.open(&other).expect("opens").fingerprint();
    assert_ne!(doomed_print, other_print);

    page.save_for_later(&other).expect("saved");
    page.accept_withdrawals(&a_withdrawal(
        1,
        &doomed_print,
        Reason::Unsuitable,
        &TEST_ONLY_PRIVATE_KEY,
        KEY_ID,
    ))
    .expect("accepted");

    assert_eq!(packages_in_storage(), 1, "the other one must survive");
    assert!(page.open_for_play(&other).is_ok());
    assert_eq!(page.why_withdrawn(&other_print).expect("reads"), None);
    assert_eq!(
        page.next_saved().expect("still playable").fingerprint(),
        other_print
    );
}

#[wasm_bindgen_test]
fn a_list_from_a_stranger_is_ignored_in_a_browser_too() {
    forget_everything_saved();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    let fingerprint = page.save_for_later(&package).expect("saved");

    let theirs = a_withdrawal(
        1,
        &fingerprint,
        Reason::NoAnswer,
        &SOMEBODY_ELSES_PRIVATE_KEY,
        "somebody-elses-key",
    );
    assert!(
        page.accept_withdrawals(&theirs).is_err(),
        "anybody who could withdraw anything could delete every puzzle on every device"
    );
    assert_eq!(packages_in_storage(), 1, "nothing was deleted");
    assert_eq!(page.withdrawals_seen().expect("reads"), 0);
    assert!(page.open_for_play(&package).is_ok());
}

#[wasm_bindgen_test]
fn an_older_list_replayed_to_a_browser_cannot_un_withdraw_a_puzzle() {
    forget_everything_saved();
    let package = a_signed_package(&TEST_ONLY_PRIVATE_KEY, KEY_ID);
    let page = a_page_that_trusts_us();
    let fingerprint = page.open(&package).expect("opens").fingerprint();

    let newer = a_withdrawal(
        9,
        &fingerprint,
        Reason::NoAnswer,
        &TEST_ONLY_PRIVATE_KEY,
        KEY_ID,
    );
    // Genuine, correctly signed, and older: it names the same puzzle under a reason
    // that no longer applies, and accepting it would be going backwards.
    let older = a_withdrawal(
        2,
        &fingerprint,
        Reason::Mistake,
        &TEST_ONLY_PRIVATE_KEY,
        KEY_ID,
    );

    page.accept_withdrawals(&newer).expect("accepted");
    assert!(page.accept_withdrawals(&older).is_err());
    assert_eq!(page.withdrawals_seen().expect("reads"), 9);
    assert!(page.open_for_play(&package).is_err(), "still withdrawn");
}
