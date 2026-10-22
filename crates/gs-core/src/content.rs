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
/// **Unenforced in M0:** no cryptographic implementation ships yet, because this
/// environment cannot reach the crate registry (see `docs/adr/0004-dependencies.md`).
/// M4 binds this to a reviewed SHA-256 or BLAKE3 implementation before anything is
/// signed. The test-only implementation in `tests/` exercises the *binding*
/// discipline, not collision resistance, and must never be used to publish.
pub trait Digest {
    fn digest(&self, bytes: &[u8]) -> [u8; 32];
}
