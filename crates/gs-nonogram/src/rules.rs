//! Nonogram's transition semantics and goal predicate.

use gs_core::dsl::{Constraint, Domain, Region, Rules as DslRules};
use gs_core::registry::{Family, FamilyId};
use gs_core::ruleset::{Illegal, Ruleset, RulesetId};
use gs_core::state::{Cell, CellChange, Delta, State};

use crate::clues::Clues;
use crate::line::Mark;

pub const FAMILY: FamilyId = FamilyId("nonogram");

/// Which version of Nonogram's semantics this build implements.
///
/// Named rather than written as a literal inside [`Nonogram::new`], because a reader
/// has to be able to compare a stored package's `ruleset_version` against it. A replay
/// against different semantics is not a replay, so a reader that found a version it
/// does not implement has to refuse the package rather than attempt it — and it cannot
/// do that without something to compare to.
pub const RULESET_VERSION: u32 = 1;

/// The three cell values, as integers. Exact and small; see `gs_core::state`.
pub const EMPTY: Cell = Cell(0);
pub const FILLED: Cell = Cell(1);

/// Translate a stored cell into a line mark. `Cell::UNKNOWN` is a real third state,
/// distinct from `EMPTY`: a player marking a cell known-empty has made a move.
#[must_use]
pub fn mark_of(cell: Cell) -> Mark {
    if cell == FILLED {
        Mark::Filled
    } else if cell == EMPTY {
        Mark::Empty
    } else {
        Mark::Unknown
    }
}

/// What a player can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Set one cell. `Cell::UNKNOWN` clears it, which is a legal move: a player
    /// retracting a guess has not undone it, they have made a new one.
    Set { index: u32, value: Cell },
}

/// Derives `Clone` and `Eq` so a generated puzzle can be carried around and
/// compared. Comparing two `Nonogram`s compares their clues and ruleset version,
/// which is the right notion of sameness here: a puzzle *is* its clues, and nothing
/// in it remembers an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nonogram {
    pub clues: Clues,
    pub version: u32,
}

impl Nonogram {
    #[must_use]
    pub fn new(clues: Clues) -> Self {
        Nonogram {
            clues,
            version: RULESET_VERSION,
        }
    }

    /// The puzzle's rules in the shared DSL.
    ///
    /// Every constraint is a [`Constraint::RunLengths`] over a row or a column, so a
    /// Nonogram's rules are Tier A — which is the whole reason this family is the
    /// slice. Nothing here produces a [`gs_core::dsl::Global`].
    #[must_use]
    pub fn dsl_rules(&self) -> DslRules {
        let mut constraints = Vec::new();
        for (y, runs) in self.clues.rows.iter().enumerate() {
            constraints.push(Constraint::RunLengths {
                region: Region::Row(u16::try_from(y).unwrap_or(u16::MAX)),
                value: FILLED.0,
                runs: runs.clone(),
            });
        }
        for (x, runs) in self.clues.columns.iter().enumerate() {
            constraints.push(Constraint::RunLengths {
                region: Region::Column(u16::try_from(x).unwrap_or(u16::MAX)),
                value: FILLED.0,
                runs: runs.clone(),
            });
        }
        DslRules {
            domain: Domain {
                min: EMPTY.0,
                max: FILLED.0,
            },
            constraints,
        }
    }

    /// The marks of one row, as the line solver wants them.
    #[must_use]
    pub fn row_marks(&self, state: &State, y: u16) -> Vec<Mark> {
        (0..self.clues.dims.width)
            .map(|x| self.mark_at(state, x, y))
            .collect()
    }

    #[must_use]
    pub fn column_marks(&self, state: &State, x: u16) -> Vec<Mark> {
        (0..self.clues.dims.height)
            .map(|y| self.mark_at(state, x, y))
            .collect()
    }

    fn mark_at(&self, state: &State, x: u16, y: u16) -> Mark {
        self.clues
            .dims
            .index_of(x, y)
            .and_then(|i| state.get(i).ok())
            .map_or(Mark::Unknown, mark_of)
    }
}

impl Family for Nonogram {
    fn id(&self) -> FamilyId {
        FAMILY
    }
}

impl Ruleset for Nonogram {
    type Action = Action;

    fn id(&self) -> RulesetId {
        RulesetId {
            family: FAMILY,
            version: self.version,
        }
    }

    fn transition(&self, state: &State, action: &Self::Action) -> Result<Delta, Illegal> {
        let Action::Set { index, value } = *action;
        if value != EMPTY && value != FILLED && value != Cell::UNKNOWN {
            return Err(Illegal::NotPermitted("a cell is filled, empty or unknown"));
        }
        let from = state.get(index).map_err(|_| Illegal::OutOfBounds)?;
        if from == value {
            return Err(Illegal::NoOp);
        }
        Ok(Delta {
            changes: vec![CellChange {
                index,
                from,
                to: value,
            }],
        })
    }

    /// `G`: every line's filled squares match its clue.
    ///
    /// Checking the clues rather than comparing against a stored answer is
    /// deliberate. A Nonogram's goal is its clues; comparing to one remembered
    /// solution would quietly reject a second valid answer, and whether a second
    /// answer exists is the solver's question, asked before publication, not a thing
    /// to paper over at play time.
    ///
    /// **A square left undecided counts as not filled.** Crossing a square out says
    /// nothing the absence of a filled square does not already say, so a player who
    /// fills the right squares has finished whether or not they marked the rest.
    /// This changed on 2026-10-23, on the project owner's decision, after playing a
    /// real 15 by 15 puzzle took 372 taps for 93 filled squares; see ADR 0010.
    ///
    /// Three things this does **not** loosen, each with a test:
    ///
    /// * A board with extra filled squares still fails, because the runs will not
    ///   match. "Only the filled squares count" is not "only the filled squares the
    ///   answer has".
    /// * A blank board still fails any puzzle with a clue in it, because an empty
    ///   line's runs are empty and the clue's are not.
    /// * Nothing published gets easier to produce. [`crate::publish`] requires a
    ///   witness to decide every square, separately from this, so a stored answer is
    ///   still a complete board rather than only the half of one a player needs.
    fn is_solved(&self, state: &State) -> bool {
        let dims = self.clues.dims;
        let rows_match = (0..dims.height).all(|y| {
            let line: Vec<bool> = self
                .row_marks(state, y)
                .iter()
                .map(|m| *m == Mark::Filled)
                .collect();
            self.clues
                .rows
                .get(usize::from(y))
                .is_some_and(|runs| crate::clues::runs_of(&line) == *runs)
        });
        let columns_match = (0..dims.width).all(|x| {
            let line: Vec<bool> = self
                .column_marks(state, x)
                .iter()
                .map(|m| *m == Mark::Filled)
                .collect();
            self.clues
                .columns
                .get(usize::from(x))
                .is_some_and(|runs| crate::clues::runs_of(&line) == *runs)
        });
        rows_match && columns_match
    }
}
