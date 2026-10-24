//! Kakuro: GatedSpark's third puzzle family.
//!
//! A crossword done with numbers. The board has black squares and white squares.
//! Each straight run of white squares, going across or going down, carries a total.
//! Write a digit from one to nine in every white square so that each run adds up to
//! its total, and so that no digit appears twice in the same run.
//!
//! # Why this family is third
//!
//! Not because it is cheap — Sudoku is cheaper and was passed over twice. Because of
//! two things neither of the first two families has.
//!
//! **A square holds a number the player chooses.** Nonogram's squares are filled or
//! not. Slitherlink's edges are drawn or not. Every layer built so far — the board,
//! the page, the hints, the accessibility rules — quietly assumes a square has two or
//! three states. This family has ten. If all of that survives, the platform's claim
//! to be general is worth something; if it does not, the place it breaks is the
//! thing worth knowing.
//!
//! **Arithmetic rules that have never been used.** The core's DSL has carried
//! [`gs_core::dsl::Constraint::Sum`] and [`gs_core::dsl::Constraint::Distinct`] since
//! the slice's first day, and no family has ever produced one. This crate is the
//! first thing to emit either, so it is also the first real test of whether those two
//! reservations were worth anything — the same role Slitherlink played for
//! [`gs_core::dsl::Global::SingleClosedLoop`].
//!
//! And one thing that is new in a smaller way: **the shape is a free choice.** A
//! Nonogram and a Slitherlink of a given size are one board each. A Kakuro is not,
//! because where the blocks go is not implied by the size. So this is the first
//! family whose generator has to invent a shape before it can fill one in. See
//! [`layout`].
//!
//! # What is trusted, and what is not
//!
//! The same arrangement as the other two families, for the same reason.
//!
//! * [`rules`] **checks** a board. It does not search and it decides nothing about
//!   publication.
//! * [`smtlib`] turns a puzzle into text. It does not answer anything either, and it
//!   **refuses** a rule it cannot encode rather than encoding what it can.
//! * Only a real SMT solver, reading that text, produces a
//!   [`gs_core::verification::Outcome`], and only an `Outcome` reaches the
//!   publication gate.
//!
//! Parts one and two of four are here: the board and the rules, and the SMT-LIB2
//! encoding that lets a real solver answer the two questions this platform asks of
//! every puzzle. Making puzzles is part three.

pub mod layout;
pub mod rules;
pub mod smtlib;
pub mod totals;

pub use rules::{Action, Kakuro};
