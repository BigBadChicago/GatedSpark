//! What a package file must refuse.
//!
//! The package is the one artifact that leaves the building, so the tests here are
//! mostly about rejection: a changed byte, a missing byte, an extra byte, a tag
//! nobody can read, a signature from a key nobody trusts. A format that accepts any
//! of those is a format that cannot be the basis of "bound by id and hash".
//!
//! Two of these tests are about *order* rather than outcome — they check that a bad
//! digest or a rejected signature is reported even when the body is also malformed,
//! which is the only way to show from outside that the body was never decoded.
//!
//! The container is written out by hand in [`wrap`] rather than produced by
//! `package::seal`. That is deliberate: if the test used `seal` for both sides it
//! would only prove the module agrees with itself, and the layout documented in
//! `package.rs` could drift from the layout actually written. `wrap` is an
//! independent encoder of that documented layout.

use gs_core::content::{ContentId, Digest};
use gs_core::hash::StateHash;
use gs_core::package::{
    open, seal, EncodeError, Manifest, OpenError, Opened, RecordedMeasurement, RecordedQuery,
    RecordedReplay, SignatureVerifier, Signer, FORMAT_VERSION, MAGIC,
};
use gs_core::verification::Outcome;

/// A stand-in digest, for tests only, and **not** a cryptographic hash.
///
/// It is FNV-1a over every byte, widened to 32, rather than the first-bytes copy used
/// in `validation_report.rs`: these tests need a digest that notices a change deep
/// inside a long body, which that one would not. It is still trivially invertible and
/// offers no collision resistance, so using it to publish anything would be a
/// security hole. What the tests below prove with it is the *wiring* — that the
/// digest is recomputed from the body and compared — not that the digest is strong.
/// Strength arrives with the reviewed implementation ADR 0004 is waiting on.
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
///
/// It returns the content id's bytes with a fixed marker. Anyone can produce the same
/// "signature", which is the point: it exercises the binding, and it could never be
/// mistaken for the real thing in a review.
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

/// Accepts the stand-in signature, and only from the keys it was told to trust.
///
/// The trusted-key list is what makes this a verifier at all. A verifier that
/// accepted whatever key the file named would verify nothing, which is the warning on
/// `SignatureVerifier`, and `a_key_the_verifier_does_not_trust_is_refused` is what
/// holds this one to it.
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

fn manifest() -> Manifest {
    Manifest {
        family: "nonogram".to_owned(),
        ruleset_version: 1,
        generator_version: 1,
        seed: 1_058,
        encoding: "qf_uf one bool per cell, exact line disjunctions".to_owned(),
        band: Some("tricky".to_owned()),
        payload: vec![12, 0, 12, 0, 3, 1, 4, 2, 2, 7],
        queries: vec![
            RecordedQuery {
                name: "solvability".to_owned(),
                required: Outcome::Sat,
                returned: Outcome::Sat,
            },
            RecordedQuery {
                name: "uniqueness_blocking_the_witness".to_owned(),
                required: Outcome::Unsat,
                returned: Outcome::Unsat,
            },
        ],
        measurements: vec![
            RecordedMeasurement {
                name: "propagation_rounds".to_owned(),
                value: 6,
            },
            RecordedMeasurement {
                name: "decision_points".to_owned(),
                value: 0,
            },
        ],
        replay: RecordedReplay::Reproduced(StateHash(0x0123_4567_89ab_cdef)),
    }
}

fn sealed() -> Vec<u8> {
    seal(&manifest(), &digest(), &signer()).expect("manifest is well within the length limits")
}

fn opened(bytes: &[u8]) -> Result<Opened, OpenError> {
    open(bytes, &digest(), &verifier())
}

/// Build the container by hand from the layout `package.rs` documents.
///
/// magic | format version (u16) | body length (u32) | body | content id (32)
/// | key id length (u16) | key id | signature length (u32) | signature
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

/// Wrap a body honestly: the stated content id really is its digest, and the
/// signature really is over that id. Used by the tests that need a well-formed
/// container around a deliberately broken *body*.
fn wrap_honestly(body: &[u8]) -> Vec<u8> {
    let id = ContentId::of(&digest(), body);
    let sig = signer().sign(&id);
    wrap(body, FORMAT_VERSION, id.as_bytes(), KEY, &sig)
}

/// A body that no reader can decode: a single zero byte, which is a zero-length
/// family name followed by nothing where a ruleset version should be.
const MALFORMED_BODY: &[u8] = &[0];

#[test]
fn a_sealed_package_opens_and_round_trips() {
    let got = opened(&sealed()).expect("a package this module just sealed must open");
    assert_eq!(got.manifest, manifest());
    assert_eq!(got.key_id, KEY);
}

#[test]
fn the_content_id_is_recomputed_from_the_body_rather_than_taken_from_the_file() {
    let bytes = sealed();
    let got = opened(&bytes).expect("opens");
    let body = manifest().canonical_bytes().expect("encodes");
    assert_eq!(
        got.content_id,
        ContentId::of(&digest(), &body),
        "the id handed back must be the digest of the body, not whatever the file said"
    );
}

#[test]
fn the_encoding_has_no_slack() {
    // Decoding and re-encoding must give byte-identical output. If it did not, two
    // different byte strings would describe the same manifest, and a fingerprint over
    // them would depend on which one you happened to have.
    let body = manifest().canonical_bytes().expect("encodes");
    let got = opened(&sealed()).expect("opens");
    let again = got.manifest.canonical_bytes().expect("re-encodes");
    assert_eq!(again, body);
}

#[test]
fn lengths_come_before_contents_so_a_split_cannot_be_moved() {
    // The classic concatenation bug: without a length in front of each field,
    // ("a", "bc") and ("ab", "c") encode to the same bytes and therefore share a
    // fingerprint. These two manifests differ only in where that split falls.
    let mut left = manifest();
    left.family = "a".to_owned();
    left.encoding = "bc".to_owned();

    let mut right = manifest();
    right.family = "ab".to_owned();
    right.encoding = "c".to_owned();

    assert_ne!(
        left.canonical_bytes().expect("encodes"),
        right.canonical_bytes().expect("encodes")
    );
}

#[test]
fn an_absent_band_is_not_an_empty_band() {
    let mut absent = manifest();
    absent.band = None;
    let mut empty = manifest();
    empty.band = Some(String::new());

    assert_ne!(
        absent.canonical_bytes().expect("encodes"),
        empty.canonical_bytes().expect("encodes")
    );

    for m in [absent, empty] {
        let bytes = seal(&m, &digest(), &signer()).expect("seals");
        assert_eq!(opened(&bytes).expect("opens").manifest, m);
    }
}

#[test]
fn unknown_survives_as_unknown() {
    // The architecture plan's rule, carried all the way into the stored file: a
    // solver that did not decide is written down as not having decided.
    let mut m = manifest();
    m.queries[0].returned = Outcome::Unknown;
    let bytes = seal(&m, &digest(), &signer()).expect("seals");
    let got = opened(&bytes).expect("opens");
    assert_eq!(got.manifest.queries[0].returned, Outcome::Unknown);
    assert!(got.manifest.has_undecided_query());
}

#[test]
fn a_tag_nobody_can_read_is_a_broken_file_and_not_an_unknown_outcome() {
    // Conflating these would put a solver result into an audit record that no solver
    // ever returned. The outcome byte of the first query is the last-but-one byte of
    // its entry; find it by rebuilding the body with a tag no version has used.
    let mut body = manifest().canonical_bytes().expect("encodes");
    let needle = position_of_first_query_tag(&body);
    body[needle] = 9;

    match opened(&wrap_honestly(&body)) {
        Err(OpenError::Malformed { field }) => assert_eq!(field, "outcome tag"),
        other => panic!("an unreadable tag must be a malformed file, got {other:?}"),
    }
}

#[test]
fn a_zero_byte_is_not_a_valid_outcome_or_replay() {
    // A zero-filled page, a short read padded out, a buffer never written: none of
    // these may decode as a valid record, and `Sat` would be the worst possible
    // reading of a byte that means nothing.
    let mut body = manifest().canonical_bytes().expect("encodes");
    let needle = position_of_first_query_tag(&body);
    body[needle] = 0;
    assert!(
        matches!(
            opened(&wrap_honestly(&body)),
            Err(OpenError::Malformed { .. })
        ),
        "a zero outcome tag must be refused"
    );

    let mut body = manifest().canonical_bytes().expect("encodes");
    let last = body.len() - 9; // the replay tag, before its eight-byte state hash
    body[last] = 0;
    assert!(
        matches!(
            opened(&wrap_honestly(&body)),
            Err(OpenError::Malformed { .. })
        ),
        "a zero replay tag must be refused"
    );
}

/// Where the first query's `required` outcome byte sits in an encoded body.
///
/// Worked out from the layout rather than searched for, so that a change to the
/// layout breaks this helper loudly instead of quietly moving the tests' aim.
fn position_of_first_query_tag(body: &[u8]) -> usize {
    let m = manifest();
    let mut at = 2 + m.family.len(); // family
    at += 4 + 4 + 8; // ruleset version, generator version, seed
    at += 2 + m.encoding.len(); // encoding
    at += 1 + 2 + m.band.as_ref().expect("fixture has a band").len(); // band
    at += 4 + m.payload.len(); // payload
    at += 2; // query count
    at += 2 + m.queries[0].name.len(); // first query's name
    assert!(at < body.len(), "the layout moved; fix this helper");
    at
}

#[test]
fn a_flipped_bit_anywhere_in_the_body_is_refused() {
    let bytes = sealed();
    let body_start = MAGIC.len() + 2 + 4;
    let body_len = manifest().canonical_bytes().expect("encodes").len();
    let mut refused = 0;
    for i in body_start..body_start + body_len {
        let mut broken = bytes.clone();
        broken[i] ^= 0x01;
        match opened(&broken) {
            Err(OpenError::ContentIdMismatch { .. }) => refused += 1,
            other => panic!("flipping byte {i} of the body gave {other:?}"),
        }
    }
    assert_eq!(
        refused, body_len,
        "every byte of the body must be covered by the fingerprint"
    );
}

#[test]
fn a_flipped_bit_in_the_stated_content_id_is_refused() {
    let bytes = sealed();
    let id_start = MAGIC.len() + 2 + 4 + manifest().canonical_bytes().expect("encodes").len();
    for i in id_start..id_start + 32 {
        let mut broken = bytes.clone();
        broken[i] ^= 0x80;
        assert!(
            matches!(opened(&broken), Err(OpenError::ContentIdMismatch { .. })),
            "byte {i} of the content id went unchecked"
        );
    }
}

#[test]
fn a_body_that_fails_its_digest_is_never_decoded() {
    // The body here is malformed *and* its stated id is wrong. If the error names the
    // malformed field, the decoder ran before the check — which is the bug this
    // module is arranged to prevent, and it cannot be seen any other way from outside.
    let wrong_id = [0u8; 32];
    let sig = signer().sign(&ContentId::of(&digest(), MALFORMED_BODY));
    let bytes = wrap(MALFORMED_BODY, FORMAT_VERSION, &wrong_id, KEY, &sig);
    match opened(&bytes) {
        Err(OpenError::ContentIdMismatch { .. }) => {}
        other => panic!("the digest must be checked before the body is read, got {other:?}"),
    }
}

#[test]
fn a_rejected_signature_is_never_decoded() {
    // Same shape: the body is malformed, the digest is right, and the verifier says
    // no. The refusal must be the signature, not the body.
    let bytes = wrap_honestly(MALFORMED_BODY);
    match open(&bytes, &digest(), &RejectsEverything) {
        Err(OpenError::SignatureRejected) => {}
        other => panic!("the signature must be checked before the body is read, got {other:?}"),
    }
}

#[test]
fn a_malformed_body_that_passes_both_checks_is_still_refused() {
    // The counterpart to the two tests above: once the checks pass, the decoder is
    // reached, and it must not accept nonsense either.
    let bytes = wrap_honestly(MALFORMED_BODY);
    assert!(matches!(
        opened(&bytes),
        Err(OpenError::Truncated { .. }) | Err(OpenError::Malformed { .. })
    ));
}

#[test]
fn a_key_the_verifier_does_not_trust_is_refused() {
    let m = manifest();
    let stranger = NotASignature {
        key_id: "someone-elses-key",
    };
    let bytes = seal(&m, &digest(), &stranger).expect("seals");
    // The signature itself is perfectly good — the stand-in scheme is public, which
    // is exactly the situation a real signature scheme is meant to make impossible.
    // What refuses it is the verifier not trusting the key.
    assert_eq!(opened(&bytes), Err(OpenError::SignatureRejected));
}

#[test]
fn a_body_cannot_be_swapped_under_a_signature() {
    let mine = manifest().canonical_bytes().expect("encodes");
    let mut other = manifest();
    other.seed = 9_999;
    let theirs = other.canonical_bytes().expect("encodes");

    // Their body, my content id and my signature.
    let my_id = ContentId::of(&digest(), &mine);
    let sig = signer().sign(&my_id);
    let bytes = wrap(&theirs, FORMAT_VERSION, my_id.as_bytes(), KEY, &sig);
    assert!(matches!(
        opened(&bytes),
        Err(OpenError::ContentIdMismatch { .. })
    ));
}

#[test]
fn bytes_that_are_not_a_package_are_refused() {
    assert_eq!(
        opened(b"hello"),
        Err(OpenError::Truncated { field: "magic" })
    );
    let mut bytes = sealed();
    bytes[0] = b'X';
    assert_eq!(opened(&bytes), Err(OpenError::NotAPackage));
}

#[test]
fn an_unknown_format_version_is_refused_rather_than_guessed_at() {
    let body = manifest().canonical_bytes().expect("encodes");
    let id = ContentId::of(&digest(), &body);
    let sig = signer().sign(&id);
    let future = FORMAT_VERSION + 1;
    let bytes = wrap(&body, future, id.as_bytes(), KEY, &sig);
    assert_eq!(opened(&bytes), Err(OpenError::UnsupportedFormat(future)));
}

#[test]
fn trailing_bytes_are_refused() {
    let mut bytes = sealed();
    bytes.push(0);
    assert_eq!(opened(&bytes), Err(OpenError::TrailingBytes(1)));
}

#[test]
fn every_truncation_is_refused_and_none_panics() {
    // A short file is the most ordinary corruption there is — a cut-off download, a
    // full disk — and the reader has to say where it stopped rather than read past
    // the end. Every prefix is tried, so this covers every field boundary without
    // naming them.
    let bytes = sealed();
    for n in 0..bytes.len() {
        let got = opened(&bytes[..n]);
        assert!(
            got.is_err(),
            "a {n}-byte prefix of a {}-byte package was accepted",
            bytes.len()
        );
    }
    assert!(opened(&bytes).is_ok(), "the whole file must still open");
}

#[test]
fn text_that_is_not_utf8_is_refused() {
    let mut body = manifest().canonical_bytes().expect("encodes");
    // The family name's bytes start after its two-byte length.
    body[2] = 0xff;
    match opened(&wrap_honestly(&body)) {
        Err(OpenError::Malformed { field }) => assert_eq!(field, "family"),
        other => panic!("invalid text must be refused, got {other:?}"),
    }
}

#[test]
fn an_over_long_field_is_an_error_rather_than_a_truncation() {
    // A length that does not fit is the start of two manifests sharing a
    // fingerprint, so encoding refuses instead of writing a shortened one. Note the
    // contrast with `State::canonical_bytes`, which saturates; that is a hash input,
    // not a published file.
    let mut m = manifest();
    m.encoding = "x".repeat(usize::from(u16::MAX) + 1);
    assert_eq!(
        m.canonical_bytes(),
        Err(EncodeError::TooLong {
            field: "encoding",
            len: 65_536
        })
    );
}

#[test]
fn a_measurement_that_was_not_recorded_is_not_zero() {
    let got = opened(&sealed()).expect("opens");
    assert_eq!(got.manifest.measurement("propagation_rounds"), Some(6));
    assert_eq!(got.manifest.measurement("decision_points"), Some(0));
    assert_eq!(got.manifest.measurement("guessing_depth"), None);
}

#[test]
fn the_payload_is_carried_without_being_interpreted() {
    // The core must not grow an opinion about a family's content. Bytes that are not
    // valid anything still round-trip, because the platform's job is to carry them.
    let mut m = manifest();
    m.payload = vec![0xff, 0x00, 0xff, 0x00];
    let bytes = seal(&m, &digest(), &signer()).expect("seals");
    assert_eq!(opened(&bytes).expect("opens").manifest.payload, m.payload);
}
