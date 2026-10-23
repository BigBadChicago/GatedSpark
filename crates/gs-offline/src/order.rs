//! A player's own shuffled order, and the one number that says how far along it they are.
//!
//! The alternative is remembering every puzzle a player has finished, which is one row
//! per puzzle played, for every player, forever. This is one number per level instead.
//!
//! It works because the order is *theirs*: a shuffle seeded from their own id, so two
//! players get different orders from the same pool and neither needs a record of what
//! the other played. A bookmark part way along means "everything before this, I have
//! played".
//!
//! The whole thing rests on one promise: **what is behind the bookmark never moves.**
//! If the order could be reshuffled, a bookmark would point at a different puzzle
//! tomorrow than it did today, and the player would be handed something they had
//! already finished. So new puzzles join at the end and nothing else is touched —
//! [`Order::grow_to`] is the only way the order changes, and it is append-only by
//! construction rather than by care.

use gs_core::rng::Rng;

/// The order one player works through a pool in.
///
/// Holds the realised sequence rather than computing it on demand. A pool of a few
/// thousand entries is a few thousand numbers, which is nothing to keep; and a stored
/// sequence can be *checked* against the promise above, where a formula could only be
/// argued about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    seed: u64,
    /// Pool positions, in the order this player meets them.
    sequence: Vec<u32>,
}

impl Order {
    /// The order for a player, over a pool of `pool_size` entries.
    #[must_use]
    pub fn for_player(seed: u64, pool_size: u32) -> Self {
        let mut order = Order {
            seed,
            sequence: Vec::new(),
        };
        order.grow_to(pool_size);
        order
    }

    /// The seed this order was built from.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// How many entries are in it.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.sequence.len()).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }

    /// Which pool entry is at `position`.
    #[must_use]
    pub fn at(&self, position: u32) -> Option<u32> {
        self.sequence.get(usize::try_from(position).ok()?).copied()
    }

    /// Take account of a pool that has grown.
    ///
    /// The new entries are shuffled among themselves and appended. Everything already
    /// in the sequence keeps its position, which is the promise this type exists to
    /// keep. A `pool_size` no larger than the current length does nothing: a pool does
    /// not shrink, and if one ever appeared to, dropping entries a bookmark might
    /// already be past is the worst available response.
    pub fn grow_to(&mut self, pool_size: u32) {
        let have = self.len();
        if pool_size <= have {
            return;
        }
        let mut fresh: Vec<u32> = (have..pool_size).collect();
        // Seeded from this player's seed *and* from where the block starts, so the
        // second block is not a repeat of the first for a player whose pool grew twice
        // by the same amount.
        let mut rng = Rng::from_seed(self.seed ^ u64::from(have).wrapping_mul(0x9E37_79B9));
        shuffle(&mut fresh, &mut rng);
        self.sequence.extend_from_slice(&fresh);
    }
}

/// Fisher-Yates, written out because it is four lines and because the one in a
/// dependency would be the one that changed under us.
///
/// Walks from the end so that each step picks uniformly from what is left. Going the
/// other way, or picking from the whole slice each time, is the classic wrong version:
/// it still looks shuffled and is not uniform.
fn shuffle(items: &mut [u32], rng: &mut Rng) {
    if items.len() < 2 {
        return;
    }
    for i in (1..items.len()).rev() {
        let upper = u32::try_from(i + 1).unwrap_or(u32::MAX);
        if let Some(j) = rng.below(upper) {
            items.swap(i, usize::try_from(j).unwrap_or(0));
        }
    }
}

/// How far along their order a player has got.
///
/// Advances **only when a puzzle is actually played**. Downloading five puzzles ahead
/// does not move it, which is the whole reason it is a separate thing from the buffer:
/// a bookmark cannot express "I have this but have not opened it".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bookmark {
    position: u32,
}

impl Bookmark {
    /// At the very start, nothing played.
    #[must_use]
    pub fn new() -> Self {
        Bookmark::default()
    }

    /// At a position read back from storage.
    ///
    /// No validation against an order here on purpose: a bookmark and the order it
    /// belongs to are stored separately and can be read back in either sequence. What
    /// stops an out-of-range bookmark mattering is [`Bookmark::next_in`], which returns
    /// nothing rather than reaching past the end.
    #[must_use]
    pub fn at(position: u32) -> Self {
        Bookmark { position }
    }

    #[must_use]
    pub fn position(&self) -> u32 {
        self.position
    }

    /// The pool entry this player should be given next, if there is one.
    #[must_use]
    pub fn next_in(&self, order: &Order) -> Option<u32> {
        order.at(self.position)
    }

    /// Record that the puzzle at this position has been played.
    ///
    /// Takes the order so that it cannot walk off the end: a bookmark past the last
    /// entry would claim the player had finished puzzles that do not exist, and the
    /// next pool growth would hand them whatever landed at that position.
    pub fn played(&mut self, order: &Order) {
        if self.position < order.len() {
            self.position += 1;
        }
    }

    /// How many are left after this one.
    #[must_use]
    pub fn remaining_in(&self, order: &Order) -> u32 {
        order.len().saturating_sub(self.position)
    }
}
