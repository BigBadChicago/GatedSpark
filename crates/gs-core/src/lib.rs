//! GatedSpark deterministic core.
//!
//! Milestone M0 contains only the platform's own vocabulary and the harness that
//! proves the platform holds together with **no puzzle family installed**. The DSL,
//! transition semantics, state hashing and replay arrive in M1.
//!
//! What is enforced here, and by which test, is listed in
//! `docs/adr/0003-engine-game-boundary.md`. Nothing in this crate claims to be
//! enforced without naming the test that enforces it.

pub mod content;
pub mod platform_suite;
pub mod registry;
pub mod verification;
