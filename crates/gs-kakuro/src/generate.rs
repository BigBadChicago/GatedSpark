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

use crate::construct::{
    corners_of_swap_rectangles, gives_nothing_away, swap_rectangles, weave, NotWoven, Weave,
};
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
///
/// Two, as of ADR 0021: boards are now built rather than drawn at random, so a seed means
/// a different puzzle than it did under version one, and the measurements are over a
/// different kind of board.
pub const GENERATOR_VERSION: u32 = 2;

/// How many rounds of repair one seed gets, at the least.
///
/// Each round of the older method removes one white square, so this also bounds how much
/// of the board that repair may eat. Measured need is about five rounds at 5 by 5 and
/// twenty at 9 by 9, so this is generous; it exists because nothing in this platform
/// searches without a limit.
pub const REPAIR_ROUNDS: u32 = 80;

/// How many rounds of repair a board of this many white squares gets.
///
/// The built method needs more rounds than the older one, and the number is not a guess.
/// Its digit lever only ever moves a digit farther from five, and a digit cannot be more
/// than four away, so a board of `whites` squares offers at most `4 * whites` such moves
/// before there are none left. A board allowed fewer rounds than that would be refused
/// for running out of rounds when what actually happened is that it ran out of moves,
/// and those two want different answers from the caller.
///
/// The spare sixteen covers the blacking-out rounds, which do not move a digit at all.
#[must_use]
pub fn rounds_for(whites: usize) -> u32 {
    let whites = u32::try_from(whites).unwrap_or(u32::MAX);
    REPAIR_ROUNDS.max(whites.saturating_mul(4).saturating_add(16))
}

/// Where the board's shape comes from.
///
/// Both are kept, and the random one is not dead code: `tests/generation.rs` measures
/// the two against each other, so the claim that building the board is worth the work
/// cannot quietly stop being true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutFrom {
    /// A random block pattern, repaired into uniqueness with no fairness rule. The
    /// method ADR 0019 measured, which gives about 38 in 100 squares away.
    RandomPattern,
    /// A board built so that no run is one square long, repaired only by moves that
    /// keep it that way. See [`crate::construct`] and ADR 0021.
    Construction(Weave),
}

/// The size and taste of the puzzles to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub width: u16,
    pub height: u16,
    /// Where the shape comes from.
    pub layout_from: LayoutFrom,
    /// How often a square starts out as a block, in tenths of a percent.
    ///
    /// Only a starting point: repair adds more. Start sparse — a dense start wastes
    /// the repair's judgement about *which* squares matter.
    ///
    /// Used only by [`LayoutFrom::RandomPattern`]. A built board's density comes from
    /// its run limits instead: see [`Weave::block_first_per_mille`].
    pub block_per_mille: u16,
    /// The most white squares that may sit in a run of one square, in tenths of a
    /// percent of the white squares. A run of one names its digit, so these are the
    /// board the player is given rather than works out.
    pub max_given_away_per_mille: u32,
}

impl Shape {
    /// A middling board of this size, built rather than drawn at random.
    ///
    /// The numbers are starting points for measurement, not claims about what plays
    /// well. Difficulty levels are each game's own business, settled on 2026-10-22, and
    /// Kakuro's will be decided from measurements of real puzzles rather than guessed
    /// at here.
    ///
    /// The giveaway cap is **zero**, which is not a tightened guess but the property a
    /// built board has by construction. It is left in place rather than removed, because
    /// a cap that can never fire is the cheapest possible check that the construction
    /// and the repair are both still doing what they claim.
    #[must_use]
    pub fn middling(width: u16, height: u16) -> Self {
        Shape {
            width,
            height,
            layout_from: LayoutFrom::Construction(Weave::middling()),
            block_per_mille: 200,
            max_given_away_per_mille: 0,
        }
    }

    /// The older method: a random starting pattern, and a repair with no fairness rule.
    ///
    /// Kept so the two can be measured against each other. The giveaway cap is the one
    /// ADR 0019 set, a little above what that method achieves.
    #[must_use]
    pub fn random_pattern(width: u16, height: u16) -> Self {
        Shape {
            width,
            height,
            layout_from: LayoutFrom::RandomPattern,
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
    /// No board of this size obeys the run limits asked for.
    NoBoardBuilt(NotWoven),
    /// Every built board for this seed still had more than one answer, and the repair
    /// had no move left that would keep the board fair. A refusal particular to the
    /// built method: the random one can always black out another square, because it
    /// does not mind what that does to the runs.
    NoFairRepair {
        rounds: u32,
    },
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

/// How many of a filling's digits are ones or nines, in tenths of a percent.
///
/// The price of [`pin_the_swaps`], kept where it can be seen. See
/// [`Features::digits_at_the_ends_per_mille`].
#[must_use]
pub fn digits_at_the_ends_per_mille(digits: &[i16]) -> u32 {
    let all = u32::try_from(digits.len()).unwrap_or(u32::MAX);
    if all == 0 {
        return 0;
    }
    let ends =
        u32::try_from(digits.iter().filter(|d| **d == 1 || **d == 9).count()).unwrap_or(u32::MAX);
    ends.saturating_mul(1000) / all
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
    match shape.layout_from {
        LayoutFrom::RandomPattern => {
            let layout = starting_pattern(seed, shape).ok_or(Rejected::NoStartingPattern)?;
            refine(layout, seed, shape, budget, Fairness::DoNotMind)
        }
        LayoutFrom::Construction(asked) => {
            // Escalation, and the reason it is the right shape of answer.
            //
            // A board with too many answers is under-determined, and the lever for that
            // is shorter runs: each run is one adding-up rule, so cutting the board into
            // shorter pieces adds rules over the same squares. So when the fair repair
            // runs out of moves on this board, the next thing tried is not another
            // random board of the same kind — it is a board of the same size whose runs
            // are one square shorter, which is a board with strictly more rules.
            //
            // This is also what a printed Kakuro looks like as it gets harder to make:
            // blacker, with shorter runs. Density is not a style choice in this puzzle.
            let mut weave_now = Some(asked);
            let mut last = Rejected::NoFairRepair { rounds: 0 };
            while let Some(this_weave) = weave_now {
                match weave(shape.width, shape.height, seed, this_weave) {
                    Ok(layout) => match refine(layout, seed, shape, budget, Fairness::Require) {
                        Ok(candidate) => return Ok(candidate),
                        Err(
                            why @ (Rejected::NoFairRepair { .. } | Rejected::StillAmbiguous { .. }),
                        ) => {
                            last = why;
                        }
                        Err(why) => return Err(why),
                    },
                    Err(why) => last = Rejected::NoBoardBuilt(why),
                }
                weave_now = this_weave.with_shorter_runs();
            }
            Err(last)
        }
    }
}

/// Whether the repair has to keep the board fair.
///
/// Two values rather than a boolean, because at the call sites `true` would not say
/// which way round it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fairness {
    /// Any differing square may be blacked out. The older method.
    DoNotMind,
    /// Only a square whose blacking leaves no run of one square.
    Require,
}

/// Fill the board in, read the totals off it, and repair the ambiguity out of it.
///
/// Two levers, and which ones are allowed depends on `fairness`:
///
/// * **Change a digit** ([`nudge`]), which leaves the board's shape alone. Only used
///   when fairness is required, because it is the lever that was added for it.
/// * **Black out a square** ([`black_out`]), which changes the shape.
///
/// The digit is tried first and the square second, because blacking out a square takes
/// a square away from the player and changing a digit takes nothing away at all.
fn refine(
    mut layout: Layout,
    seed: u64,
    shape: Shape,
    budget: Budget,
    fairness: Fairness,
) -> Result<Candidate, Rejected> {
    // Carried between rounds, because a changed digit keeps the board and so keeps the
    // white numbering. A changed board does not, which is why blacking out a square
    // clears this and the next round fills the new board in from scratch.
    let mut carried: Option<Vec<i16>> = None;
    let rounds = match fairness {
        Fairness::DoNotMind => REPAIR_ROUNDS,
        Fairness::Require => rounds_for(layout.white_count()),
    };

    for round in 0..rounds {
        let digits = match carried.take() {
            Some(digits) => digits,
            None => {
                let filled = fill(&layout, seed, budget).ok_or(Rejected::Undecidable)?;
                // Pin the rectangles before asking anything. Cheaper than a solver call
                // and it settles the ambiguity this family actually suffers from, so the
                // solver is not asked the same easy question dozens of times over.
                match fairness {
                    Fairness::Require => {
                        let pinned = pin_the_swaps(&layout, &filled);
                        pinned.unwrap_or(filled)
                    }
                    Fairness::DoNotMind => filled,
                }
            }
        };
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
                features.swap_rectangles = swap_rectangles(&layout);
                features.digits_at_the_ends_per_mille = digits_at_the_ends_per_mille(&digits);
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
                let Some(other) = second_answer(&puzzle, &ours, budget) else {
                    return Err(Rejected::Undecidable);
                };
                let nudged = match fairness {
                    Fairness::Require => nudge(&puzzle, &digits, &other),
                    // The older method is left exactly as it was measured, so the
                    // comparison between the two stays honest.
                    Fairness::DoNotMind => None,
                };
                match nudged {
                    Some(next) => carried = Some(next),
                    None => match black_out(&layout, &puzzle, &ours, &other, fairness) {
                        Some(next) => layout = next,
                        None => return Err(Rejected::NoFairRepair { rounds: round }),
                    },
                }
            }
            // No answer at all cannot happen — the board was filled in before the
            // totals were read off it — and an exhausted budget leaves nothing to
            // repair towards. Both stop, rather than blacking out a square chosen for
            // no reason.
            Solved::None | Solved::Undecided => return Err(Rejected::Undecidable),
        }
    }
    Err(Rejected::StillAmbiguous { rounds })
}

/// Black out the square that costs the least, among those where a second answer
/// differs from ours.
///
/// The differing squares are where the board has slack, so blacking out any of them
/// removes some of it.
///
/// # What a fair move is, and why so few squares offer one
///
/// Under [`Fairness::Require`] a move is only allowed if the board it leaves has no run
/// of one square. That is a far smaller set of moves than it sounds, and the arithmetic
/// is worth writing down, because it is why this lever alone was not enough and the
/// digit lever had to be added.
///
/// Blacking out a square sits it inside two runs and cuts each of them in two. A run of
/// `len` squares cut at position `at` leaves pieces of `at` and `len - 1 - at`. A piece
/// of nothing is fine, since it is not a run; any other piece has to be two or more. So:
///
/// * Blacking an **end** square trims the run to `len - 1`, which is fair when the run
///   was three squares or longer.
/// * Blacking an **inside** square splits it, which is fair only when the run was five
///   squares or longer.
///
/// And both of the square's runs have to come out fair, not just one. On a board whose
/// runs are two or three squares long there is often no such square anywhere, which is
/// what the first measurement of this method ran into: it kept 9 of 40 at 4 by 4 and
/// **none at all** at 7 by 7, refused for want of a fair move.
#[must_use]
fn black_out(
    layout: &Layout,
    puzzle: &Kakuro,
    ours: &State,
    other: &State,
    fairness: Fairness,
) -> Option<Layout> {
    let width = layout.width();
    let height = layout.height();
    layout
        .whites()
        .filter(|white| puzzle.digit(other, *white) != puzzle.digit(ours, *white))
        .filter_map(|white| {
            let square = layout.square_of(white)?;
            let mut blocks: Vec<bool> = layout.squares().map(|s| layout.is_block(s)).collect();
            *blocks.get_mut(usize::try_from(square.0).ok()?)? = true;
            Layout::new(width, height, blocks).ok()
        })
        .filter(|candidate| match fairness {
            Fairness::DoNotMind => true,
            Fairness::Require => gives_nothing_away(candidate),
        })
        // The choice between the allowed moves is a quality judgement, and both
        // tie-breaks are fixed so that one seed always gives one puzzle. Fewest runs of
        // one square first: under `Require` every candidate scores zero there, so the
        // real tie-break is the second, keeping as much of the board to play as
        // possible.
        .min_by_key(|candidate| {
            let singles = candidate.runs().filter(|(_, r)| r.len() == 1).count();
            (singles, usize::MAX - candidate.white_count())
        })
}

/// Pin the corners of every swap rectangle, the way a published Kakuro does.
///
/// # What a swap is, and when it is dead
///
/// `crate::construct` has the shape of it: four white squares at the corners of a
/// rectangle, two sharing an across run and two sharing a down run. Add `k` to two
/// opposite corners and take `k` from the other two, and every run keeps its total.
///
/// Call the corners `a b` over `c d`. There are two directions:
///
/// * up the `a`/`d` diagonal: `a + k`, `d + k`, `b - k`, `c - k`
/// * up the `b`/`c` diagonal: `b + k`, `c + k`, `a - k`, `d - k`
///
/// A direction is dead when any one of its four moves has no room at all — a nine cannot
/// go up, a one cannot go down — because then no `k` works, not merely `k = 1`. So the
/// rectangle is dead when **both** directions have a corner with no room.
///
/// # Which corners to pin
///
/// One diagonal, set to a one and a nine. Take `a = 9` and `d = 1`: the first direction
/// is dead because `a` cannot go up, and the second because `d` cannot go down. The same
/// works for `a = 1, d = 9`, and for either way round on the other diagonal, which gives
/// four ways to try.
///
/// It has to be a **diagonal** pair. The two corners in a row share an across run and the
/// two in a column share a down run, and a run holds no digit twice, so no run-sharing
/// pair can both be a nine. A diagonal pair shares no run, so it can.
///
/// This is the one piece of this method that is ordinary Kakuro craft rather than
/// something the measurements turned up: in a published puzzle what stops a swap is
/// always a digit at the end of its range, which is why real Kakuro are so full of ones
/// and nines. [`Features::digits_at_the_ends`] records how many this leaves, because a
/// board of nothing but ones and nines would be a dull puzzle, and a number nobody can
/// see does not get improved.
///
/// Returns the changed filling, or `None` when nothing could be pinned — every rectangle
/// already dead, or no legal digit left to pin with.
#[must_use]
fn pin_the_swaps(layout: &Layout, ours: &[i16]) -> Option<Vec<i16>> {
    let mut digits = ours.to_vec();
    let mut changed = false;

    // Fixed order over a fixed list, so one seed gives one puzzle.
    for pass in 0..PINNING_PASSES {
        let mut pinned_any = false;
        for [a, b, c, d] in corners_of_swap_rectangles(layout) {
            if swap_is_dead(&digits, [a, b, c, d]) {
                continue;
            }
            // The four ways to kill both directions at once, in a fixed order.
            for (high, low) in [(a, d), (d, a), (b, c), (c, b)] {
                if set_digit(layout, &mut digits, high, 9) && set_digit(layout, &mut digits, low, 1)
                {
                    pinned_any = true;
                    changed = true;
                    break;
                }
            }
        }
        if !pinned_any {
            break;
        }
        // A later rectangle can undo an earlier one's pinning, since the corners overlap,
        // so the passes repeat until a pass changes nothing. Bounded, because overlapping
        // rectangles can pull against each other for ever.
        let _ = pass;
    }

    changed.then_some(digits)
}

/// How many times [`pin_the_swaps`] goes round the rectangles.
///
/// Rectangles share corners, so pinning one can unpin another, and the passes repeat
/// until a pass changes nothing. The limit is here because that can pull back and forth
/// for ever on a crowded board.
pub const PINNING_PASSES: u32 = 6;

/// Whether this rectangle's swap has no room left in either direction.
#[must_use]
fn swap_is_dead(digits: &[i16], corners: [WhiteId; 4]) -> bool {
    let [a, b, c, d] = corners;
    let at = |white: WhiteId| -> i16 {
        usize::try_from(white.0)
            .ok()
            .and_then(|index| digits.get(index))
            .copied()
            .unwrap_or(5)
    };
    // Room to move, in each of the two directions. Zero anywhere kills that direction
    // for every step size, not just for one.
    let up_the_a_d = [9 - at(a), 9 - at(d), at(b) - 1, at(c) - 1];
    let up_the_b_c = [9 - at(b), 9 - at(c), at(a) - 1, at(d) - 1];
    up_the_a_d.iter().any(|room| *room <= 0) && up_the_b_c.iter().any(|room| *room <= 0)
}

/// Write `digit` into `white`, if no other square in either of its runs already holds it.
///
/// True when the digit was written, which includes the case where it was already there.
#[must_use]
fn set_digit(layout: &Layout, digits: &mut [i16], white: WhiteId, digit: i16) -> bool {
    let Ok(index) = usize::try_from(white.0) else {
        return false;
    };
    if digits.get(index) == Some(&digit) {
        return true;
    }
    if let Some((across, down)) = layout.runs_of(white) {
        for run in [across, down] {
            let Some(run) = layout.run(run) else {
                return false;
            };
            for other in &run.squares {
                if *other == white {
                    continue;
                }
                let Ok(other_index) = usize::try_from(other.0) else {
                    return false;
                };
                if digits.get(other_index) == Some(&digit) {
                    return false;
                }
            }
        }
    }
    match digits.get_mut(index) {
        Some(slot) => {
            *slot = digit;
            true
        }
        None => false,
    }
}

/// Change one digit, so that the second answer stops being an answer.
///
/// # Why there is a second lever at all
///
/// Blacking out a square is the only lever ADR 0019 had, and on a board with no run of
/// one square it is nearly always unavailable, for the arithmetic in [`black_out`].
/// Something else had to give, and the something is the digits.
///
/// # Why changing any digit works
///
/// The totals are read off our filling. Change the digit in one square and the totals of
/// the two runs through it change with it. The second answer's own sums did not change,
/// and they matched the **old** totals, so it cannot match the new ones: it stops being
/// an answer the moment the digit moves. That is true of any square and any different
/// digit, so correctness here costs nothing and the whole question is which change to
/// make.
///
/// # Which change to make
///
/// Always **outwards**: the new digit has to sit farther from five than the one it
/// replaces. One or nine if the run allows it.
///
/// The reason is the shape of Kakuro's ambiguity. Take four white squares at the corners
/// of a rectangle, two sharing an across run and two sharing a down run. Add one to two
/// opposite corners and take one from the other two: every run through them keeps its
/// total, so if the digits stay in range and stay distinct, there are two answers. That
/// rectangle is in nearly every Kakuro board, so what stops the swap is never the shape
/// — it is a digit that cannot move. A nine cannot go up and a one cannot go down.
///
/// This is **not** the global bias towards extreme digits that ADR 0019 tried and
/// measured as making no difference. That one pushed every square in the board towards
/// the ends while nothing was known about where the slack was. This one changes a single
/// square that a second answer has just pointed at.
///
/// # Why outwards, and not merely different
///
/// Any different digit kills the second answer, so the first version of this allowed any
/// change at all. It cycled: the repair would move a square out and a later round would
/// move it back, and the board would still have many answers when the rounds ran out.
/// Measured at 0 of 20 kept at 5 by 5.
///
/// Insisting on outwards gives the repair something that cannot go backwards. Add up how
/// far every digit sits from five, and each change makes that total strictly larger. It
/// cannot exceed four per white square, so the repair either finishes or runs out of
/// moves, and it can never go round in a circle. That bound is also where this family's
/// round allowance comes from; see [`rounds_for`].
///
/// Returns `None` when no square the two answers disagree about can legally hold a digit
/// farther out than the one it has, which leaves the caller the blacking-out lever.
#[must_use]
fn nudge(puzzle: &Kakuro, ours: &[i16], other: &State) -> Option<Vec<i16>> {
    let layout = &puzzle.layout;
    let at = |white: WhiteId| -> Option<i16> { ours.get(usize::try_from(white.0).ok()?).copied() };

    let best = layout
        .whites()
        .filter(|white| at(*white).is_some_and(|mine| puzzle.digit(other, *white) != Some(mine)))
        .flat_map(|white| {
            // Every digit this square could hold instead, which means distinct from the
            // other digits in both of its runs. The run sums move with the digit, and a
            // sum read off a legal filling is always inside its run's reachable range, so
            // there is nothing else to check.
            let taken = digits_around(puzzle, ours, white);
            (1..=9_i16)
                .filter(move |digit| !taken.contains(digit))
                // Strictly farther from five, which is what makes the repair's progress
                // measurable and stops it going round in a circle.
                .filter(move |digit| {
                    at(white).is_some_and(|mine| (digit - 5).abs() > (mine - 5).abs())
                })
                .map(move |digit| (white, digit))
        })
        // Fixed order, so one seed gives one puzzle: farthest from five first, then the
        // earlier square, then the smaller digit.
        .min_by_key(|(white, digit)| (5 - (digit - 5).abs(), white.0, *digit))?;

    let (white, digit) = best;
    let mut next = ours.to_vec();
    *next.get_mut(usize::try_from(white.0).ok()?)? = digit;
    Some(next)
}

/// The digits already used by the other squares in both of a square's runs.
#[must_use]
fn digits_around(puzzle: &Kakuro, digits: &[i16], white: WhiteId) -> Vec<i16> {
    let layout = &puzzle.layout;
    let mut taken = Vec::new();
    if let Some((across, down)) = layout.runs_of(white) {
        for run in [across, down] {
            if let Some(run) = layout.run(run) {
                for other in &run.squares {
                    if *other == white {
                        continue;
                    }
                    if let Some(digit) = usize::try_from(other.0).ok().and_then(|i| digits.get(i)) {
                        taken.push(*digit);
                    }
                }
            }
        }
    }
    taken
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
    /// Ran out of fair moves on every board tried, at every run length. Counted on its
    /// own because it is the built method's one real failure, and a batch where it is
    /// common is a batch asking for a board size the run limits cannot pin down.
    pub no_fair_repair: u32,
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
        self.kept
            + self.still_ambiguous
            + self.too_much_given_away
            + self.no_fair_repair
            + self.undecidable
            + self.other
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
            Err(Rejected::NoFairRepair { .. }) => {
                report.no_fair_repair = report.no_fair_repair.saturating_add(1);
            }
            Err(Rejected::Undecidable) => {
                report.undecidable = report.undecidable.saturating_add(1);
            }
            Err(_) => report.other = report.other.saturating_add(1),
        }
    }
    (kept, report)
}
