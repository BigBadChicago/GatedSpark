//! Nonogram's own difficulty levels.
//!
//! # Why this lives here and not in the engine
//!
//! The user settled this on 2026-10-22: **how many difficulty levels there are, and
//! where the boundaries fall, is decided by each game, not by the platform.** So
//! `gs-core` has no idea what a band is, measures nothing about difficulty, and has
//! no field for one. This file is Nonogram answering the question for itself. A
//! second family will answer it differently, and that is the point.
//!
//! # What a level is graded on, and why
//!
//! **Propagation rounds**: how many times the solver has to sweep every row and
//! column before nothing more is forced. Among puzzles solvable without guessing,
//! that is the only measured quantity in [`Features`] that separates one from
//! another.
//!
//! Two measurements from 2026-10-22 decided this, both in
//! `design/nonogram-difficulty-measurements.md`:
//!
//! * The forced-move ratio is **exactly 1000 for every puzzle needing no guessing**,
//!   because it is counted at the first fixpoint and such a puzzle is finished there.
//!   It grades nothing here. An earlier reading of the sweep got this wrong.
//! * Puzzles that *do* need guessing are rare — at most about 7 in 100 — so a level
//!   defined as "needs guessing" would cost 8 to 25 times more generation per
//!   accepted puzzle.
//!
//! So a puzzle that needs a guess is in **no** band: [`band_of`] returns `None` for
//! it. That is a deliberate exclusion rather than an oversight. A classic Nonogram is
//! solvable by reasoning, and a puzzle that forces a guess feels unfair whatever a
//! solver says about it. Whether to add a rare guess-required top level is a decision
//! left open rather than taken quietly.
//!
//! # These thresholds are provisional, and say so
//!
//! Every window below was chosen because a measured run put puzzles in it, not
//! because the number felt right. What cannot be checked yet is whether a human
//! agrees: no play data exists. A threshold calibrated against output the system
//! cannot reach fails silently as a shuffle, so the honest position is that these are
//! reachable and ordered, and that the mapping to the words a player reads is an
//! informed guess to revisit once real players exist.

use gs_core::state::Dims;

use crate::generate::{propose, Candidate, Shape};
use crate::solve::{Budget, Features};

/// One difficulty level: a board shape, and the range of propagation rounds that
/// counts as this level on that shape.
///
/// The shape travels with the band on purpose. A band a generator cannot aim at is
/// a label with no production behind it, and the batch worker needs to know what to
/// build in order to fill a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// What a player would see. Plain words, not a number.
    pub name: &'static str,
    /// What to generate to fill this band.
    pub shape: Shape,
    /// Fewest propagation rounds that still counts as this band, inclusive.
    pub min_rounds: u32,
    /// Most propagation rounds that still counts, inclusive.
    pub max_rounds: u32,
}

impl Band {
    /// Whether this band claims a puzzle of the given size with the given features.
    ///
    /// Three things must hold, and the first is the one that matters: a puzzle
    /// needing a guess is claimed by nobody.
    #[must_use]
    pub fn claims(&self, dims: Dims, features: &Features) -> bool {
        features.line_solvable()
            && dims == self.shape.dims
            && (self.min_rounds..=self.max_rounds).contains(&features.propagation_rounds)
    }
}

/// Nonogram's levels, easiest first.
///
/// Each window was measured over 400 seeds at its own shape on 2026-10-22, and each
/// had puzzles in it. The counts are not recorded here: a number in a comment that
/// nothing regenerates is the thing this project does not do. Re-measure with
/// `cargo run --release --example sweep`.
pub const BANDS: &[Band] = &[
    Band {
        name: "gentle",
        shape: Shape {
            dims: Dims {
                width: 8,
                height: 8,
            },
            fill_per_mille: 600,
            smoothing_rounds: 3,
        },
        min_rounds: 2,
        max_rounds: 3,
    },
    Band {
        name: "steady",
        shape: Shape {
            dims: Dims {
                width: 10,
                height: 10,
            },
            fill_per_mille: 500,
            smoothing_rounds: 3,
        },
        min_rounds: 3,
        max_rounds: 5,
    },
    Band {
        name: "tricky",
        shape: Shape {
            dims: Dims {
                width: 12,
                height: 12,
            },
            fill_per_mille: 500,
            smoothing_rounds: 3,
        },
        min_rounds: 5,
        max_rounds: 8,
    },
    Band {
        name: "stern",
        shape: Shape {
            dims: Dims {
                width: 15,
                height: 15,
            },
            fill_per_mille: 450,
            smoothing_rounds: 2,
        },
        min_rounds: 7,
        // Open-ended at the top. A puzzle that takes more sweeps than this is still
        // the hardest thing on offer; there is nowhere above it to put it, and
        // discarding it for being too hard would be throwing away the scarcest
        // output the generator has.
        max_rounds: u32::MAX,
    },
];

/// Which level a puzzle belongs to, if any.
///
/// `None` is a real answer and the common one in two cases: the puzzle needed a
/// guess, or its round count falls outside every window for its size. Such puzzles
/// are discarded rather than squeezed into the nearest band, because a band that
/// quietly absorbs whatever is left over stops describing anything.
#[must_use]
pub fn band_of(dims: Dims, features: &Features) -> Option<&'static Band> {
    BANDS.iter().find(|band| band.claims(dims, features))
}

/// Try one seed for one level. `None` if that seed did not produce a puzzle this
/// level claims.
///
/// This is the call a batch worker makes: it names a level, and gets back either a
/// candidate for that level or nothing. It never returns a candidate for a different
/// level, so a worker filling one band cannot silently fill another.
#[must_use]
pub fn propose_in_band(band: &Band, seed: u64, budget: Budget) -> Option<Candidate> {
    let candidate = propose(seed, band.shape, budget).ok()?;
    band.claims(band.shape.dims, &candidate.features)
        .then_some(candidate)
}
