//! A small deterministic random number generator.
//!
//! Generation has to be reproducible: the architecture plan's "reproducible
//! releases" principle means a recorded seed must rebuild the same puzzle, and the
//! validation report records the seed for exactly that reason.
//!
//! This is **SplitMix64**, written out here rather than taken from a dependency, for
//! the same reason the state hash is: a generator whose stream could change between
//! library versions would silently break the promise that a seed reproduces a
//! puzzle. It is also why this is not `std`'s `RandomState`, which is seeded
//! unpredictably on purpose.
//!
//! **Not cryptographic, and never used as if it were.** Its only job is to make a
//! seed reproduce a sequence. Signatures and content ids use
//! [`crate::content::Digest`], which is a different mechanism with a different
//! strength requirement.
//!
//! It lives here rather than in a game crate because more than one thing needs a
//! reproducible sequence and they must be the same sequence. Nonogram generation
//! rebuilds a pattern from a recorded seed; the offline buffer rebuilds a player's
//! shuffled order from one. Two copies of SplitMix64 in one workspace would be two
//! spellings of one thing, which is how two layers come to disagree.

/// A reproducible stream of numbers from a seed.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    #[must_use]
    pub fn from_seed(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        // SplitMix64, as published. The constants are part of the algorithm; changing
        // any of them changes every puzzle every recorded seed would rebuild.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// True with probability `per_mille` in 1000.
    ///
    /// Per-mille rather than a fraction because the core forbids floating-point
    /// arithmetic, and because an integer threshold is exactly reproducible where a
    /// float comparison might not be.
    pub fn chance_per_mille(&mut self, per_mille: u16) -> bool {
        u64::from(per_mille.min(1000)) > self.next_u64() % 1000
    }

    /// A number in `0..n`, or `None` when `n` is zero.
    pub fn below(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return None;
        }
        Some(u32::try_from(self.next_u64() % u64::from(n)).unwrap_or(0))
    }
}
