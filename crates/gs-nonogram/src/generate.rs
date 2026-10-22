//! Generation: a pattern, then clues derived from it, then a decision about whether
//! the result is worth verifying.
//!
//! # The generator is never an oracle
//!
//! Nothing here produces a [`gs_core::verification::Outcome`], and that is
//! structural: the only thing this module hands back is a [`Candidate`], which is
//! named for what it is. A candidate has passed the cheap filter in
//! [`crate::solve`]; it has not been verified. The publication gate is an SMT
//! solver reading [`crate::smtlib`], and it is the only thing whose answer counts.
//!
//! This is the point where the mistake would be easy. A generator that "knows" its
//! puzzle has one answer, because it built it that way, is the most natural shortcut
//! in this whole project and would quietly make the proof decorative. So the
//! pipeline is: build a pattern, **forget how it was built**, derive clues, and ask
//! the solver what those clues actually say. The pattern is thrown away; only the
//! clues survive into the candidate.
//!
//! # Why a cellular automaton
//!
//! A cellular automaton is a simple rule applied to a grid over and over, where each
//! square changes based on its neighbours. Start from random noise, smooth it a few
//! times, and the result has connected blobs instead of static. That matters for
//! Nonogram: noise produces clue lists like `1 1 1 1 1`, which are tedious rather
//! than interesting, while blobs produce longer runs that propagate.
//!
//! It is a generator and nothing more. It has no idea whether what it made has one
//! answer, and it is never asked.

use gs_core::state::Dims;

use crate::clues::{Clues, Pattern};
use crate::rng::Rng;
use crate::rules::Nonogram;
use crate::solve::{solve, Analysis, Budget, Features, Solved};

/// What kind of board to try for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub dims: Dims,
    /// Starting chance that a cell is filled, in parts per thousand. Parts per
    /// thousand rather than a fraction because the core forbids floating-point
    /// arithmetic and an integer threshold is exactly reproducible.
    pub fill_per_mille: u16,
    /// How many times to smooth. Zero leaves the noise alone, which is legal and
    /// mostly produces boring puzzles.
    pub smoothing_rounds: u8,
}

impl Shape {
    /// A starting point that produces blobby boards on small grids.
    ///
    /// These three numbers are a starting point, not a tuned result. Tuning them
    /// against a measured yield is work for the batch worker, and any number quoted
    /// as a yield has to come from a run rather than from here.
    #[must_use]
    pub fn blobby(dims: Dims) -> Self {
        Shape {
            dims,
            fill_per_mille: 500,
            smoothing_rounds: 3,
        }
    }
}

/// Build a pattern from a seed. Same seed and shape, same pattern, always.
#[must_use]
pub fn pattern(seed: u64, shape: Shape) -> Pattern {
    let mut rng = Rng::from_seed(seed);
    let cells = shape.dims.cell_count();
    let mut filled: Vec<bool> = (0..cells)
        .map(|_| rng.chance_per_mille(shape.fill_per_mille))
        .collect();

    for _ in 0..shape.smoothing_rounds {
        filled = smooth(&filled, shape.dims);
    }

    // `new` only rejects a length that disagrees with the dimensions, and this
    // vector is built from `cell_count`, so it cannot.
    Pattern::new(shape.dims, filled).expect("the vector is exactly cell_count long")
}

/// One round of the automaton.
///
/// A cell with five or more filled neighbours becomes filled; with two or fewer it
/// becomes empty; otherwise it stays as it is. Cells off the edge count as empty, so
/// boards tend to pull away from the border rather than smear along it.
///
/// Double-buffered on purpose: updating in place would make a cell's new value
/// depend on the scan order, which is the kind of accident that makes a seed stop
/// reproducing its board.
fn smooth(filled: &[bool], dims: Dims) -> Vec<bool> {
    let mut next = vec![false; filled.len()];
    for y in 0..dims.height {
        for x in 0..dims.width {
            let here = at(filled, dims, i32::from(x), i32::from(y));
            let neighbours = neighbour_count(filled, dims, x, y);
            next[index(dims, x, y)] = match neighbours {
                5..=8 => true,
                0..=2 => false,
                _ => here,
            };
        }
    }
    next
}

fn neighbour_count(filled: &[bool], dims: Dims, x: u16, y: u16) -> u8 {
    let mut count = 0u8;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if at(filled, dims, i32::from(x) + dx, i32::from(y) + dy) {
                count += 1;
            }
        }
    }
    count
}

/// Reading off the edge of the board gives empty rather than wrapping around. A
/// board that wrapped would have clues that do not match what a player sees.
fn at(filled: &[bool], dims: Dims, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 || x >= i32::from(dims.width) || y >= i32::from(dims.height) {
        return false;
    }
    filled[index(dims, x as u16, y as u16)]
}

fn index(dims: Dims, x: u16, y: u16) -> usize {
    dims.index_of(x, y).unwrap_or(0) as usize
}

/// Why a seed produced nothing worth verifying.
///
/// Every variant is a cheap rejection. None of them is a proof of anything: a
/// `Rejected::Undecided` in particular means the filter ran out of budget, not that
/// the puzzle is bad, and the only safe thing to do with it is move on to the next
/// seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// Almost blank or almost solid. Both make dull puzzles, and a fully blank board
    /// is trivially solvable in a way that would waste a verification.
    Lopsided { filled_per_mille: u32 },
    /// The clues have no answer at all. This should be impossible, since the clues
    /// were derived from a real board, so it is kept as a variant that would catch a
    /// bug in derivation rather than as an expected outcome.
    NoAnswer,
    /// The clues have more than one answer. The ordinary reason a seed is discarded.
    Ambiguous,
    /// The filter hit its budget. Not an answer; see [`Solved::Undecided`].
    Undecided,
}

/// A puzzle worth paying to verify.
///
/// "Candidate" is the whole point of the name. The filter says this looks like a
/// single-answer puzzle. Only a solver reading [`crate::smtlib`] can say so, and
/// until it has, nothing here may be published or called unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The seed that rebuilds this puzzle, for the validation report.
    pub seed: u64,
    pub shape: Shape,
    pub puzzle: Nonogram,
    /// The difficulty features measured while filtering. The platform carries these;
    /// what they mean for a player-facing level is each game's decision.
    pub features: Features,
}

/// Try one seed: pattern, clues, filter.
///
/// The pattern does not travel with the result. What the solver is later asked about
/// is the clues alone, exactly as a player would see them.
pub fn propose(seed: u64, shape: Shape, budget: Budget) -> Result<Candidate, Rejected> {
    let pattern = pattern(seed, shape);

    let filled_per_mille = filled_share_per_mille(&pattern);
    if !(LOPSIDED_LOW..=LOPSIDED_HIGH).contains(&filled_per_mille) {
        return Err(Rejected::Lopsided { filled_per_mille });
    }

    let puzzle = Nonogram::new(Clues::of(&pattern));
    let Analysis {
        solved, features, ..
    } = solve(&puzzle, budget);

    match solved {
        Solved::One => Ok(Candidate {
            seed,
            shape,
            puzzle,
            features,
        }),
        Solved::Several => Err(Rejected::Ambiguous),
        Solved::None => Err(Rejected::NoAnswer),
        Solved::Undecided => Err(Rejected::Undecided),
    }
}

/// How much of the board is filled, in parts per thousand.
fn filled_share_per_mille(pattern: &Pattern) -> u32 {
    let dims = pattern.dims();
    let cells = u32::try_from(dims.cell_count()).unwrap_or(u32::MAX);
    let mut filled = 0u32;
    for y in 0..dims.height {
        for x in 0..dims.width {
            if pattern.is_filled(x, y) {
                filled += 1;
            }
        }
    }
    filled.saturating_mul(1000).checked_div(cells).unwrap_or(0)
}

/// Boards outside this range of filled cells are discarded unseen.
const LOPSIDED_LOW: u32 = 200;
const LOPSIDED_HIGH: u32 = 800;

/// What a run of seeds produced. Counts only; no judgement.
///
/// This exists so that a yield is **measured** rather than asserted. No number in a
/// document or a comment in this repository may claim a yield unless a run like this
/// produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    pub proposed: u32,
    pub kept: u32,
    pub lopsided: u32,
    pub no_answer: u32,
    pub ambiguous: u32,
    pub undecided: u32,
}

impl Report {
    /// Candidates per thousand seeds tried. Integer, like every other ratio here.
    #[must_use]
    pub fn kept_per_mille(&self) -> u32 {
        if self.proposed == 0 {
            return 0;
        }
        self.kept.saturating_mul(1000) / self.proposed
    }

    /// Whether the counts add up. A report whose parts do not sum to the whole has
    /// lost a seed somewhere, and a lost seed is a bug rather than a rounding
    /// detail.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.kept + self.lopsided + self.no_answer + self.ambiguous + self.undecided
            == self.proposed
    }

    fn record(&mut self, outcome: &Result<Candidate, Rejected>) {
        self.proposed += 1;
        match outcome {
            Ok(_) => self.kept += 1,
            Err(Rejected::Lopsided { .. }) => self.lopsided += 1,
            Err(Rejected::NoAnswer) => self.no_answer += 1,
            Err(Rejected::Ambiguous) => self.ambiguous += 1,
            Err(Rejected::Undecided) => self.undecided += 1,
        }
    }
}

/// Run a block of seeds and report what came out.
///
/// Seeds are consecutive and the block is given explicitly, so a batch is
/// reproducible as a whole and not only seed by seed. There is no clock and no
/// randomness outside the seed: the same block always produces the same candidates
/// in the same order.
#[must_use]
pub fn batch(
    seeds: core::ops::Range<u64>,
    shape: Shape,
    budget: Budget,
) -> (Vec<Candidate>, Report) {
    let mut kept = Vec::new();
    let mut report = Report::default();
    for seed in seeds {
        let outcome = propose(seed, shape, budget);
        report.record(&outcome);
        if let Ok(candidate) = outcome {
            kept.push(candidate);
        }
    }
    (kept, report)
}
