//! Taking a puzzle back off a device that already has it.
//!
//! The tests here are about the three rules in `notices.rs`: newer lists only, delete
//! rather than hide, and check the stored list every time it is read.
//!
//! The middle one is the one that is easy to get wrong and hard to notice. A buffer that
//! simply filtered withdrawn puzzles on the way out would pass a test that asked "is it
//! handed over?" and still be broken, because the bytes would still be sitting on the
//! device for anyone who cleared its notes. So several of these tests look at the shelf
//! directly rather than at what the buffer returns.

mod common;

use common::{a_package, a_withdrawal, a_withdrawal_from_a_stranger, id_of, this_device_trusts_us};
use gs_core::withdrawal::Reason;
use gs_crypto::{Sha256, TrustedKeys};
use gs_offline::buffer::{name_for, NotKept};
use gs_offline::notices::{NotNoticed, WHERE};
use gs_offline::shelf::{AlwaysFails, Memory, Shelf};
use gs_offline::{Buffer, NothingToPlay, Notices, Standing};

/// A device with nothing on it yet.
fn a_blank_device() -> (Buffer, Notices, Memory, TrustedKeys) {
    (
        Buffer::new(),
        Notices::new(),
        Memory::new(),
        this_device_trusts_us(),
    )
}

// ---------------------------------------------------------------------------
// Delete, do not hide
// ---------------------------------------------------------------------------

#[test]
fn a_withdrawn_puzzle_is_deleted_from_the_device_and_not_merely_hidden() {
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = a_package(1);
    let id = buffer
        .keep(&mut shelf, &bad, &Sha256, &keys, &Standing::nothing())
        .expect("kept while nothing was withdrawn");
    assert!(
        shelf.get(&name_for(&id)).expect("reads").is_some(),
        "it must be there before the withdrawal, or this test proves nothing"
    );

    let list = a_withdrawal(1, &[(id, Reason::NoAnswer)]);
    let noticed = notices
        .accept(&mut shelf, &list, &Sha256, &keys)
        .expect("a list signed by the trusted key is accepted");

    assert_eq!(noticed.serial, 1);
    assert_eq!(noticed.named, 1);
    assert_eq!(noticed.dropped, 1, "the device was holding it");
    assert_eq!(
        shelf.get(&name_for(&id)).expect("reads"),
        None,
        "the bytes themselves must be gone, not filtered"
    );
}

#[test]
fn clearing_the_devices_notes_cannot_bring_a_withdrawn_puzzle_back() {
    // The reason the rule is "delete" and not "hide". Somebody with the developer tools
    // open can wipe the stored list; they cannot un-delete the package.
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let id = id_of(&a_package(1));
    buffer
        .keep(
            &mut shelf,
            &a_package(1),
            &Sha256,
            &keys,
            &Standing::nothing(),
        )
        .expect("kept");
    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(id, Reason::Unsuitable)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");

    notices.forget(&mut shelf).expect("the notes are wiped");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(standing.serial, 0, "the device has indeed forgotten");
    assert_eq!(
        buffer.next(&shelf, &Sha256, &keys, &standing).err(),
        Some(NothingToPlay::Empty),
        "and there is still nothing to play, because the bytes are gone"
    );
}

#[test]
fn a_withdrawal_naming_a_puzzle_this_device_never_had_is_still_remembered() {
    // So that a later download of it is refused. A list is about puzzles, not about
    // what happens to be on this shelf today.
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = a_package(9);
    let id = id_of(&bad);

    let noticed = notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(id, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");
    assert_eq!(noticed.named, 1);
    assert_eq!(noticed.dropped, 0, "it was not here to drop");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(
        buffer.keep(&mut shelf, &bad, &Sha256, &keys, &standing),
        Err(NotKept::Withdrawn(Reason::NoAnswer)),
        "downloading it afterwards must be refused"
    );
    assert!(
        shelf.get(&name_for(&id)).expect("reads").is_none(),
        "and nothing was stored on the way to refusing"
    );
}

#[test]
fn a_puzzle_that_is_not_named_is_left_completely_alone() {
    // The anti-vacuity half of all of the above. A `notices` that deleted everything
    // would pass every test so far.
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = id_of(&a_package(1));
    let fine = a_package(2);
    buffer
        .keep(
            &mut shelf,
            &a_package(1),
            &Sha256,
            &keys,
            &Standing::nothing(),
        )
        .expect("kept");
    let fine_id = buffer
        .keep(&mut shelf, &fine, &Sha256, &keys, &Standing::nothing())
        .expect("kept");

    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(bad, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(standing.against(&fine_id), None);
    let delivered = buffer
        .next(&shelf, &Sha256, &keys, &standing)
        .expect("the other puzzle is still playable");
    assert_eq!(delivered.content_id, fine_id);
    assert_eq!(delivered.package, fine);
}

// ---------------------------------------------------------------------------
// Newer only
// ---------------------------------------------------------------------------

#[test]
fn an_older_list_served_again_cannot_un_withdraw_a_puzzle() {
    // The attack the serial number exists for, and it needs no key: keep handing the
    // device the genuine list from before the bad puzzle was named.
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = a_package(1);
    let id = id_of(&bad);

    let before = a_withdrawal(4, &[]);
    let after = a_withdrawal(5, &[(id, Reason::MoreThanOneAnswer)]);

    notices
        .accept(&mut shelf, &before, &Sha256, &keys)
        .expect("accepted");
    notices
        .accept(&mut shelf, &after, &Sha256, &keys)
        .expect("accepted");

    // Now replay the earlier one. It is genuine and correctly signed.
    assert_eq!(
        notices.accept(&mut shelf, &before, &Sha256, &keys),
        Err(NotNoticed::NotNewer {
            held: 5,
            offered: 4
        })
    );

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(standing.serial, 5);
    assert_eq!(standing.against(&id), Some(Reason::MoreThanOneAnswer));
    assert_eq!(
        buffer.keep(&mut shelf, &bad, &Sha256, &keys, &standing),
        Err(NotKept::Withdrawn(Reason::MoreThanOneAnswer))
    );
}

#[test]
fn the_same_list_twice_is_refused_the_second_time() {
    // Equal is not newer. Accepting an equal serial would let a list be swapped for a
    // different one carrying the same number.
    let (_, notices, mut shelf, keys) = a_blank_device();
    let list = a_withdrawal(3, &[]);
    notices
        .accept(&mut shelf, &list, &Sha256, &keys)
        .expect("accepted");
    assert_eq!(
        notices.accept(&mut shelf, &list, &Sha256, &keys),
        Err(NotNoticed::NotNewer {
            held: 3,
            offered: 3
        })
    );
}

#[test]
fn a_newer_list_replaces_the_older_one_outright() {
    // A list is the whole truth, not an addition to it, so a puzzle dropped from a
    // newer list is no longer withdrawn. That is how a mistaken withdrawal is undone.
    let (_, notices, mut shelf, keys) = a_blank_device();
    let first = id_of(&a_package(1));
    let second = id_of(&a_package(2));

    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(first, Reason::Mistake)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");
    notices
        .accept(
            &mut shelf,
            &a_withdrawal(2, &[(second, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(standing.len(), 1);
    assert_eq!(standing.against(&second), Some(Reason::NoAnswer));
    assert_eq!(
        standing.against(&first),
        None,
        "a withdrawal published by mistake must be undoable"
    );
}

#[test]
fn a_list_numbered_zero_is_never_accepted() {
    // Zero is what "this device has never seen a list" is written as, and the rule is a
    // strict increase, so a published zero can never get in. Worth a test because the
    // two meanings share a value.
    let (_, notices, mut shelf, keys) = a_blank_device();
    assert_eq!(
        notices.accept(&mut shelf, &a_withdrawal(0, &[]), &Sha256, &keys),
        Err(NotNoticed::NotNewer {
            held: 0,
            offered: 0
        })
    );
}

// ---------------------------------------------------------------------------
// Checked, every time
// ---------------------------------------------------------------------------

#[test]
fn a_list_from_a_key_this_device_does_not_trust_is_ignored() {
    let (_, notices, mut shelf, keys) = a_blank_device();
    let id = id_of(&a_package(1));
    let theirs = a_withdrawal_from_a_stranger(1, &[(id, Reason::NoAnswer)]);

    assert!(matches!(
        notices.accept(&mut shelf, &theirs, &Sha256, &keys),
        Err(NotNoticed::NotAcceptable(_))
    ));
    assert_eq!(
        shelf.get(WHERE).expect("reads"),
        None,
        "nothing from a stranger is stored, even to be ignored later"
    );
}

#[test]
fn a_changed_byte_in_a_list_makes_it_ignored() {
    let (_, notices, mut shelf, keys) = a_blank_device();
    let honest = a_withdrawal(1, &[(id_of(&a_package(1)), Reason::NoAnswer)]);
    for at in 0..honest.len() {
        let mut changed = honest.clone();
        changed[at] ^= 0x01;
        assert!(
            matches!(
                notices.accept(&mut shelf, &changed, &Sha256, &keys),
                Err(NotNoticed::NotAcceptable(_))
            ),
            "byte {at} was changed and the list was still accepted"
        );
    }
    // And the honest one does get in, so the loop above is not passing because the
    // reader refuses everything.
    assert!(notices.accept(&mut shelf, &honest, &Sha256, &keys).is_ok());
}

#[test]
fn a_stored_list_that_has_been_tampered_with_is_reported_and_not_obeyed() {
    // Storage on a device is a few clicks from being edited by hand. A forged list that
    // was believed because of where it was found would let anybody withdraw anything.
    let (_, notices, mut shelf, keys) = a_blank_device();
    let id = id_of(&a_package(1));
    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(id, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");

    let mut stored = shelf.get(WHERE).expect("reads").expect("stored");
    let last = stored.len() - 1;
    stored[last] ^= 0x01;
    shelf.put(WHERE, &stored).expect("writes");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert!(standing.stored_list_was_unreadable);
    assert_eq!(standing.serial, 0);
    assert!(standing.is_empty());
}

#[test]
fn a_device_with_no_list_at_all_says_so_without_complaining() {
    let (_, notices, shelf, keys) = a_blank_device();
    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(standing, Standing::nothing());
    assert!(!standing.stored_list_was_unreadable);
}

#[test]
fn a_list_signed_by_a_key_added_to_the_device_later_is_accepted() {
    // The anti-vacuity half of the signature tests: these refusals must be about the
    // key, not about the file being a withdrawal list.
    let (_, notices, mut shelf, _) = a_blank_device();
    let theirs = a_withdrawal_from_a_stranger(1, &[]);
    let mut keys = TrustedKeys::none();
    let signer =
        gs_crypto::Ed25519Signer::new(common::SOMEBODY_ELSE, &common::SOMEBODY_ELSES_PRIVATE_KEY);
    keys.trust(common::SOMEBODY_ELSE, &signer.public_key())
        .expect("a valid key");
    assert!(notices.accept(&mut shelf, &theirs, &Sha256, &keys).is_ok());
}

// ---------------------------------------------------------------------------
// The buffer's own behaviour
// ---------------------------------------------------------------------------

#[test]
fn a_withdrawn_puzzle_that_comes_back_onto_the_shelf_is_still_not_handed_over() {
    // Storage restored from a backup, or edited by hand. `accept` deleted the bytes, so
    // in normal running this cannot happen; the check on the way out is what a player's
    // screen actually depends on.
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = a_package(1);
    let id = id_of(&bad);
    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(id, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");

    // Put it back behind the buffer's back, the way a restored backup would.
    shelf.put(&name_for(&id), &bad).expect("writes");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    assert_eq!(
        buffer.next(&shelf, &Sha256, &keys, &standing).err(),
        Some(NothingToPlay::Empty)
    );
}

#[test]
fn a_withdrawn_puzzle_still_on_the_shelf_counts_as_spoiled_and_is_asked_for_again() {
    let (buffer, notices, mut shelf, keys) = a_blank_device();
    let bad = a_package(1);
    let id = id_of(&bad);
    notices
        .accept(
            &mut shelf,
            &a_withdrawal(1, &[(id, Reason::NoAnswer)]),
            &Sha256,
            &keys,
        )
        .expect("accepted");
    shelf.put(&name_for(&id), &bad).expect("writes");

    let standing = notices.standing(&shelf, &Sha256, &keys).expect("reads");
    let stock = buffer
        .stock(&shelf, &Sha256, &keys, &standing)
        .expect("counted");
    assert_eq!(stock.sound, 0);
    assert_eq!(stock.spoiled, 1);
    assert_eq!(
        stock.wanted(),
        gs_offline::HOW_MANY_AHEAD,
        "the device should ask for a full set, not count the withdrawn one"
    );

    assert_eq!(
        buffer
            .discard_spoiled(&mut shelf, &Sha256, &keys, &standing)
            .expect("discarded"),
        1
    );
    assert_eq!(shelf.get(&name_for(&id)).expect("reads"), None);
}

#[test]
fn storage_that_will_not_answer_is_reported_rather_than_treated_as_nothing_withdrawn() {
    // The dangerous default. A broken shelf that read as "nothing is withdrawn" would
    // quietly turn the whole mechanism off.
    let notices = Notices::new();
    let keys = this_device_trusts_us();
    let broken = AlwaysFails("this device will not say".to_owned());
    assert!(notices.standing(&broken, &Sha256, &keys).is_err());

    let mut broken = AlwaysFails("this device will not say".to_owned());
    assert!(matches!(
        notices.accept(&mut broken, &a_withdrawal(1, &[]), &Sha256, &keys),
        Err(NotNoticed::ShelfBroke(_))
    ));
}

// ---------------------------------------------------------------------------
// What a person is told
// ---------------------------------------------------------------------------

#[test]
fn every_refusal_a_person_can_see_is_written_in_words() {
    let said = [
        NotNoticed::NotAcceptable(gs_core::package::OpenError::SignatureRejected).to_string(),
        NotNoticed::NotNewer {
            held: 9,
            offered: 2,
        }
        .to_string(),
        NotNoticed::ShelfBroke(gs_offline::ShelfError::new("it is full")).to_string(),
        NotKept::Withdrawn(Reason::NoAnswer).to_string(),
        NotKept::Withdrawn(Reason::MoreThanOneAnswer).to_string(),
        NotKept::Withdrawn(Reason::Unsuitable).to_string(),
        NotKept::Withdrawn(Reason::Mistake).to_string(),
    ];
    for words in said {
        assert!(!words.contains("::"), "{words:?} names a type");
        assert!(!words.contains('{'), "{words:?} is a debug print");
        let mut previous = ' ';
        for letter in words.chars() {
            assert!(
                !(previous.is_ascii_lowercase() && letter.is_ascii_uppercase()),
                "{words:?} has a capital inside a word, so it is a name and not a sentence"
            );
            previous = letter;
        }
        // A message that ends by quoting the device's own complaint cannot be made to
        // end in a full stop, because what the device said is not ours to punctuate.
        assert!(
            words.ends_with('.') || words.contains(": "),
            "{words:?} is not a sentence"
        );
    }
}
