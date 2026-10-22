//! State hashes, for replay validation.
//!
//! The architecture plan's authoritative record includes "state hashes for replay
//! validation". This is that, and it is deliberately **not** the same thing as a
//! content id:
//!
//! | | purpose | strength needed |
//! |---|---|---|
//! | [`StateHash`] | does this replay reproduce the same logical state? | none; an accidental collision is what is being guarded against |
//! | [`crate::content::ContentId`] | is this the artifact that was signed? | cryptographic |
//!
//! Keeping them separate means neither can be mistaken for the other, and the
//! replay check does not have to wait on a crypto dependency.
//!
//! The algorithm is FNV-1a over [`State::canonical_bytes`], written out here rather
//! than taken from the standard library, because `DefaultHasher` is explicitly not
//! stable across Rust versions or platforms and replay equality across platforms is
//! exactly the property being checked.

use crate::state::State;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A hash of one logical state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateHash(pub u64);

impl StateHash {
    /// Hash a state through its canonical encoding — the single definition of what a
    /// state's bytes are.
    #[must_use]
    pub fn of(state: &State) -> Self {
        Self::of_bytes(&state.canonical_bytes())
    }

    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut h = FNV_OFFSET_BASIS;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(FNV_PRIME);
        }
        StateHash(h)
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(16);
        for b in self.0.to_be_bytes() {
            s.push(char::from_digit(u32::from(b >> 4), 16).expect("nibble"));
            s.push(char::from_digit(u32::from(b & 0x0f), 16).expect("nibble"));
        }
        s
    }
}
