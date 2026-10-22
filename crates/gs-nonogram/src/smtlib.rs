//! Encoding a Nonogram as SMT-LIB2 text.
//!
//! The engine owns the encoding, and the solver is replaceable: that is why this
//! emits standard SMT-LIB2 rather than calling a solver's API directly. The same
//! text can go to Z3, to CVC5 as a cross-check, or to a SAT solver later, and
//! nothing about the puzzle's meaning moves when the solver does.
//!
//! # The encoding
//!
//! One Boolean per cell, `c_x_y`, true when filled. One line's clue becomes a
//! disjunction over the positions its runs can occupy: for runs of lengths
//! `l_0..l_k` in a line of `n` cells, enumerate every placement (there are
//! `C(n - sum(l) + 1, k + 1)` of them, few for the board sizes in the slice) and
//! assert that the line matches exactly one. Each placement fixes every cell in the
//! line, so a placement is a full conjunction and the disjunction over placements is
//! exact — no implied constraints and no leftover freedom.
//!
//! This is the plain encoding rather than the compact automaton one. It is exact,
//! obviously faithful to the clue, and small at slice board sizes. If it ever becomes
//! the bottleneck, the automaton encoding is tested against this one; faithfulness is
//! not something to take on trust from a cleverer encoding.
//!
//! # Uniqueness
//!
//! [`uniqueness_query`] emits the blocking constraint the architecture plan
//! prescribes: given a witness, assert that at least one cell differs. `UNSAT` then
//! establishes uniqueness **relative to this encoding** — which is the only claim
//! available and the one the validation report must record.

use gs_core::dsl::{Constraint, Rules};
use gs_core::state::{Dims, State};

use crate::clues::Clues;
use crate::line::Mark;
use crate::rules::{mark_of, Nonogram};

/// Why a puzzle could not be encoded.
///
/// A compiler that cannot encode a constraint **refuses the puzzle**. It never drops
/// the constraint and verifies what is left: that is how PuzzleGen came to publish a
/// board whose answer had been fabricated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A global constraint was present. Named, so the refusal says what it refused.
    /// Expected for Slitherlink; impossible for Nonogram.
    UnsupportedGlobal(String),
    /// A clue's runs cannot fit the line at all, so there is nothing to ask.
    ClueLongerThanLine {
        line: String,
        needed: usize,
        length: u16,
    },
    /// The clue list does not match the board's dimensions.
    ClueCountMismatch { expected: usize, found: usize },
}

/// Refuse any rule set this encoder cannot express, naming what it refused.
///
/// Called before emitting anything. Enforced by
/// `tests/encoding::a_global_constraint_is_refused_by_name`.
pub fn reject_unsupported(rules: &Rules) -> Result<(), EncodeError> {
    let mut globals = Vec::new();
    for constraint in &rules.constraints {
        for global in constraint.globals() {
            globals.push(format!("{global:?}"));
        }
    }
    if !globals.is_empty() {
        return Err(EncodeError::UnsupportedGlobal(globals.join(", ")));
    }
    // Every remaining constraint must be one this encoder actually handles. A
    // constraint that is local but unencoded is just as dangerous as a global one.
    for constraint in &rules.constraints {
        if !matches!(constraint, Constraint::RunLengths { .. }) {
            return Err(EncodeError::UnsupportedGlobal(format!(
                "no encoder for {constraint:?}"
            )));
        }
    }
    Ok(())
}

#[must_use]
fn cell_name(x: u16, y: u16) -> String {
    format!("c_{x}_{y}")
}

/// Every way `runs` can be laid out in a line of `length` cells, as a list of
/// filled/empty assignments.
///
/// Exhaustive and in a fixed order, so the emitted text is byte-identical for the
/// same clue every time — a reproducible release needs the query to be reproducible
/// too.
pub fn placements(runs: &[u16], length: u16) -> Result<Vec<Vec<bool>>, usize> {
    let n = usize::from(length);
    let total: usize = runs.iter().map(|r| usize::from(*r)).sum();
    let needed = total + runs.len().saturating_sub(1);
    if needed > n {
        return Err(needed);
    }
    let mut out = Vec::new();
    let mut current = vec![false; n];
    lay_out(runs, 0, 0, n, &mut current, &mut out);
    Ok(out)
}

fn lay_out(
    runs: &[u16],
    j: usize,
    from: usize,
    n: usize,
    current: &mut Vec<bool>,
    out: &mut Vec<Vec<bool>>,
) {
    if j == runs.len() {
        out.push(current.clone());
        return;
    }
    let len = usize::from(runs[j]);
    // Space the remaining runs need after this one, including their gaps.
    let rest: usize = runs[j + 1..].iter().map(|r| usize::from(*r)).sum();
    let rest_gaps = runs.len() - j - 1;
    let last_start = n.saturating_sub(len + rest + rest_gaps);
    for start in from..=last_start {
        current[start..start + len].fill(true);
        lay_out(runs, j + 1, start + len + 1, n, current, out);
        current[start..start + len].fill(false);
    }
}

/// One line's assertion: the line matches exactly one of its clue's placements.
fn line_assertion(
    runs: &[u16],
    length: u16,
    name: &str,
    cell: &dyn Fn(u16) -> String,
) -> Result<String, EncodeError> {
    let options = placements(runs, length).map_err(|needed| EncodeError::ClueLongerThanLine {
        line: name.to_string(),
        needed,
        length,
    })?;
    if options.is_empty() {
        return Err(EncodeError::ClueLongerThanLine {
            line: name.to_string(),
            needed: 0,
            length,
        });
    }
    let mut clauses = Vec::with_capacity(options.len());
    for option in &options {
        let literals: Vec<String> = option
            .iter()
            .enumerate()
            .map(|(i, filled)| {
                let c = cell(u16::try_from(i).unwrap_or(0));
                if *filled {
                    c
                } else {
                    format!("(not {c})")
                }
            })
            .collect();
        clauses.push(format!("(and {})", literals.join(" ")));
    }
    let body = if clauses.len() == 1 {
        clauses.remove(0)
    } else {
        format!("(or {})", clauses.join(" "))
    };
    Ok(format!("; {name}: {runs:?}\n(assert {body})"))
}

/// The solvability query: is there any board matching these clues?
pub fn solvability_query(puzzle: &Nonogram) -> Result<String, EncodeError> {
    let clues = &puzzle.clues;
    let dims = clues.dims;
    if clues.rows.len() != usize::from(dims.height) {
        return Err(EncodeError::ClueCountMismatch {
            expected: usize::from(dims.height),
            found: clues.rows.len(),
        });
    }
    if clues.columns.len() != usize::from(dims.width) {
        return Err(EncodeError::ClueCountMismatch {
            expected: usize::from(dims.width),
            found: clues.columns.len(),
        });
    }
    reject_unsupported(&puzzle.dsl_rules())?;

    let mut out = String::new();
    out.push_str("(set-logic QF_UF)\n");
    out.push_str("(set-option :produce-models true)\n");
    out.push_str(&format!("; nonogram {}x{}\n", dims.width, dims.height));
    for y in 0..dims.height {
        for x in 0..dims.width {
            out.push_str(&format!("(declare-const {} Bool)\n", cell_name(x, y)));
        }
    }
    for (y, runs) in clues.rows.iter().enumerate() {
        let y16 = u16::try_from(y).unwrap_or(0);
        let assertion = line_assertion(runs, dims.width, &format!("row {y}"), &|x| {
            cell_name(x, y16)
        })?;
        out.push_str(&assertion);
        out.push('\n');
    }
    for (x, runs) in clues.columns.iter().enumerate() {
        let x16 = u16::try_from(x).unwrap_or(0);
        let assertion = line_assertion(runs, dims.height, &format!("column {x}"), &|y| {
            cell_name(x16, y)
        })?;
        out.push_str(&assertion);
        out.push('\n');
    }
    out.push_str("(check-sat)\n");
    Ok(out)
}

/// The uniqueness query: the solvability query, plus a blocking constraint that
/// rules out the witness.
///
/// `UNSAT` here establishes uniqueness relative to this encoding. `SAT` means a
/// second board exists and the candidate is rejected. `UNKNOWN` means the question
/// was not answered and the candidate is rejected — never published as unique.
pub fn uniqueness_query(puzzle: &Nonogram, witness: &State) -> Result<String, EncodeError> {
    let mut out = solvability_query(puzzle)?;
    // Drop the trailing check-sat; the blocking constraint goes before it.
    out.truncate(out.len() - "(check-sat)\n".len());
    out.push_str(&blocking_constraint(puzzle.clues.dims, witness));
    out.push_str("(check-sat)\n");
    Ok(out)
}

/// "At least one cell differs from the witness."
fn blocking_constraint(dims: Dims, witness: &State) -> String {
    let mut literals = Vec::new();
    for y in 0..dims.height {
        for x in 0..dims.width {
            let filled = dims
                .index_of(x, y)
                .and_then(|i| witness.get(i).ok())
                .map(mark_of)
                == Some(Mark::Filled);
            let c = cell_name(x, y);
            literals.push(if filled { format!("(not {c})") } else { c });
        }
    }
    format!(
        "; uniqueness: at least one cell differs from the witness\n(assert (or {}))\n",
        literals.join(" ")
    )
}

/// The clue set a query was built from, for the validation report.
///
/// The report has to record "the encoding it was relative to", and the honest way to
/// do that is to record the query text's own identity rather than a description of
/// it. M4 hashes the query; this returns what gets hashed.
#[must_use]
pub fn encoded_clues(clues: &Clues) -> String {
    format!(
        "{}x{} rows={:?} columns={:?}",
        clues.dims.width, clues.dims.height, clues.rows, clues.columns
    )
}
