//! A test-only ruleset.
//!
//! Deliberately **not** a puzzle family: it lives in the test tree, is not
//! registered, and has no clue structure, so the no-family-installed suite is
//! unaffected. Its only job is to exercise the transition and timeline machinery
//! without waiting for Nonogram, whose encoding belongs to M2.

use gs_core::registry::FamilyId;
use gs_core::ruleset::{Illegal, Ruleset, RulesetId};
use gs_core::state::{Cell, CellChange, Delta, State};

/// Set one cell to one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub index: u32,
    pub value: Cell,
}

pub struct PaintRules {
    pub version: u32,
}

impl Default for PaintRules {
    fn default() -> Self {
        PaintRules { version: 1 }
    }
}

impl Ruleset for PaintRules {
    type Action = Paint;

    fn id(&self) -> RulesetId {
        RulesetId {
            family: FamilyId("test.paint"),
            version: self.version,
        }
    }

    fn transition(&self, state: &State, action: &Self::Action) -> Result<Delta, Illegal> {
        let from = state.get(action.index).map_err(|_| Illegal::OutOfBounds)?;
        if from == action.value {
            return Err(Illegal::NoOp);
        }
        Ok(Delta {
            changes: vec![CellChange {
                index: action.index,
                from,
                to: action.value,
            }],
        })
    }

    fn is_solved(&self, state: &State) -> bool {
        (0..u32::try_from(state.dims().cell_count()).expect("fits"))
            .all(|i| state.get(i).map(|c| !c.is_unknown()).unwrap_or(false))
    }
}

#[must_use]
pub fn paint(index: u32, value: i16) -> Paint {
    Paint {
        index,
        value: Cell(value),
    }
}
