//! Packages built by hand, on purpose.
//!
//! Every other test file in this workspace that needs a package builds a real one
//! through `gs_nonogram::publish`, and says so for a good reason: a test that invented
//! its own file layout would be checking the code against that invention.
//!
//! This one is different, and the difference is the point. The buffer is supposed to
//! know nothing about any game. These packages are sealed with a made-up family and a
//! payload that is not a puzzle in any family at all — and the buffer keeps them,
//! returns them and counts them exactly as it would a real one. A buffer that needed
//! `gs-nonogram` to compile its own tests would have been a buffer that knew too much.
//!
//! They are still *real* packages: real SHA-256, real Ed25519, sealed by `gs_core::seal`
//! and opened by `gs_core::open`. Nothing about the signature is pretended.

// Shared by `buffering.rs` and `withdrawing.rs`. Each test binary gets its own copy of
// this module and uses some of it, so every helper is unused somewhere. CI builds with
// warnings as errors, and the alternative is splitting the helpers by which file wants
// them, which is a worse arrangement than this line.
#![allow(dead_code)]

use gs_core::package::{seal, Manifest, RecordedReplay};
use gs_core::withdrawal::{seal_list, Reason, WithdrawalList, Withdrawn};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};

/// A private key printed in a public repository, which is all anybody needs to know
/// about it. Here so these tests use real Ed25519 rather than a stand-in.
pub const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
/// A second one, standing in for somebody else entirely.
pub const SOMEBODY_ELSES_PRIVATE_KEY: [u8; 32] = [
    0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e, 0x0f,
    0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed, 0x4f, 0xb8, 0xa6, 0xfb,
];
pub const KEY_ID: &str = "the-device-trusts-this-one";
pub const SOMEBODY_ELSE: &str = "a-key-this-device-never-heard-of";

#[must_use]
pub fn our_signer() -> Ed25519Signer {
    Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY)
}

/// The key list a device ships with: ours, and nobody else's.
#[must_use]
pub fn this_device_trusts_us() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &our_signer().public_key())
        .expect("our test key is valid and the list starts empty");
    keys
}

/// A package signed by somebody this device has never heard of.
#[must_use]
pub fn signed_by_a_stranger(number: u64) -> Vec<u8> {
    let signer = Ed25519Signer::new(SOMEBODY_ELSE, &SOMEBODY_ELSES_PRIVATE_KEY);
    seal(&a_manifest(number), &Sha256, &signer)
        .expect("a hand-built manifest seals")
        .bytes
}

/// A package this device will accept. `number` makes each one different.
#[must_use]
pub fn a_package(number: u64) -> Vec<u8> {
    seal(&a_manifest(number), &Sha256, &our_signer())
        .expect("a hand-built manifest seals")
        .bytes
}

/// The same, with a level on it.
#[must_use]
pub fn a_package_at(number: u64, band: &str) -> Vec<u8> {
    let mut manifest = a_manifest(number);
    manifest.band = Some(band.to_owned());
    seal(&manifest, &Sha256, &our_signer())
        .expect("a hand-built manifest seals")
        .bytes
}

/// A manifest for a family that does not exist, carrying bytes that are not a puzzle.
fn a_manifest(number: u64) -> Manifest {
    Manifest {
        // Not a real family, deliberately. The buffer must not care.
        family: "a-game-nobody-has-written".to_owned(),
        ruleset_version: 1,
        generator_version: 1,
        seed: number,
        encoding: "an encoding nobody has defined".to_owned(),
        band: None,
        payload: format!("this is not a puzzle, it is package {number}").into_bytes(),
        queries: Vec::new(),
        measurements: Vec::new(),
        replay: RecordedReplay::NotAttempted,
    }
}

/// The fingerprint of a package, learnt the way a device learns it.
#[must_use]
pub fn id_of(package: &[u8]) -> gs_core::content::ContentId {
    gs_core::package::open(package, &Sha256, &this_device_trusts_us())
        .expect("our own package opens")
        .content_id
}

/// A signed "do not play these" list, from the key this device trusts.
#[must_use]
pub fn a_withdrawal(serial: u64, named: &[(gs_core::content::ContentId, Reason)]) -> Vec<u8> {
    seal_a_withdrawal(serial, named, &our_signer())
}

/// The same, from somebody this device has never heard of.
#[must_use]
pub fn a_withdrawal_from_a_stranger(
    serial: u64,
    named: &[(gs_core::content::ContentId, Reason)],
) -> Vec<u8> {
    seal_a_withdrawal(
        serial,
        named,
        &Ed25519Signer::new(SOMEBODY_ELSE, &SOMEBODY_ELSES_PRIVATE_KEY),
    )
}

fn seal_a_withdrawal(
    serial: u64,
    named: &[(gs_core::content::ContentId, Reason)],
    signer: &Ed25519Signer,
) -> Vec<u8> {
    let list = WithdrawalList {
        serial,
        withdrawn: named
            .iter()
            .map(|(id, reason)| Withdrawn {
                content_id: *id.as_bytes(),
                reason: *reason,
            })
            .collect(),
    };
    seal_list(&list, &Sha256, signer).expect("a short list seals")
}
