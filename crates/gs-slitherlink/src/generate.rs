//! Making puzzles: draw a loop, number everything, then take numbers away.
//!
//! # Why this is shaped differently from Nonogram's generator
//!
//! For Nonogram the picture is the whole of the work: draw it, count the runs, and
//! the counts *are* the puzzle. There is nothing to leave out, because a row with no
//! clue would not be a row with a missing clue, it would be a claim that the row is
//! empty.
//!
//! Here a cell may simply have no number, and that is an ordinary, correct puzzle. So
//! numbering every cell gives a board that tells the player the answer several times
//! over, and the real work is deciding which numbers to **take away** while the
//! puzzle still has exactly one answer.
//!
//! That costs about one uniqueness question per cell, where Nonogram pays one per
//! puzzle. It is affordable only because every one of them goes to the fast solver in
//! [`crate::solve`] and not to an outside program, and because generation runs ahead
//! of demand rather than inside a player's request (decided 2026-10-21).
//!
//! # The loop is thrown away
//!
//! [`Candidate`] carries the clues and nothing else. The loop the generator drew is
//! dropped on purpose, exactly as Nonogram drops its pattern: a package that carried
//! the loop alongside the numbers would be shipping the answer, and a generator whose
//! output still remembers the intended answer invites every later step to compare
//! against it instead of checking the clues.
//!
//! The witness the verifier needs is found by **solving** the finished clue grid, not
//! by remembering. So if removing numbers ever created a second answer, the witness
//! would be whichever answer the solver found first, and the uniqueness query would
//! catch it. Remembering the loop would hide exactly that fault.

use gs_core::rng::Rng;

use crate::board::{CellId, Geometry, SizeError};
use crate::clues::Clues;
use crate::loops::Loop;
use crate::rules::Slitherlink;
use crate::solve::{solve, Budget, Features, Solved};

/// How hard the fast solver may try on each question in the removal loop.
///
/// **Measured on 2026-10-23, and the measurement is the whole reason this constant
/// exists.** The removal loop asks one uniqueness question per cell, and on a sparse
/// board our own solver can spend tens of thousands of guesses on one of them. Capping
/// the work and **keeping the number whenever the question is not settled inside the
/// cap** turns out to cost almost nothing:
///
/// | board | cap | numbers left | time per puzzle |
/// | --- | --- | --- | --- |
/// | 7x7 | 2,000 | 14 to 18 | 16 to 122 ms |
/// | 7x7 | 200,000 | 13 to 18 | 17 to 234 ms |
/// | 10x10 | 2,000 | 30 to 36 | 378 ms to 1.2 s |
/// | 10x10 | 200,000 | 27 to 34 | 380 ms to 13.7 s |
///
/// Six seeds each. The cap leaves one to three more numbers on a 10 by 10 board of a
/// hundred, and makes it ten to thirty times faster. That is the trade, taken on
/// purpose: a slightly more generous puzzle, for a pool that can actually be filled.
///
/// Nothing unsafe follows from a cap. A number comes off only on a clear "exactly one
/// answer"; an exhausted budget puts it back, because a budget running out is not
/// permission. And the real solver still has the last word on the finished puzzle.
pub const REMOVAL_BUDGET: Budget = Budget {
    max_nodes: 2_000,
    max_depth: 128,
};

/// Bumped when a change here would produce different puzzles from the same seed.
///
/// A validation report records it, so an old report is never mistaken for a current
/// one. **Changing a deduction rule in [`crate::solve`] changes what this generator
/// keeps**, because the removal loop asks that solver after every removal — so such a
/// change belongs with a bump here.
pub const GENERATOR_VERSION: u32 = 1;

/// What size of board to make, and how much of it the loop should enclose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub width: u16,
    pub height: u16,
    /// How many of the board's cells the loop should enclose, in tenths of a percent.
    ///
    /// Integer-only like every other measurement here. A loop enclosing very few
    /// cells is a small ring in a big empty board, which is a dull puzzle; one
    /// enclosing nearly everything is a ring round the rim, which is duller. The
    /// middle is where the interesting shapes are, and the right value is a thing to
    /// measure rather than to guess.
    pub enclosed_per_mille: u32,
}

impl Shape {
    /// A reasonable starting shape for a board of this size.
    ///
    /// 400 per mille is a starting point for measurement, not a tuned value. The
    /// Nonogram sweep found its own equivalent by measuring, and found that the
    /// number everyone would have guessed was a trap; this family has not been swept
    /// yet.
    #[must_use]
    pub fn middling(width: u16, height: u16) -> Self {
        Shape {
            width,
            height,
            enclosed_per_mille: 400,
        }
    }

    pub fn geometry(self) -> Result<Geometry, SizeError> {
        Geometry::new(self.width, self.height)
    }
}

/// Why a seed produced no puzzle.
///
/// Every variant is an ordinary outcome of generation rather than a fault. A
/// generator that could not refuse would be a generator that published whatever it
/// drew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// The board size is not one a puzzle can be made on.
    Size(SizeError),
    /// No loop could be grown from this seed within the attempts allowed.
    NoLoop,
    /// The fully numbered board does not have exactly one answer, so there is nothing
    /// to remove numbers from. Carries what the solver said.
    FullBoardNotUnique(Solved),
    /// The fast solver ran out of budget, so nothing is known. Never treated as a
    /// pass; see [`Solved::Undecided`].
    Undecided,
}

/// A puzzle the fast solver believes has exactly one answer.
///
/// Called a candidate and not an answer. Nothing in this type can become a
/// [`gs_core::verification::Outcome`]: only a real solver produces one of those, and
/// only one of those reaches the publication gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub puzzle: Slitherlink,
    /// The seed this puzzle came from, so the whole of it can be rebuilt from the
    /// validation report.
    pub seed: u64,
    pub shape: Shape,
    /// What the fast solver measured on the finished puzzle.
    pub features: Features,
    /// How many numbers came off the fully numbered board.
    pub clues_removed: u32,
    /// How many uniqueness questions the removal loop asked. The cost of this family,
    /// measured rather than assumed, because it is what decides how fast a pool can
    /// be filled.
    pub questions_asked: u32,
}

/// Grow a random loop on a board.
///
/// Builds a set of cells for the loop to enclose, one neighbour at a time, and keeps
/// only additions whose boundary is still one closed loop. Growing the *inside* and
/// checking the boundary is what keeps this honest: the check is
/// [`Loop::of`], the same one the engine uses, so the generator cannot produce a
/// shape the engine would not accept.
///
/// Returns `None` when nothing grew, which happens on a board too small to hold a
/// loop.
#[must_use]
pub fn grow_loop(seed: u64, shape: Shape) -> Option<Loop> {
    let geometry = shape.geometry().ok()?;
    let mut rng = Rng::from_seed(seed);
    let cells = geometry.cell_count();
    let target =
        (cells.max(1) * usize::try_from(shape.enclosed_per_mille.min(1000)).unwrap_or(400)) / 1000;
    let target = target.clamp(1, cells);

    let mut inside = vec![false; cells];
    let first = rng
        .below(u32::try_from(cells).unwrap_or(u32::MAX))
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    inside[first] = true;
    let mut best = boundary_loop(geometry, &inside);

    // Each round offers one randomly chosen cell next to the current shape. An
    // addition that would put a hole in the middle is dropped, because the boundary
    // of a shape with a hole is two loops.
    let attempts = cells.saturating_mul(8);
    let mut held = 1;
    for _ in 0..attempts {
        if held >= target {
            break;
        }
        let candidates = edge_of(geometry, &inside);
        if candidates.is_empty() {
            break;
        }
        let pick = rng
            .below(u32::try_from(candidates.len()).unwrap_or(1))
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        let cell = candidates[pick];
        let at = usize::try_from(cell.0).unwrap_or(0);
        inside[at] = true;
        match boundary_loop(geometry, &inside) {
            Some(found) => {
                best = Some(found);
                held += 1;
            }
            None => inside[at] = false,
        }
    }
    best
}

/// The edges with an enclosed cell on exactly one side, as a loop if they make one.
fn boundary_loop(geometry: Geometry, inside: &[bool]) -> Option<Loop> {
    let is_inside = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= i32::from(geometry.width()) || y >= i32::from(geometry.height()) {
            return false;
        }
        let (x, y) = (u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0));
        geometry
            .cell(x, y)
            .and_then(|c| usize::try_from(c.0).ok())
            .and_then(|i| inside.get(i).copied())
            .unwrap_or(false)
    };
    let mut drawn = vec![false; geometry.edge_count()];
    for y in 0..i32::from(geometry.height()) {
        for x in 0..i32::from(geometry.width()) {
            if !is_inside(x, y) {
                continue;
            }
            let (ux, uy) = (u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0));
            // A side is on the boundary when the cell across it is not enclosed.
            for (dx, dy, edge) in [
                (0, -1, geometry.across(ux, uy)),
                (0, 1, geometry.across(ux, uy + 1)),
                (-1, 0, geometry.down(ux, uy)),
                (1, 0, geometry.down(ux + 1, uy)),
            ] {
                if !is_inside(x + dx, y + dy) {
                    if let Some(edge) = edge {
                        if let Some(slot) = drawn.get_mut(usize::try_from(edge.0).unwrap_or(0)) {
                            *slot = true;
                        }
                    }
                }
            }
        }
    }
    Loop::of(geometry, drawn).ok()
}

/// Cells not enclosed yet that touch the enclosed shape, or every cell when nothing
/// is enclosed.
fn edge_of(geometry: Geometry, inside: &[bool]) -> Vec<CellId> {
    let mut out = Vec::new();
    for cell in geometry.cells() {
        let at = usize::try_from(cell.0).unwrap_or(usize::MAX);
        if inside.get(at).copied().unwrap_or(false) {
            continue;
        }
        let Some((x, y)) = geometry.cell_at(cell) else {
            continue;
        };
        let touches = [
            (x.checked_sub(1), Some(y)),
            (Some(x + 1), Some(y)),
            (Some(x), y.checked_sub(1)),
            (Some(x), Some(y + 1)),
        ]
        .into_iter()
        .filter_map(|(nx, ny)| Some((nx?, ny?)))
        .filter_map(|(nx, ny)| geometry.cell(nx, ny))
        .any(|n| {
            usize::try_from(n.0)
                .ok()
                .and_then(|i| inside.get(i).copied())
                .unwrap_or(false)
        });
        if touches {
            out.push(cell);
        }
    }
    out
}

/// Make one puzzle from one seed.
///
/// Draw a loop, number every cell, then try to remove each number in a seeded random
/// order, keeping a removal only while the fast solver still finds exactly one
/// answer.
pub fn propose(seed: u64, shape: Shape, budget: Budget) -> Result<Candidate, Rejected> {
    shape.geometry().map_err(Rejected::Size)?;
    let the_loop = grow_loop(seed, shape).ok_or(Rejected::NoLoop)?;
    let geometry = the_loop.geometry();

    let full = Clues::of(&the_loop);
    let mut questions = 1u32;
    let first = solve(&Slitherlink::new(full.clone()), budget);
    match first.solved {
        Solved::One => {}
        Solved::Undecided => return Err(Rejected::Undecided),
        other => return Err(Rejected::FullBoardNotUnique(other)),
    }

    // A seeded order, so the same seed gives the same puzzle. A fixed order would
    // bias every puzzle towards bare top-left corners.
    let mut order: Vec<CellId> = geometry.cells().collect();
    let mut rng = Rng::from_seed(seed ^ 0x5115_4e21_1ee5_1a1b);
    for i in (1..order.len()).rev() {
        let j = rng
            .below(u32::try_from(i + 1).unwrap_or(1))
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        order.swap(i, j);
    }

    let mut clues = full;
    let mut removed = 0u32;
    let mut features = first.features;
    for cell in order {
        if clues.at(cell).is_none() {
            continue;
        }
        let fewer = clues.without(cell);
        questions = questions.saturating_add(1);
        let found = solve(&Slitherlink::new(fewer.clone()), budget);
        // Only a clear "exactly one answer" keeps a removal. `Undecided` puts the
        // number back, because an exhausted budget is not permission.
        if found.solved == Solved::One {
            clues = fewer;
            removed = removed.saturating_add(1);
            features = found.features;
        }
    }

    Ok(Candidate {
        puzzle: Slitherlink::new(clues),
        seed,
        shape,
        features,
        clues_removed: removed,
        questions_asked: questions,
    })
}

/// What a batch of seeds produced. For measuring yield and cost, not for shipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    pub tried: u32,
    pub kept: u32,
    pub no_loop: u32,
    pub not_unique: u32,
    pub undecided: u32,
    /// Total uniqueness questions asked across the batch, including for seeds that
    /// produced nothing. The honest cost figure.
    pub questions_asked: u32,
}

impl Report {
    #[must_use]
    pub fn kept_per_mille(&self) -> u32 {
        if self.tried == 0 {
            return 0;
        }
        self.kept.saturating_mul(1000) / self.tried
    }

    /// Whether the counts add up. A report whose parts do not sum to its whole is
    /// measuring something other than what it says.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.kept + self.no_loop + self.not_unique + self.undecided == self.tried
    }
}

/// Run a batch of seeds, returning what was kept and what it cost.
pub fn batch(
    seeds: impl IntoIterator<Item = u64>,
    shape: Shape,
    budget: Budget,
) -> (Vec<Candidate>, Report) {
    let mut kept = Vec::new();
    let mut report = Report::default();
    for seed in seeds {
        report.tried = report.tried.saturating_add(1);
        match propose(seed, shape, budget) {
            Ok(candidate) => {
                report.questions_asked = report
                    .questions_asked
                    .saturating_add(candidate.questions_asked);
                report.kept = report.kept.saturating_add(1);
                kept.push(candidate);
            }
            Err(Rejected::NoLoop) | Err(Rejected::Size(_)) => {
                report.no_loop = report.no_loop.saturating_add(1);
            }
            Err(Rejected::FullBoardNotUnique(_)) => {
                report.not_unique = report.not_unique.saturating_add(1);
            }
            Err(Rejected::Undecided) => {
                report.undecided = report.undecided.saturating_add(1);
            }
        }
    }
    (kept, report)
}
