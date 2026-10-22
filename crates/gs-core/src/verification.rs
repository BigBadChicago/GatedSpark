//! Verification outcomes.
//!
//! The architecture plan requires SAT, UNSAT and UNKNOWN/timeout to stay distinct
//! outcomes, and states that a timeout is not a proof of unsatisfiability. This
//! module makes that a type rather than a convention: there is no way to spell
//! "unknown, treated as fine".

/// The result of one solver query.
///
/// Deliberately not `Result<bool, _>`: an exhausted budget is a third outcome, not
/// an error on the way to a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A model was found. Carries no trust by itself: the witness must still replay
    /// through the real engine (M2).
    Sat,
    /// Proved unsatisfiable, relative to the encoding that was asserted.
    Unsat,
    /// The budget was exhausted, or the solver returned `unknown`. Never evidence.
    Unknown,
}

impl Outcome {
    /// Whether this outcome proves the asserted constraints have no model.
    ///
    /// `Unknown` answers `false`. Enforced by
    /// `platform_suite::CHECK_UNKNOWN_IS_NOT_A_PROOF`.
    #[must_use]
    pub fn proves_unsatisfiable(self) -> bool {
        matches!(self, Outcome::Unsat)
    }

    /// Whether a gate may treat this outcome as satisfied.
    ///
    /// Only an outcome that decided the question counts. `Unknown` is a rejection,
    /// which is what makes "no weaker tier of proof" (content-delivery.md D4)
    /// mean something.
    #[must_use]
    pub fn is_decisive(self) -> bool {
        !matches!(self, Outcome::Unknown)
    }
}

/// Something that can answer an SMT-LIB2 query.
///
/// The one rule: **never guess.** Returning [`Outcome::Unknown`] is a legitimate
/// answer and an implementation is expected to use it for a timeout, a crash, output
/// it could not parse, or a solver that would not start at all. What it must never do
/// is turn any of those into [`Outcome::Unsat`], which would make a missing dependency
/// look like a proof — and `Unsat` on the uniqueness query is the single claim this
/// whole platform exists to make.
///
/// **No implementation in this crate.** Running a solver means launching a program,
/// which belongs to whatever is driving the pipeline. The real one is in `gs-solver`.
/// Two things follow, both deliberate: nothing in the core or in a puzzle family can
/// quietly answer its own questions, since the only way to obtain an `Outcome` is to
/// be handed one; and a caller can pass in two independent solvers and compare, which
/// is the ADR 0002 cross-check.
///
/// It lives here rather than beside a particular puzzle family because an SMT-LIB2
/// query is a string and this trait says nothing about Nonogram. `gs_nonogram::publish`
/// re-exports it, which is where it used to live.
pub trait Verifier {
    /// A name for the report, such as the solver and its version.
    fn name(&self) -> &'static str;
    /// Ask one query.
    fn ask(&self, query: &str) -> Outcome;
}

/// Why a candidate was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A required query did not decide. Carries the query's name for the report.
    Undecided(&'static str),
    /// A required query decided against the candidate.
    Failed(&'static str),
}

/// One required query and the outcome it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query {
    pub name: &'static str,
    /// The outcome the gate requires in order to pass this query.
    pub required: Outcome,
    /// What the solver actually returned.
    pub returned: Outcome,
}

/// Decide whether a candidate may be published, from its recorded query outcomes.
///
/// Three properties this signature carries deliberately:
///
/// * It takes no clock, no environment and no configuration. Nothing that decides
///   what gets published may read a wall clock (PuzzleGen lesson 7), and the easiest
///   way to keep that true is for the decision to have nothing to read.
/// * An empty query list is a refusal, not a pass. A gate with nothing asserted
///   against it is how PuzzleGen published a board with a fabricated answer.
/// * `Unknown` is reported as `Undecided`, distinct from `Failed`, so the validation
///   report can keep the distinction the architecture plan requires.
///
/// Enforced by `platform_suite::CHECK_GATE_IS_A_PURE_FUNCTION`,
/// `CHECK_EMPTY_GATE_REFUSES` and `CHECK_UNKNOWN_IS_NOT_PUBLISHABLE`.
pub fn decide(queries: &[Query]) -> Result<(), Refusal> {
    if queries.is_empty() {
        return Err(Refusal::Failed("no_queries_asserted"));
    }
    for q in queries {
        if !q.returned.is_decisive() {
            return Err(Refusal::Undecided(q.name));
        }
        if q.returned != q.required {
            return Err(Refusal::Failed(q.name));
        }
    }
    Ok(())
}
