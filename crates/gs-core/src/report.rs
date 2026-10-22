//! The validation report: the evidence that a puzzle was checked.
//!
//! `design/content-delivery.md` section 6 asks that every accepted artifact be
//! persisted with a content id and a machine-readable report, and names four jobs the
//! report has to serve: re-verifying the corpus when the rules or the solver change,
//! reproducing a player's bug report, retraining difficulty calibration, and audit.
//!
//! # The one rule this file exists to enforce
//!
//! **A report cannot claim a puzzle is publishable unless the queries in it say so.**
//! [`ValidationReport::publishable`] does not re-decide anything; it hands the queries
//! to [`verification::decide`], the single definition of that judgement. So a report
//! and the gate cannot drift apart, and a report with no queries in it refuses rather
//! than defaulting to yes.
//!
//! The three outcomes stay distinct all the way into the stored record. An `UNKNOWN`
//! is written down as `UNKNOWN`, never flattened into a failure or a pass, because the
//! difference is what a later audit needs: a timeout means nobody knows, and that is
//! not the same as a refutation.
//!
//! # No clock
//!
//! There is no timestamp field, and that is deliberate rather than an omission. The
//! architecture plan's rule is that nothing deciding publication may read a wall
//! clock. A `generated_at` would be the obvious thing to add and the obvious thing to
//! start comparing against, and a record whose validity depends on the reader's clock
//! cannot be re-verified years later, which is the first of the four jobs above.
//! Whatever needs to order these records can do it outside the decision.

use crate::content::ContentId;
use crate::hash::StateHash;
use crate::package::Sealed;
use crate::ruleset::RulesetId;
use crate::verification::{decide, Query, Refusal};

/// What happened when the witness solution was replayed through the real engine.
///
/// A witness the engine will not accept is not evidence of anything, however happy a
/// solver was with it: the thing a player will actually run is the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replay {
    /// The engine reproduced the witness and finished at this state.
    Reproduced(StateHash),
    /// The engine did not reproduce it. The reason is a static string so a report can
    /// be compared byte for byte.
    Refused(&'static str),
    /// Not attempted. Distinct from a refusal, for the same reason `UNKNOWN` is
    /// distinct from `UNSAT`.
    NotAttempted,
}

impl Replay {
    #[must_use]
    pub fn reproduced(self) -> bool {
        matches!(self, Replay::Reproduced(_))
    }
}

/// One measured difficulty number, with the name the game gave it.
///
/// Carried as name-and-number pairs rather than a fixed struct because the platform
/// does not know what a family measures, and the user settled that difficulty is a
/// game-level matter. The platform stores and transports these; it never interprets
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    pub name: &'static str,
    pub value: u32,
}

/// Why a report cannot be published.
/// Not `Copy`, because `Refusal` is not. Nothing here is on a hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPublishable {
    /// The verification queries themselves say no. Carries the gate's own refusal.
    Verification(Refusal),
    /// The witness was never replayed through the engine, or the engine refused it.
    WitnessNotReproduced,
    /// No content id, so there is nothing to bind a signature to. This is the normal
    /// state of a report that has not been sealed yet, rather than a fault: see
    /// [`ValidationReport::bind_to`].
    NoContentId,
}

/// The evidence for one puzzle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// The fingerprint of the packaged bytes. `None` until a digest exists, which is
    /// an honest gap rather than a zero standing in for one.
    pub content_id: Option<ContentId>,
    /// Which family and which version of its rules. A report from different semantics
    /// is not comparable with this one, and the version is how that is noticed.
    pub ruleset: RulesetId,
    /// Which version of the generator produced it.
    pub generator_version: u32,
    /// The seed, so the puzzle can be rebuilt from scratch and re-checked.
    pub seed: u64,
    /// Every solver query and what it returned. The order is the order they were
    /// asked.
    pub queries: Vec<Query>,
    /// What the encoding's uniqueness claim is relative to. Uniqueness is only ever
    /// true relative to an encoding, and a report that did not say which encoding
    /// would be claiming more than was proved.
    pub encoding: &'static str,
    /// The difficulty numbers the game measured.
    pub measurements: Vec<Measurement>,
    /// The level the game assigned, if any. `None` is a real answer: a puzzle the
    /// game grades into no level is recorded that way rather than given one.
    pub band: Option<&'static str>,
    pub replay: Replay,
}

impl ValidationReport {
    /// Whether this report is enough to publish on.
    ///
    /// Three conditions, and the first is not re-implemented here: it is
    /// [`verification::decide`] over the stored queries, so the report and the gate
    /// are the same judgement. An empty query list is refused by that function, so a
    /// report that asked nothing cannot pass.
    pub fn publishable(&self) -> Result<(), NotPublishable> {
        decide(&self.queries).map_err(NotPublishable::Verification)?;
        if !self.replay.reproduced() {
            return Err(NotPublishable::WitnessNotReproduced);
        }
        if self.content_id.is_none() {
            return Err(NotPublishable::NoContentId);
        }
        Ok(())
    }

    /// Whether any query came back undecided. Separate from [`Self::publishable`]
    /// because an audit wants to find these even among reports that were refused for
    /// some other reason.
    #[must_use]
    pub fn has_undecided_query(&self) -> bool {
        self.queries.iter().any(|q| !q.returned.is_decisive())
    }

    /// Record which package this report is the evidence for.
    ///
    /// Takes the whole [`Sealed`] rather than a bare [`ContentId`] on purpose. The id
    /// a report carries has to be the id of the package that was actually written from
    /// it, and the only thing that knows that is [`crate::package::seal`]. Asking for
    /// the sealed package means the caller cannot supply an id it worked out some other
    /// way, which would be two derivations of one value — the failure
    /// [`crate::content`] exists to prevent.
    ///
    /// The ordering this implies is worth stating: a report is sealed *before* it is
    /// complete, because the package's fingerprint cannot exist until the package does.
    /// So [`Self::publishable`] returning [`NotPublishable::NoContentId`] is the normal
    /// state of a freshly built report, and the only state from which sealing is the
    /// right next step. A caller that wants to know "is this worth sealing" asks
    /// `publishable` and treats `NoContentId` — and nothing else — as yes.
    pub fn bind_to(&mut self, sealed: &Sealed) {
        self.content_id = Some(sealed.content_id);
    }

    /// Look up one measurement by name. Returns `None` rather than zero when the
    /// game did not record it, because a missing measurement and a measurement of
    /// zero are different facts.
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<u32> {
        self.measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.value)
    }
}
