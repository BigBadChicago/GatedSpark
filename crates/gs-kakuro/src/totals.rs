//! Totals, and deriving them from a filled board.
//!
//! A total sits beside a run and says what that run's digits add up to. **Every run
//! has one**, which is the first real difference from the other two families and it
//! runs the generator backwards from both of them.
//!
//! * Nonogram derives every clue from a picture and keeps them all. The clues *are*
//!   the puzzle, and a clue is never left out.
//! * Slitherlink derives a number for every cell and then the generator's real work
//!   is deciding which numbers to **take away**, because the full grid is far too
//!   easy.
//! * Kakuro cannot take a total away. A run with no total would mean "any nine
//!   digits", which is not a harder puzzle, it is a puzzle with no answer worth
//!   finding. So the generator's choice is not which totals to keep, it is **where
//!   the blocks go**. See [`crate::layout`].
//!
//! Derivation itself is total and deterministic: every filled board has exactly one
//! set of totals, and adding up at most nine digits cannot go wrong, which keeps
//! this step out of the trust boundary entirely.

use crate::layout::{Layout, RunId};

/// The smallest digit a white square may hold.
pub const LEAST_DIGIT: i16 = 1;
/// The largest digit a white square may hold.
pub const MOST_DIGIT: i16 = 9;

/// A puzzle's totals: one per run, in the run numbering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Totals {
    numbers: Vec<u16>,
}

/// Why a set of numbers is not a set of totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TotalsError {
    /// The entry count does not match the layout's run count.
    WrongLength { expected: usize, found: usize },
    /// No run of this length could add up to this total, whatever digits are used.
    ///
    /// Refused here rather than passed to the solver, because a total of 40 on a run
    /// of three squares is not a puzzle the solver should be asked about: its
    /// impossibility is arithmetic, not a search result, and a refusal with a reason
    /// is more use than an UNSAT with none.
    Unreachable {
        run: u32,
        total: u16,
        least: u16,
        most: u16,
    },
}

impl Totals {
    /// Build a set of totals, checking the count and every number against its run.
    pub fn new(layout: &Layout, numbers: Vec<u16>) -> Result<Self, TotalsError> {
        if numbers.len() != layout.run_count() {
            return Err(TotalsError::WrongLength {
                expected: layout.run_count(),
                found: numbers.len(),
            });
        }
        for (id, run) in layout.runs() {
            let Some(total) = numbers.get(usize::try_from(id.0).unwrap_or(usize::MAX)) else {
                continue;
            };
            let (least, most) = run.total_bounds();
            if *total < least || *total > most {
                return Err(TotalsError::Unreachable {
                    run: id.0,
                    total: *total,
                    least,
                    most,
                });
            }
        }
        Ok(Totals { numbers })
    }

    /// Derive the totals a filled board implies.
    ///
    /// `digit_of` gives the digit in a white square. The only way totals are
    /// produced from an answer, and the single definition of what a total means.
    ///
    /// Returns `None` if any square in any run has no digit, because a partly filled
    /// board does not imply a total and guessing one would invent a puzzle nobody
    /// drew.
    #[must_use]
    pub fn of(
        layout: &Layout,
        digit_of: &dyn Fn(crate::layout::WhiteId) -> Option<i16>,
    ) -> Option<Self> {
        let mut numbers = Vec::with_capacity(layout.run_count());
        for (_, run) in layout.runs() {
            let mut sum: u16 = 0;
            for white in &run.squares {
                let digit = digit_of(*white)?;
                if !(LEAST_DIGIT..=MOST_DIGIT).contains(&digit) {
                    return None;
                }
                sum = sum.saturating_add(u16::try_from(digit).ok()?);
            }
            numbers.push(sum);
        }
        Some(Totals { numbers })
    }

    #[must_use]
    pub fn get(&self, run: RunId) -> Option<u16> {
        usize::try_from(run.0)
            .ok()
            .and_then(|index| self.numbers.get(index))
            .copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.numbers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.numbers.is_empty()
    }

    /// The totals in run order. For storing and for emitting a query.
    #[must_use]
    pub fn as_slice(&self) -> &[u16] {
        &self.numbers
    }
}
