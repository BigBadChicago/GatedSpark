//! Slitherlink: GatedSpark's second puzzle family.
//!
//! Draw lines between the dots so that they make **one single loop**, and so that
//! every numbered cell has exactly that many of its four sides used by the loop.
//!
//! # Why this family is second, and not fifth
//!
//! The family options review picked it on purpose. Every rule Nonogram has is local:
//! a line's clue is about that line and nothing else. Slitherlink's central rule is
//! not. "These edges make one loop" is a fact about the whole board at once, and
//! there is no way to say it as a pile of small local rules — two separate rings
//! satisfy every local rule a person would think to write down.
//!
//! The core's DSL reserved [`gs_core::dsl::Global::SingleClosedLoop`] for exactly
//! this on the slice's first day, because learning this on family two is far cheaper
//! than discovering it on family five with the rules frozen round local constraints.
//! This crate is the first thing to produce one, so it is also the first real test of
//! whether that reservation was worth anything.
//!
//! # What is trusted, and what is not
//!
//! The same arrangement as `gs_nonogram`, for the same reason.
//!
//! * [`loops`] **checks** a finished board. It does not search and it decides
//!   nothing about publication.
//! * [`solve`] is a fast solver used as a generation filter and as the source of the
//!   difficulty numbers. It produces no [`gs_core::verification::Outcome`] and there
//!   is no path from its results to one.
//! * Only a real SMT solver, fed by [`smtlib`], produces an `Outcome`, and only an
//!   `Outcome` reaches the publication gate.

pub mod board;
pub mod clues;
pub mod loops;
pub mod rules;
