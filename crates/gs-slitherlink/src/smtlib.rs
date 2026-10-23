//! Encoding a Slitherlink as SMT-LIB2 text.
//!
//! The engine owns the encoding and the solver is replaceable, so this emits
//! standard SMT-LIB2 text rather than calling any solver's own interface. The same
//! text goes to Z3 and to CVC5 as the ADR 0002 cross-check, and nothing about the
//! puzzle's meaning moves when the solver does.
//!
//! # The encoding
//!
//! One Boolean per edge, `e_<n>`, true when the line is drawn. Then three groups of
//! assertions.
//!
//! **The clues.** For each clued cell, the number of its four sides that are drawn
//! equals the clue. A sum of four `ite` terms; the easiest thing in the file.
//!
//! **Nought or two lines at every dot.** For each dot, the number of drawn edges
//! meeting it is 0 or 2. One would be a loose end, three or four a crossing.
//!
//! **All in one piece.** This is the part with no small local encoding, and the
//! reason this family exists in the plan. It is done with distance labels:
//!
//! * One integer `root`, naming a dot, and one integer `d_<n>` per dot.
//! * The dot named by `root` must be on the loop, with `d = 0`.
//! * Every *other* dot on the loop must have a drawn edge to a neighbour whose `d`
//!   is strictly smaller.
//!
//! # Why that is exactly "one loop", in both directions
//!
//! *It admits no second loop.* Nought-or-two already makes every piece of the drawn
//! edges a ring. Suppose there were a second ring, not the one holding `root`. Take
//! the dot in it with the smallest `d`. It is on the loop and is not `root`, so it
//! needs a neighbour in its own ring with a strictly smaller `d` — and it was the
//! smallest. So there is no second ring.
//!
//! *It admits every real loop.* Label each dot with how many steps round the loop it
//! is from `root`. Every dot except `root` then has a neighbour one step closer,
//! going each way round, so the labels exist for any single loop and the encoding
//! never refuses a board it should accept. Held by
//! `tests/encoding.rs::a_real_loop_is_accepted_whatever_its_length`.
//!
//! The upper bound on each `d` is a hint to the solver rather than part of the rule.
//! It is sound because the labels above never exceed the number of dots, so bounding
//! them cannot rule out a board that should be accepted.
//!
//! # The aside that matters for the uniqueness query
//!
//! `root` and the `d` labels are working-out, not part of the answer. One board can
//! be labelled many ways, so the same drawn edges appear in many models. That would
//! be a problem if the uniqueness query blocked *models*. It blocks **edges**: the
//! blocking constraint says at least one edge differs from the witness, so a model
//! that differs only in its labels does not satisfy it, and a second board is the
//! only thing that can. Held by
//! `tests/encoding.rs::relabelling_the_same_loop_is_not_a_second_answer`.

use gs_core::dsl::{Cmp, Constraint, Global, Region, Rules};
use gs_core::state::State;

use crate::board::{DotId, EdgeId, Geometry};
use crate::clues::Clues;
use crate::loops::drawn_in;
use crate::rules::Slitherlink;

/// Why a puzzle could not be encoded.
///
/// A compiler that cannot encode a constraint **refuses the puzzle**. It never drops
/// the constraint and verifies what is left: that is how PuzzleGen came to publish a
/// board whose answer had been fabricated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A global constraint this encoder does not implement. Named, so the refusal
    /// says what it refused. `SingleClosedLoop` is encoded; the DSL's other two
    /// globals are not, and a puzzle carrying one is refused here rather than
    /// verified without it.
    UnsupportedGlobal(String),
    /// A constraint that is local but that this encoder has no translation for. Just
    /// as dangerous as an unencoded global one, and refused the same way.
    UnsupportedConstraint(String),
    /// The loop rule is missing from the puzzle's own rules.
    ///
    /// Refused rather than added. A Slitherlink without the loop rule is a different
    /// puzzle, and an encoder that supplied the missing rule itself would be deciding
    /// what the puzzle is — which is the family's job, in `rules.rs`.
    NoLoopRule,
    /// A clue names an edge that is not on the board.
    EdgeOffTheBoard(u32),
}

/// Refuse any rule set this encoder cannot express, naming what it refused.
///
/// Called before emitting anything. Unlike the Nonogram encoder, this one *does*
/// encode a global constraint — but only the one. The others are refused by name, so
/// adding `Connected` to a future family cannot quietly ride in on this encoder.
pub fn reject_unsupported(rules: &Rules) -> Result<(), EncodeError> {
    let mut loop_rule = false;
    for global in rules.constraints.iter().flat_map(Constraint::globals) {
        match global {
            Global::SingleClosedLoop { .. } => loop_rule = true,
            other => return Err(EncodeError::UnsupportedGlobal(format!("{other:?}"))),
        }
    }
    for constraint in &rules.constraints {
        let known = matches!(
            constraint,
            Constraint::Global(Global::SingleClosedLoop { .. })
                | Constraint::Count {
                    region: Region::Cells(_),
                    cmp: Cmp::Eq,
                    ..
                }
        );
        if !known {
            return Err(EncodeError::UnsupportedConstraint(format!(
                "no encoder for {constraint:?}"
            )));
        }
    }
    if !loop_rule {
        return Err(EncodeError::NoLoopRule);
    }
    Ok(())
}

#[must_use]
fn edge_name(edge: EdgeId) -> String {
    format!("e_{}", edge.0)
}

#[must_use]
fn distance_name(dot: DotId) -> String {
    format!("d_{}", dot.0)
}

/// `1` when the edge is drawn, `0` when it is not, as a term a sum can hold.
#[must_use]
fn one_or_nought(edge: EdgeId) -> String {
    format!("(ite {} 1 0)", edge_name(edge))
}

/// The solvability query: is there any board that satisfies this puzzle?
pub fn solvability_query(puzzle: &Slitherlink) -> Result<String, EncodeError> {
    reject_unsupported(&puzzle.dsl_rules())?;
    let geometry = puzzle.geometry();

    let mut out = String::new();
    // Booleans and integers together, with the Booleans free, which is QF_UFLIA.
    out.push_str("(set-logic QF_UFLIA)\n");
    out.push_str("(set-option :produce-models true)\n");
    out.push_str(&format!(
        "; slitherlink {}x{}, {} edges, {} dots, {} clues\n",
        geometry.width(),
        geometry.height(),
        geometry.edge_count(),
        geometry.dot_count(),
        puzzle.clues.given_count()
    ));

    for edge in geometry.edges() {
        out.push_str(&format!("(declare-const {} Bool)\n", edge_name(edge)));
    }

    out.push_str("\n; the clues: that many of the cell's four sides are drawn\n");
    out.push_str(&clue_assertions(geometry, &puzzle.clues)?);

    out.push_str("\n; nought or two lines at every dot: no loose ends, no crossings\n");
    out.push_str(&degree_assertions(geometry));

    out.push_str("\n; all in one piece: see this module's notes for why this is exact\n");
    out.push_str(&connectivity_assertions(geometry));

    out.push_str("\n(check-sat)\n");
    Ok(out)
}

/// One assertion per clued cell.
fn clue_assertions(geometry: Geometry, clues: &Clues) -> Result<String, EncodeError> {
    let mut out = String::new();
    for (cell, clue) in clues.given() {
        let sides = geometry
            .edges_of_cell(cell)
            .ok_or(EncodeError::EdgeOffTheBoard(cell.0))?;
        for side in sides {
            if usize::try_from(side.0).unwrap_or(usize::MAX) >= geometry.edge_count() {
                return Err(EncodeError::EdgeOffTheBoard(side.0));
            }
        }
        let terms: Vec<String> = sides.iter().map(|e| one_or_nought(*e)).collect();
        let (x, y) = geometry.cell_at(cell).unwrap_or((0, 0));
        out.push_str(&format!(
            "; cell ({x}, {y}) = {clue}\n(assert (= {clue} (+ {})))\n",
            terms.join(" ")
        ));
    }
    Ok(out)
}

/// One assertion per dot: nought or two drawn edges.
fn degree_assertions(geometry: Geometry) -> String {
    let mut out = String::new();
    for dot in geometry.dots() {
        let terms: Vec<String> = geometry
            .edges_of_dot(dot)
            .iter()
            .map(|e| one_or_nought(*e))
            .collect();
        // `let` so the sum is written once. Two copies of one sum is two chances for
        // an edit to change only one of them.
        out.push_str(&format!(
            "(assert (let ((n (+ {}))) (or (= n 0) (= n 2))))\n",
            terms.join(" ")
        ));
    }
    out
}

/// The whole-board half of the loop rule.
fn connectivity_assertions(geometry: Geometry) -> String {
    let dots = u32::try_from(geometry.dot_count()).unwrap_or(u32::MAX);
    let mut out = String::new();
    out.push_str("(declare-const root Int)\n");
    out.push_str(&format!("(assert (and (<= 0 root) (< root {dots})))\n"));
    for dot in geometry.dots() {
        out.push_str(&format!("(declare-const {} Int)\n", distance_name(dot)));
    }
    // The bound is a hint, not part of the rule. Sound because step-counts round a
    // loop never exceed the number of dots, so no acceptable board is ruled out.
    for dot in geometry.dots() {
        let d = distance_name(dot);
        out.push_str(&format!("(assert (and (<= 0 {d}) (<= {d} {dots})))\n"));
    }
    for dot in geometry.dots() {
        let edges = geometry.edges_of_dot(dot);
        // "On the loop" is "has a drawn edge". Given nought-or-two, that is the same
        // as having two, and an `or` is cheaper and plainer than a second sum.
        let on = format!(
            "(or {})",
            edges
                .iter()
                .map(|e| edge_name(*e))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let is_root = format!("(= root {})", dot.0);
        let d = distance_name(dot);
        out.push_str(&format!(
            "; dot {}: if it is the root it is on the loop at distance 0\n(assert (=> {is_root} (and {on} (= {d} 0))))\n",
            dot.0
        ));
        let steps: Vec<String> = edges
            .iter()
            .filter_map(|edge| {
                let (a, b) = geometry.ends_of(*edge)?;
                let other = if a == dot { b } else { a };
                Some(format!(
                    "(and {} (< {} {d}))",
                    edge_name(*edge),
                    distance_name(other)
                ))
            })
            .collect();
        out.push_str(&format!(
            "(assert (=> (and {on} (not {is_root})) (or {})))\n",
            steps.join(" ")
        ));
    }
    out
}

/// The uniqueness query: the solvability query, plus a constraint ruling out the
/// witness.
///
/// `unsat` here establishes uniqueness **relative to this encoding**, which is the
/// only claim available and the one the validation report records. `sat` means a
/// second board exists and the candidate is rejected. `unknown` means the question
/// was not answered, and is also a rejection — never published as unique.
pub fn uniqueness_query(puzzle: &Slitherlink, witness: &State) -> Result<String, EncodeError> {
    let mut out = solvability_query(puzzle)?;
    out.truncate(out.len() - "\n(check-sat)\n".len());
    out.push_str(&blocking_constraint(puzzle.geometry(), witness));
    out.push_str("\n(check-sat)\n");
    Ok(out)
}

/// "At least one edge differs from the witness."
///
/// Over edges only, and deliberately so: the `root` and distance labels are
/// working-out and one board can carry many sets of them. Blocking the whole model
/// would let the solver answer `sat` by relabelling the same board, which would read
/// as a second answer and throw away a perfectly good puzzle — or worse, would be
/// taken as the measure of uniqueness and mean nothing.
fn blocking_constraint(geometry: Geometry, witness: &State) -> String {
    let literals: Vec<String> = geometry
        .edges()
        .map(|edge| {
            let name = edge_name(edge);
            if drawn_in(witness, edge) {
                format!("(not {name})")
            } else {
                name
            }
        })
        .collect();
    format!(
        "\n; uniqueness: at least one edge differs from the witness\n(assert (or {}))\n",
        literals.join(" ")
    )
}

/// The clue grid a query was built from, for the validation report.
///
/// The report has to record what its uniqueness claim was relative to, and the
/// honest way to do that is to record the query's own identity rather than a
/// description of it.
#[must_use]
pub fn encoded_clues(clues: &Clues) -> String {
    let geometry = clues.geometry();
    let mut listed = String::new();
    for (cell, clue) in clues.given() {
        let (x, y) = geometry.cell_at(cell).unwrap_or((0, 0));
        listed.push_str(&format!("({x},{y})={clue} "));
    }
    format!(
        "{}x{} clues={}",
        geometry.width(),
        geometry.height(),
        listed.trim_end()
    )
}
