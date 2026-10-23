//! Clues, and deriving them from a loop.
//!
//! A clue sits in a cell and says how many of that cell's four sides the loop uses.
//! Cells without a clue say nothing, and most cells on a good puzzle have no clue —
//! which is the difference from Nonogram, where every line carries its clue and
//! generation is "derive and keep". Here, derivation gives a full grid of numbers
//! that is nearly always far too easy, and the generator's real work is deciding
//! which numbers to **take away**. See [`crate::generate`].
//!
//! Derivation itself is total and deterministic: every loop has exactly one full
//! clue grid, and counting four sides cannot go wrong, which keeps this step out of
//! the trust boundary entirely.

use crate::board::{CellId, Geometry};
use crate::loops::Loop;

/// The most sides of one cell a loop can use.
pub const MOST: u8 = 4;

/// One cell's clue: a number, or nothing.
pub type Clue = Option<u8>;

/// A puzzle's clues: one entry per cell, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clues {
    geometry: Geometry,
    numbers: Vec<Clue>,
}

/// Why a set of numbers is not a clue grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CluesError {
    /// The entry count does not match the board.
    WrongLength { expected: usize, found: usize },
    /// A clue is larger than four. A cell has four sides, so a five is not a hard
    /// puzzle, it is a number that means nothing.
    TooLarge { cell: u32, clue: u8 },
}

impl Clues {
    /// Build a clue grid, checking the board's shape and every number.
    ///
    /// A `4` is permitted by the format and is almost always impossible to satisfy:
    /// a cell with all four sides drawn is a loop round that one cell, and since
    /// there may only be one loop, nothing else on the board could be enclosed. It
    /// is allowed through here rather than refused, because "four sides used" is
    /// what the rule says and a clue grid is not the place to decide a puzzle is
    /// unsolvable. The solver decides that, and it is a refusal with evidence.
    pub fn new(geometry: Geometry, numbers: Vec<Clue>) -> Result<Self, CluesError> {
        if numbers.len() != geometry.cell_count() {
            return Err(CluesError::WrongLength {
                expected: geometry.cell_count(),
                found: numbers.len(),
            });
        }
        for (index, clue) in numbers.iter().enumerate() {
            if let Some(clue) = clue {
                if *clue > MOST {
                    return Err(CluesError::TooLarge {
                        cell: u32::try_from(index).unwrap_or(u32::MAX),
                        clue: *clue,
                    });
                }
            }
        }
        Ok(Clues { geometry, numbers })
    }

    /// An empty clue grid: a board with no numbers on it at all.
    ///
    /// Legal to build and never uniquely solvable, which is the point of being able
    /// to build it: the generator starts from the full grid and removes, and the
    /// empty grid is where that would end up if nothing stopped it.
    #[must_use]
    pub fn none(geometry: Geometry) -> Self {
        Clues {
            geometry,
            numbers: vec![None; geometry.cell_count()],
        }
    }

    /// Derive the full clue grid a loop implies: every cell numbered.
    ///
    /// The only way clues are produced from a loop, and the single definition of
    /// what a clue means.
    #[must_use]
    pub fn of(the_loop: &Loop) -> Self {
        let geometry = the_loop.geometry();
        let numbers = geometry
            .cells()
            .map(|cell| Some(sides_used(geometry, the_loop, cell)))
            .collect();
        Clues { geometry, numbers }
    }

    #[must_use]
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }

    #[must_use]
    pub fn at(&self, cell: CellId) -> Clue {
        usize::try_from(cell.0)
            .ok()
            .and_then(|i| self.numbers.get(i).copied())
            .flatten()
    }

    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Clue {
        self.geometry.cell(x, y).and_then(|c| self.at(c))
    }

    /// Every clued cell and its number.
    pub fn given(&self) -> impl Iterator<Item = (CellId, u8)> + '_ {
        self.numbers.iter().enumerate().filter_map(|(index, clue)| {
            clue.map(|clue| (CellId(u32::try_from(index).unwrap_or(u32::MAX)), clue))
        })
    }

    /// How many cells carry a clue. The usual measure of how bare a puzzle looks.
    #[must_use]
    pub fn given_count(&self) -> u32 {
        u32::try_from(self.numbers.iter().filter(|c| c.is_some()).count()).unwrap_or(u32::MAX)
    }

    /// The same grid with one clue taken away.
    #[must_use]
    pub fn without(&self, cell: CellId) -> Self {
        let mut numbers = self.numbers.clone();
        if let Ok(index) = usize::try_from(cell.0) {
            if let Some(slot) = numbers.get_mut(index) {
                *slot = None;
            }
        }
        Clues {
            geometry: self.geometry,
            numbers,
        }
    }

    /// The same grid with one clue put back.
    #[must_use]
    pub fn with(&self, cell: CellId, clue: u8) -> Self {
        let mut numbers = self.numbers.clone();
        if let Ok(index) = usize::try_from(cell.0) {
            if let Some(slot) = numbers.get_mut(index) {
                *slot = Some(clue.min(MOST));
            }
        }
        Clues {
            geometry: self.geometry,
            numbers,
        }
    }

    /// These clues as bytes, one way only.
    ///
    /// The same job `gs_nonogram::clues::Clues::canonical_bytes` does for that
    /// family: one definition of "this puzzle as bytes", so two layers asking
    /// whether two puzzles are the same cannot answer differently. Two clue grids
    /// give equal bytes exactly when they are equal.
    ///
    /// Size and count come first, then one byte per cell, so the encoding cannot be
    /// read two ways. `NO_CLUE` is 255 rather than 0, because 0 is a real clue and
    /// the most common one on a finished puzzle.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.numbers.len());
        out.extend_from_slice(&self.geometry.width().to_le_bytes());
        out.extend_from_slice(&self.geometry.height().to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(self.numbers.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for clue in &self.numbers {
            out.push(clue.unwrap_or(NO_CLUE));
        }
        out
    }

    /// Read back what [`Clues::canonical_bytes`] wrote.
    ///
    /// Strict: anything it cannot read exactly is a refusal, never a best effort.
    /// The bytes it is given have already been vouched for by a signature, so bytes
    /// that do not decode mean something is wrong that guessing would hide.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let width = u16_at(bytes, 0, "width")?;
        let height = u16_at(bytes, 2, "height")?;
        let stated = u32::from_le_bytes(
            bytes
                .get(4..8)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::Truncated { field: "count" })?,
        );
        let geometry = Geometry::new(width, height).map_err(DecodeError::Size)?;
        let expected = geometry.cell_count();
        if usize::try_from(stated) != Ok(expected) {
            return Err(DecodeError::CountDisagreesWithSize {
                expected,
                found: stated,
            });
        }
        let body = bytes
            .get(8..)
            .ok_or(DecodeError::Truncated { field: "clues" })?;
        if body.len() < expected {
            return Err(DecodeError::Truncated { field: "clues" });
        }
        if body.len() > expected {
            return Err(DecodeError::TrailingBytes(body.len() - expected));
        }
        let mut numbers = Vec::with_capacity(expected);
        for (index, byte) in body.iter().enumerate() {
            let cell = u32::try_from(index).unwrap_or(u32::MAX);
            match *byte {
                NO_CLUE => numbers.push(None),
                clue if clue <= MOST => numbers.push(Some(clue)),
                clue => return Err(DecodeError::NotAClue { cell, byte: clue }),
            }
        }
        Clues::new(geometry, numbers).map_err(DecodeError::Clues)
    }
}

/// The byte that means "this cell has no clue".
///
/// Not zero. Zero is a clue, and a common one, so a zero-filled page must not decode
/// as a board covered in zeroes — the same reasoning that keeps every tag constant
/// in `gs_core::package` away from zero.
pub const NO_CLUE: u8 = 255;

/// Why a byte string is not a clue grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Truncated {
        field: &'static str,
    },
    /// Bytes left over after a complete grid. Refused rather than ignored, so that
    /// [`Clues::canonical_bytes`] stays the single spelling of a puzzle.
    TrailingBytes(usize),
    /// The stated cell count does not match the stated board size.
    CountDisagreesWithSize {
        expected: usize,
        found: u32,
    },
    /// A byte is neither a clue nor [`NO_CLUE`]. Never read as "no clue": a byte
    /// nobody wrote on purpose is a damaged file, not a blank cell.
    NotAClue {
        cell: u32,
        byte: u8,
    },
    Size(crate::board::SizeError),
    Clues(CluesError),
}

fn u16_at(bytes: &[u8], at: usize, field: &'static str) -> Result<u16, DecodeError> {
    bytes
        .get(at..at + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(DecodeError::Truncated { field })
}

/// How many of a cell's four sides a loop uses. The single definition of a clue.
#[must_use]
pub fn sides_used(geometry: Geometry, the_loop: &Loop, cell: CellId) -> u8 {
    let Some(edges) = geometry.edges_of_cell(cell) else {
        return 0;
    };
    u8::try_from(edges.iter().filter(|e| the_loop.is_drawn(**e)).count()).unwrap_or(MOST)
}
