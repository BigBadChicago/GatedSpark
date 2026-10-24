//! Kakuro's transition semantics and goal predicate.

use gs_core::dsl::{Cmp, Constraint, Domain, Region, Rules as DslRules};
use gs_core::registry::{Family, FamilyId};
use gs_core::ruleset::{Illegal, Ruleset, RulesetId};
use gs_core::state::{Cell, CellChange, Delta, State};

use crate::layout::{Layout, RunId, WhiteId};
use crate::totals::{Totals, LEAST_DIGIT, MOST_DIGIT};

pub const FAMILY: FamilyId = FamilyId("kakuro");

/// Which version of Kakuro's semantics this build implements.
///
/// Named rather than written as a literal, for the reason the other two families do
/// it: a reader has to be able to compare a stored package's version against what it
/// implements, and refuse rather than attempt a replay against semantics it does not
/// have.
pub const RULESET_VERSION: u32 = 1;

/// Whether a cell holds a digit this family permits.
///
/// `Cell::UNKNOWN` is permitted: clearing a square is a real move. A zero is not,
/// and that is worth being explicit about — zero is a perfectly good `i16` and the
/// other two families both use it as a meaningful value, so nothing but this check
/// stops a zero being written into a Kakuro and quietly satisfying a total of nine
/// with `9 + 0`.
#[must_use]
pub fn is_digit(value: Cell) -> bool {
    value.is_unknown() || (LEAST_DIGIT..=MOST_DIGIT).contains(&value.0)
}

/// What a player can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write one digit in one white square. `Cell::UNKNOWN` rubs it out, which is a
    /// legal move.
    Write { square: WhiteId, value: Cell },
}

/// A Kakuro puzzle: a layout, its totals, and the version of the rules it was
/// written against.
///
/// Holds no answer. Comparing two puzzles compares their layout and totals, which is
/// the right notion of sameness: a puzzle *is* its blocks and its totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kakuro {
    pub layout: Layout,
    pub totals: Totals,
    pub version: u32,
}

/// Why a layout and a set of totals do not make a puzzle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PuzzleError {
    /// There is not one total per run.
    TotalsDoNotFit { runs: usize, totals: usize },
}

impl Kakuro {
    pub fn new(layout: Layout, totals: Totals) -> Result<Self, PuzzleError> {
        if totals.len() != layout.run_count() {
            return Err(PuzzleError::TotalsDoNotFit {
                runs: layout.run_count(),
                totals: totals.len(),
            });
        }
        Ok(Kakuro {
            layout,
            totals,
            version: RULESET_VERSION,
        })
    }

    /// A blank board for this puzzle: every white square empty.
    #[must_use]
    pub fn blank(&self) -> State {
        State::blank(self.layout.state_dims())
    }

    /// The digit in a white square, or `None` if it is empty.
    #[must_use]
    pub fn digit(&self, state: &State, square: WhiteId) -> Option<i16> {
        let cell = state.get(square.0).ok()?;
        if cell.is_unknown() {
            return None;
        }
        Some(cell.0)
    }

    /// The puzzle's rules in the shared DSL.
    ///
    /// Two constraints per run, and both of them are **Tier A**, meaning local — each
    /// one is about one run of squares and nothing else. That makes Kakuro the first
    /// family to be Tier A and still need arithmetic, and it is why this family is
    /// worth building: [`Constraint::Sum`] and [`Constraint::Distinct`] were written
    /// into the rule language on the slice's first day and no family has ever
    /// produced one. Either they say what we thought they said, or we find out now.
    ///
    /// * One [`Constraint::Sum`] per run, with `cmp: Eq`.
    /// * One [`Constraint::Distinct`] per run.
    ///
    /// They are two constraints rather than one, unlike Slitherlink's loop rule
    /// which is deliberately welded together. The reason the two cases differ:
    /// splitting the loop rule lets a compiler encode the easy half and silently
    /// accept two separate loops, so a partial encoding is *wrong*. Here a compiler
    /// that encodes only one of these is not wrong about the other, it simply does
    /// not support the family, and the DSL's tier check already refuses a constraint
    /// nothing can encode. Welding them would hide which one a solver disagreed
    /// about, and knowing that is most of the value of running two solvers.
    ///
    /// The domain is one to nine, not nought to nine. See [`is_digit`].
    #[must_use]
    pub fn dsl_rules(&self) -> DslRules {
        let mut constraints = Vec::new();
        for (id, run) in self.layout.runs() {
            let cells: Vec<u32> = run.squares.iter().map(|w| w.0).collect();
            if let Some(total) = self.totals.get(id) {
                constraints.push(Constraint::Sum {
                    region: Region::Cells(cells.clone()),
                    cmp: Cmp::Eq,
                    total: i64::from(total),
                });
            }
            constraints.push(Constraint::Distinct {
                region: Region::Cells(cells),
            });
        }
        DslRules {
            domain: Domain {
                min: LEAST_DIGIT,
                max: MOST_DIGIT,
            },
            constraints,
        }
    }

    /// What a run adds up to on this board, or `None` if any of its squares is empty.
    #[must_use]
    pub fn run_sum(&self, state: &State, run: RunId) -> Option<u16> {
        let run = self.layout.run(run)?;
        let mut sum: u16 = 0;
        for white in &run.squares {
            let digit = self.digit(state, *white)?;
            sum = sum.saturating_add(u16::try_from(digit).ok()?);
        }
        Some(sum)
    }

    /// Whether a run has a digit written twice.
    ///
    /// Repeats are checked over the digits actually written, so a half-filled run can
    /// already be wrong. That is what lets the page tell a player about a clash
    /// before the board is full, which is the whole of this game's moment-to-moment
    /// feedback.
    #[must_use]
    pub fn run_repeats(&self, state: &State, run: RunId) -> bool {
        let Some(run) = self.layout.run(run) else {
            return false;
        };
        let mut seen = [false; 10];
        for white in &run.squares {
            let Some(digit) = self.digit(state, *white) else {
                continue;
            };
            let Ok(index) = usize::try_from(digit) else {
                continue;
            };
            match seen.get_mut(index) {
                Some(slot) if *slot => return true,
                Some(slot) => *slot = true,
                None => {}
            }
        }
        false
    }

    /// Which runs are not yet right: wrong total, or a repeated digit.
    ///
    /// For the page, which greys out a total that is already satisfied and marks one
    /// that cannot be. It reports rather than decides: the page asks this, it does
    /// not work it out.
    pub fn runs_not_met<'a>(&'a self, state: &'a State) -> impl Iterator<Item = RunId> + 'a {
        self.layout.runs().filter_map(move |(id, _)| {
            if self.run_repeats(state, id) {
                return Some(id);
            }
            match (self.run_sum(state, id), self.totals.get(id)) {
                (Some(sum), Some(total)) if sum == total => None,
                _ => Some(id),
            }
        })
    }

    /// Which white squares are still empty.
    pub fn empty_squares<'a>(&'a self, state: &'a State) -> impl Iterator<Item = WhiteId> + 'a {
        self.layout
            .whites()
            .filter(move |white| self.digit(state, *white).is_none())
    }
}

impl Family for Kakuro {
    fn id(&self) -> FamilyId {
        FAMILY
    }
}

impl Ruleset for Kakuro {
    type Action = Action;

    fn id(&self) -> RulesetId {
        RulesetId {
            family: FAMILY,
            version: self.version,
        }
    }

    fn transition(&self, state: &State, action: &Self::Action) -> Result<Delta, Illegal> {
        let Action::Write { square, value } = *action;
        if !is_digit(value) {
            return Err(Illegal::NotPermitted(
                "a white square holds a digit from one to nine, or nothing",
            ));
        }
        if self.layout.square_of(square).is_none() {
            return Err(Illegal::OutOfBounds);
        }
        let from = state.get(square.0).map_err(|_| Illegal::OutOfBounds)?;
        if from == value {
            return Err(Illegal::NoOp);
        }
        Ok(Delta {
            changes: vec![CellChange {
                index: square.0,
                from,
                to: value,
            }],
        })
    }

    /// `G`: every white square holds a digit, every run adds up to its total, and no
    /// run repeats a digit.
    ///
    /// All three are needed and none implies the others. Digits that add up can still
    /// repeat — `4 + 4` and `3 + 5` both make eight — and distinct digits can still
    /// add up wrong. The puzzle is all three at once, and the test file that matters
    /// most in this crate is the one that checks each half fails on its own.
    ///
    /// **Every white square must be filled**, which is a real difference from the
    /// other two families. There, the project owner decided on 2026-10-23 that a
    /// player whose filled squares are right has finished, because crossing out the
    /// rest is a note they keep for themselves and would mean many taps that change
    /// nothing. That reasoning does not carry over. Here an empty square is not an
    /// optional note, it is a square with no answer in it, and no run containing one
    /// can be said to add up. So there is nothing to relax, and the stricter rule is
    /// the same rule, not a stricter one.
    fn is_solved(&self, state: &State) -> bool {
        if self.empty_squares(state).next().is_some() {
            return false;
        }
        self.runs_not_met(state).next().is_none()
    }
}
