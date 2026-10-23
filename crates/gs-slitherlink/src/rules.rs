//! Slitherlink's transition semantics and goal predicate.

use gs_core::dsl::{Constraint, Domain, Global, Region, Rules as DslRules};
use gs_core::registry::{Family, FamilyId};
use gs_core::ruleset::{Illegal, Ruleset, RulesetId};
use gs_core::state::{Cell, CellChange, Delta, State};

use crate::board::{CellId, EdgeId, Geometry};
use crate::clues::Clues;
use crate::loops::{drawn_in, is_edge_value, shape_of, Shape};

pub const FAMILY: FamilyId = FamilyId("slitherlink");

/// Which version of Slitherlink's semantics this build implements.
///
/// Named rather than written as a literal, for the reason
/// `gs_nonogram::rules::RULESET_VERSION` is: a reader has to be able to compare a
/// stored package's version against what it implements, and refuse rather than
/// attempt a replay against semantics it does not have.
pub const RULESET_VERSION: u32 = 1;

/// The edge is part of the loop.
pub const LINE: Cell = Cell(1);
/// The edge is known not to be part of the loop. A real mark, not the absence of
/// one: a player ruling an edge out has made a move, the same way marking a Nonogram
/// square empty is a move.
pub const NO_LINE: Cell = Cell(0);

/// What a player can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Set one edge. `Cell::UNKNOWN` clears it, which is a legal move.
    Set { edge: EdgeId, value: Cell },
}

/// A Slitherlink puzzle: a board of clues, and the version of the rules it was
/// written against.
///
/// Holds no answer. Comparing two puzzles compares their clues, which is the right
/// notion of sameness: a puzzle *is* its clues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slitherlink {
    pub clues: Clues,
    pub version: u32,
}

impl Slitherlink {
    #[must_use]
    pub fn new(clues: Clues) -> Self {
        Slitherlink {
            clues,
            version: RULESET_VERSION,
        }
    }

    #[must_use]
    pub fn geometry(&self) -> Geometry {
        self.clues.geometry()
    }

    /// A blank board for this puzzle: every edge undecided.
    #[must_use]
    pub fn blank(&self) -> State {
        State::blank(self.geometry().state_dims())
    }

    /// The puzzle's rules in the shared DSL.
    ///
    /// This is the first thing in the project to produce a
    /// [`Global`]. Two kinds of constraint come out:
    ///
    /// * One [`Constraint::Count`] per clued cell, over that cell's four edges. Tier
    ///   A: local, and a count over four things is the easiest constraint in the DSL.
    /// * One [`Global::SingleClosedLoop`], which carries the whole loop rule — both
    ///   "nought or two lines at every corner" and "all in one piece". Those are not
    ///   two constraints here, because splitting them would invite a compiler to
    ///   encode the easy half and quietly skip the other, and the easy half on its
    ///   own accepts two separate loops.
    ///
    /// So these rules are Tier B, and a compiler with no loop encoding must refuse
    /// the whole puzzle by name rather than verify what is left. `gs_nonogram`'s
    /// encoder does exactly that, and its test asserting so is now checked against a
    /// real Tier B family instead of a hand-written stand-in.
    #[must_use]
    pub fn dsl_rules(&self) -> DslRules {
        let geometry = self.geometry();
        let mut constraints = Vec::new();
        for (cell, clue) in self.clues.given() {
            let Some(edges) = geometry.edges_of_cell(cell) else {
                continue;
            };
            constraints.push(Constraint::Count {
                region: Region::Cells(edges.iter().map(|e| e.0).collect()),
                value: LINE.0,
                cmp: gs_core::dsl::Cmp::Eq,
                count: u32::from(clue),
            });
        }
        constraints.push(Constraint::Global(Global::SingleClosedLoop {
            value: LINE.0,
        }));
        DslRules {
            domain: Domain {
                min: NO_LINE.0,
                max: LINE.0,
            },
            constraints,
        }
    }

    /// How many of a cell's four sides are drawn on this board.
    ///
    /// An undecided edge counts as not drawn; see [`is_solved`](Slitherlink::is_solved).
    #[must_use]
    pub fn sides_drawn(&self, state: &State, cell: CellId) -> u8 {
        let geometry = self.geometry();
        let Some(edges) = geometry.edges_of_cell(cell) else {
            return 0;
        };
        u8::try_from(edges.iter().filter(|e| drawn_in(state, **e)).count()).unwrap_or(4)
    }

    /// Which clued cells do not yet have the right number of sides drawn.
    ///
    /// For the page, which wants to grey out a clue that is already satisfied. It
    /// reports rather than decides: the page asks this, it does not work it out.
    pub fn clues_not_met<'a>(&'a self, state: &'a State) -> impl Iterator<Item = CellId> + 'a {
        self.clues
            .given()
            .filter(move |(cell, clue)| self.sides_drawn(state, *cell) != *clue)
            .map(|(cell, _)| cell)
    }

    /// What the drawn edges are: one loop, nothing, or one of the specific ways a
    /// board can be wrong.
    #[must_use]
    pub fn shape(&self, state: &State) -> Shape {
        shape_of(self.geometry(), &|edge| drawn_in(state, edge))
    }
}

impl Family for Slitherlink {
    fn id(&self) -> FamilyId {
        FAMILY
    }
}

impl Ruleset for Slitherlink {
    type Action = Action;

    fn id(&self) -> RulesetId {
        RulesetId {
            family: FAMILY,
            version: self.version,
        }
    }

    fn transition(&self, state: &State, action: &Self::Action) -> Result<Delta, Illegal> {
        let Action::Set { edge, value } = *action;
        if !is_edge_value(value) {
            return Err(Illegal::NotPermitted(
                "an edge is drawn, ruled out, or undecided",
            ));
        }
        let from = state.get(edge.0).map_err(|_| Illegal::OutOfBounds)?;
        if from == value {
            return Err(Illegal::NoOp);
        }
        Ok(Delta {
            changes: vec![CellChange {
                index: edge.0,
                from,
                to: value,
            }],
        })
    }

    /// `G`: the drawn edges make one closed loop, and every clue is met.
    ///
    /// Both halves are needed and neither implies the other. A loop that ignores the
    /// numbers is a loop; numbers that are all satisfied by two separate rings are
    /// satisfied numbers. The puzzle is the pair.
    ///
    /// **An undecided edge counts as not drawn**, which follows the decision the
    /// project owner made for Nonogram on 2026-10-23: ruling an edge out is a note
    /// the player keeps for themselves, and a player whose drawn lines are right has
    /// finished whether or not they marked the rest. It matters more here than it
    /// does there, because a 10 by 10 board has 220 edges and a loop uses perhaps 40
    /// of them, so requiring every edge to be decided would mean 180 extra taps that
    /// change nothing.
    ///
    /// What this does **not** loosen, and `crate::publish` holds separately: a
    /// *stored answer* must still decide every edge.
    fn is_solved(&self, state: &State) -> bool {
        if !self.shape(state).is_one_loop() {
            return false;
        }
        self.clues_not_met(state).next().is_none()
    }
}
