//! Transition semantics: `S(t+1) = F(S(t), A(t), R)`.
//!
//! `R` is the versioned ruleset, and it is versioned in the type's data rather than
//! by convention, because the authoritative record has to name the ruleset that
//! produced it for a replay to mean anything.

use crate::registry::FamilyId;
use crate::state::{Delta, State};

/// A ruleset's identity. Two records that disagree here are not comparable, and
/// replay must refuse rather than guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RulesetId {
    pub family: FamilyId,
    /// Bumped whenever the *semantics* change, which is what invalidates a replay.
    pub version: u32,
}

/// Why an action was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Illegal {
    /// The action refers to something outside the board.
    OutOfBounds,
    /// The action is well-formed but not permitted in this state.
    NotPermitted(&'static str),
    /// The action would leave the state unchanged. A no-op is not a move: letting
    /// one into the timeline makes undo counts disagree with what the player did.
    NoOp,
}

/// The rules of one puzzle family.
///
/// `F` is `apply`, and it is the *only* way the authoritative state changes. Nothing
/// speculative may call it on real state — the prediction engine, when it exists,
/// works on clones.
pub trait Ruleset {
    /// The family's action type. Families define their own, so an action for one
    /// family cannot be fed to another.
    type Action: Clone + PartialEq;

    fn id(&self) -> RulesetId;

    /// Compute the effect of `action` on `state` **without** mutating it.
    ///
    /// Returning the delta rather than applying it is what keeps one definition of
    /// "apply": the timeline applies every delta through [`Delta::apply`], whether it
    /// came from a move, a redo, or a replay.
    fn transition(&self, state: &State, action: &Self::Action) -> Result<Delta, Illegal>;

    /// Whether `state` satisfies the goal predicate `G`.
    fn is_solved(&self, state: &State) -> bool;
}
