//! One buffer, every game, on an ordinary machine.
//!
//! The browser tests cover the same ground, but only where there is a browser and a
//! matching driver. This file runs everywhere, and it exists because of a mistake:
//! the browser version of this test asked for two saved puzzles in a row and expected
//! two different ones. It got the same puzzle twice, because being handed a puzzle is
//! not having played it — the buffer keeps holding it until `played` is called, which
//! is the "delivered is not played" rule working rather than failing.
//!
//! So the rule is checked here, with every game, in a test that cannot be skipped for
//! want of a browser.
//!
//! What is being proved: `gs-offline` knows nothing about any game and does not need
//! to. The packages below are a real Nonogram, a real Slitherlink and a real Kakuro,
//! sealed and opened the way a device does it, and the buffer treats them identically.
//!
//! Adding the third game to this file changed the list of packages and the list of
//! names it expects, and nothing else. That was the claim the arrangement made when it
//! was built for two, so it is worth saying out loud that it held — including for the
//! game whose squares hold ten states rather than three, since nothing in a buffer, a
//! shelf, a withdrawal list or a package ever enumerated them.

use gs_core::content::ContentId;
use gs_core::verification::{Outcome, Verifier};
use gs_core::withdrawal::{seal_list, Reason, WithdrawalList, Withdrawn};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_offline::buffer::Buffer;
use gs_offline::notices::{Notices, Standing};
use gs_offline::shelf::Memory;

const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const KEY_ID: &str = "the-device-trusts-this-one";

/// Stands in for a real solver so this file runs anywhere. It proves nothing and is not
/// pretending to: the proving is tested against Z3 and CVC5 in each family's own crate.
struct AgreesWithAUniquePuzzle {
    asked: std::cell::RefCell<usize>,
}

impl AgreesWithAUniquePuzzle {
    fn new() -> Self {
        AgreesWithAUniquePuzzle {
            asked: std::cell::RefCell::new(0),
        }
    }
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

fn this_device_trusts_us() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &our_signer().public_key())
        .expect("a valid key into an empty list");
    keys
}

fn nothing_withdrawn() -> Standing {
    Standing::default()
}

fn a_nonogram() -> Vec<u8> {
    let candidate = (1u64..3_000)
        .find_map(|seed| {
            gs_nonogram::difficulty::propose_in_band(
                &gs_nonogram::difficulty::BANDS[0],
                seed,
                gs_nonogram::solve::Budget::default(),
            )
        })
        .expect("some seed in the first few thousand produces a candidate");
    gs_nonogram::publish::publish(
        &candidate,
        &AgreesWithAUniquePuzzle::new(),
        gs_nonogram::solve::Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes")
    .package
}

fn a_slitherlink() -> Vec<u8> {
    let candidate = gs_slitherlink::generate::propose(
        3,
        gs_slitherlink::generate::Shape {
            width: 5,
            height: 5,
            enclosed_per_mille: 400,
        },
        gs_slitherlink::generate::REMOVAL_BUDGET,
    )
    .expect("this seed makes a puzzle");
    gs_slitherlink::publish::publish(
        &candidate,
        &AgreesWithAUniquePuzzle::new(),
        gs_slitherlink::solve::Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes")
    .package
}

/// The first seed that makes a Kakuro of this size.
///
/// Not a fixed seed. Which seeds produce a puzzle is a fact about the generator, and it
/// changed when boards started being built rather than drawn at random (ADR 0021), which
/// broke three test files at once. Asking for the first seed that works keeps these tests
/// about what they are each testing.
fn a_kakuro_candidate(width: u16, height: u16) -> gs_kakuro::generate::Candidate {
    (0..40_u64)
        .find_map(|seed| {
            gs_kakuro::generate::propose(
                seed,
                gs_kakuro::generate::Shape::middling(width, height),
                gs_kakuro::solve::Budget::default(),
            )
            .ok()
        })
        .expect("some seed in forty makes a puzzle this size")
}

fn a_kakuro() -> Vec<u8> {
    let candidate = a_kakuro_candidate(5, 5);
    gs_kakuro::publish::publish(
        &candidate,
        &AgreesWithAUniquePuzzle::new(),
        gs_kakuro::solve::Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate publishes")
    .package
}

/// Which game a package in the buffer turns out to be, read the way the page reads it.
fn family_of(package: &[u8]) -> String {
    gs_web::load(package, &Sha256, &this_device_trusts_us())
        .expect("a package this build published is one this build accepts")
        .family()
        .to_owned()
}

#[test]
fn a_device_holds_every_game_at_once_and_hands_each_one_back() {
    // The thing being ruled out is a device that can hold five of one game or five of
    // another, but not some of each.
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let keys = this_device_trusts_us();

    buffer
        .keep(
            &mut shelf,
            &a_nonogram(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("a real package is kept");
    buffer
        .keep(
            &mut shelf,
            &a_slitherlink(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("a real package of the other game is kept too");
    buffer
        .keep(
            &mut shelf,
            &a_kakuro(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("and the third game, whose squares hold a digit rather than a mark");

    let mut found = Vec::new();
    let mut played: Vec<ContentId> = Vec::new();
    while let Ok(one) = buffer.next(&shelf, &Sha256, &keys, &nothing_withdrawn()) {
        found.push(family_of(&one.package));
        played.push(one.content_id);
        buffer
            .played(&mut shelf, &played[played.len() - 1])
            .expect("marking it played");
    }
    found.sort();
    assert_eq!(found, vec!["kakuro", "nonogram", "slitherlink"]);
    assert_eq!(played.len(), 3, "each one came out exactly once");
}

#[test]
fn asking_twice_without_playing_hands_back_the_same_puzzle() {
    // "Delivered is not played", stated directly. This is the rule the browser version
    // of the test above got wrong, so it is written down here as a claim rather than
    // left as an assumption.
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let keys = this_device_trusts_us();

    buffer
        .keep(
            &mut shelf,
            &a_nonogram(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("kept");
    buffer
        .keep(
            &mut shelf,
            &a_slitherlink(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("kept");

    let first = buffer
        .next(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("one to play");
    let again = buffer
        .next(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("still one to play");
    assert_eq!(
        first.content_id, again.content_id,
        "being handed a puzzle is not having played it, so it is still the one on offer"
    );

    buffer
        .played(&mut shelf, &first.content_id)
        .expect("now it has been played");
    let next = buffer
        .next(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("and the other one comes out");
    assert_ne!(next.content_id, first.content_id);
}

#[test]
fn the_buffer_never_learns_which_game_it_is_holding() {
    // Not a behaviour test — a shape test. `gs-offline` reports what it holds without
    // naming a game, because the moment it could name one it would be a buffer that has
    // to learn about every family after it.
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let keys = this_device_trusts_us();

    buffer
        .keep(
            &mut shelf,
            &a_nonogram(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("kept");
    buffer
        .keep(
            &mut shelf,
            &a_slitherlink(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("kept");
    buffer
        .keep(
            &mut shelf,
            &a_kakuro(),
            &Sha256,
            &keys,
            &nothing_withdrawn(),
        )
        .expect("kept");

    let stock = buffer
        .stock(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("reads the shelf");
    assert_eq!(
        stock.sound, 3,
        "it counts what it holds, without sorting by game"
    );
    assert_eq!(stock.spoiled, 0);

    // And what it hands over says nothing about which game it is either. The only way
    // to learn that is to open the package, which is the page's job and not the
    // buffer's — `Delivered` carries the fingerprint, the key and the level, and a
    // level is a thing a game may or may not have rather than a name for one.
    let delivered = buffer
        .next(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("one to play");
    let family = family_of(&delivered.package);
    assert!(
        family == "nonogram" || family == "slitherlink",
        "the page can tell which game it is: {family}"
    );
    if family == "slitherlink" {
        assert_eq!(
            delivered.band, None,
            "and a game with no levels is handed over with none, not with an invented one"
        );
    }
}

#[test]
fn a_withdrawn_kakuro_is_refused_by_the_buffer_like_any_other_puzzle() {
    // The "do not play these" list, reached through the third game. The list names
    // fingerprints, so nothing in it knows about families — and that is the claim: the
    // game whose squares hold ten states is withdrawn by exactly the same machinery as
    // the games whose squares hold three.
    //
    // Reached through a real signed list rather than by building a `Standing` by hand,
    // because the point is the whole path a device takes.
    let buffer = Buffer::new();
    let mut shelf = Memory::new();
    let keys = this_device_trusts_us();
    let package = a_kakuro();

    // Kept first, while nothing is withdrawn.
    buffer
        .keep(&mut shelf, &package, &Sha256, &keys, &nothing_withdrawn())
        .expect("a sound package is kept");
    let held = buffer
        .next(&shelf, &Sha256, &keys, &nothing_withdrawn())
        .expect("one to play");

    // Now publish a list naming it, and let the device accept the list.
    let list = WithdrawalList {
        serial: 1,
        withdrawn: vec![Withdrawn {
            content_id: *held.content_id.as_bytes(),
            reason: Reason::Unsuitable,
        }],
    };
    let sealed = seal_list(&list, &Sha256, &our_signer()).expect("the list seals");
    let notices = Notices::new();
    notices
        .accept(&mut shelf, &sealed, &Sha256, &keys)
        .expect("a signed list this device trusts is accepted");
    let standing = notices
        .standing(&shelf, &Sha256, &keys)
        .expect("reads the stored list");

    assert_eq!(standing.len(), 1);
    assert_eq!(
        standing.against(&held.content_id),
        Some(Reason::Unsuitable),
        "the list has to name the puzzle by its fingerprint"
    );

    // And the buffer will not hand it out any more. A device that had already
    // downloaded it stops playing it, which is the whole reason the list exists.
    assert!(
        buffer.next(&shelf, &Sha256, &keys, &standing).is_err(),
        "a withdrawn puzzle must not be handed to a player"
    );
    // Nor accept it again, so a second download of the same puzzle does not get round
    // the list.
    assert!(
        buffer
            .keep(&mut shelf, &package, &Sha256, &keys, &standing)
            .is_err(),
        "a withdrawn puzzle must not be saved either"
    );
}
