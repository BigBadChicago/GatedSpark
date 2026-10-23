//! GatedSpark deterministic core.
//!
//! The build order every foundation plan prescribes starts here: the DSL, transition
//! semantics, state hashing and replay underpin the solver integration, the
//! generator and the delivery format, so they come first.
//!
//! * [`dsl`] — the formal representation of a family's rules, with Tier B global
//!   constraints reserved from the start even though Nonogram is Tier A.
//! * [`state`] — logical state, integer-only, with a single canonical encoding.
//! * [`ruleset`] — `S(t+1) = F(S(t), A(t), R)`.
//! * [`timeline`] — undo, redo, branching, and replay against recorded hashes.
//! * [`hash`] — state hashes for replay validation. Not content ids.
//! * [`verification`] — SAT / UNSAT / UNKNOWN, and the publication gate.
//! * [`registry`] — the engine/game boundary.
//! * [`content`] — artifact identity.
//! * [`package`] — the published file: canonical bytes, bound to a fingerprint and
//!   a signature, checked before anything in it is decoded.
//! * [`platform_suite`] — the checks that must hold with no family installed.
//!
//! What is enforced, and by which test, is listed in
//! `docs/adr/0003-engine-game-boundary.md`. Nothing in this crate claims to be
//! enforced without naming the test that enforces it, or labelling itself
//! unenforced.

pub mod content;
pub mod dsl;
pub mod hash;
pub mod package;
pub mod platform_suite;
pub mod registry;
pub mod report;
pub mod rng;
pub mod ruleset;
pub mod state;
pub mod timeline;
pub mod verification;
pub mod withdrawal;
