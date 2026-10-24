//! Encoding a Kakuro as SMT-LIB2 text.
//!
//! The engine owns the encoding and the solver is replaceable, so this emits standard
//! SMT-LIB2 text rather than calling any solver's own interface. The same text goes to
//! Z3 and to CVC5 as the ADR 0002 cross-check, and nothing about the puzzle's meaning
//! moves when the solver does.
//!
//! # The encoding
//!
//! One integer per white square, `w_<n>`, holding that square's digit. Then three
//! groups of assertions.
//!
//! **The digits.** Each `w_<n>` is between one and nine. This is a rule, not a hint:
//! the zero that the domain excludes is exactly the value that would let a run of two
//! satisfy a total of nine with `9 + 0`, and the no-repeats rule would not catch it
//! because one zero does not repeat. See ADR 0018.
//!
//! **The totals.** For each run, its digits add up to its total. A plain sum of
//! integers, with no `ite` wrapper, which is the first encoding in this project that
//! is arithmetic rather than counting.
//!
//! **No repeats.** For each run of two or more squares, `distinct`.
//!
//! # The only subtlety in the whole file
//!
//! `distinct` needs at least two arguments to be well-formed SMT-LIB2, and a run may
//! be one square long. So a run of one gets no `distinct` assertion.
//!
//! That skip is sound — a single digit cannot repeat itself — but a skip in a trust
//! chain is worth being nervous about, because it is the same shape as the mistake
//! this project most wants to avoid: dropping a constraint and verifying what is
//! left. Two things keep it honest. [`distinct_assertions`] skips only at length one
//! and nothing else, and `tests/encoding.rs::every_run_of_two_or_more_gets_a_distinct`
//! walks a board of mixed run lengths and checks the count, so a skip that widened
//! would fail the build rather than quietly weaken a proof.
//!
//! # Why this is simpler than Slitherlink's encoding, and what that costs
//!
//! Slitherlink needs helper variables — a root and a distance label per dot — because
//! "one closed loop" has no local encoding. Those helpers are working-out rather than
//! part of the answer, which is why its uniqueness query has to block *edges* and not
//! models: one board can be labelled many ways, and blocking models would let the
//! solver answer `sat` by relabelling the board it was given.
//!
//! Kakuro has no helper variables at all. Every variable in the query is a square the
//! player writes in, so blocking the digits and blocking the model are the same thing
//! here, and the trap Slitherlink had to avoid cannot arise. That is worth writing
//! down rather than leaving as an absence, because the next family may need helpers
//! again and the reader should know which case they are in.

use gs_core::dsl::{Cmp, Constraint, Region, Rules};
use gs_core::state::State;

use crate::layout::{Layout, WhiteId};
use crate::rules::Kakuro;
use crate::totals::{LEAST_DIGIT, MOST_DIGIT};

/// Why a puzzle could not be encoded.
///
/// A compiler that cannot encode a constraint **refuses the puzzle**. It never drops
/// the constraint and verifies what is left: that is how PuzzleGen came to publish a
/// board whose answer had been fabricated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A global constraint. This encoder implements none, so a puzzle carrying one is
    /// refused by name rather than verified without it. Kakuro produces no globals, so
    /// this fires only if the family's rules change or another family's rules are
    /// handed to this encoder.
    UnsupportedGlobal(String),
    /// A constraint that is local but that this encoder has no translation for. Just
    /// as dangerous as an unencoded global one, and refused the same way.
    UnsupportedConstraint(String),
    /// The puzzle's rules carry no adding-up rule at all.
    ///
    /// Refused rather than added. A Kakuro without its totals is a different puzzle,
    /// and an encoder that supplied the missing rule itself would be deciding what the
    /// puzzle is — which is the family's job, in `rules.rs`.
    NoTotalRule,
    /// A run names a square that is not on the board.
    SquareOffTheBoard(u32),
    /// A run of two or more squares has an adding-up rule and no no-repeats rule.
    ///
    /// The specific failure this family is most exposed to. The two rules are emitted
    /// separately on purpose (ADR 0018), and the price of separating them is that one
    /// could go missing while the other still looks like a complete puzzle. A board
    /// verified with only its sums would have its `4 + 4` accepted, so the mismatch is
    /// refused here rather than encoded.
    SumWithoutDistinct { squares: Vec<u32> },
}

/// Refuse any rule set this encoder cannot express, naming what it refused.
///
/// Called before emitting anything. Three checks, in order of how badly each would
/// hurt: no globals at all, nothing but the two constraints this file knows, and —
/// the one specific to this family — no run that has a sum without a matching
/// `distinct`.
pub fn reject_unsupported(rules: &Rules) -> Result<(), EncodeError> {
    if let Some(global) = rules
        .constraints
        .iter()
        .flat_map(Constraint::globals)
        .next()
    {
        return Err(EncodeError::UnsupportedGlobal(format!("{global:?}")));
    }

    let mut sums: Vec<Vec<u32>> = Vec::new();
    let mut distincts: Vec<Vec<u32>> = Vec::new();
    for constraint in &rules.constraints {
        match constraint {
            Constraint::Sum {
                region: Region::Cells(cells),
                cmp: Cmp::Eq,
                ..
            } => sums.push(cells.clone()),
            Constraint::Distinct {
                region: Region::Cells(cells),
            } => distincts.push(cells.clone()),
            other => {
                return Err(EncodeError::UnsupportedConstraint(format!(
                    "no encoder for {other:?}"
                )))
            }
        }
    }

    if sums.is_empty() {
        return Err(EncodeError::NoTotalRule);
    }
    for squares in &sums {
        if squares.len() < 2 {
            continue;
        }
        if !distincts.iter().any(|d| d == squares) {
            return Err(EncodeError::SumWithoutDistinct {
                squares: squares.clone(),
            });
        }
    }
    Ok(())
}

#[must_use]
fn square_name(white: WhiteId) -> String {
    format!("w_{}", white.0)
}

/// The solvability query: is there any way to fill this board in?
pub fn solvability_query(puzzle: &Kakuro) -> Result<String, EncodeError> {
    reject_unsupported(&puzzle.dsl_rules())?;
    let layout = &puzzle.layout;
    check_runs_are_on_the_board(layout)?;

    let mut out = String::new();
    // Integers with addition and comparison, no quantifiers: QF_LIA. `distinct` over
    // integers is part of the core theory, so nothing wider is needed.
    out.push_str("(set-logic QF_LIA)\n");
    out.push_str("(set-option :produce-models true)\n");
    out.push_str(&format!(
        "; kakuro {}x{}, {} white squares, {} runs\n",
        layout.width(),
        layout.height(),
        layout.white_count(),
        layout.run_count()
    ));

    for white in layout.whites() {
        out.push_str(&format!("(declare-const {} Int)\n", square_name(white)));
    }

    out.push_str("\n; every white square holds a digit from one to nine, never nought\n");
    out.push_str(&digit_assertions(layout));

    out.push_str("\n; the totals: each run's digits add up to its number\n");
    out.push_str(&total_assertions(puzzle)?);

    out.push_str("\n; no repeats: see this module's note on runs of one square\n");
    out.push_str(&distinct_assertions(layout));

    out.push_str("\n(check-sat)\n");
    Ok(out)
}

/// Refuse a layout whose runs point off the board.
///
/// Cannot happen for a layout built by [`Layout::new`], which derives its runs from
/// its own blocks. Checked anyway, because the alternative is emitting a query about
/// a variable that was never declared, and a solver's answer to a malformed query is
/// not evidence about the puzzle.
fn check_runs_are_on_the_board(layout: &Layout) -> Result<(), EncodeError> {
    for (_, run) in layout.runs() {
        for white in &run.squares {
            if layout.square_of(*white).is_none() {
                return Err(EncodeError::SquareOffTheBoard(white.0));
            }
        }
    }
    Ok(())
}

/// One assertion per white square, bounding its digit.
fn digit_assertions(layout: &Layout) -> String {
    let mut out = String::new();
    for white in layout.whites() {
        let w = square_name(white);
        out.push_str(&format!(
            "(assert (and (<= {LEAST_DIGIT} {w}) (<= {w} {MOST_DIGIT})))\n"
        ));
    }
    out
}

/// One assertion per run, fixing its sum.
fn total_assertions(puzzle: &Kakuro) -> Result<String, EncodeError> {
    let mut out = String::new();
    for (id, run) in puzzle.layout.runs() {
        let Some(total) = puzzle.totals.get(id) else {
            continue;
        };
        let names: Vec<String> = run.squares.iter().map(|w| square_name(*w)).collect();
        let start = puzzle
            .layout
            .square_at(run.start)
            .ok_or(EncodeError::SquareOffTheBoard(run.start.0))?;
        // A run of one square is written `(= 5 w_3)` and **not** `(= 5 (+ w_3))`.
        //
        // That is not a style choice. SMT-LIB2's `+` takes two or more arguments, so a
        // unary `+` is not standard. Z3 accepts it anyway; CVC5 exits with an error and
        // prints no verdict, which `gs-solver` correctly reports as "nobody knows".
        // Every query for a board with a lone white square — and every board one square
        // tall or wide, since there every column is a run of one — was unanswerable by
        // CVC5 until this was fixed.
        //
        // This is exactly what the two-solver cross-check of ADR 0002 is for, and the
        // first time it has caught anything. One solver's leniency had hidden a
        // malformed query, and a malformed query's answer is not evidence about a
        // puzzle.
        let sum = if names.len() == 1 {
            names.join(" ")
        } else {
            format!("(+ {})", names.join(" "))
        };
        out.push_str(&format!(
            "; {:?} run from ({}, {}) = {total}\n(assert (= {total} {sum}))\n",
            run.along, start.0, start.1
        ));
    }
    Ok(out)
}

/// One assertion per run of two or more squares.
///
/// **A run of one square gets none**, because `distinct` needs at least two arguments
/// to be well-formed and a single digit cannot repeat itself. That is the only case
/// skipped, and this function is written so that is visible in one line rather than
/// buried in a condition. See the module note, and the test that counts them.
fn distinct_assertions(layout: &Layout) -> String {
    let mut out = String::new();
    for (_, run) in layout.runs() {
        if run.len() < 2 {
            continue;
        }
        let names: Vec<String> = run.squares.iter().map(|w| square_name(*w)).collect();
        out.push_str(&format!("(assert (distinct {}))\n", names.join(" ")));
    }
    out
}

/// The uniqueness query: the solvability query, plus a constraint ruling out the
/// witness.
///
/// `unsat` here establishes uniqueness **relative to this encoding**, which is the
/// only claim available and the one the validation report records. `sat` means a
/// second answer exists and the candidate is rejected. `unknown` means the question
/// was not answered, and is also a rejection — never published as unique.
pub fn uniqueness_query(puzzle: &Kakuro, witness: &State) -> Result<String, EncodeError> {
    let mut out = solvability_query(puzzle)?;
    out.truncate(out.len() - "\n(check-sat)\n".len());
    out.push_str(&blocking_constraint(puzzle, witness)?);
    out.push_str("\n(check-sat)\n");
    Ok(out)
}

/// Why a witness cannot be used to ask the uniqueness question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessError {
    /// The witness leaves a white square empty.
    ///
    /// Refused rather than worked around. "At least one square differs from the
    /// witness" has no meaning at a square the witness never filled in, and the two
    /// obvious repairs are both wrong: leaving that square out of the blocking
    /// constraint would accept a second answer that differs *only* there, and
    /// inventing a digit for it would ask about a board nobody produced.
    Incomplete { square: u32 },
}

impl From<WitnessError> for EncodeError {
    fn from(error: WitnessError) -> Self {
        match error {
            WitnessError::Incomplete { square } => EncodeError::SquareOffTheBoard(square),
        }
    }
}

/// "At least one square differs from the witness."
///
/// Over the digits, which here is the whole model: this family declares no helper
/// variables, so there is no relabelling for a solver to answer `sat` with. Compare
/// `gs_slitherlink::smtlib`, where blocking the model rather than the edges would have
/// made every puzzle look ambiguous.
fn blocking_constraint(puzzle: &Kakuro, witness: &State) -> Result<String, EncodeError> {
    let mut literals = Vec::new();
    for white in puzzle.layout.whites() {
        let digit = puzzle
            .digit(witness, white)
            .ok_or(WitnessError::Incomplete { square: white.0 })?;
        literals.push(format!("(not (= {} {digit}))", square_name(white)));
    }
    Ok(format!(
        "\n; uniqueness: at least one square differs from the witness\n(assert (or {}))\n",
        literals.join(" ")
    ))
}

/// The layout and totals a query was built from, for the validation report.
///
/// The report has to record what its uniqueness claim was relative to, and the honest
/// way to do that is to record the query's own identity rather than a description of
/// it.
#[must_use]
pub fn encoded_puzzle(puzzle: &Kakuro) -> String {
    let layout = &puzzle.layout;
    let mut blocks = String::new();
    for square in layout.squares() {
        blocks.push(if layout.is_block(square) { '#' } else { '.' });
    }
    let totals: Vec<String> = puzzle
        .totals
        .as_slice()
        .iter()
        .map(ToString::to_string)
        .collect();
    format!(
        "kakuro {}x{} blocks={blocks} totals={}",
        layout.width(),
        layout.height(),
        totals.join(",")
    )
}
