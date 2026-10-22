//! Nonogram: GatedSpark's launch puzzle family.
//!
//! Row and column clues are run-length sequences. A cell is filled, marked empty, or
//! unknown — three states, which is why marking a cell empty is a real move and not
//! the absence of one.
//!
//! # What is trusted, and what is not
//!
//! This crate contains a line solver ([`solve`]). It is **not** the verification
//! gate, and the type system is arranged so it cannot become one: it produces
//! [`solve::Features`], which has no path to [`gs_core::verification::Outcome`].
//! Only a real SMT solver, fed by [`smtlib`], produces an `Outcome`, and only an
//! `Outcome` reaches the publication gate.
//!
//! The line solver's two jobs are both legitimate and neither is a correctness
//! claim:
//!
//! * **A generation filter.** Reject candidates cheaply before paying for
//!   verification. A filter that is wrong costs throughput, not trust.
//! * **The difficulty feature vector.** Whether a puzzle is solvable by line
//!   propagation alone, and if not how deep the search goes, is the explainable
//!   difficulty structure the family was chosen for.
//!
//! Treating a fast heuristic as a correctness oracle is the mistake the whole
//! platform is arranged to avoid. WFC gets the same treatment in M3.

pub mod clues;
pub mod line;
pub mod rng;
pub mod rules;
pub mod smtlib;
pub mod solve;
