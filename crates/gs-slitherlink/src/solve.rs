//! A fast solver of our own: the generation filter, and where the difficulty
//! numbers come from.
//!
//! # This is not the verification gate, and it cannot become one
//!
//! The same arrangement as `gs_nonogram::solve`, for the same reason. This module
//! produces [`Solved`] and [`Features`], and there is no path from either of them to
//! a [`gs_core::verification::Outcome`]. Only a real SMT solver, fed by
//! [`crate::smtlib`], produces an `Outcome`, and only an `Outcome` reaches the gate.
//! A filter that is wrong costs throughput; a trusted heuristic that is wrong
//! publishes a puzzle with no answer.
//!
//! Its two legitimate jobs:
//!
//! * **The generation filter.** Making a Slitherlink means taking numbers away and
//!   asking "is there still exactly one answer?" after each one. On a 10 by 10 board
//!   that is about a hundred questions per puzzle, so they cannot all go to an
//!   outside program. This answers them cheaply, and the real solver has the last
//!   word on whatever survives.
//! * **The difficulty numbers.** How much of the board falls out without guessing,
//!   and how deep the guessing goes when it is needed.
//!
//! # How it works
//!
//! Propagate forced deductions to a fixpoint, then guess an edge and recurse,
//! counting answers and stopping at two. The deductions are the ones a person makes:
//!
//! * A cell whose clue is already met has no more sides drawn; a cell that needs
//!   every remaining side has them all drawn.
//! * A dot with two lines has no more; a dot with one line and one undecided edge
//!   must use it; a dot with no lines and one undecided edge must not, because one
//!   line at a dot is not allowed.
//! * **The loop-closing rule**, which is this family's own and the one that stops the
//!   search wandering: once every dot has nought or two lines and at least one line
//!   is drawn, nothing more can ever be added — extending a ring would need a third
//!   line at some dot, and starting another ring would make a second loop. So the
//!   board is final then and there, and it is either the answer or a dead end.

use gs_core::state::State;

use crate::board::{CellId, DotId, EdgeId, Geometry};
use crate::loops::Shape;
use crate::rules::{Slitherlink, LINE, NO_LINE};

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solved {
    /// No board satisfies the clues.
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
/// never names a level, and this struct has no level in it. Slitherlink's own levels
/// will be decided from measurements of real puzzles, in [`crate::difficulty`], and
/// not guessed at here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    /// Edges decided by forced deductions alone, before any guess.
    pub propagation_only_edges: u32,
    /// How many times a forced deduction settled at least one edge, before the first
    /// guess.
    ///
    /// Not the same as Nonogram's `propagation_rounds`, which counts passes over the
    /// whole board. The deductions here run from a worklist — only the clues and
    /// corners next to a just-decided edge are looked at again — so there are no
    /// passes to count, and counting them would have meant doing useless work to
    /// produce a number. This counts the deductions themselves, which is the more
    /// useful measure anyway: it is how long the chain of reasoning was.
    pub forced_steps: u32,
    /// Guesses taken.
    ///
    /// Zero means the puzzle needed no guessing **from this solver, with the
    /// deductions this solver knows** — which is not the same as "a person needs no
    /// guesses", and the difference is bigger here than it is for Nonogram. That
    /// family's line solver is complete for a single line, so "no guess needed" is a
    /// fact about the puzzle. The deductions below are a small, honest subset of what
    /// a Slitherlink player knows; the well-known corner and diagonal patterns are
    /// not in them. So a board a person would find easy can still cost this solver a
    /// guess, and measured on 2026-10-23 a fully numbered 5 by 5 board costs exactly
    /// one.
    ///
    /// That matters for difficulty, so it is written here rather than discovered
    /// later: this number grades puzzles **against this solver**, and it may only be
    /// compared with other numbers from the same solver. Adding a deduction rule
    /// changes it for every puzzle, which is why `GENERATOR_VERSION` exists.
    pub decision_points: u32,
    /// Deepest nesting of guesses.
    pub max_search_depth: u32,
    /// Edges on the board.
    pub edges: u32,
    /// How many cells carry a clue. Not a measurement of difficulty on its own — a
    /// bare board can be easy and a crowded one hard — but it is what a player sees
    /// first, so it is worth carrying.
    pub clues_given: u32,
}

impl Features {
    /// Whether the puzzle needed no guessing from this solver. Read the warning on
    /// [`Features::decision_points`] before treating that as a fact about the puzzle.
    #[must_use]
    pub fn needs_no_guessing(&self) -> bool {
        self.decision_points == 0
    }

    /// The share of edges the forced deductions settled on their own, in tenths of a
    /// percent, so the feature vector stays integer-only like everything else.
    ///
    /// **Read the warning on `gs_nonogram::solve::Features::forced_ratio_per_mille`
    /// before using this to grade anything.** That number turned out to be exactly
    /// 1000 for every Nonogram that needs no guessing, because it is counted at the
    /// first fixpoint and such a puzzle is already finished there, so it carried no
    /// more information than the yes-or-no. The same trap is here. Whether this
    /// family's levels should be graded on [`Features::forced_steps`] instead
    /// is a question for measurement, not for a guess, and it is not answered yet.
    #[must_use]
    pub fn forced_ratio_per_mille(&self) -> u32 {
        if self.edges == 0 {
            return 0;
        }
        self.propagation_only_edges.saturating_mul(1000) / self.edges
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
        // Generous enough for the board sizes a person plays, small enough that a
        // pathological candidate is dropped rather than chased. Slitherlink gets more
        // than Nonogram's 20,000 because a board has far more edges than a Nonogram
        // of the same size has squares.
        Budget {
            max_nodes: 200_000,
            max_depth: 128,
        }
    }
}

/// One edge's state during a solve. `None` is undecided.
type Marks = Vec<Option<bool>>;

/// Why a partial board cannot become an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeadEnd;

/// What a propagation run settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settled {
    /// Nothing more is forced, and undecided edges remain.
    Fixpoint,
    /// A ring is closed and this board satisfies the puzzle, so it is an answer.
    /// Nothing can be added to it; see the loop-closing rule in this module's notes.
    Answer,
}

/// Solve, counting answers up to two.
#[must_use]
pub fn solve(puzzle: &Slitherlink, budget: Budget) -> Analysis {
    let geometry = puzzle.geometry();
    let mut marks: Marks = vec![None; geometry.edge_count()];
    let mut features = Features {
        edges: u32::try_from(geometry.edge_count()).unwrap_or(u32::MAX),
        clues_given: puzzle.clues.given_count(),
        ..Features::default()
    };

    // The first run to a fixpoint is measured separately, because what it settles on
    // its own is the difficulty signal. Anything after a guess is not.
    let mut steps = 0;
    let first = propagate(puzzle, &mut marks, &mut steps);
    features.forced_steps = steps;
    features.propagation_only_edges =
        u32::try_from(marks.iter().filter(|m| m.is_some()).count()).unwrap_or(u32::MAX);

    let mut found: Vec<Marks> = Vec::new();
    let mut nodes = 0u32;
    let ran_out = match first {
        Err(DeadEnd) => false,
        Ok(settled) => search(
            puzzle,
            marks,
            settled,
            0,
            &mut Work {
                budget,
                nodes: &mut nodes,
                features: &mut features,
                found: &mut found,
            },
        ),
    };

    let witness = found.first().map(|m| state_of(geometry, m));
    let solved = match (found.len(), ran_out) {
        (0, true) => Solved::Undecided,
        (0, false) => Solved::None,
        (1, true) => Solved::Undecided,
        (1, false) => Solved::One,
        _ => Solved::Several,
    };
    Analysis {
        solved,
        features,
        witness,
    }
}

/// The mutable parts of a search, in one place so the recursion takes one argument
/// rather than six.
struct Work<'a> {
    budget: Budget,
    nodes: &'a mut u32,
    features: &'a mut Features,
    found: &'a mut Vec<Marks>,
}

/// Search from a propagated board. Returns whether the budget ran out.
///
/// Stops as soon as two answers are found: more than one is already disqualifying.
fn search(
    puzzle: &Slitherlink,
    marks: Marks,
    settled: Settled,
    depth: u32,
    work: &mut Work<'_>,
) -> bool {
    *work.nodes = work.nodes.saturating_add(1);
    if *work.nodes > work.budget.max_nodes || depth > work.budget.max_depth {
        return true;
    }
    work.features.max_search_depth = work.features.max_search_depth.max(depth);

    if settled == Settled::Answer {
        // Already checked against the engine's own goal test by `ring_state`, so this
        // is recorded rather than re-judged: one definition of "finished" per crate.
        //
        // The board is completed first. A closed ring can never be added to, so every
        // still-undecided edge is forced to be undrawn — this is filling in a
        // deduction, not making an assumption. It matters because a *stored answer*
        // must decide every edge (`publish::replay_of`), while the engine's own
        // "finished" test does not care, so an answer handed on undecided would be
        // refused by the very next step for a reason that is nobody's fault.
        work.found.push(finished(marks));
        return false;
    }

    let Some(edge) = next_guess(puzzle, &marks) else {
        // Nothing undecided and not final: some dot has one line, or there are no
        // lines at all. Either way it is not an answer.
        return false;
    };
    work.features.decision_points = work.features.decision_points.saturating_add(1);

    let mut ran_out = false;
    for value in [true, false] {
        if work.found.len() >= 2 {
            break;
        }
        let mut branch = marks.clone();
        branch[usize::try_from(edge.0).unwrap_or(0)] = Some(value);
        // Deductions after a guess are not part of the difficulty measurement, so
        // they are counted into a number nothing reads.
        let mut after_a_guess = 0;
        match propagate(puzzle, &mut branch, &mut after_a_guess) {
            Err(DeadEnd) => {}
            Ok(settled) => {
                ran_out |= search(puzzle, branch, settled, depth + 1, work);
            }
        }
    }
    ran_out
}

/// Which undecided edge to guess next.
///
/// Prefers an edge at a dot that already has exactly one line, because the loop has
/// to continue there and the guess is between few options. That is the difference
/// between a search that finishes and one that wanders: without it, an early guess in
/// an empty corner of the board decides nothing and the whole subtree is explored
/// twice for nothing.
fn next_guess(puzzle: &Slitherlink, marks: &Marks) -> Option<EdgeId> {
    let geometry = puzzle.geometry();
    let mut fallback = None;
    for dot in geometry.dots() {
        let (drawn, undecided) = at_dot(geometry, marks, dot);
        if undecided.is_empty() {
            continue;
        }
        if drawn == 1 {
            return undecided.first().copied();
        }
        if fallback.is_none() {
            fallback = undecided.first().copied();
        }
    }
    fallback
}

/// Apply every forced deduction until nothing more is forced.
///
/// Driven by a worklist rather than by passes over the board. When an edge is
/// settled, only the clues and corners touching that edge can have changed, so only
/// those go back on the list. The pass-over-everything version was measured on
/// 2026-10-23 at about 100 times the work per search node, which is the difference
/// between a 10 by 10 board being practical and not.
fn propagate(puzzle: &Slitherlink, marks: &mut Marks, steps: &mut u32) -> Result<Settled, DeadEnd> {
    let geometry = puzzle.geometry();
    let mut cell_queued = vec![false; geometry.cell_count()];
    let mut dot_queued = vec![false; geometry.dot_count()];
    let mut cells: Vec<(CellId, u8)> = puzzle.clues.given().collect();
    for (cell, _) in &cells {
        if let Some(slot) = cell_queued.get_mut(usize::try_from(cell.0).unwrap_or(usize::MAX)) {
            *slot = true;
        }
    }
    let mut dots: Vec<DotId> = geometry.dots().collect();
    for dot in &dots {
        if let Some(slot) = dot_queued.get_mut(usize::try_from(dot.0).unwrap_or(usize::MAX)) {
            *slot = true;
        }
    }

    while !cells.is_empty() || !dots.is_empty() {
        let mut settled: Vec<EdgeId> = Vec::new();
        if let Some((cell, clue)) = cells.pop() {
            if let Some(slot) = cell_queued.get_mut(usize::try_from(cell.0).unwrap_or(usize::MAX)) {
                *slot = false;
            }
            from_clue(geometry, marks, cell, clue, &mut settled)?;
        } else if let Some(dot) = dots.pop() {
            if let Some(slot) = dot_queued.get_mut(usize::try_from(dot.0).unwrap_or(usize::MAX)) {
                *slot = false;
            }
            from_dot(geometry, marks, dot, &mut settled)?;
        }
        if settled.is_empty() {
            continue;
        }
        *steps = steps.saturating_add(1);
        for edge in settled {
            for cell in geometry.cells_of(edge) {
                let at = usize::try_from(cell.0).unwrap_or(usize::MAX);
                if cell_queued.get(at) == Some(&false) {
                    if let Some(clue) = puzzle.clues.at(cell) {
                        cell_queued[at] = true;
                        cells.push((cell, clue));
                    }
                }
            }
            if let Some((a, b)) = geometry.ends_of(edge) {
                for dot in [a, b] {
                    let at = usize::try_from(dot.0).unwrap_or(usize::MAX);
                    if dot_queued.get(at) == Some(&false) {
                        dot_queued[at] = true;
                        dots.push(dot);
                    }
                }
            }
        }
    }

    // The loop-closing rule, checked once nothing else is forced.
    match ring_state(geometry, marks, puzzle) {
        RingState::Answer => return Ok(Settled::Answer),
        RingState::DeadEnd => return Err(DeadEnd),
        RingState::NoRing => {}
    }
    if marks.iter().all(Option::is_some) {
        // Everything decided, no ring closed: some dot has a single line, or nothing
        // is drawn at all. A dead end rather than a fixpoint, so the caller does not
        // treat it as a board worth guessing on.
        return Err(DeadEnd);
    }
    Ok(Settled::Fixpoint)
}

/// The drawn count and the undecided edges at a dot.
fn at_dot(geometry: Geometry, marks: &Marks, dot: DotId) -> (usize, Vec<EdgeId>) {
    let mut drawn = 0;
    let mut undecided = Vec::new();
    for edge in geometry.edges_of_dot(dot) {
        match marks.get(usize::try_from(edge.0).unwrap_or(usize::MAX)) {
            Some(Some(true)) => drawn += 1,
            Some(None) => undecided.push(edge),
            _ => {}
        }
    }
    (drawn, undecided)
}

/// What a cell's clue forces.
fn from_clue(
    geometry: Geometry,
    marks: &mut Marks,
    cell: CellId,
    clue: u8,
    settled: &mut Vec<EdgeId>,
) -> Result<(), DeadEnd> {
    let Some(sides) = geometry.edges_of_cell(cell) else {
        return Err(DeadEnd);
    };
    let clue = usize::from(clue);
    let mut drawn = 0;
    let mut undecided = Vec::new();
    for side in sides {
        match marks.get(usize::try_from(side.0).unwrap_or(usize::MAX)) {
            Some(Some(true)) => drawn += 1,
            Some(None) => undecided.push(side),
            _ => {}
        }
    }
    if drawn > clue || drawn + undecided.len() < clue {
        return Err(DeadEnd);
    }
    if undecided.is_empty() {
        return Ok(());
    }
    if drawn == clue {
        set_all(marks, &undecided, false, settled);
        return Ok(());
    }
    if drawn + undecided.len() == clue {
        set_all(marks, &undecided, true, settled);
        return Ok(());
    }
    Ok(())
}

/// What the nought-or-two rule forces at a dot.
fn from_dot(
    geometry: Geometry,
    marks: &mut Marks,
    dot: DotId,
    settled: &mut Vec<EdgeId>,
) -> Result<(), DeadEnd> {
    let (drawn, undecided) = at_dot(geometry, marks, dot);
    if drawn > 2 {
        return Err(DeadEnd);
    }
    if drawn == 1 && undecided.is_empty() {
        // One line into a dot and nowhere to go: a loose end.
        return Err(DeadEnd);
    }
    if undecided.is_empty() {
        return Ok(());
    }
    if drawn == 2 {
        set_all(marks, &undecided, false, settled);
        return Ok(());
    }
    if drawn == 1 && undecided.len() == 1 {
        set_all(marks, &undecided, true, settled);
        return Ok(());
    }
    if drawn == 0 && undecided.len() == 1 {
        // Drawing it would leave exactly one line at this dot, which is not allowed.
        set_all(marks, &undecided, false, settled);
        return Ok(());
    }
    Ok(())
}

fn set_all(marks: &mut Marks, edges: &[EdgeId], value: bool, settled: &mut Vec<EdgeId>) {
    for edge in edges {
        if let Some(slot) = marks.get_mut(usize::try_from(edge.0).unwrap_or(usize::MAX)) {
            *slot = Some(value);
            settled.push(*edge);
        }
    }
}

/// Whether a closed ring exists in the drawn edges, and if so whether this board is
/// final or already doomed.
///
/// This is the rule that keeps the search from wandering, and it is this family's
/// own. Once a ring is closed, nothing can ever be added to it: extending it would
/// need a third line at some dot, and starting a second ring would make a second
/// loop. So a closed ring plus anything else at all — another drawn line somewhere
/// else, or a clue still short of its number — is a dead end *now*, rather than
/// after the rest of the board has been explored.
fn ring_state(geometry: Geometry, marks: &Marks, puzzle: &Slitherlink) -> RingState {
    let drawn_at = |edge: EdgeId| {
        marks.get(usize::try_from(edge.0).unwrap_or(usize::MAX)) == Some(&Some(true))
    };
    let mut degree = vec![0u8; geometry.dot_count()];
    let mut drawn_total = 0u32;
    for edge in geometry.edges() {
        if !drawn_at(edge) {
            continue;
        }
        drawn_total += 1;
        if let Some((a, b)) = geometry.ends_of(edge) {
            for end in [a, b] {
                if let Some(slot) = degree.get_mut(usize::try_from(end.0).unwrap_or(usize::MAX)) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
    }
    if drawn_total == 0 {
        return RingState::NoRing;
    }

    // Walk each piece of the drawn edges. A piece every one of whose dots has two
    // lines is a closed ring.
    let mut seen = vec![false; geometry.dot_count()];
    for start in geometry.dots() {
        let at = usize::try_from(start.0).unwrap_or(usize::MAX);
        if degree.get(at).copied().unwrap_or(0) == 0 || seen.get(at).copied().unwrap_or(true) {
            continue;
        }
        let mut ends_counted = 0u32;
        let mut all_two = true;
        let mut pending = vec![start];
        seen[at] = true;
        while let Some(dot) = pending.pop() {
            let here = usize::try_from(dot.0).unwrap_or(usize::MAX);
            if degree.get(here).copied().unwrap_or(0) != 2 {
                all_two = false;
            }
            for edge in geometry.edges_of_dot(dot) {
                if !drawn_at(edge) {
                    continue;
                }
                ends_counted += 1;
                let Some((a, b)) = geometry.ends_of(edge) else {
                    continue;
                };
                let other = if a == dot { b } else { a };
                let index = usize::try_from(other.0).unwrap_or(usize::MAX);
                if seen.get(index) == Some(&false) {
                    seen[index] = true;
                    pending.push(other);
                }
            }
        }
        if !all_two {
            continue;
        }
        // Each edge of the piece was counted once from each of its two ends.
        let piece_edges = ends_counted / 2;
        // A closed ring, so nothing can be added and the board is decided here.
        if piece_edges < drawn_total {
            return RingState::DeadEnd;
        }
        let state = state_of(geometry, marks);
        if gs_core::ruleset::Ruleset::is_solved(puzzle, &state) {
            return RingState::Answer;
        }
        return RingState::DeadEnd;
    }
    RingState::NoRing
}

/// What the closed-ring rule found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RingState {
    /// No ring is closed yet, so the board can still grow.
    NoRing,
    /// A ring is closed and this board satisfies the puzzle.
    Answer,
    /// A ring is closed and this board does not, so nothing it could become will.
    DeadEnd,
}

/// Every undecided edge ruled out, which is forced once a ring is closed.
fn finished(mut marks: Marks) -> Marks {
    for mark in &mut marks {
        if mark.is_none() {
            *mark = Some(false);
        }
    }
    marks
}

/// A partial board as logical state. Undecided edges stay undecided.
fn state_of(geometry: Geometry, marks: &Marks) -> State {
    let mut state = State::blank(geometry.state_dims());
    for edge in geometry.edges() {
        let value = match marks.get(usize::try_from(edge.0).unwrap_or(usize::MAX)) {
            Some(Some(true)) => LINE,
            Some(Some(false)) => NO_LINE,
            _ => continue,
        };
        let _ = state.set(edge.0, value);
    }
    state
}

/// What this solver thinks of a finished board, for comparing against the engine.
///
/// Reaches the engine's own goal test rather than repeating it, so there is one
/// definition of "finished" in this crate and not two.
#[must_use]
pub fn shape_of_answer(puzzle: &Slitherlink, witness: &State) -> Shape {
    puzzle.shape(witness)
}
