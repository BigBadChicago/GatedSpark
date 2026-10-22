//! The batch worker: fill a quota for each difficulty level, unattended.
//!
//! This is what produces the pre-verified pool that `design/content-delivery.md`
//! describes. Generation timing is separate from delivery timing: this runs ahead of
//! demand, so a player's request is a lookup rather than a generation.
//!
//! # What makes it safe to leave running
//!
//! * **No clock.** Nothing here reads the time. The same start seed, quotas and
//!   limits produce the same puzzles in the same order, on any machine, forever.
//!   Nothing deciding what gets published may depend on a wall clock.
//! * **A hard seed limit per level.** The loop cannot run forever on a level the
//!   generator struggles to fill. When the limit is reached, the run says the quota
//!   was not met instead of carrying on or pretending.
//! * **An unmet quota is reported, never hidden.** [`BandOutcome::shortfall`] is the
//!   number a human has to look at. A worker that quietly delivered fewer puzzles
//!   than asked would be discovered as an empty difficulty level in front of a
//!   player.
//! * **No duplicates.** Two different seeds can produce the same clues, and at the
//!   smaller sizes they do. A pool with duplicates hands a player the same puzzle
//!   twice, so identical clue sets are counted and dropped.
//!
//! # What this is not
//!
//! It is not verification. Everything it returns is a [`Candidate`]: the cheap filter
//! believes the puzzle has one answer. Publication requires an SMT solver reading
//! [`crate::smtlib`], which is M4's work, and nothing here may be published on the
//! strength of this run alone.

use std::collections::BTreeSet;

use crate::difficulty::{propose_in_band, Band};
use crate::generate::Candidate;
use crate::solve::Budget;

/// How many puzzles of one level to make.
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub band: &'static Band,
    pub wanted: u32,
}

/// The stopping conditions. Both are required; neither has a default that could
/// quietly become "unlimited".
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The most seeds to try for one level before giving up on its quota.
    pub max_seeds_per_band: u64,
    /// The per-puzzle work limit handed to the filter.
    pub budget: Budget,
}

/// What happened for one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandOutcome {
    pub band_name: &'static str,
    pub wanted: u32,
    pub produced: u32,
    /// Seeds tried. With `first_seed`, this is what makes the run repeatable.
    pub seeds_tried: u64,
    pub first_seed: u64,
    /// Puzzles the filter kept that were the same as one already produced.
    pub duplicates_dropped: u32,
    /// True when the seed limit stopped the search rather than the quota being met.
    pub hit_seed_limit: bool,
}

impl BandOutcome {
    /// How many short of the quota. Zero means the level was filled.
    #[must_use]
    pub fn shortfall(&self) -> u32 {
        self.wanted.saturating_sub(self.produced)
    }

    #[must_use]
    pub fn filled(&self) -> bool {
        self.shortfall() == 0
    }

    /// Seeds tried per puzzle produced. The number that matters for capacity
    /// planning: it says what one puzzle of this level costs.
    #[must_use]
    pub fn seeds_per_puzzle(&self) -> u64 {
        self.seeds_tried
            .checked_div(u64::from(self.produced))
            .unwrap_or(0)
    }
}

/// A whole run.
#[derive(Debug, Clone)]
pub struct Run {
    pub outcomes: Vec<BandOutcome>,
    pub candidates: Vec<Candidate>,
}

impl Run {
    /// Whether every level met its quota.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.outcomes.iter().all(BandOutcome::filled)
    }

    /// The levels that came up short, for a human to read.
    #[must_use]
    pub fn shortfalls(&self) -> Vec<&BandOutcome> {
        self.outcomes.iter().filter(|o| !o.filled()).collect()
    }
}

/// Fill each quota in turn.
///
/// Each level gets its own seed run starting at `first_seed`. The same seed means a
/// different board for a different level, because the board shape differs, so there
/// is no need to partition the seed space and every level's run is independently
/// repeatable from the two numbers its outcome records.
#[must_use]
pub fn run(first_seed: u64, quotas: &[Quota], limits: Limits) -> Run {
    let mut outcomes = Vec::with_capacity(quotas.len());
    let mut candidates = Vec::new();

    for quota in quotas {
        // Duplicate detection is per level, keyed on the canonical clue bytes rather
        // than on a hash: the comparison has to be exact, and a hash would introduce
        // a collision question for no benefit at this scale.
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut produced = 0u32;
        let mut duplicates = 0u32;
        let mut tried = 0u64;

        while produced < quota.wanted && tried < limits.max_seeds_per_band {
            let seed = first_seed.wrapping_add(tried);
            tried += 1;

            let Some(candidate) = propose_in_band(quota.band, seed, limits.budget) else {
                continue;
            };
            if seen.insert(candidate.puzzle.clues.canonical_bytes()) {
                produced += 1;
                candidates.push(candidate);
            } else {
                duplicates += 1;
            }
        }

        outcomes.push(BandOutcome {
            band_name: quota.band.name,
            wanted: quota.wanted,
            produced,
            seeds_tried: tried,
            first_seed,
            duplicates_dropped: duplicates,
            hit_seed_limit: produced < quota.wanted,
        });
    }

    Run {
        outcomes,
        candidates,
    }
}
