//! Making Kakuro puzzles.
//!
//! # Why this is a different shape from both earlier families
//!
//! Nonogram draws a picture and works the clues out from it. Every clue is kept, and a
//! candidate is thrown away when it turns out to have a second answer.
//!
//! Slitherlink draws a loop, numbers every cell, and then does its real work by
//! **taking numbers away** for as long as the puzzle keeps exactly one answer.
//!
//! Kakuro can do neither. A run with no total would mean "any digits at all", which is
//! not a harder puzzle but a puzzle with nothing to work out, so **no total can be
//! removed**. And the totals are read off the answer, so they cannot be chosen either.
//! That leaves exactly one lever: **where the blocks go.**
//!
//! # What was measured first, and why the obvious method does not work
//!
//! The obvious method is the Nonogram one: propose a random block pattern, fill it in,
//! read the totals off it, and keep it if it has exactly one answer. Measured on
//! 2026-10-24, that keeps almost nothing, and gets worse with size — at a quarter
//! blocks, roughly 20 in 100 at 4 by 4, 9 in 100 at 5 by 5, and **1 in 100 at 7 by 7**.
//! Worse, the few it keeps are the boards nobody would want: cranking the block density
//! up to make uniqueness likely gives boards where half the squares sit in a run of one
//! square, and a run of one names its digit outright.
//!
//! Three things were tried and did not help. Capping run length at five made clean
//! layouts almost impossible to find by chance and did not improve uniqueness when one
//! was found. Biasing the fill towards extreme digits, so totals would have fewer
//! combinations, changed nothing measurable. Trying forty different fillings of one
//! clean layout produced **no** unique puzzle at all, at any size tried.
//!
//! That last result is the one that explains the rest. A layout with no run of one
//! square is not merely unlucky, it is **under-determined**. Each run gives one
//! adding-up equation, so a layout has about as many equations as it has runs and as
//! many unknowns as it has white squares. Random quarter-block layouts have long runs,
//! which means few runs per square — measured at 0.5 to 0.7 runs per white square — and
//! a system with half as many equations as unknowns has room for many answers whatever
//! digits you put in it. The second answers found were not near misses: on a 5 by 5
//! with 23 white squares, 18 of them held a different digit.
//!
//! So no amount of refilling helps, because the problem is the shape of the board.
//!
//! # What this module does instead
//!
//! Fix the shape, using the ambiguity itself as the guide.
//!
//! 1. Propose a sparse block pattern and fill it in.
//! 2. Read the totals off the filling and ask our own solver for an answer that is
//!    **not** the one we started from ([`crate::solve::second_answer`]).
//! 3. If there is one, the squares where the two answers differ are exactly where the
//!    board has slack. **Turn one of them black.** That removes an unknown and adds a
//!    run boundary, so it adds equations and takes away a degree of freedom — the thing
//!    the diagnosis above says is missing.
//! 4. Repeat until the board has exactly one answer.
//!
//! Measured on 2026-10-24: **40 of 40 seeds** produce a puzzle at 5 by 5 and 7 by 7,
//! and 34 of 40 at 9 by 9, in about five, twelve and twenty rounds of repair. See
//! `examples/sweep.rs`.
//!
//! This also explains, after the fact, why real Kakuro grids are so black. Density is
//! not a style choice in this puzzle. It is what buys uniqueness.
//!
//! # Which square to black out, and the quality rule
//!
//! Any differing square would do for correctness, and the first one found was tried
//! first. It works and gives bad puzzles: about **59 in 100** white squares end up in a
//! run of one, which is to say the player is given most of the board.
//!
//! So the choice is greedy on quality instead: among the differing squares, take the
//! one whose blocking creates the fewest runs of a single square, breaking ties towards
//! keeping more white squares. Same keep rate, and the giveaway share falls to about
//! **38 in 100**. That is still high, and it is recorded as the number to improve rather
//! than hidden: see [`Features::given_away_squares`] and the note in ADR 0019.
//!
//! The cap in [`Shape::max_given_away_per_mille`] is a final gate, applied to the
//! finished layout. It lives in the generator and not in `rules.rs` on purpose: a board
//! that is mostly giveaways is a perfectly legal Kakuro, so the rules must accept it,
//! and only the thing choosing what to publish may decline it.

use gs_core::rng::Rng;
use gs_core::state::{Cell, State};

use crate::layout::{Layout, LayoutError, WhiteId};
use crate::rules::{Kakuro, PuzzleError};
use crate::solve::{fill, second_answer, solve, Budget, Features, Solved};
use crate::totals::{Totals, TotalsError};

/// Which version of the making-puzzles process this build implements.
///
/// Bumped whenever generation or the fast solver's deductions change. The difficulty
/// numbers in a validation report are measurements **against this solver**, so two
/// reports from different versions are not comparable, and a reader must be able to
/// tell rather than assume.
pub const GENERATOR_VERSION: u32 = 1;

/// How many squares one seed may black out before giving up.
///
/// Each round removes one white square, so this also bounds how much of the board the
/// repair may eat. Measured need is about five rounds at 5 by 5 and twenty at 9 by 9,
/// so this is generous; it exists because nothing in this platform searches without a
/// limit.
pub const REPAIR_ROUNDS: u32 = 80;

/// The size and taste of the puzzles to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub width: u16,
    pub height: u16,
    /// How often a square starts out as a block, in tenths of a percent.
    ///
    /// Only a starting point: repair adds more. Start sparse — a dense start wastes
    /// the repair's judgement about *which* squares matter.
    pub block_per_mille: u16,
    /// The most white squares that may sit in a run of one square, in tenths of a
    /// percent of the white squares. A run of one names its digit, so these are the
    /// board the player is given rather than works out.
    pub max_given_away_per_mille: u32,
}

impl Shape {
    /// A middling board of this size.
    ///
    /// The numbers are starting points for measurement, not claims about what plays
    /// well. Difficulty levels are each game's own business, settled on 2026-10-22, and
    /// Kakuro's will be decided from measurements of real puzzles rather than guessed
    /// at here. The giveaway cap is set a little above what the measured method
    /// achieves (about 380 in 1000), so it refuses the bad tail without refusing
    /// everything.
    #[must_use]
    pub fn middling(width: u16, height: u16) -> Self {
        Shape {
            width,
            height,
            block_per_mille: 200,
            max_given_away_per_mille: 500,
        }
    }
}

/// Why a candidate was thrown away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// The starting block pattern was not a legal layout.
    NoStartingPattern,
    /// Repair ran out of rounds without reaching a single answer.
    StillAmbiguous {
        rounds: u32,
    },
    /// Repair reached a single answer, and the result gives too much of the board
    /// away. A quality refusal, not a correctness one.
    TooMuchGivenAway {
        per_mille: u32,
        cap: u32,
    },
    /// Our own solver could not decide, or found no answer at all, so there is nothing
    /// to repair towards.
    Undecidable,
    /// The board size itself cannot be laid out.
    Unlayoutable(LayoutError),
    /// A filled board implied a total outside its run's reachable range, or the totals
    /// did not fit the runs. Neither can happen — the digits come from the runs
    /// themselves — and both are reported rather than unwrapped, because a generator
    /// that panicked on its own arithmetic would take a pool-filling run down with it.
    Untotalable(TotalsError),
    Unpuzzleable(PuzzleError),
}

/// A puzzle that passed every cheap check, and the evidence for it.
///
/// Not publishable. A candidate becomes publishable only once a real solver has
/// answered both questions and the report has a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub seed: u64,
    /// How many squares the repair blacked out. Recorded because it is this family's
    /// equivalent of Slitherlink's removal count: the measure of how much work making
    /// this particular puzzle took.
    pub repairs: u32,
    pub puzzle: Kakuro,
    /// The board the totals were read off. Our own solver's witness, not a proof.
    pub witness: State,
    pub features: Features,
    pub generator_version: u32,
}

/// Propose a starting block pattern.
///
/// Sparse on purpose; see [`Shape::block_per_mille`]. Returns `None` when the pattern
/// is not a legal layout at all — usually a run longer than nine, which repair would
/// fix anyway but which [`Layout`] refuses to represent.
#[must_use]
pub fn starting_pattern(seed: u64, shape: Shape) -> Option<Layout> {
    let mut rng = Rng::from_seed(seed);
    let squares = usize::from(shape.width) * usize::from(shape.height);
    let blocks: Vec<bool> = (0..squares)
        .map(|_| rng.chance_per_mille(shape.block_per_mille))
        .collect();
    Layout::new(shape.width, shape.height, blocks).ok()
}

/// White squares sitting in a run of a single square: the part of the board the player
/// is given rather than works out.
#[must_use]
pub fn given_away(layout: &Layout) -> u32 {
    u32::try_from(
        layout
            .whites()
            .filter(|white| {
                layout.runs_of(*white).is_some_and(|(across, down)| {
                    layout.run(across).is_some_and(|r| r.len() == 1)
                        || layout.run(down).is_some_and(|r| r.len() == 1)
                })
            })
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// That share, in tenths of a percent, so the number stays integer-only like every
/// other measurement here.
#[must_use]
pub fn given_away_per_mille(layout: &Layout) -> u32 {
    let whites = u32::try_from(layout.white_count()).unwrap_or(u32::MAX);
    if whites == 0 {
        return 0;
    }
    given_away(layout).saturating_mul(1000) / whites
}

/// Make one puzzle from one seed, or say why not.
pub fn propose(seed: u64, shape: Shape, budget: Budget) -> Result<Candidate, Rejected> {
    let mut layout = starting_pattern(seed, shape).ok_or(Rejected::NoStartingPattern)?;

    for round in 0..REPAIR_ROUNDS {
        let digits = fill(&layout, seed, budget).ok_or(Rejected::Undecidable)?;
        let totals = Totals::of(&layout, &|white: WhiteId| {
            digits.get(usize::try_from(white.0).ok()?).copied()
        })
        .ok_or(Rejected::Undecidable)?;
        let totals =
            Totals::new(&layout, totals.as_slice().to_vec()).map_err(Rejected::Untotalable)?;
        let puzzle = Kakuro::new(layout.clone(), totals).map_err(Rejected::Unpuzzleable)?;
        let ours = board_of(&layout, &digits);

        let analysis = solve(&puzzle, budget);
        match analysis.solved {
            Solved::One => {
                // The quality gate, applied to the finished board.
                let per_mille = given_away_per_mille(&layout);
                if per_mille > shape.max_given_away_per_mille {
                    return Err(Rejected::TooMuchGivenAway {
                        per_mille,
                        cap: shape.max_given_away_per_mille,
                    });
                }
                let mut features = analysis.features;
                features.given_away_squares = given_away(&layout);
                return Ok(Candidate {
                    seed,
                    repairs: round,
                    puzzle,
                    witness: analysis.witness.unwrap_or(ours),
                    features,
                    generator_version: GENERATOR_VERSION,
                });
            }
            Solved::Several => {
                layout = repair(&layout, &puzzle, &ours, budget).ok_or(Rejected::Undecidable)?;
            }
            // No answer at all cannot happen — the board was filled in before the
            // totals were read off it — and an exhausted budget leaves nothing to
            // repair towards. Both stop, rather than blacking out a square chosen for
            // no reason.
            Solved::None | Solved::Undecided => return Err(Rejected::Undecidable),
        }
    }
    Err(Rejected::StillAmbiguous {
        rounds: REPAIR_ROUNDS,
    })
}

/// Black out the square that costs the least, among those where a second answer
/// differs from ours.
///
/// The differing squares are where the board has slack, so blacking out any of them
/// removes some of it. The choice between them is a quality judgement: the fewest new
/// runs of one square, then the most white squares kept. Both tie-breaks are fixed, so
/// one seed always gives one puzzle.
#[must_use]
fn repair(layout: &Layout, puzzle: &Kakuro, ours: &State, budget: Budget) -> Option<Layout> {
    let other = second_answer(puzzle, ours, budget)?;
    let width = layout.width();
    let height = layout.height();
    layout
        .whites()
        .filter(|white| puzzle.digit(&other, *white) != puzzle.digit(ours, *white))
        .filter_map(|white| {
            let square = layout.square_of(white)?;
            let mut blocks: Vec<bool> = layout.squares().map(|s| layout.is_block(s)).collect();
            *blocks.get_mut(usize::try_from(square.0).ok()?)? = true;
            Layout::new(width, height, blocks).ok()
        })
        .min_by_key(|candidate| {
            let singles = candidate.runs().filter(|(_, r)| r.len() == 1).count();
            (singles, usize::MAX - candidate.white_count())
        })
}

/// The board a list of digits describes, in white order.
fn board_of(layout: &Layout, digits: &[i16]) -> State {
    let mut state = State::blank(layout.state_dims());
    for (index, digit) in digits.iter().enumerate() {
        if let Ok(at) = u32::try_from(index) {
            let _ = state.set(at, Cell(*digit));
        }
    }
    state
}

/// What a batch did. The answer to the plan's question "how often does generation
/// produce a usable puzzle", for this family.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub seeds: u32,
    pub kept: u32,
    pub still_ambiguous: u32,
    pub too_much_given_away: u32,
    pub undecidable: u32,
    pub other: u32,
}

impl Report {
    /// Kept per thousand seeds tried.
    #[must_use]
    pub fn kept_per_mille(&self) -> u32 {
        if self.seeds == 0 {
            return 0;
        }
        self.kept.saturating_mul(1000) / self.seeds
    }

    /// Whether the counts add up to the seeds tried.
    ///
    /// Worth asserting in tests rather than trusting: a report whose parts do not sum
    /// to its whole is measuring something other than what it says, and a keep rate is
    /// the number every cost estimate in `content-delivery.md` rests on.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.kept + self.still_ambiguous + self.too_much_given_away + self.undecidable + self.other
            == self.seeds
    }
}

/// Try a range of seeds, and report what happened.
pub fn batch(
    seeds: impl Iterator<Item = u64>,
    shape: Shape,
    budget: Budget,
) -> (Vec<Candidate>, Report) {
    let mut kept = Vec::new();
    let mut report = Report::default();
    for seed in seeds {
        report.seeds = report.seeds.saturating_add(1);
        match propose(seed, shape, budget) {
            Ok(candidate) => {
                report.kept = report.kept.saturating_add(1);
                kept.push(candidate);
            }
            Err(Rejected::StillAmbiguous { .. }) => {
                report.still_ambiguous = report.still_ambiguous.saturating_add(1);
            }
            Err(Rejected::TooMuchGivenAway { .. }) => {
                report.too_much_given_away = report.too_much_given_away.saturating_add(1);
            }
            Err(Rejected::Undecidable) => {
                report.undecidable = report.undecidable.saturating_add(1);
            }
            Err(_) => report.other = report.other.saturating_add(1),
        }
    }
    (kept, report)
}
