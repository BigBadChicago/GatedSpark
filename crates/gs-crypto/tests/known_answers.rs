//! Known-answer tests: does this crate compute what the standards say?
//!
//! Everywhere else in this repository, a test compares our work with our work. That is
//! the right shape for most things, and it is useless here. The way a thin wrapper over
//! a hash or a signature goes wrong is by feeding in the wrong bytes, or the right
//! bytes in the wrong order, or a key into the wrong slot — and every one of those
//! produces output that is perfectly self-consistent. A round-trip test passes happily
//! over all of them.
//!
//! So the numbers below are not ours. They are the published test vectors from
//! FIPS 180-4 (SHA-256) and RFC 8032 section 7.1 (Ed25519), transcribed as hex. A
//! failure here means this crate does not compute the function it claims to, which is
//! the one thing no amount of internal agreement can tell us.

use gs_core::content::{ContentId, Digest as _};
use gs_core::package::{SignatureVerifier, Signer};
use gs_crypto::{Ed25519Signer, KeyError, Sha256, TrustedKeys, SIGNATURE_DOMAIN};

/// Parse a hex string in a test. Panics on anything that is not clean hex, which is
/// correct for a transcribed constant: a typo in a test vector must stop the test, not
/// quietly become a different vector.
fn hex(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2),
        "hex string has an odd number of digits"
    );
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = core::str::from_utf8(pair).expect("ascii");
            u8::from_str_radix(text, 16).expect("hex digits")
        })
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    hex(s).try_into().expect("32 bytes of hex")
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// SHA-256, against FIPS 180-4 and the NIST example set.
// ---------------------------------------------------------------------------

#[test]
fn sha256_matches_the_published_vectors() {
    let cases: &[(&[u8], &str)] = &[
        // FIPS 180-4, the one-block example.
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        // The empty string. Worth having by itself: a wrapper that forgot to call the
        // hash at all and returned zeros would be caught here and almost nowhere else.
        (
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        // FIPS 180-4, the two-block example: longer than one 64-byte block, so this is
        // the case that would fail if the wrapper mishandled buffering.
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ];

    for (input, expected) in cases {
        let got = Sha256.digest(input);
        assert_eq!(
            to_hex(&got),
            *expected,
            "SHA-256 of {} bytes does not match the published vector",
            input.len()
        );
    }
}

#[test]
fn a_content_id_is_the_sha256_of_its_bytes_and_prints_as_that() {
    // The hex a content id prints must be the hex anything else computing SHA-256 over
    // the same bytes would print, because that reproducibility is the stated reason for
    // choosing SHA-256 at all. If `to_hex` reversed the bytes, every internal test
    // would still pass and this claim would be false.
    let id = ContentId::of(&Sha256, b"abc");
    assert_eq!(
        id.to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn one_changed_byte_changes_the_content_id() {
    // Not a cryptographic claim — three pairs prove nothing about collision resistance.
    // It is here because the stand-in digests this crate replaces included one that
    // copied only the first bytes it was given, and a test at this altitude is what
    // notices if something like that is ever wired back in.
    let a = ContentId::of(&Sha256, b"the quick brown fox");
    let b = ContentId::of(&Sha256, b"the quick brown fox.");
    let c = ContentId::of(&Sha256, b"the quick brown fox ");
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}

// ---------------------------------------------------------------------------
// Ed25519, against RFC 8032 section 7.1.
// ---------------------------------------------------------------------------

/// RFC 8032 section 7.1, TEST 1: a 32-byte private key and the public key derived from
/// it. The derivation is a SHA-512 and a scalar multiplication, so getting the right
/// public key out of the right private key is already a real check on the wiring.
const RFC8032_TEST1_SECRET: &str =
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const RFC8032_TEST1_PUBLIC: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

/// RFC 8032 section 7.1, TEST 2.
const RFC8032_TEST2_SECRET: &str =
    "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
const RFC8032_TEST2_PUBLIC: &str =
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";

#[test]
fn the_public_key_derived_from_a_published_private_key_is_the_published_one() {
    for (secret, public) in [
        (RFC8032_TEST1_SECRET, RFC8032_TEST1_PUBLIC),
        (RFC8032_TEST2_SECRET, RFC8032_TEST2_PUBLIC),
    ] {
        let signer = Ed25519Signer::new("rfc8032", &hex32(secret));
        assert_eq!(
            to_hex(&signer.public_key()),
            public,
            "the public key this crate derives is not the one RFC 8032 publishes"
        );
    }
}

#[test]
fn a_signature_is_sixty_four_bytes() {
    // The package format stores the signature with its length, so nothing breaks if it
    // were another size — which is exactly why this is worth asserting rather than
    // assuming. A 64-byte signature is what Ed25519 is, and a different length would
    // mean this is not the scheme the ADR says it is.
    let signer = Ed25519Signer::new("k", &hex32(RFC8032_TEST1_SECRET));
    let id = ContentId::of(&Sha256, b"anything");
    assert_eq!(signer.sign(&id).len(), 64);
}

// ---------------------------------------------------------------------------
// The signer and the verifier, as the package format uses them.
// ---------------------------------------------------------------------------

const KEY_ID: &str = "gatedspark-test-2026-10";

fn signer() -> Ed25519Signer {
    Ed25519Signer::new(KEY_ID, &hex32(RFC8032_TEST1_SECRET))
}

fn trusting_the_signer() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &signer().public_key())
        .expect("a freshly derived public key is valid and the list is empty");
    keys
}

#[test]
fn a_signature_this_crate_made_is_one_it_accepts() {
    // The control. Without it every refusal test below could pass because nothing is
    // ever accepted.
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = signer().sign(&id);
    assert!(trusting_the_signer().verify(&id, KEY_ID, &sig));
}

#[test]
fn a_signature_over_a_different_content_id_is_refused() {
    let signed = ContentId::of(&Sha256, b"the body that was signed");
    let other = ContentId::of(&Sha256, b"the body that was swapped in");
    let sig = signer().sign(&signed);
    assert!(!trusting_the_signer().verify(&other, KEY_ID, &sig));
}

#[test]
fn an_empty_trusted_list_accepts_nothing() {
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = signer().sign(&id);
    assert!(
        !TrustedKeys::none().verify(&id, KEY_ID, &sig),
        "a reader that has not been told whose content it wants must accept none of it"
    );
}

#[test]
fn a_valid_signature_from_an_untrusted_key_is_refused() {
    // The attack the trusted-key list exists to stop: a package signed properly, by a
    // key nobody asked for, naming itself in the file. The signature is genuine. The
    // key is not ours.
    let stranger = Ed25519Signer::new("stranger", &hex32(RFC8032_TEST2_SECRET));
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = stranger.sign(&id);
    assert!(
        !trusting_the_signer().verify(&id, "stranger", &sig),
        "an unknown key id must be refused however valid its signature"
    );
    // And it is refused because of the key, not because the signature is bad: the same
    // signature is accepted once that key is trusted.
    let mut keys = trusting_the_signer();
    keys.trust("stranger", &stranger.public_key()).expect("new");
    assert!(keys.verify(&id, "stranger", &sig));
}

#[test]
fn a_signature_presented_under_the_wrong_key_id_is_refused() {
    // Both keys are trusted, so this is not the unknown-key case. The signature was
    // made by one and is claimed to be by the other.
    let stranger = Ed25519Signer::new("second", &hex32(RFC8032_TEST2_SECRET));
    let mut keys = trusting_the_signer();
    keys.trust("second", &stranger.public_key()).expect("new");
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = signer().sign(&id);
    assert!(keys.verify(&id, KEY_ID, &sig), "the honest claim holds");
    assert!(
        !keys.verify(&id, "second", &sig),
        "the false claim does not"
    );
}

#[test]
fn a_flipped_bit_anywhere_in_a_signature_is_refused() {
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = signer().sign(&id);
    let keys = trusting_the_signer();
    for byte in 0..sig.len() {
        for bit in 0..8 {
            let mut broken = sig.clone();
            broken[byte] ^= 1 << bit;
            assert!(
                !keys.verify(&id, KEY_ID, &broken),
                "flipping bit {bit} of byte {byte} must break the signature"
            );
        }
    }
}

#[test]
fn a_signature_of_the_wrong_length_is_refused_rather_than_padded() {
    let id = ContentId::of(&Sha256, b"a package body");
    let sig = signer().sign(&id);
    let keys = trusting_the_signer();
    for len in [0usize, 1, 32, 63, 65] {
        let mut wrong = sig.clone();
        wrong.resize(len, 0);
        assert!(
            !keys.verify(&id, KEY_ID, &wrong),
            "a {len}-byte signature is not an Ed25519 signature and must not be repaired into one"
        );
    }
}

#[test]
fn the_domain_string_is_really_part_of_what_is_signed() {
    // The domain prefix is only worth having if it is actually in the signed message.
    // Sign the bare content id with the same key, by hand, and check the verifier
    // refuses it: that is the difference between a documented domain and a real one.
    use ed25519_dalek::{Signer as _, SigningKey};

    let key = SigningKey::from_bytes(&hex32(RFC8032_TEST1_SECRET));
    let id = ContentId::of(&Sha256, b"a package body");
    let bare = key.sign(id.as_bytes()).to_bytes().to_vec();
    assert!(
        !trusting_the_signer().verify(&id, KEY_ID, &bare),
        "a signature over the bare content id must not pass as a package signature"
    );

    // And the domain-separated message really is the prefix followed by the id, which
    // is what lets anything outside this crate reproduce it.
    let mut message = SIGNATURE_DOMAIN.to_vec();
    message.extend_from_slice(id.as_bytes());
    let by_hand = key.sign(&message).to_bytes().to_vec();
    assert_eq!(
        by_hand,
        signer().sign(&id),
        "the signed message is the domain string followed by the content id"
    );
}

#[test]
fn bytes_that_are_not_a_public_key_are_refused_when_trusted_not_when_used() {
    // A 2 in the low byte is a y coordinate with no matching x on the curve, so these
    // bytes are not a point at all. Caught at configuration time, so a reader with a
    // typo in its key material fails where somebody is watching rather than rejecting
    // every package later for no visible reason.
    let mut not_a_point = [0u8; 32];
    not_a_point[0] = 2;
    let mut keys = TrustedKeys::none();
    assert_eq!(
        keys.trust("bad", &not_a_point),
        Err(KeyError::NotAPublicKey)
    );
    assert!(keys.is_empty());
}

#[test]
fn a_small_order_public_key_is_refused_even_though_it_decodes() {
    // The dangerous case, and the reason `trust` checks more than "does this decode".
    // All-zero bytes are a perfectly valid encoding of a small-order point, and anybody
    // can produce signatures that verify under it without holding any private key. A
    // reader that trusted it would accept forged packages with every check passing.
    //
    // It decodes successfully — that is the point of the test — so nothing short of an
    // explicit weak-key check would stop it.
    let mut keys = TrustedKeys::none();
    assert_eq!(
        keys.trust("weak", &[0u8; 32]),
        Err(KeyError::WeakPublicKey),
        "a small-order public key must never reach a trusted list"
    );
    assert!(keys.is_empty());
}

#[test]
fn one_key_id_cannot_name_two_keys() {
    let mut keys = trusting_the_signer();
    let other = Ed25519Signer::new("ignored", &hex32(RFC8032_TEST2_SECRET));
    assert_eq!(
        keys.trust(KEY_ID, &other.public_key()),
        Err(KeyError::DuplicateKeyId),
        "two keys under one id would give 'do we trust this signer' two answers"
    );
    assert_eq!(keys.len(), 1);
}

#[test]
fn a_signer_never_prints_its_private_key() {
    // The derived Debug would print the key bytes, and a key in a log is a key that has
    // left the building. This is the test that stops a future `#[derive(Debug)]`.
    let printed = format!("{:?}", signer());
    assert!(
        printed.contains(KEY_ID),
        "the id is public and worth printing"
    );
    assert!(
        !printed.contains("9d61b1") && !printed.contains("157, 97"),
        "the private key must not appear in Debug output, got: {printed}"
    );
}
