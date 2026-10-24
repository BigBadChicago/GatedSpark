//! A fast Kakuro solver, used as a generation filter and as the source of the
//! difficulty numbers.
//!
//! **This solver proves nothing.** It produces no [`gs_core::verification::Outcome`]
//! and there is no path from its results to one. Only a real SMT solver, fed by
//! [`crate::smtlib`], produces an outcome, and only an outcome reaches the
//! publication gate. The same arrangement as the other two families, for the same
//! reason: a puzzle family must not be able to answer its own questions.
//!
//! What it is for is cheapness. Generation tries many layouts and keeps few, and
//! asking an outside program about every attempt would make that unaffordable. So
//! this decides which candidates are worth asking about, and the real solver decides
//! whether the answer is true.
//!
//! # How it works
//!
//! Each white square carries a **candidate set**: which of the digits one to nine it
//! could still hold, as nine bits. Then two deductions run from a worklist until
//! nothing more is forced:
//!
//! * **No repeats.** A digit written in a run is removed from every other square in
//!   that run.
//! * **The total.** For a run, take what is already written and subtract it from the
//!   total to get what is left to find. A candidate is dropped when the rest of the
//!   run could not possibly make up the difference.
//!
//! When nothing is forced, it guesses: the square with the fewest candidates, lowest
//! numbered first, trying its digits in order. Both tie-breaks are fixed so that two
//! runs on the same puzzle do the same work, which is what makes the difficulty
//! numbers comparable at all.
//!
//! # The one approximation, stated plainly
//!
//! The total deduction asks whether the other unassigned squares in a run could make
//! up a given difference. Answering that exactly means searching the run, which is
//! the work this deduction exists to avoid. So it uses a **relaxation**: it takes the
//! smallest and largest sums available from the union of the other squares'
//! candidates, ignoring which square each digit came from.
//!
//! That is sound in the direction that matters. The real set of reachable sums is a
//! subset of the relaxed range, so a digit this deduction drops could not have been
//! part of an answer — it never prunes a real answer. It is simply weaker than a
//! perfect deduction would be, which costs search time and never costs correctness.
//! Held by [`solve`]'s agreement with the SMT encoding in
//! `tests/solver_agreement.rs`.

use gs_core::state::{Cell, State};

use crate::layout::{Layout, RunId, WhiteId};
use crate::rules::Kakuro;
use crate::totals::{LEAST_DIGIT, MOST_DIGIT};

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solved {
    /// No way of filling the board satisfies the totals.
    None,
    /// Exactly one does, within the budget.
    One,
    /// At least two do. Counting stops at two: more than one is already
    /// disqualifying, and the exact number is not a question anybody asks.
    Several,
    /// The budget ran out before the question was decided.
    ///
    /// Distinct from every other answer, for the reason
    /// [`gs_core::verification::Outcome::Unknown`] is: an exhausted budget is not an
    /// answer. A filter may treat it as "reject and move on". Nothing may treat it as
    /// "unique".
    Undecided,
}

/// The difficulty feature vector, measured while solving.
///
/// Measurements of what this solver did, not predictions about a person.
///
/// Turning these into difficulty levels a player sees is **the game's job, not the
/// platform's**, settled on 2026-10-22. So the platform carries these numbers and
/// never names a level, and this struct has no level in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    /// Squares decided by forced deductions alone, before any guess.
    pub propagation_only_squares: u32,
    /// How many times a forced deduction settled at least one square, before the
    /// first guess. How long the chain of reasoning was.
    pub forced_steps: u32,
    /// Guesses taken.
    ///
    /// Zero means the puzzle needed no guessing **from this solver, with the
    /// deductions this solver knows** — not that a person needs no guesses. The
    /// warning on the other two families applies here too, and one part of it is
    /// specific to this one: a Kakuro player works from memorised tables of which
    /// digits can make a given total over a given number of squares, and this solver
    /// has no such table. It reasons only from the smallest and largest sums still
    /// available. So a board a person finds easy may cost this solver a guess.
    ///
    /// This number grades puzzles **against this solver** and may only be compared
    /// with other numbers from the same solver. Adding a deduction changes it for
    /// every puzzle, which is why `GENERATOR_VERSION` exists.
    pub decision_points: u32,
    /// Deepest nesting of guesses.
    pub max_search_depth: u32,
    /// White squares on the board.
    pub white_squares: u32,
    /// Runs on the board.
    pub runs: u32,
    /// The longest run, in squares. A nine-square run is fully determined as a set —
    /// it holds every digit — so it is a different kind of clue from a two-square
    /// one, and a player notices.
    pub longest_run: u32,
    /// White squares sitting in a run of a single square.
    ///
    /// The part of the board the player is **given** rather than works out, because a
    /// run of one names its digit outright. Filled in by `crate::generate`, which is
    /// the only thing that knows the finished layout; a bare `solve` leaves it zero.
    /// Carried in the feature vector because it is the number this family most needs
    /// to bring down, and a number nobody can see does not get improved.
    pub given_away_squares: u32,
    /// Runs of a single square.
    ///
    /// Carried because it is this family's giveaway: a run of one names its digit
    /// outright, with no reasoning at all. A board made mostly of them is unique and
    /// worthless, so the generator limits them and the number is kept where anyone
    /// grading difficulty can see it.
    pub single_square_runs: u32,
}

impl Features {
    /// Whether the puzzle needed no guessing from this solver. Read the warning on
    /// [`Features::decision_points`] before treating that as a fact about the puzzle.
    #[must_use]
    pub fn needs_no_guessing(&self) -> bool {
        self.decision_points == 0
    }

    /// The share of squares the forced deductions settled on their own, in tenths of
    /// a percent, so the feature vector stays integer-only like everything else.
    ///
    /// **Read the warning on `gs_nonogram::solve::Features::forced_ratio_per_mille`
    /// before grading anything on this.** There it turned out to be exactly 1000 for
    /// every puzzle needing no guesses, carrying no more information than the
    /// yes-or-no. Whether the same trap applies here is a question for measurement,
    /// and it is not answered yet.
    #[must_use]
    pub fn forced_ratio_per_mille(&self) -> u32 {
        if self.white_squares == 0 {
            return 0;
        }
        self.propagation_only_squares.saturating_mul(1000) / self.white_squares
    }
}

/// What a solve found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub solved: Solved,
    pub features: Features,
    /// The first answer found, if any. A candidate witness for the real verifier,
    /// never evidence on its own.
    pub witness: Option<State>,
}

/// Work limits. Every search in this platform is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_nodes: u32,
    pub max_depth: u32,
}

impl Default for Budget {
    fn default() -> Self {
        // A Kakuro board has at most nine squares in any run and rarely more than a
        // hundred squares in all, and each square has nine possibilities rather than
        // two, so the branching is wider than either earlier family and the trees are
        // shallower. Generous for the sizes a person plays, small enough that a
        // pathological candidate is dropped rather than chased.
        Budget {
            max_nodes: 300_000,
            max_depth: 128,
        }
    }
}

/// Which digits a square could still hold, as bits 1 to 9.
type Marks = Vec<u16>;

/// Every digit, as a mask.
const ALL: u16 = 0b11_1111_1110;

#[must_use]
fn mask_of(digit: i16) -> u16 {
    if !(LEAST_DIGIT..=MOST_DIGIT).contains(&digit) {
        return 0;
    }
    1_u16 << digit
}

#[must_use]
fn is_assigned(mask: u16) -> bool {
    mask.count_ones() == 1
}

#[must_use]
fn only_digit(mask: u16) -> Option<i16> {
    if is_assigned(mask) {
        i16::try_from(mask.trailing_zeros()).ok()
    } else {
        None
    }
}

/// The digits in a mask, smallest first.
fn digits(mask: u16) -> impl Iterator<Item = i16> {
    (LEAST_DIGIT..=MOST_DIGIT).filter(move |d| mask & mask_of(*d) != 0)
}

/// Why a partial board cannot become an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeadEnd;

/// Solve, counting answers up to two.
#[must_use]
pub fn solve(puzzle: &Kakuro, budget: Budget) -> Analysis {
    let layout = &puzzle.layout;
    let mut features = Features {
        white_squares: u32::try_from(layout.white_count()).unwrap_or(u32::MAX),
        runs: u32::try_from(layout.run_count()).unwrap_or(u32::MAX),
        longest_run: layout
            .runs()
            .map(|(_, r)| u32::try_from(r.len()).unwrap_or(u32::MAX))
            .max()
            .unwrap_or(0),
        single_square_runs: u32::try_from(layout.runs().filter(|(_, r)| r.len() == 1).count())
            .unwrap_or(u32::MAX),
        ..Features::default()
    };

    let mut marks: Marks = vec![ALL; layout.white_count()];

    // The first run to a fixpoint is measured separately, because what it settles on
    // its own is the difficulty signal. Anything after a guess is not.
    let before_guessing = propagate(puzzle, &mut marks, &mut features, true);
    if before_guessing.is_err() {
        return Analysis {
            solved: Solved::None,
            features,
            witness: None,
        };
    }
    features.propagation_only_squares =
        u32::try_from(marks.iter().filter(|m| is_assigned(**m)).count()).unwrap_or(u32::MAX);

    let mut search = Search {
        puzzle,
        budget,
        nodes: 0,
        found: 0,
        witness: None,
    };
    let ran_out = search.descend(&mut marks, &mut features, 0);

    let solved = if ran_out {
        Solved::Undecided
    } else {
        match search.found {
            0 => Solved::None,
            1 => Solved::One,
            _ => Solved::Several,
        }
    };
    Analysis {
        solved,
        features,
        witness: search.witness,
    }
}

/// Run both deductions to a fixpoint.
///
/// `measuring` counts the steps into the feature vector; it is false once guessing
/// has started, because work done inside a guess is not a difficulty signal.
fn propagate(
    puzzle: &Kakuro,
    marks: &mut Marks,
    features: &mut Features,
    measuring: bool,
) -> Result<(), DeadEnd> {
    loop {
        let mut changed = false;
        for (id, _) in puzzle.layout.runs() {
            if narrow_run(puzzle, marks, id)? {
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
        if measuring {
            features.forced_steps = features.forced_steps.saturating_add(1);
        }
    }
}

/// Both deductions for one run. Returns whether anything changed.
fn narrow_run(puzzle: &Kakuro, marks: &mut Marks, id: RunId) -> Result<bool, DeadEnd> {
    let Some(run) = puzzle.layout.run(id) else {
        return Ok(false);
    };
    let Some(total) = puzzle.totals.get(id) else {
        return Ok(false);
    };
    let squares: Vec<usize> = run
        .squares
        .iter()
        .filter_map(|w| usize::try_from(w.0).ok())
        .collect();
    if squares.len() != run.len() {
        return Err(DeadEnd);
    }

    let mut changed = false;

    // No repeats: a digit written here is not available anywhere else in the run.
    let mut taken = 0_u16;
    for index in &squares {
        let mask = marks.get(*index).copied().ok_or(DeadEnd)?;
        if mask == 0 {
            return Err(DeadEnd);
        }
        if is_assigned(mask) {
            if taken & mask != 0 {
                // Two squares in this run already hold the same digit.
                return Err(DeadEnd);
            }
            taken |= mask;
        }
    }
    for index in &squares {
        let mask = marks[*index];
        if is_assigned(mask) {
            continue;
        }
        let narrowed = mask & !taken;
        if narrowed != mask {
            marks[*index] = narrowed;
            changed = true;
        }
        if narrowed == 0 {
            return Err(DeadEnd);
        }
    }

    // The total. What is left to find, over the squares not yet written in.
    let written: i32 = squares
        .iter()
        .filter_map(|i| only_digit(marks[*i]).map(i32::from))
        .sum();
    let unassigned: Vec<usize> = squares
        .iter()
        .copied()
        .filter(|i| !is_assigned(marks[*i]))
        .collect();
    let left = i32::from(total) - written;

    if unassigned.is_empty() {
        return if left == 0 { Ok(changed) } else { Err(DeadEnd) };
    }
    if left <= 0 {
        // Every remaining square holds at least one, so a non-positive remainder
        // cannot be made up.
        return Err(DeadEnd);
    }

    for (position, index) in unassigned.iter().enumerate() {
        let mut keep = 0_u16;
        for digit in digits(marks[*index]) {
            let needed = left - i32::from(digit);
            let others: Vec<usize> = unassigned
                .iter()
                .enumerate()
                .filter(|(p, _)| *p != position)
                .map(|(_, i)| *i)
                .collect();
            if reachable(marks, &others, mask_of(digit), needed) {
                keep |= mask_of(digit);
            }
        }
        if keep == 0 {
            return Err(DeadEnd);
        }
        if keep != marks[*index] {
            marks[*index] = keep;
            changed = true;
        }
    }
    Ok(changed)
}

/// Could the squares in `others` add up to `needed`, as distinct digits none of which
/// is in `used`?
///
/// **A relaxation.** It pools every candidate of every square in `others` and asks
/// whether `needed` lies between the sum of the smallest `n` available digits and the
/// sum of the largest `n`, where `n` is how many squares there are. It does not check
/// that a particular digit is available at a particular square.
///
/// Sound in the direction that matters: the sums actually reachable are a subset of
/// this range, so a digit dropped because of it could not have been in an answer. It
/// is weaker than a perfect check, which costs search time and never correctness. See
/// the module note.
#[must_use]
fn reachable(marks: &Marks, others: &[usize], used: u16, needed: i32) -> bool {
    if others.is_empty() {
        return needed == 0;
    }
    let mut pool = 0_u16;
    for index in others {
        pool |= marks.get(*index).copied().unwrap_or(0);
    }
    pool &= !used;
    let available: Vec<i32> = digits(pool).map(i32::from).collect();
    let n = others.len();
    if available.len() < n {
        return false;
    }
    let least: i32 = available.iter().take(n).sum();
    let most: i32 = available.iter().rev().take(n).sum();
    needed >= least && needed <= most
}

struct Search<'a> {
    puzzle: &'a Kakuro,
    budget: Budget,
    nodes: u32,
    found: u32,
    witness: Option<State>,
}

impl Search<'_> {
    /// Guess, recurse, and count answers up to two. Returns whether the budget ran
    /// out, which is not an answer and must never be read as one.
    fn descend(&mut self, marks: &mut Marks, features: &mut Features, depth: u32) -> bool {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.budget.max_nodes || depth > self.budget.max_depth {
            return true;
        }
        features.max_search_depth = features.max_search_depth.max(depth);

        // The square with the fewest candidates, lowest numbered to break the tie.
        // Both tie-breaks are fixed so two runs on one puzzle do the same work.
        let next = marks
            .iter()
            .enumerate()
            .filter(|(_, m)| !is_assigned(**m))
            .min_by_key(|(index, m)| (m.count_ones(), *index))
            .map(|(index, _)| index);

        let Some(index) = next else {
            // Every square is written in. Propagation has already checked every run's
            // total and repeats on the way here, so this is an answer — but it is
            // confirmed against the family's own rules rather than trusted, because a
            // solver that agreed with itself would be worth nothing.
            let state = as_state(self.puzzle, marks);
            if !gs_core::ruleset::Ruleset::is_solved(self.puzzle, &state) {
                return false;
            }
            self.found = self.found.saturating_add(1);
            if self.witness.is_none() {
                self.witness = Some(state);
            }
            return false;
        };

        // A guess is about to be taken. Counted here rather than in the loop below,
        // so that one guess at one square counts once however many digits it tries.
        // Reaching this line means a square is unassigned, so propagation did not
        // finish the board and a guess really is needed.
        features.decision_points = features.decision_points.saturating_add(1);

        for digit in digits(marks[index]).collect::<Vec<_>>() {
            let mut branch = marks.clone();
            branch[index] = mask_of(digit);
            if propagate(self.puzzle, &mut branch, features, false).is_err() {
                continue;
            }
            if self.descend(&mut branch, features, depth + 1) {
                return true;
            }
            if self.found >= 2 {
                // Two is already disqualifying, so the rest of the tree is not worth
                // walking.
                return false;
            }
        }
        false
    }
}

/// The board a set of fully narrowed candidates describes.
fn as_state(puzzle: &Kakuro, marks: &Marks) -> State {
    let mut state = State::blank(puzzle.layout.state_dims());
    for (index, mask) in marks.iter().enumerate() {
        let Some(digit) = only_digit(*mask) else {
            continue;
        };
        let Ok(at) = u32::try_from(index) else {
            continue;
        };
        let _ = state.set(at, Cell(digit));
    }
    state
}

/// Fill a layout with digits that break no no-repeats rule, ignoring totals.
///
/// The first step of making a puzzle, and the reason it is here rather than in
/// [`crate::generate`]: it is a search over the same candidate sets, and two searches
/// over one representation is one too many.
///
/// There are no totals yet — they are read off whatever this produces — so the only
/// rule is that no run repeats a digit. That is a graph colouring with nine colours
/// and runs of at most nine squares, which always succeeds, but the search is bounded
/// anyway because nothing in this platform searches without a limit.
#[must_use]
pub fn fill(layout: &Layout, seed: u64, budget: Budget) -> Option<Vec<i16>> {
    let mut rng = gs_core::rng::Rng::from_seed(seed);
    let mut chosen: Vec<Option<i16>> = vec![None; layout.white_count()];
    let order: Vec<WhiteId> = layout.whites().collect();
    let mut nodes = 0_u32;
    if fill_from(layout, &order, 0, &mut chosen, &mut rng, &mut nodes, budget) {
        Some(
            chosen
                .into_iter()
                .map(|d| d.unwrap_or(LEAST_DIGIT))
                .collect(),
        )
    } else {
        None
    }
}

fn fill_from(
    layout: &Layout,
    order: &[WhiteId],
    at: usize,
    chosen: &mut Vec<Option<i16>>,
    rng: &mut gs_core::rng::Rng,
    nodes: &mut u32,
    budget: Budget,
) -> bool {
    *nodes = nodes.saturating_add(1);
    if *nodes > budget.max_nodes {
        return false;
    }
    let Some(white) = order.get(at) else {
        return true;
    };
    // A shuffled digit order per square, so two seeds give different boards rather
    // than the same board with a different layout.
    let mut candidates: Vec<i16> = (LEAST_DIGIT..=MOST_DIGIT).collect();
    for i in (1..candidates.len()).rev() {
        let j = u32::try_from(i + 1)
            .ok()
            .and_then(|n| rng.below(n))
            .and_then(|j| usize::try_from(j).ok())
            .unwrap_or(0);
        candidates.swap(i, j);
    }
    for digit in candidates {
        if clashes(layout, chosen, *white, digit) {
            continue;
        }
        let Ok(index) = usize::try_from(white.0) else {
            continue;
        };
        chosen[index] = Some(digit);
        if fill_from(layout, order, at + 1, chosen, rng, nodes, budget) {
            return true;
        }
        chosen[index] = None;
    }
    false
}

/// Would writing `digit` in `white` repeat a digit in either of its runs?
fn clashes(layout: &Layout, chosen: &[Option<i16>], white: WhiteId, digit: i16) -> bool {
    let Some((across, down)) = layout.runs_of(white) else {
        return true;
    };
    [across, down].into_iter().any(|id| {
        layout.run(id).is_some_and(|run| {
            run.squares.iter().any(|other| {
                *other != white
                    && usize::try_from(other.0)
                        .ok()
                        .and_then(|i| chosen.get(i).copied().flatten())
                        == Some(digit)
            })
        })
    })
}

/// Find an answer that is **not** the board given, if there is one.
///
/// [`solve`] reports whether there are several answers but hands back the first it
/// finds, which may be the very board the puzzle was made from. The generator needs
/// the other one: knowing *where* a second answer differs is what tells it which
/// square is carrying the ambiguity. See [`crate::generate`].
///
/// Returns `None` when the board given is the only answer, when there is none, or
/// when the budget ran out — the caller treats all three the same way, as "no
/// counterexample to work with", and none of them is a claim that the puzzle is
/// unique. Only [`solve`] answers that question, and only an SMT solver proves it.
#[must_use]
pub fn second_answer(puzzle: &Kakuro, ours: &State, budget: Budget) -> Option<State> {
    let layout = &puzzle.layout;
    let mut marks: Marks = vec![ALL; layout.white_count()];
    let mut features = Features::default();
    propagate(puzzle, &mut marks, &mut features, false).ok()?;

    let mut search = Other {
        puzzle,
        ours,
        budget,
        nodes: 0,
        found: None,
    };
    search.descend(&mut marks, 0);
    search.found
}

struct Other<'a> {
    puzzle: &'a Kakuro,
    ours: &'a State,
    budget: Budget,
    nodes: u32,
    found: Option<State>,
}

impl Other<'_> {
    /// The same search as [`Search::descend`], stopping at the first answer that is
    /// not `ours`. Separate rather than a flag on the other one, because the two have
    /// different stopping rules and a shared function with a mode is how a counting
    /// bug gets in.
    fn descend(&mut self, marks: &mut Marks, depth: u32) -> bool {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.budget.max_nodes || depth > self.budget.max_depth {
            return true;
        }
        let next = marks
            .iter()
            .enumerate()
            .filter(|(_, m)| !is_assigned(**m))
            .min_by_key(|(index, m)| (m.count_ones(), *index))
            .map(|(index, _)| index);

        let Some(index) = next else {
            let state = as_state(self.puzzle, marks);
            if state != *self.ours && gs_core::ruleset::Ruleset::is_solved(self.puzzle, &state) {
                self.found = Some(state);
            }
            return false;
        };

        for digit in digits(marks[index]).collect::<Vec<_>>() {
            let mut branch = marks.clone();
            branch[index] = mask_of(digit);
            let mut ignored = Features::default();
            if propagate(self.puzzle, &mut branch, &mut ignored, false).is_err() {
                continue;
            }
            if self.descend(&mut branch, depth + 1) {
                return true;
            }
            if self.found.is_some() {
                return false;
            }
        }
        false
    }
}
