//! The only cryptography in GatedSpark.
//!
//! Two things live here and nothing else does: the function that turns bytes into a
//! content id, and the pair that signs and checks a package. Both are thin wrappers
//! over reviewed implementations from crates with a long public history; neither
//! contains any arithmetic of our own.
//!
//! # Why this is its own crate
//!
//! `gs-core` has no dependencies, and keeping it that way is worth something: it is
//! the crate whose determinism the whole platform rests on, and every dependency
//! added to it is something else that has to be pinned, audited and reproduced. So
//! the core defines the *interfaces* — [`gs_core::content::Digest`],
//! [`gs_core::package::Signer`], [`gs_core::package::SignatureVerifier`] — and this
//! crate is the one place in the workspace that implements them.
//!
//! There is a second reason, and it is the stronger one. A crate that can sign is a
//! crate that can forge, and a crate that verifies its own signatures proves nothing
//! about the real thing. Because `gs-core` cannot sign, every test in `gs-core` that
//! looks like it checks a signature is visibly using a stand-in, named so that nobody
//! could mistake it for the real one. That property would be lost the moment a real
//! signer were reachable from the core's own test tree.
//!
//! # What is proved here, and how
//!
//! Wrapping a library correctly is its own failure mode: the usual way to get this
//! wrong is to hash the wrong bytes, or to feed a key into the wrong slot, and the
//! result looks exactly like working code. So the tests in `tests/` are **known-answer
//! tests**: fixed inputs with published expected outputs, from FIPS 180-4 for SHA-256
//! and RFC 8032 for Ed25519. They compare this crate's output with a number written
//! down by a standards body rather than with another run of the same code.

use ed25519_dalek::Signer as _;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use gs_core::content::{ContentId, Digest};
use gs_core::package::{SignatureVerifier, Signer};
use sha2::Digest as _;

/// The content digest: SHA-256, as published in FIPS 180-4.
///
/// # Why SHA-256 and not something newer
///
/// A content id is the name an artifact is known by for as long as the artifact
/// exists, and the one property worth more than raw speed is that *anything else* can
/// recompute it. SHA-256 is implemented everywhere, has published test vectors, and is
/// what `sha256sum` computes. A content id nobody outside this codebase can
/// independently reproduce is a weaker guarantee than one anybody can, even if the
/// hash function underneath is in some respect better.
///
/// # What this covers
///
/// Exactly the bytes it is handed. [`gs_core::package::seal`] decides what those bytes
/// are, and it digests the container version together with the body, so the signature
/// covers which rules the body is to be read under. That choice lives there, not here:
/// this type is a hash function and knows nothing about packages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sha256;

impl Digest for Sha256 {
    fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        let mut h = sha2::Sha256::new();
        h.update(bytes);
        h.finalize().into()
    }
}

/// The domain string mixed into every package signature.
///
/// A bare Ed25519 signature is a signature over 32 arbitrary bytes, and a content id
/// is 32 bytes. Without something to say what those bytes *were*, a signature made
/// over one kind of 32-byte value could be presented as a signature over another kind
/// — a hash of something else entirely, a key fingerprint, a nonce — as long as the
/// bytes matched. Prefixing a fixed, purpose-naming string means a signature produced
/// here can only ever be read as "this key vouches for this package content id".
///
/// The trailing version number is so a future change of what is signed is a new
/// domain rather than a quiet reinterpretation of old signatures.
pub const SIGNATURE_DOMAIN: &[u8] = b"gatedspark/package-content-id/v1\x00";

/// The exact bytes signed for a content id.
///
/// One definition, used by both the signer and the verifier below, so the two cannot
/// drift apart. If they did, the symptom would be every signature failing, which is at
/// least loud; the dangerous version of that bug is a verifier that checks less than
/// the signer committed to, and a shared definition is what rules it out.
fn signed_bytes(content_id: &ContentId) -> Vec<u8> {
    let mut out = Vec::with_capacity(SIGNATURE_DOMAIN.len() + 32);
    out.extend_from_slice(SIGNATURE_DOMAIN);
    out.extend_from_slice(content_id.as_bytes());
    out
}

/// Signs package content ids with one Ed25519 private key.
///
/// # Why Ed25519
///
/// Deterministic signatures: the same key over the same message gives the same bytes
/// every time, with no random nonce to go wrong. That matters twice over here. It fits
/// a platform whose whole argument is reproducibility, and it removes the failure that
/// has broken ECDSA deployments repeatedly, where a repeated or predictable nonce
/// leaks the private key. Signatures are also a fixed 64 bytes and public keys 32,
/// which keeps the package format simple.
///
/// # The key
///
/// [`Ed25519Signer::new`] takes the private key bytes. This crate does not generate,
/// store, load or rotate keys, and deliberately offers no "make me a key" function
/// outside of tests: where a production key comes from is an operational decision that
/// has not been made yet, and a convenient generator is how a key ends up living in a
/// repository. See `docs/adr/0006-content-id-and-signature.md`.
pub struct Ed25519Signer {
    key_id: String,
    key: SigningKey,
}

impl Ed25519Signer {
    /// Build a signer from a key id and 32 private key bytes.
    ///
    /// The key id is the name a verifier will look this key up by. It travels in the
    /// package in the clear and the signature does not cover it, which is documented
    /// on [`gs_core::package::SignatureVerifier`]: what protects against a stranger
    /// naming their own key is the verifier's list of keys it trusts, not the file.
    #[must_use]
    pub fn new(key_id: impl Into<String>, private_key: &[u8; 32]) -> Self {
        Ed25519Signer {
            key_id: key_id.into(),
            key: SigningKey::from_bytes(private_key),
        }
    }

    /// The public key matching this signer, for putting into a [`TrustedKeys`] list.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }
}

// No `Debug`: the derived one would print the private key, and the day that happens is
// the day it is in a log. The id is public and can be asked for on its own.
impl core::fmt::Debug for Ed25519Signer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ed25519Signer")
            .field("key_id", &self.key_id)
            .field("key", &"<private>")
            .finish()
    }
}

impl Signer for Ed25519Signer {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn sign(&self, content_id: &ContentId) -> Vec<u8> {
        self.key.sign(&signed_bytes(content_id)).to_bytes().to_vec()
    }
}

/// The public keys a reader is willing to accept packages from.
///
/// # This list is the security boundary, not the signature
///
/// A package names the key that signed it, and anybody can sign a package of their own
/// making with a key of their own making and write their own id next to it. The
/// signature on such a package is perfectly valid. What makes it unacceptable is that
/// its key is not on this list.
///
/// So an empty `TrustedKeys` trusts nothing and refuses every package, which is the
/// right behaviour for a reader that has not been told whose content it wants, and is
/// the reason there is no `TrustedKeys::trusting_anything`.
#[derive(Debug, Clone, Default)]
pub struct TrustedKeys {
    keys: Vec<(String, VerifyingKey)>,
}

/// Why a public key could not be added to a trusted list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The 32 bytes are not a valid Ed25519 public key. Refused at the point of being
    /// trusted rather than at the point of being used, so a misconfigured reader fails
    /// when it is set up instead of silently rejecting every package later.
    NotAPublicKey,
    /// A different key is already trusted under this id. Refused rather than replaced:
    /// two keys under one name means "is this package signed by a key we trust" has
    /// two answers, and silently keeping one of them picks an answer at random.
    DuplicateKeyId,
    /// The bytes decode to a valid curve point, but to one of the small-order points
    /// for which a signature can be produced without knowing any private key.
    ///
    /// These are valid encodings, so nothing earlier catches them, and a reader that
    /// trusted one would accept forged packages while every signature check passed.
    /// There is no legitimate reason for a signing key to have such a public key, so
    /// the only way one appears on a trusted list is a mistake or an attack.
    WeakPublicKey,
}

impl TrustedKeys {
    /// A list that trusts nothing, and therefore accepts nothing.
    #[must_use]
    pub fn none() -> Self {
        TrustedKeys { keys: Vec::new() }
    }

    /// Trust `public_key` under the name `key_id`.
    pub fn trust(
        &mut self,
        key_id: impl Into<String>,
        public_key: &[u8; 32],
    ) -> Result<(), KeyError> {
        let key_id = key_id.into();
        let key = VerifyingKey::from_bytes(public_key).map_err(|_| KeyError::NotAPublicKey)?;
        if key.is_weak() {
            return Err(KeyError::WeakPublicKey);
        }
        if self.keys.iter().any(|(id, _)| *id == key_id) {
            return Err(KeyError::DuplicateKeyId);
        }
        self.keys.push((key_id, key));
        Ok(())
    }

    /// How many keys are trusted. Zero means nothing will be accepted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl SignatureVerifier for TrustedKeys {
    fn verify(&self, content_id: &ContentId, key_id: &str, signature: &[u8]) -> bool {
        let Some((_, key)) = self.keys.iter().find(|(id, _)| id == key_id) else {
            // An unknown key id is a refusal before any cryptography is attempted.
            return false;
        };
        // A signature that is not 64 bytes is not an Ed25519 signature. `try_from` is
        // how that is noticed; there is no padding or truncation to a usable length,
        // because a signature a reader had to repair is not a signature.
        let Ok(bytes) = <[u8; 64]>::try_from(signature) else {
            return false;
        };
        // `verify_strict`, not `verify`. The permissive check is the one RFC 8032
        // describes, and under it a signature is not unique: more than one byte string
        // can verify for the same key and message. In a format whose whole premise is
        // that a file has exactly one identity, that is the wrong property to have —
        // two different package files, one signature's worth of bytes apart, would
        // both open. The strict check also refuses small-order points in the
        // signature itself, which is the other half of what `KeyError::WeakPublicKey`
        // guards at configuration time.
        key.verify_strict(&signed_bytes(content_id), &Signature::from_bytes(&bytes))
            .is_ok()
    }
}
