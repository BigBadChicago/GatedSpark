//! Content identity.
//!
//! PuzzleGen's recurring failure was values that two layers compared while each
//! derived them separately. The rule carried over: every artifact is bound to its
//! bytes by exactly one definition of its identity, and that definition lives here.

/// A content id: the digest of the exact bytes of an artifact.
///
/// There is no constructor from a name, a seed or a counter. The only way to obtain
/// one is to digest bytes, which is what makes "bound by id and hash" true rather
/// than customary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId([u8; 32]);

impl ContentId {
    /// Digest `bytes` with `d` and bind the result.
    pub fn of<D: Digest>(d: &D, bytes: &[u8]) -> Self {
        ContentId(d.digest(bytes))
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push(char::from_digit(u32::from(b >> 4), 16).expect("nibble"));
            s.push(char::from_digit(u32::from(b & 0x0f), 16).expect("nibble"));
        }
        s
    }
}

/// A 32-byte digest function.
///
/// The implementation is SHA-256, in `gs-crypto`, and it is deliberately not in this
/// crate: see `docs/adr/0006-content-id-and-signature.md`, and the note on
/// [`crate::package`] about why a crate that can sign should not be the crate whose
/// tests check signatures. The weak stand-ins in this crate's `tests/` exercise the
/// *binding* discipline — that an id comes from digesting bytes and is compared against
/// what a file claims — and never collision resistance. Using one to publish would be a
/// security hole, which is why they are named the way they are.
pub trait Digest {
    fn digest(&self, bytes: &[u8]) -> [u8; 32];
}
