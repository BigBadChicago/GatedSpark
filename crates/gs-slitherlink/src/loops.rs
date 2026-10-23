//! The one rule that makes this family different: the drawn edges must form a
//! single closed loop.
//!
//! Nonogram's goal test is a per-line comparison, so it reads the board one line at
//! a time and never looks at the board as a whole. This one cannot: whether a set of
//! edges is *one* loop rather than two is a fact about the whole board at once. That
//! is what the core's DSL reserved [`gs_core::dsl::Global::SingleClosedLoop`] for,
//! and it is why the family options review called this the right second family.
//!
//! # What "one closed loop" means here, exactly
//!
//! A set of drawn edges is one closed loop when both of these hold:
//!
//! * **Every dot has nought or two drawn edges.** Nought means the loop does not
//!   pass through; two means it goes in and out. One would be a loose end and three
//!   or four would be a crossing, and neither is a loop.
//! * **The drawn edges are all in one piece.** Two separate rings each satisfy the
//!   first rule on their own, so the first rule alone is not enough — and missing
//!   that is the classic way a Slitherlink checker comes to accept a wrong board.
//!
//! Those two together are exactly "one loop": nought-or-two makes every piece a
//! ring, and one piece makes it one ring.
//!
//! # This is a checker, not a solver
//!
//! Everything here reads a finished board and says yes or no. Nothing in this module
//! searches, guesses, or produces a [`gs_core::verification::Outcome`]. The same
//! separation the Nonogram line solver has: a fast local judgement may filter and
//! may measure, and only a real solver answers the question the platform publishes.

use gs_core::state::{Cell, State};

use crate::board::{DotId, EdgeId, Geometry};

/// What a set of drawn edges turned out to be.
///
/// Spelled out rather than collapsed to a boolean so a refusal can say what is
/// wrong. A player's board is nearly always wrong in one of these specific ways, and
/// "not a loop" is not a helpful thing to put on a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Exactly one closed loop.
    OneLoop,
    /// No edges are drawn at all. Distinct from a wrong loop: a blank board is where
    /// everyone starts, and calling that a mistake would be unkind as well as wrong.
    Nothing,
    /// A dot has one drawn edge: a line that stops in mid-air.
    LooseEnd(DotId),
    /// A dot has three or four drawn edges: lines crossing or branching.
    Crossing(DotId),
    /// Every dot is nought-or-two, so every piece is a ring, but there is more than
    /// one piece. Carries how many.
    SeveralLoops(u32),
}

impl Shape {
    #[must_use]
    pub fn is_one_loop(self) -> bool {
        matches!(self, Shape::OneLoop)
    }

    /// A sentence a player can read.
    ///
    /// Written here next to the judgement, rather than in the page, because a page
    /// that writes its own wording for a rule has started deciding what the rule
    /// means. See `gs_web::plainly` for the same arrangement for refused files.
    #[must_use]
    pub fn plainly(self) -> &'static str {
        match self {
            Shape::OneLoop => "the lines make one closed loop",
            Shape::Nothing => "no lines are drawn yet",
            Shape::LooseEnd(_) => "a line stops in mid-air instead of coming back round",
            Shape::Crossing(_) => "too many lines meet at one corner",
            Shape::SeveralLoops(_) => "there is more than one loop, and there may only be one",
        }
    }
}

/// How a board's marks read as drawn edges.
///
/// An undecided edge counts as **not drawn**, matching the decision the project
/// owner made for Nonogram on 2026-10-23: crossing a thing out is a note the player
/// keeps for themselves, and a player whose drawn lines are right has finished
/// whether or not they marked the rest. See `rules::is_solved`.
#[must_use]
pub fn drawn_in(state: &State, edge: EdgeId) -> bool {
    state.get(edge.0).map(|c| c == crate::rules::LINE) == Ok(true)
}

/// How many drawn edges meet at each dot, in dot order.
#[must_use]
pub fn degrees(geometry: Geometry, drawn: &dyn Fn(EdgeId) -> bool) -> Vec<u8> {
    geometry
        .dots()
        .map(|dot| {
            let count = geometry
                .edges_of_dot(dot)
                .into_iter()
                .filter(|e| drawn(*e))
                .count();
            u8::try_from(count).unwrap_or(u8::MAX)
        })
        .collect()
}

/// Decide what a set of drawn edges is.
///
/// Takes a predicate rather than a board so that the generator, the fast solver and
/// the engine all reach the same judgement through the same function. Two copies of
/// this rule would be two chances to disagree about whether a board is finished.
#[must_use]
pub fn shape_of(geometry: Geometry, drawn: &dyn Fn(EdgeId) -> bool) -> Shape {
    let degrees = degrees(geometry, drawn);
    let mut any = false;
    for (index, degree) in degrees.iter().enumerate() {
        let dot = DotId(u32::try_from(index).unwrap_or(u32::MAX));
        match degree {
            0 => {}
            1 => return Shape::LooseEnd(dot),
            2 => any = true,
            _ => return Shape::Crossing(dot),
        }
    }
    if !any {
        return Shape::Nothing;
    }
    let pieces = count_pieces(geometry, drawn, &degrees);
    if pieces == 1 {
        Shape::OneLoop
    } else {
        Shape::SeveralLoops(pieces)
    }
}

/// How many separate pieces the drawn edges form.
///
/// A plain flood fill over the dots the loop passes through, walking only drawn
/// edges. Called after the nought-or-two check, so every piece is already known to
/// be a ring and counting pieces is the whole of what is left.
fn count_pieces(geometry: Geometry, drawn: &dyn Fn(EdgeId) -> bool, degrees: &[u8]) -> u32 {
    let mut seen = vec![false; geometry.dot_count()];
    let mut pieces = 0u32;
    for start in geometry.dots() {
        let at = usize::try_from(start.0).unwrap_or(usize::MAX);
        if degrees.get(at).copied().unwrap_or(0) == 0 || seen.get(at).copied().unwrap_or(true) {
            continue;
        }
        pieces = pieces.saturating_add(1);
        let mut pending = vec![start];
        seen[at] = true;
        while let Some(dot) = pending.pop() {
            for edge in geometry.edges_of_dot(dot) {
                if !drawn(edge) {
                    continue;
                }
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
    }
    pieces
}

/// A set of edges that **is** one closed loop.
///
/// The same job [`crate::clues::Pattern`]'s Nonogram counterpart does: hold the thing
/// the generator drew, in a type that cannot hold something else. There is no way to
/// build one except through [`Loop::of`], which checks, so a function taking a `Loop`
/// does not have to re-check and cannot forget to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    geometry: Geometry,
    drawn: Vec<bool>,
}

impl Loop {
    /// Check a set of drawn edges and keep it if it is one loop.
    ///
    /// Returns the [`Shape`] it found instead, so a caller that was trying to draw a
    /// loop learns what it drew.
    pub fn of(geometry: Geometry, drawn: Vec<bool>) -> Result<Self, Shape> {
        if drawn.len() != geometry.edge_count() {
            return Err(Shape::Nothing);
        }
        let at = |edge: EdgeId| {
            usize::try_from(edge.0)
                .ok()
                .and_then(|i| drawn.get(i).copied())
                .unwrap_or(false)
        };
        match shape_of(geometry, &at) {
            Shape::OneLoop => Ok(Loop { geometry, drawn }),
            other => Err(other),
        }
    }

    #[must_use]
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }

    #[must_use]
    pub fn is_drawn(&self, edge: EdgeId) -> bool {
        usize::try_from(edge.0)
            .ok()
            .and_then(|i| self.drawn.get(i).copied())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn edges_drawn(&self) -> u32 {
        u32::try_from(self.drawn.iter().filter(|d| **d).count()).unwrap_or(u32::MAX)
    }

    /// This loop as a board a player could be holding: every edge decided.
    ///
    /// Every edge, not only the drawn ones. A stored answer has to decide every edge
    /// for the same reason a Nonogram witness has to decide every square: it is the
    /// record of what the solver found, and a half-marked one would hash to a
    /// different value than the board anyone checked.
    #[must_use]
    pub fn as_state(&self) -> State {
        let mut state = State::blank(self.geometry.state_dims());
        for edge in self.geometry.edges() {
            let value = if self.is_drawn(edge) {
                crate::rules::LINE
            } else {
                crate::rules::NO_LINE
            };
            let _ = state.set(edge.0, value);
        }
        state
    }
}

/// Read a board's marks as a loop, if they are one.
pub fn loop_in(geometry: Geometry, state: &State) -> Result<Loop, Shape> {
    let drawn = geometry.edges().map(|e| drawn_in(state, e)).collect();
    Loop::of(geometry, drawn)
}

/// Whether a cell value is a mark this family understands.
#[must_use]
pub fn is_edge_value(value: Cell) -> bool {
    value == crate::rules::LINE || value == crate::rules::NO_LINE || value == Cell::UNKNOWN
}
