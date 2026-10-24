//! Building a Kakuro's shape on purpose, instead of starting from a random pattern.
//!
//! # The problem this solves
//!
//! [`crate::generate`] makes a puzzle by proposing a random block pattern and then
//! repairing the ambiguity out of it. It works, and the puzzles it makes give about
//! **38 in every 100** white squares away: those squares sit alone in a run of one, and
//! a run of one square names its digit outright. ADR 0019 recorded that as the number to
//! improve and said why no amount of refilling would improve it.
//!
//! The cause is not the repair. It is the starting pattern. A random pattern has runs of
//! every length including one, so repair begins from a board that is already partly
//! given away, and every square it blacks out can make another run of one.
//!
//! # Why the first attempt at this was thrown away
//!
//! A published Kakuro has no run of one square anywhere. Stated as a rule over a board:
//! **every white square's across run and down run must both be at least two squares
//! long.**
//!
//! The obvious way to get there is to fix a board that breaks the rule: find a run of
//! one and add or remove a block next to it. That was written on 2026-10-24 and thrown
//! away the same day, because it does not converge. Lengthening a row's run of one means
//! clearing a block, which lengthens a column's run past its limit; blacking a square to
//! fix the column makes a new run of one in some row. The two directions pull against
//! each other and the loop oscillates.
//!
//! # What this module does instead
//!
//! Never build a board that breaks the rule in the first place.
//!
//! The board is filled in reading order, one square at a time, choosing black or white.
//! Both directions are checked **at the moment of choosing**, against the part of the
//! board already decided:
//!
//! - Putting a block down closes the run of whites to its left and the run of whites
//!   above it. Either of those, if it is not empty, has to be long enough already.
//! - Leaving a square white lengthens both of those runs, so neither may go past the
//!   longest allowed. And if this square is the last in its row or its column, the run
//!   it closes has to be long enough.
//!
//! A choice that breaks either rule is not made, so there is never a board to repair.
//! When no choice is left at some square, the search takes back the previous square's
//! choice and tries the other one. That is the difference from the discarded attempt:
//! going back is how a conflict is resolved, rather than another local fix that creates
//! the next conflict.
//!
//! Both rules are about runs the choice **closes**, which is the reason this works in
//! reading order at all: a square's across run reaches left and its down run reaches up,
//! so both lie entirely in the part of the board already decided.
//!
//! # The thing that actually decides whether a board can have one answer
//!
//! Two squares in a row and two in a column, at the corners of a rectangle, with each
//! pair sharing a run: add one to two opposite corners and take one from the other two.
//! Every run through those squares keeps its total, because each one gained as much as it
//! lost. If the four digits stay between one and nine and stay distinct inside their
//! runs, the board has a second answer, and no total can tell the two apart.
//!
//! Call that a **swap rectangle**. It is the ordinary way a Kakuro has more than one
//! answer, and a board with no run of one square is *more* prone to it, not less, because
//! every square has two runs of real length to be swapped along.
//!
//! This was measured rather than assumed. Fair boards with their rectangles left alone
//! almost never have one answer: across ten built boards each at 5 by 5, 7 by 7 and 9 by
//! 9, and twenty different fillings of each, **one** of six hundred gave a puzzle with a
//! single answer. More adding-up rules did not help either, and the reason is that the
//! rules are not independent: a board's across totals and its down totals both add up to
//! the same grand total, so a board always has at least one fewer real rule than it
//! appears to.
//!
//! # Why the rectangles are avoided and not forbidden
//!
//! The first version of this refused to lay a square that would close a rectangle. The
//! result is worth writing down: **there is no such board.** Not at 4 by 4, not at any
//! size tried. The search is exhaustive, so that is a fact about Kakuro rather than a
//! limit of the search, and the short argument is that every white square needs a
//! neighbour across and a neighbour down, which drags the fourth corner in behind them.
//!
//! Which is also why real Kakuro grids are full of swap rectangles. What stops the swap
//! in a published puzzle is never the shape; it is a digit that cannot move, because a
//! nine cannot go up and a one cannot go down.
//!
//! So the rectangles are a **preference** here, not a rule. Among the boards the run
//! rules allow, the search tries first the ones with fewer rectangles, and leaves the
//! rest to the digits. [`crate::generate`]'s digit lever is the other half of this, and
//! neither half works without the other.
//!
//! **This is what makes a published Kakuro look like offset brickwork.** A board whose
//! runs line up row under row is thick with these rectangles; one whose runs are offset
//! has far fewer. The pattern is not decoration, and that is the thing worth taking from
//! this module: the look of a printed Kakuro grid is a consequence of what it takes for
//! the puzzle to have one answer.
//!
//! # What this cannot do
//!
//! It makes a shape, and a shape is not a puzzle. A board with no run of one can still
//! have many answers — [`crate::generate`]'s diagnosis still holds, and the repair is
//! still what settles uniqueness. What changes is that the repair now starts from a
//! board that gives nothing away, and is only allowed moves that keep it that way.
//!
//! It also does not place the totals in the board the way a printed Kakuro does, with a
//! black first row and column carrying them. Totals for runs that start against an edge
//! are carried in a strip outside the board instead; see ADR 0020. That is a drawing
//! decision and has nothing to do with fairness.

use gs_core::rng::Rng;

use crate::layout::{Layout, LayoutError, WhiteId, LONGEST_RUN};

/// The shortest run a fair board may contain.
///
/// Two, and not a setting. A run of one square has its digit named by its own total, so
/// a board containing one is a board partly handed to the player. Everything in this
/// module exists to keep this number at two.
pub const SHORTEST_FAIR_RUN: usize = 2;

/// How many choices the search may make before giving up.
///
/// A limit rather than a search without one, like every other search in this project.
/// Measured need is a few thousand at the sizes we make; this is generous enough that
/// hitting it means the rules asked for a board that does not exist, not that the search
/// was unlucky.
pub const CHOICES: u32 = 400_000;

/// What shape of board to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weave {
    /// The longest run the board may contain, at most [`LONGEST_RUN`].
    ///
    /// This is the lever that matters, and the reason is in `generate`'s diagnosis: each
    /// run contributes one adding-up rule, so **runs per white square** is the ratio
    /// that decides whether a board can have one answer. Short runs mean more runs,
    /// which means more rules over the same unknowns. A board of nothing but runs of two
    /// has exactly one rule per unknown.
    pub longest_run: usize,
    /// How often the search tries a block before a white square, in tenths of a percent.
    ///
    /// Taste, not correctness: any board the search returns obeys the run rules whatever
    /// this is set to. It decides how black the board looks, and a blacker board has
    /// shorter runs.
    pub block_first_per_mille: u16,
    /// Whether to refuse a board containing a **swap rectangle**: four white squares at
    /// the corners of a rectangle, where the two in each row share an across run and the
    /// two in each column share a down run.
    ///
    /// This is the one rule here that is about the puzzle having one answer rather than
    /// about it being fair, and it is the rule that makes the built method work at all.
    /// See this module's notes.
    pub avoid_swaps: bool,
}

impl Weave {
    /// The default taste: runs of two to five squares, and a board about a third black.
    ///
    /// Five because that is the span of a printed Kakuro's runs, and because the sum of
    /// a run of five has enough combinations to be worth working out. A third black
    /// because that is roughly what the run limits produce anyway.
    #[must_use]
    pub fn middling() -> Self {
        Weave {
            longest_run: 5,
            block_first_per_mille: 350,
            avoid_swaps: true,
        }
    }

    /// The same taste with a shorter longest run, which is how a caller escalates when
    /// a board turns out to have too many answers to repair cleanly.
    #[must_use]
    pub fn with_shorter_runs(self) -> Option<Self> {
        let shorter = self.longest_run.checked_sub(1)?;
        if shorter < SHORTEST_FAIR_RUN {
            return None;
        }
        Some(Weave {
            longest_run: shorter,
            ..self
        })
    }
}

/// Why no board could be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotWoven {
    /// The run limits cannot be met on a board this size at all. A board narrower than
    /// the shortest run, for instance, has no legal row.
    NoSuchBoard { width: u16, height: u16 },
    /// The search used up its [`CHOICES`] without finding a board.
    OutOfChoices,
    /// The longest run asked for is longer than any run can be, or shorter than the
    /// shortest fair one.
    ImpossibleRunLength { asked: usize },
    /// The board was built and then refused by [`Layout`]. Cannot happen — the search
    /// obeys the same run limit — and reported rather than unwrapped, because a
    /// generator that panicked on its own arithmetic would take a whole batch down.
    Unlayoutable(LayoutError),
}

/// Build a board whose every run is between two and `weave.longest_run` squares long.
///
/// The same seed and weave always give the same board.
pub fn weave(width: u16, height: u16, seed: u64, weave: Weave) -> Result<Layout, NotWoven> {
    if weave.longest_run > LONGEST_RUN || weave.longest_run < SHORTEST_FAIR_RUN {
        return Err(NotWoven::ImpossibleRunLength {
            asked: weave.longest_run,
        });
    }
    let (w, h) = (usize::from(width), usize::from(height));
    if w == 0 || h == 0 {
        return Err(NotWoven::NoSuchBoard { width, height });
    }
    // A row narrower than the shortest run can hold no white square at all, because any
    // white run in it closes at the edge too short. The same for a column. Caught here
    // so the answer is a sentence rather than an exhausted search.
    if w < SHORTEST_FAIR_RUN || h < SHORTEST_FAIR_RUN {
        return Err(NotWoven::NoSuchBoard { width, height });
    }

    let mut loom = Loom {
        width: w,
        height: h,
        longest: weave.longest_run,
        avoid_swaps: weave.avoid_swaps,
        blocks: vec![false; w * h],
        down_open: vec![0; w],
        across_open: 0,
        whites: 0,
        choices: CHOICES,
        rng: Rng::from_seed(seed),
        block_first_per_mille: weave.block_first_per_mille,
    };
    if !loom.choose(0) {
        return Err(if loom.choices == 0 {
            NotWoven::OutOfChoices
        } else {
            NotWoven::NoSuchBoard { width, height }
        });
    }
    Layout::new(width, height, loom.blocks).map_err(NotWoven::Unlayoutable)
}

/// The search's working state.
///
/// `down_open` and `across_open` are the lengths of the white runs still open above and
/// to the left of the square being decided. They are the whole reason the rules can be
/// checked one square at a time.
struct Loom {
    width: usize,
    height: usize,
    longest: usize,
    avoid_swaps: bool,
    blocks: Vec<bool>,
    /// Per column: how many whites in a row sit immediately above this square.
    down_open: Vec<usize>,
    /// How many whites in a row sit immediately to the left, within this row.
    across_open: usize,
    whites: usize,
    choices: u32,
    rng: Rng,
    block_first_per_mille: u16,
}

impl Loom {
    /// Decide square `index` and everything after it. True when the whole board is done.
    fn choose(&mut self, index: usize) -> bool {
        if self.choices == 0 {
            return false;
        }
        self.choices -= 1;

        if index == self.blocks.len() {
            // Every run closed legally on the way here, so the only thing left to rule
            // out is a board with nothing to play.
            return self.whites > 0;
        }

        let x = index % self.width;
        let y = index / self.width;
        if x == 0 {
            self.across_open = 0;
        }
        let across_before = self.across_open;

        // Which choice to try first. Taste decides, except that a white square which
        // would close a swap rectangle is tried **last**, so the search spends its
        // freedom on boards with fewer of them. A preference and not a rule: see this
        // module's notes for why forbidding them outright leaves no board at all.
        let mut black_first = self.rng.chance_per_mille(self.block_first_per_mille);
        if self.avoid_swaps && !black_first && self.completes_a_swap(x, y) {
            black_first = true;
        }
        for black in [black_first, !black_first] {
            if !self.fits(x, y, black) {
                continue;
            }
            let down_before = self.down_open[x];
            self.blocks[index] = black;
            if black {
                self.across_open = 0;
                self.down_open[x] = 0;
            } else {
                self.across_open = across_before + 1;
                self.down_open[x] = down_before + 1;
                self.whites += 1;
            }

            if self.choose(index + 1) {
                return true;
            }

            // Take the choice back. `across_open` is restored for the sibling branch,
            // and also because the next row will reset it at its first square.
            if !black {
                self.whites -= 1;
            }
            self.down_open[x] = down_before;
            self.across_open = across_before;
            self.blocks[index] = false;
        }
        false
    }

    /// Whether this choice breaks either run rule, judged against the part of the board
    /// already decided.
    fn fits(&self, x: usize, y: usize, black: bool) -> bool {
        let across = self.across_open;
        let down = self.down_open[x];
        if black {
            // A block closes both open runs. An empty run closes nothing.
            let closes_across_too_short = across > 0 && across < SHORTEST_FAIR_RUN;
            let closes_down_too_short = down > 0 && down < SHORTEST_FAIR_RUN;
            !closes_across_too_short && !closes_down_too_short
        } else {
            if across + 1 > self.longest || down + 1 > self.longest {
                return false;
            }
            // The board's edge closes a run just as a block does.
            let last_in_row = x + 1 == self.width;
            let last_in_column = y + 1 == self.height;
            if last_in_row && across + 1 < SHORTEST_FAIR_RUN {
                return false;
            }
            if last_in_column && down + 1 < SHORTEST_FAIR_RUN {
                return false;
            }
            true
        }
    }

    /// Whether putting a white square here would complete a swap rectangle.
    ///
    /// Checked only for the square being decided, and only against squares already
    /// decided, which is enough: a rectangle has a square nearest the bottom right, and
    /// that square is the last of the four to be reached in reading order. So every
    /// rectangle on the finished board is caught when its last corner is offered.
    fn completes_a_swap(&self, x: usize, y: usize) -> bool {
        // The other three corners are a square to the left in this across run, a square
        // above in this down run, and the square where those two lines cross.
        let leftmost = x - self.across_open;
        let topmost = y - self.down_open[x];
        for other_x in leftmost..x {
            for other_y in topmost..y {
                if self.is_block(other_x, other_y) {
                    continue;
                }
                // The corner is white. It shares an across run with (x, other_y) only if
                // nothing between them is a block, and a down run with (other_x, y) only
                // if nothing between them is a block either. Both of those stretches are
                // already decided.
                if self.all_white_across(other_y, other_x, x)
                    && self.all_white_down(other_x, other_y, y)
                {
                    return true;
                }
            }
        }
        false
    }

    fn is_block(&self, x: usize, y: usize) -> bool {
        self.blocks[y * self.width + x]
    }

    /// Whether row `y` is white from column `from` up to but not including `to`.
    fn all_white_across(&self, y: usize, from: usize, to: usize) -> bool {
        (from..to).all(|x| !self.is_block(x, y))
    }

    /// Whether column `x` is white from row `from` up to but not including `to`.
    fn all_white_down(&self, x: usize, from: usize, to: usize) -> bool {
        (from..to).all(|y| !self.is_block(x, y))
    }
}

/// Whether every run on this board is at least [`SHORTEST_FAIR_RUN`] squares long.
///
/// The property [`weave`] promises, written separately so it can be asserted about a
/// board that came from anywhere — a woven one, a repaired one, or one read back out of
/// a file.
#[must_use]
pub fn gives_nothing_away(layout: &Layout) -> bool {
    layout.runs().all(|(_, run)| run.len() >= SHORTEST_FAIR_RUN)
}

/// How many swap rectangles this board contains.
///
/// Four white squares at the corners of a rectangle, the two in each row sharing an
/// across run and the two in each column sharing a down run. Every one of them is a place
/// the board could have a second answer, stopped only by the digits that end up there.
/// See this module's notes.
///
/// Worked out from the finished board rather than carried along from the search, so it
/// can be asked about any board, including one read back out of a file.
#[must_use]
pub fn swap_rectangles(layout: &Layout) -> u32 {
    u32::try_from(corners_of_swap_rectangles(layout).len()).unwrap_or(u32::MAX)
}

/// The four corners of every swap rectangle, as `[top left, top right, bottom left,
/// bottom right]`.
///
/// The order matters to the caller: blocking a swap means pinning one **diagonal** pair,
/// and a diagonal pair is the only pair of the four that shares no run, so it is the only
/// pair that may both hold the same kind of extreme digit. See
/// [`crate::generate`]'s digit lever.
#[must_use]
pub fn corners_of_swap_rectangles(layout: &Layout) -> Vec<[WhiteId; 4]> {
    let white_at = |x: u16, y: u16| -> Option<WhiteId> {
        let square = layout.square(x, y)?;
        layout.white_at(square)
    };
    let joined_across = |y: u16, from: u16, to: u16| (from..=to).all(|x| white_at(x, y).is_some());
    let joined_down = |x: u16, from: u16, to: u16| (from..=to).all(|y| white_at(x, y).is_some());

    let mut found = Vec::new();
    for top in 0..layout.height() {
        for bottom in top.saturating_add(1)..layout.height() {
            for left in 0..layout.width() {
                for right in left.saturating_add(1)..layout.width() {
                    if !joined_across(top, left, right)
                        || !joined_across(bottom, left, right)
                        || !joined_down(left, top, bottom)
                        || !joined_down(right, top, bottom)
                    {
                        continue;
                    }
                    let corners = [
                        white_at(left, top),
                        white_at(right, top),
                        white_at(left, bottom),
                        white_at(right, bottom),
                    ];
                    if let [Some(a), Some(b), Some(c), Some(d)] = corners {
                        found.push([a, b, c, d]);
                    }
                }
            }
        }
    }
    found
}

/// How many adding-up rules this board has per unknown.
///
/// In tenths, to stay integer-only like every other measurement here, so 1000 means one
/// rule per unknown. The ratio `generate`'s diagnosis turns on: random sparse boards
/// measure 500 to 700, and a board of nothing but runs of two measures 1000.
#[must_use]
pub fn rules_per_unknown_per_mille(layout: &Layout) -> u32 {
    let whites = u32::try_from(layout.white_count()).unwrap_or(u32::MAX);
    if whites == 0 {
        return 0;
    }
    let runs = u32::try_from(layout.run_count()).unwrap_or(u32::MAX);
    runs.saturating_mul(1000) / whites
}
