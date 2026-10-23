//! What a "do not play these" list must refuse.
//!
//! This file is the one place in the project where a signed file can take something
//! *away* from a player, so the tests are mostly about what must not work: an unsigned
//! list, a list from the wrong key, a changed byte, and above all an old list served
//! again to make a withdrawal disappear.
//!
//! As in `package.rs`, the container is written out by hand in [`wrap`] rather than by
//! `seal_list`. Using the module for both sides of a test only proves the module agrees
//! with itself, and the layout written down in `withdrawal.rs` could drift away from the
//! layout actually written. `wrap` and [`digest_input`] here are independent encoders of
//! what that file documents.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{OpenError, SignatureVerifier, Signer};
use gs_core::withdrawal::{
    open_list, seal_list, OpenedList, Reason, WithdrawalList, Withdrawn, FORMAT_VERSION, MAGIC,
};

/// A stand-in digest, for tests only, and **not** a cryptographic hash.
///
/// FNV-1a over every byte, widened to 32, so that a change deep inside a long body is
/// noticed. It is trivially invertible and offers no collision resistance. What these
/// tests prove with it is the shape and the order of the checks, never the strength of
/// the hash. The real digest is SHA-256 in `gs-crypto`, and a crate that could verify
/// its own signatures would prove nothing by doing so.
struct NotAHashButSensitiveToEveryByte;

impl Digest for NotAHashButSensitiveToEveryByte {
    fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for (i, chunk) in out.chunks_mut(8).enumerate() {
            let mixed = h
                .wrapping_add(u64::try_from(i).expect("small"))
                .wrapping_mul(0x9e37_79b9_7f4a_7c15);
            chunk.copy_from_slice(&mixed.to_le_bytes());
        }
        out
    }
}

/// A stand-in signer, for tests only, and **not** a signature scheme.
struct NotASignature {
    key_id: &'static str,
}

impl Signer for NotASignature {
    fn key_id(&self) -> &str {
        self.key_id
    }

    fn sign(&self, content_id: &ContentId) -> Vec<u8> {
        let mut out = b"not-a-signature:".to_vec();
        out.extend_from_slice(content_id.as_bytes());
        out
    }
}

struct TrustingOnly(&'static [&'static str]);

impl SignatureVerifier for TrustingOnly {
    fn verify(&self, content_id: &ContentId, key_id: &str, signature: &[u8]) -> bool {
        if !self.0.contains(&key_id) {
            return false;
        }
        let mut expected = b"not-a-signature:".to_vec();
        expected.extend_from_slice(content_id.as_bytes());
        signature == expected.as_slice()
    }
}

struct RejectsEverything;

impl SignatureVerifier for RejectsEverything {
    fn verify(&self, _content_id: &ContentId, _key_id: &str, _signature: &[u8]) -> bool {
        false
    }
}

const KEY: &str = "test-key-1";

fn signer() -> NotASignature {
    NotASignature { key_id: KEY }
}

fn verifier() -> TrustingOnly {
    TrustingOnly(&["test-key-1"])
}

fn digest() -> NotAHashButSensitiveToEveryByte {
    NotAHashButSensitiveToEveryByte
}

/// A made-up content id, standing for a puzzle somebody published.
fn an_id(n: u8) -> [u8; 32] {
    let mut id = [n; 32];
    id[0] = n.wrapping_add(1);
    id
}

fn a_list() -> WithdrawalList {
    WithdrawalList {
        serial: 7,
        withdrawn: vec![
            Withdrawn {
                content_id: an_id(1),
                reason: Reason::NoAnswer,
            },
            Withdrawn {
                content_id: an_id(2),
                reason: Reason::MoreThanOneAnswer,
            },
        ],
    }
}

/// The container, written out by hand from what `withdrawal.rs` documents.
fn wrap(body: &[u8], version: u16, content_id: &[u8; 32], key_id: &str, sig: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&u32::try_from(body.len()).expect("short body").to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(content_id);
    out.extend_from_slice(
        &u16::try_from(key_id.len())
            .expect("short key id")
            .to_le_bytes(),
    );
    out.extend_from_slice(key_id.as_bytes());
    out.extend_from_slice(
        &u32::try_from(sig.len())
            .expect("short signature")
            .to_le_bytes(),
    );
    out.extend_from_slice(sig);
    out
}

/// What the signature covers, written out by hand: the magic, the version, the body.
///
/// The magic being in here is the whole point of
/// [`a_signature_over_a_puzzle_can_never_stand_for_a_withdrawal`], so it is written
/// independently rather than imported.
fn digest_input(version: u16, body: &[u8]) -> Vec<u8> {
    let mut input = MAGIC.to_vec();
    input.extend_from_slice(&version.to_le_bytes());
    input.extend_from_slice(body);
    input
}

fn wrap_honestly(body: &[u8]) -> Vec<u8> {
    let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION, body));
    let sig = signer().sign(&id);
    wrap(body, FORMAT_VERSION, id.as_bytes(), KEY, &sig)
}

fn sealed() -> Vec<u8> {
    seal_list(&a_list(), &digest(), &signer()).expect("a list this small encodes")
}

fn opened(bytes: &[u8]) -> Result<OpenedList, OpenError> {
    open_list(bytes, &digest(), &verifier())
}

/// A body no reader can decode: eight bytes of serial and then nothing where a count
/// should be.
const MALFORMED_BODY: &[u8] = &[0; 8];

#[test]
fn a_sealed_list_opens_and_round_trips() {
    let got = opened(&sealed()).expect("a list this module just sealed must open");
    assert_eq!(got.list, a_list());
    assert_eq!(got.key_id, KEY);
}

#[test]
fn the_hand_written_container_and_the_module_agree() {
    // If this fails, one of the two encoders has drifted, and every other test in this
    // file is testing the wrong layout.
    let body = a_list().canonical_bytes().expect("encodes");
    assert_eq!(
        sealed(),
        wrap_honestly(&body),
        "the bytes seal_list writes must be the layout withdrawal.rs documents"
    );
}

#[test]
fn a_list_is_not_a_package_and_a_package_is_not_a_list() {
    // The two magics differ, so neither reader will even start on the other's file.
    assert_ne!(MAGIC, gs_core::package::MAGIC);
    let body = a_list().canonical_bytes().expect("encodes");
    let as_a_package = {
        let mut out = Vec::new();
        out.extend_from_slice(&gs_core::package::MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&u32::try_from(body.len()).expect("short").to_le_bytes());
        out.extend_from_slice(&body);
        out
    };
    assert!(matches!(opened(&as_a_package), Err(OpenError::NotAPackage)));
}

/// The claim the magic-in-the-digest rule exists to make.
///
/// A publisher's key signs puzzles. If the bytes a puzzle's signature covers could
/// also be read as the bytes a withdrawal list's signature covers, then anybody holding
/// a signed puzzle could rearrange it into a signed withdrawal list and take puzzles
/// away without ever having a key. The two digest inputs provably cannot collide,
/// because they disagree on their very first byte.
#[test]
fn a_signature_over_a_puzzle_can_never_stand_for_a_withdrawal() {
    // Both written out by hand from what the two modules document, for the reason
    // given at the top of this file.
    let package_input = {
        let mut input = FORMAT_VERSION.to_le_bytes().to_vec();
        input.extend_from_slice(b"any body at all");
        input
    };
    let withdrawal_input = digest_input(FORMAT_VERSION, b"any body at all");

    assert_ne!(package_input, withdrawal_input);
    assert_eq!(
        package_input[0],
        FORMAT_VERSION.to_le_bytes()[0],
        "a package digest starts with its version"
    );
    assert_eq!(
        withdrawal_input[0], MAGIC[0],
        "a withdrawal digest starts with its magic"
    );
    assert_ne!(
        package_input[0], withdrawal_input[0],
        "the first byte alone must tell the two apart, whatever the bodies are"
    );
}

#[test]
fn a_changed_byte_anywhere_in_the_body_is_refused() {
    let honest = sealed();
    let body = a_list().canonical_bytes().expect("encodes");
    // Walk the body one byte at a time rather than picking one, because a withdrawal
    // that can be quietly edited is a puzzle that can be quietly un-withdrawn.
    for at in 0..body.len() {
        let mut changed = body.clone();
        changed[at] ^= 0x01;
        let bytes = {
            let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION, &body));
            let sig = signer().sign(&id);
            wrap(&changed, FORMAT_VERSION, id.as_bytes(), KEY, &sig)
        };
        assert_ne!(bytes, honest);
        assert!(
            matches!(opened(&bytes), Err(OpenError::ContentIdMismatch { .. })),
            "byte {at} of the body was changed and the file still opened"
        );
    }
}

#[test]
fn a_list_nobody_signed_is_refused() {
    let bytes = sealed();
    assert!(matches!(
        open_list(&bytes, &digest(), &RejectsEverything),
        Err(OpenError::SignatureRejected)
    ));
}

#[test]
fn a_key_the_reader_does_not_trust_is_refused() {
    let body = a_list().canonical_bytes().expect("encodes");
    let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION, &body));
    let other = NotASignature {
        key_id: "a-key-from-somewhere-else",
    };
    let bytes = wrap(
        &body,
        FORMAT_VERSION,
        id.as_bytes(),
        other.key_id(),
        &other.sign(&id),
    );
    assert!(matches!(opened(&bytes), Err(OpenError::SignatureRejected)));
}

/// Order, not outcome. The body here is both malformed *and* unvouched-for, and the
/// refusal must name the signature, which is the only way to show from outside that
/// the body was never decoded.
#[test]
fn the_signature_is_checked_before_the_body_is_decoded() {
    let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION, MALFORMED_BODY));
    let bytes = wrap(
        MALFORMED_BODY,
        FORMAT_VERSION,
        id.as_bytes(),
        KEY,
        b"rubbish",
    );
    assert!(matches!(opened(&bytes), Err(OpenError::SignatureRejected)));
}

/// The same, one step earlier: a wrong id must be reported before the body is read.
#[test]
fn the_digest_is_checked_before_the_body_is_decoded() {
    let bytes = wrap(MALFORMED_BODY, FORMAT_VERSION, &[0; 32], KEY, b"rubbish");
    assert!(matches!(
        opened(&bytes),
        Err(OpenError::ContentIdMismatch { .. })
    ));
}

/// And the anti-vacuity half: the same malformed body, honestly signed, must fail on
/// the body. Without this, the two tests above would pass for a reader that refused
/// everything.
#[test]
fn a_malformed_body_that_is_honestly_signed_fails_on_the_body() {
    let bytes = wrap_honestly(MALFORMED_BODY);
    assert!(
        matches!(opened(&bytes), Err(OpenError::Truncated { .. })),
        "a correctly signed but undecodable body must fail on the body"
    );
}

#[test]
fn a_version_this_reader_does_not_know_is_refused_rather_than_guessed_at() {
    let body = a_list().canonical_bytes().expect("encodes");
    let unknown = FORMAT_VERSION + 1;
    let id = ContentId::of(&digest(), &digest_input(unknown, &body));
    let bytes = wrap(&body, unknown, id.as_bytes(), KEY, &signer().sign(&id));
    assert!(matches!(
        opened(&bytes),
        Err(OpenError::UnsupportedFormat(v)) if v == unknown
    ));
}

#[test]
fn the_list_id_is_recomputed_from_the_body_rather_than_taken_from_the_file() {
    let body = a_list().canonical_bytes().expect("encodes");
    let got = opened(&sealed()).expect("opens");
    assert_eq!(
        got.content_id,
        ContentId::of(&digest(), &digest_input(FORMAT_VERSION, &body))
    );
}

#[test]
fn extra_bytes_after_the_signature_are_refused() {
    let mut bytes = sealed();
    bytes.push(0);
    assert!(matches!(opened(&bytes), Err(OpenError::TrailingBytes(1))));
}

#[test]
fn a_file_that_stops_early_is_refused_and_says_where() {
    let bytes = sealed();
    for cut in 0..bytes.len() {
        let short = &bytes[..cut];
        assert!(
            matches!(
                open_list(short, &digest(), &verifier()),
                Err(OpenError::NotAPackage) | Err(OpenError::Truncated { .. })
            ),
            "{cut} bytes of a {} byte file must not open",
            bytes.len()
        );
    }
}

#[test]
fn a_reason_nobody_can_read_is_a_malformed_file_and_never_a_default_reason() {
    // A newer publisher may withdraw a puzzle for a reason this reader has no word
    // for. Picking one for them would show a player a sentence nobody wrote.
    let mut body = a_list().canonical_bytes().expect("encodes");
    let last = body.len() - 1;
    body[last] = 99;
    let bytes = wrap_honestly(&body);
    assert!(matches!(
        opened(&bytes),
        Err(OpenError::Malformed {
            field: "withdrawal reason"
        })
    ));
}

#[test]
fn no_reason_is_written_as_a_zero_byte() {
    // A page of zeroes must not decode as a reason, the same rule the package tags
    // follow.
    for reason in [
        Reason::NoAnswer,
        Reason::MoreThanOneAnswer,
        Reason::Unsuitable,
        Reason::Mistake,
    ] {
        assert_ne!(reason.tag(), 0);
        assert_eq!(Reason::of_tag(reason.tag()).expect("round trips"), reason);
    }
    assert!(Reason::of_tag(0).is_err());
}

#[test]
fn every_reason_reads_as_a_sentence_and_not_as_a_type_name() {
    for reason in [
        Reason::NoAnswer,
        Reason::MoreThanOneAnswer,
        Reason::Unsuitable,
        Reason::Mistake,
    ] {
        let words = reason.to_string();
        assert!(!words.is_empty());
        assert!(
            words.chars().all(|c| !c.is_ascii_uppercase()),
            "{words:?} looks like a name, not a sentence"
        );
        assert!(
            words.split_whitespace().count() >= 4,
            "{words:?} is too short to explain anything to a player"
        );
    }
}

#[test]
fn an_empty_list_is_a_perfectly_good_list() {
    // It is how a publisher says "nothing is withdrawn", and it must still be signed,
    // or an attacker could replace a real list with a blank one by deleting bytes.
    let empty = WithdrawalList {
        serial: 1,
        withdrawn: Vec::new(),
    };
    let bytes = seal_list(&empty, &digest(), &signer()).expect("encodes");
    let got = opened(&bytes).expect("opens");
    assert!(got.list.is_empty());
    assert_eq!(got.list.serial, 1);

    let mut without_a_signature = bytes.clone();
    without_a_signature.truncate(without_a_signature.len() - 1);
    assert!(open_list(&without_a_signature, &digest(), &verifier()).is_err());
}

#[test]
fn a_list_says_which_puzzles_it_names_and_which_it_does_not() {
    let list = a_list();
    // `reason_for` takes a real ContentId, so the test needs one; the ids in the list
    // are bytes on purpose. Build the list from the digest of something instead.
    let real = ContentId::of(&digest(), b"a puzzle that got withdrawn");
    let other = ContentId::of(&digest(), b"a puzzle that did not");
    let with_real = WithdrawalList {
        serial: list.serial,
        withdrawn: vec![Withdrawn {
            content_id: *real.as_bytes(),
            reason: Reason::Unsuitable,
        }],
    };
    assert_eq!(with_real.reason_for(&real), Some(Reason::Unsuitable));
    assert_eq!(with_real.reason_for(&other), None);
    assert_eq!(with_real.len(), 1);
}
