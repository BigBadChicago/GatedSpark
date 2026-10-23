//! What a device keeps, what it hands over, and what it refuses.

mod common;

use common::{
    a_package, a_package_at, signed_by_a_stranger, this_device_trusts_us, KEY_ID, SOMEBODY_ELSE,
    SOMEBODY_ELSES_PRIVATE_KEY,
};
use gs_core::package::OpenError;
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_offline::buffer::{name_for, NotKept, NothingToPlay, PREFIX};
use gs_offline::shelf::{AlwaysFails, Memory, Shelf};
use gs_offline::{Buffer, HOW_MANY_AHEAD};

/// A device with nothing on it yet.
fn a_blank_device() -> (Buffer, Memory, TrustedKeys) {
    (Buffer::new(), Memory::new(), this_device_trusts_us())
}

/// Fill a device with `n` acceptable puzzles.
fn fill(buffer: &Buffer, shelf: &mut Memory, keys: &TrustedKeys, n: u64) {
    for i in 0..n {
        buffer
            .keep(shelf, &a_package(i), &Sha256, keys)
            .expect("a package this device trusts is kept");
    }
}

// ---------------------------------------------------------------------------
// Keeping
// ---------------------------------------------------------------------------

#[test]
fn a_puzzle_kept_can_be_played_back_with_no_network_of_any_kind() {
    // The whole point, in one test. Nothing here reaches for anything.
    let (buffer, mut shelf, keys) = a_blank_device();
    let package = a_package_at(1, "gentle");

    let id = buffer
        .keep(&mut shelf, &package, &Sha256, &keys)
        .expect("kept");
    let delivered = buffer.next(&shelf, &Sha256, &keys).expect("handed over");

    assert_eq!(delivered.content_id, id);
    assert_eq!(delivered.signed_by, KEY_ID);
    assert_eq!(delivered.band.as_deref(), Some("gentle"));
    assert_eq!(delivered.package, package, "the bytes come back unchanged");
}

#[test]
fn the_name_a_puzzle_is_kept_under_is_its_own_fingerprint() {
    // Not a name the caller supplied. A name alongside the bytes is a claim; a content
    // id is not one, which is the whole reason the package carries it.
    let (buffer, mut shelf, keys) = a_blank_device();
    let id = buffer
        .keep(&mut shelf, &a_package(1), &Sha256, &keys)
        .expect("kept");

    assert_eq!(shelf.names().expect("names"), vec![name_for(&id)]);
    assert!(name_for(&id).starts_with(PREFIX));
    assert!(name_for(&id).ends_with(&id.to_hex()));
}

#[test]
fn keeping_the_same_puzzle_twice_keeps_one_copy() {
    let (buffer, mut shelf, keys) = a_blank_device();
    let package = a_package(1);
    let first = buffer
        .keep(&mut shelf, &package, &Sha256, &keys)
        .expect("kept");
    let again = buffer
        .keep(&mut shelf, &package, &Sha256, &keys)
        .expect("kept again");

    assert_eq!(first, again);
    assert_eq!(shelf.len(), 1);
}

#[test]
fn a_puzzle_from_a_stranger_is_never_kept() {
    let (buffer, mut shelf, keys) = a_blank_device();

    match buffer.keep(&mut shelf, &signed_by_a_stranger(1), &Sha256, &keys) {
        Err(NotKept::NotAcceptable(OpenError::SignatureRejected)) => {}
        other => panic!("a stranger's puzzle must be refused, got {other:?}"),
    }
    assert!(
        shelf.is_empty(),
        "nothing is written for a puzzle that was refused"
    );
}

#[test]
fn a_puzzle_with_one_byte_changed_is_never_kept() {
    let (buffer, mut shelf, keys) = a_blank_device();
    let mut package = a_package(1);
    let last = package.len() - 1;
    package[last] ^= 1;

    assert!(matches!(
        buffer.keep(&mut shelf, &package, &Sha256, &keys),
        Err(NotKept::NotAcceptable(_))
    ));
    assert!(shelf.is_empty());
}

#[test]
fn something_that_is_not_a_package_is_never_kept() {
    let (buffer, mut shelf, keys) = a_blank_device();
    for rubbish in [b"".as_slice(), b"hello".as_slice(), &[0xff; 200]] {
        assert!(matches!(
            buffer.keep(&mut shelf, rubbish, &Sha256, &keys),
            Err(NotKept::NotAcceptable(_))
        ));
    }
    assert!(shelf.is_empty());
}

// ---------------------------------------------------------------------------
// Handing over
// ---------------------------------------------------------------------------

#[test]
fn a_device_with_nothing_on_it_says_so_in_words_a_person_can_read() {
    let (buffer, shelf, keys) = a_blank_device();

    let why = buffer
        .next(&shelf, &Sha256, &keys)
        .expect_err("there is nothing to play");
    assert_eq!(why, NothingToPlay::Empty);

    let shown = why.to_string();
    assert!(
        shown.contains("no more puzzles") && shown.contains("internet"),
        "a player should be told what happened and what to do: {shown}"
    );
    assert!(
        !shown.contains("Empty") && !shown.contains("NothingToPlay"),
        "a person should not be shown a type name: {shown}"
    );
}

#[test]
fn a_puzzle_that_was_edited_on_the_shelf_is_never_handed_over() {
    // The check that matters. A shelf is browser storage on a real device, which is a
    // few clicks from being edited by hand. Nothing comes off it unchecked.
    let (buffer, mut shelf, keys) = a_blank_device();
    let id = buffer
        .keep(&mut shelf, &a_package(1), &Sha256, &keys)
        .expect("kept");

    // Reach around the buffer and tamper with what it stored.
    let mut stored = shelf
        .get(&name_for(&id))
        .expect("readable")
        .expect("it is there");
    let last = stored.len() - 1;
    stored[last] ^= 1;
    shelf.put(&name_for(&id), &stored).expect("written");

    assert_eq!(
        buffer.next(&shelf, &Sha256, &keys).err(),
        Some(NothingToPlay::Empty),
        "an edited puzzle must not be handed to a player"
    );
}

#[test]
fn a_stranger_s_puzzle_planted_on_the_shelf_is_never_handed_over() {
    // The same check, against somebody who did not edit a puzzle but wrote their own
    // and put it where the buffer keeps things. A properly signed file is still only
    // acceptable if it is signed by somebody this device trusts.
    let (buffer, mut shelf, keys) = a_blank_device();
    shelf
        .put("ahead/planted-by-somebody-else", &signed_by_a_stranger(1))
        .expect("written");

    assert_eq!(
        buffer.next(&shelf, &Sha256, &keys).err(),
        Some(NothingToPlay::Empty)
    );
}

#[test]
fn a_bad_puzzle_does_not_hide_the_good_ones_behind_it() {
    // Anti-vacuity for the two tests above: a buffer that refused *everything* once one
    // entry was bad would pass them both while being useless.
    let (buffer, mut shelf, keys) = a_blank_device();
    shelf
        .put("ahead/planted-by-somebody-else", &signed_by_a_stranger(1))
        .expect("written");
    let good = buffer
        .keep(&mut shelf, &a_package(7), &Sha256, &keys)
        .expect("kept");

    let delivered = buffer
        .next(&shelf, &Sha256, &keys)
        .expect("the good one is still playable");
    assert_eq!(delivered.content_id, good);
}

#[test]
fn things_on_the_shelf_that_are_not_puzzles_are_left_alone() {
    // A shelf on a real device is shared with everything else the app keeps. The buffer
    // owns the names beginning with its prefix and nothing else.
    let (buffer, mut shelf, keys) = a_blank_device();
    shelf.put("a-setting", b"dark mode").expect("written");
    shelf.put("the-players-name", b"Sam").expect("written");
    buffer
        .keep(&mut shelf, &a_package(1), &Sha256, &keys)
        .expect("kept");

    let stock = buffer.stock(&shelf, &Sha256, &keys).expect("counted");
    assert_eq!(stock.sound, 1, "the settings are not puzzles");
    assert_eq!(stock.spoiled, 0, "and they are not broken puzzles either");

    assert_eq!(
        buffer.discard_spoiled(&mut shelf, &Sha256, &keys),
        Ok(0),
        "throwing away bad puzzles must not throw away the settings"
    );
    assert_eq!(
        shelf.get("a-setting").expect("readable"),
        Some(b"dark mode".to_vec())
    );
}

// ---------------------------------------------------------------------------
// Delivered is not played
// ---------------------------------------------------------------------------

#[test]
fn being_handed_a_puzzle_is_not_the_same_as_having_played_it() {
    // The rule this whole crate is shaped around. Asking twice gives the same puzzle,
    // because nothing has said it was played.
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 3);

    let first = buffer.next(&shelf, &Sha256, &keys).expect("one");
    let again = buffer.next(&shelf, &Sha256, &keys).expect("the same one");
    assert_eq!(first.content_id, again.content_id);
    assert_eq!(
        buffer.stock(&shelf, &Sha256, &keys).expect("counted").sound,
        3,
        "handing a puzzle over does not use it up"
    );
}

#[test]
fn a_puzzle_marked_played_is_not_handed_out_again() {
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 3);

    let played = buffer.next(&shelf, &Sha256, &keys).expect("one");
    buffer
        .played(&mut shelf, &played.content_id)
        .expect("recorded");

    let next = buffer.next(&shelf, &Sha256, &keys).expect("another");
    assert_ne!(next.content_id, played.content_id);
    assert_eq!(
        buffer.stock(&shelf, &Sha256, &keys).expect("counted").sound,
        2
    );
}

#[test]
fn playing_through_everything_ends_with_the_same_plain_sentence() {
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 3);

    for _ in 0..3 {
        let one = buffer.next(&shelf, &Sha256, &keys).expect("one left");
        buffer.played(&mut shelf, &one.content_id).expect("played");
    }

    assert_eq!(
        buffer.next(&shelf, &Sha256, &keys).err(),
        Some(NothingToPlay::Empty)
    );
}

#[test]
fn every_puzzle_is_handed_out_exactly_once() {
    // Stronger than the test above, which would pass if the buffer handed out the same
    // puzzle three times and then went empty.
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 5);

    let mut seen = Vec::new();
    while let Ok(one) = buffer.next(&shelf, &Sha256, &keys) {
        buffer.played(&mut shelf, &one.content_id).expect("played");
        seen.push(one.content_id);
    }

    assert_eq!(seen.len(), 5);
    let mut sorted: Vec<_> = seen
        .iter()
        .map(gs_core::content::ContentId::to_hex)
        .collect();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5, "no puzzle was handed out twice");
}

// ---------------------------------------------------------------------------
// How many to hold
// ---------------------------------------------------------------------------

#[test]
fn the_target_is_five_and_a_device_knows_how_many_more_to_ask_for() {
    let (buffer, mut shelf, keys) = a_blank_device();
    assert_eq!(HOW_MANY_AHEAD, 5, "the agreed number");

    assert_eq!(
        buffer
            .stock(&shelf, &Sha256, &keys)
            .expect("counted")
            .wanted(),
        5
    );
    fill(&buffer, &mut shelf, &keys, 2);
    assert_eq!(
        buffer
            .stock(&shelf, &Sha256, &keys)
            .expect("counted")
            .wanted(),
        3
    );
    fill(&buffer, &mut shelf, &keys, 5);
    let stock = buffer.stock(&shelf, &Sha256, &keys).expect("counted");
    assert_eq!(stock.sound, 5);
    assert_eq!(stock.wanted(), 0);
}

#[test]
fn holding_more_than_the_target_asks_for_none_rather_than_a_negative_number() {
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 9);

    let stock = buffer.stock(&shelf, &Sha256, &keys).expect("counted");
    assert_eq!(stock.sound, 9, "the buffer holds what it is given");
    assert_eq!(stock.wanted(), 0);
}

#[test]
fn a_spoiled_puzzle_counts_against_the_target_so_the_device_asks_for_a_replacement() {
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 5);
    let id = buffer.next(&shelf, &Sha256, &keys).expect("one").content_id;

    let mut stored = shelf.get(&name_for(&id)).expect("readable").expect("there");
    stored[10] ^= 0xff;
    shelf.put(&name_for(&id), &stored).expect("written");

    let stock = buffer.stock(&shelf, &Sha256, &keys).expect("counted");
    assert_eq!(stock.sound, 4);
    assert_eq!(stock.spoiled, 1, "it is counted, not hidden");
    assert_eq!(stock.wanted(), 1, "and a replacement is asked for");
}

#[test]
fn throwing_away_the_spoiled_ones_keeps_the_rest() {
    let (buffer, mut shelf, keys) = a_blank_device();
    fill(&buffer, &mut shelf, &keys, 3);
    shelf
        .put("ahead/not-even-a-package", b"rubbish")
        .expect("written");

    assert_eq!(buffer.discard_spoiled(&mut shelf, &Sha256, &keys), Ok(1));
    let stock = buffer.stock(&shelf, &Sha256, &keys).expect("counted");
    assert_eq!(stock.sound, 3);
    assert_eq!(stock.spoiled, 0);
}

#[test]
fn reading_the_buffer_never_throws_anything_away_by_itself() {
    // Deliberate, and the reason `discard_spoiled` is a separate call. A puzzle that
    // fails today because the device's storage is misbehaving would be gone forever if
    // reading deleted it, and a player with no signal would have nothing left.
    let (buffer, mut shelf, keys) = a_blank_device();
    shelf
        .put("ahead/not-even-a-package", b"rubbish")
        .expect("written");
    fill(&buffer, &mut shelf, &keys, 1);

    let _ = buffer.next(&shelf, &Sha256, &keys).expect("the good one");
    let _ = buffer.stock(&shelf, &Sha256, &keys).expect("counted");

    assert_eq!(
        shelf.get("ahead/not-even-a-package").expect("readable"),
        Some(b"rubbish".to_vec()),
        "neither reading nor counting may delete"
    );
}

// ---------------------------------------------------------------------------
// When the storage itself misbehaves
// ---------------------------------------------------------------------------

#[test]
fn a_device_that_will_not_store_anything_says_so_rather_than_pretending() {
    let buffer = Buffer::new();
    let keys = this_device_trusts_us();
    let mut broken = AlwaysFails("this browser is in a private window".to_owned());

    match buffer.keep(&mut broken, &a_package(1), &Sha256, &keys) {
        Err(NotKept::ShelfBroke(why)) => {
            assert!(why.to_string().contains("private window"));
        }
        other => panic!("expected the storage failure to be reported, got {other:?}"),
    }
}

#[test]
fn a_device_that_will_not_read_says_so_rather_than_claiming_to_be_empty() {
    // The distinction worth keeping: "you have no puzzles left" and "this device will
    // not tell me" are different facts, and a player can act on them differently.
    let buffer = Buffer::new();
    let keys = this_device_trusts_us();
    let broken = AlwaysFails("storage is unavailable".to_owned());

    match buffer.next(&broken, &Sha256, &keys) {
        Err(NothingToPlay::ShelfBroke(why)) => {
            assert!(why.to_string().contains("unavailable"));
        }
        other => panic!("expected the storage failure, got {other:?}"),
    }
}

#[test]
fn both_storage_failures_are_explained_in_words_a_person_can_read() {
    let messages = [
        NothingToPlay::ShelfBroke(gs_offline::ShelfError::new("the disk is full")).to_string(),
        NotKept::ShelfBroke(gs_offline::ShelfError::new("the disk is full")).to_string(),
        NotKept::NotAcceptable(OpenError::SignatureRejected).to_string(),
    ];
    for shown in messages {
        assert!(
            !shown.contains("ShelfBroke")
                && !shown.contains("NotAcceptable")
                && !shown.contains("SignatureRejected"),
            "a person should not be shown a type name: {shown}"
        );
        assert!(
            shown.ends_with('.') || shown.contains("full"),
            "a sentence: {shown}"
        );
    }
}

// ---------------------------------------------------------------------------
// The buffer knows nothing about any game
// ---------------------------------------------------------------------------

#[test]
fn the_buffer_holds_a_package_from_a_family_that_does_not_exist() {
    // Stated as a test because it is the claim the crate's dependency list makes. Every
    // package in this file is sealed with a made-up family and a payload that is not a
    // puzzle, and the buffer has handled all of them without noticing.
    let (buffer, mut shelf, keys) = a_blank_device();
    let id = buffer
        .keep(&mut shelf, &a_package(1), &Sha256, &keys)
        .expect("kept");
    let delivered = buffer.next(&shelf, &Sha256, &keys).expect("handed over");

    assert_eq!(delivered.content_id, id);
    assert!(
        String::from_utf8_lossy(&delivered.package).contains("this is not a puzzle"),
        "the payload is nonsense and the buffer kept it anyway"
    );
}

#[test]
fn a_device_that_trusts_nobody_accepts_nothing() {
    // The empty key list, which is what a device has before it is set up. It must be an
    // honest refusal rather than an accidental "no checks to fail".
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let nobody = TrustedKeys::none();

    assert!(matches!(
        buffer.keep(&mut shelf, &a_package(1), &Sha256, &nobody),
        Err(NotKept::NotAcceptable(_))
    ));
    assert!(shelf.is_empty());
}

#[test]
fn a_device_that_trusts_a_different_key_accepts_nothing_of_ours() {
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let mut theirs = TrustedKeys::none();
    theirs
        .trust(
            SOMEBODY_ELSE,
            &Ed25519Signer::new(SOMEBODY_ELSE, &SOMEBODY_ELSES_PRIVATE_KEY).public_key(),
        )
        .expect("valid");

    assert!(matches!(
        buffer.keep(&mut shelf, &a_package(1), &Sha256, &theirs),
        Err(NotKept::NotAcceptable(_))
    ));
    // And the anti-vacuity half: their own package is fine under their own key.
    assert!(buffer
        .keep(&mut shelf, &signed_by_a_stranger(1), &Sha256, &theirs)
        .is_ok());
}
