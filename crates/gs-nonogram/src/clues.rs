//! Clues, and deriving them from a pattern.
//!
//! Clue derivation is the step that makes Nonogram pipeline-shaped: pick a target
//! pattern, derive the clues mechanically, then let the solver decide whether the
//! result is uniquely solvable. Derivation is total and deterministic — every
//! pattern has exactly one clue set — which is what keeps the generator's own step
//! out of the trust boundary entirely.

use gs_core::state::Dims;

/// A filled/empty pattern: the intended picture, before clues exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    dims: Dims,
    filled: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// The cell count does not match the dimensions.
    WrongLength { expected: usize, found: usize },
}

impl Pattern {
    pub fn new(dims: Dims, filled: Vec<bool>) -> Result<Self, PatternError> {
        if filled.len() != dims.cell_count() {
            return Err(PatternError::WrongLength {
                expected: dims.cell_count(),
                found: filled.len(),
            });
        }
        Ok(Pattern { dims, filled })
    }

    /// Build from rows of `#` (filled) and anything else (empty).
    ///
    /// For fixtures and tests. Rows must all be the same length.
    pub fn from_rows(rows: &[&str]) -> Result<Self, PatternError> {
        let height = u16::try_from(rows.len()).unwrap_or(u16::MAX);
        let width =
            u16::try_from(rows.first().map_or(0, |r| r.chars().count())).unwrap_or(u16::MAX);
        let dims = Dims::new(width, height);
        let filled: Vec<bool> = rows
            .iter()
            .flat_map(|r| r.chars().map(|c| c == '#'))
            .collect();
        Pattern::new(dims, filled)
    }

    #[must_use]
    pub fn dims(&self) -> Dims {
        self.dims
    }

    #[must_use]
    pub fn is_filled(&self, x: u16, y: u16) -> bool {
        self.dims
            .index_of(x, y)
            .and_then(|i| self.filled.get(i as usize).copied())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn row(&self, y: u16) -> Vec<bool> {
        (0..self.dims.width).map(|x| self.is_filled(x, y)).collect()
    }

    #[must_use]
    pub fn column(&self, x: u16) -> Vec<bool> {
        (0..self.dims.height)
            .map(|y| self.is_filled(x, y))
            .collect()
    }
}

/// One line's clue: the lengths of its filled runs, in order.
///
/// An all-empty line has an empty run list. Some presentations write that as `[0]`;
/// this does not, because an empty list and a run of length zero would otherwise be
/// two spellings of one thing — and two spellings is how two layers come to disagree.
pub type Runs = Vec<u16>;

/// A puzzle's clues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clues {
    pub dims: Dims,
    pub rows: Vec<Runs>,
    pub columns: Vec<Runs>,
}

/// The run lengths of one line. The single definition of what a clue means.
#[must_use]
pub fn runs_of(line: &[bool]) -> Runs {
    let mut runs = Vec::new();
    let mut current = 0u16;
    for filled in line {
        if *filled {
            current += 1;
        } else if current > 0 {
            runs.push(current);
            current = 0;
        }
    }
    if current > 0 {
        runs.push(current);
    }
    runs
}

impl Clues {
    /// Derive the clues a pattern implies. Total, deterministic, and the only way
    /// clues are produced.
    #[must_use]
    pub fn of(pattern: &Pattern) -> Self {
        let dims = pattern.dims();
        Clues {
            dims,
            rows: (0..dims.height).map(|y| runs_of(&pattern.row(y))).collect(),
            columns: (0..dims.width)
                .map(|x| runs_of(&pattern.column(x)))
                .collect(),
        }
    }

    /// Whether the clues are internally consistent: the rows and the columns must
    /// account for the same number of filled cells.
    ///
    /// A cheap necessary condition, not a sufficient one. Clues that fail this cannot
    /// describe any board, so there is no point encoding them; clues that pass may
    /// still be unsatisfiable, which is the solver's question and not this one's.
    #[must_use]
    pub fn totals_agree(&self) -> bool {
        let rows: u32 = self.rows.iter().flatten().copied().map(u32::from).sum();
        let columns: u32 = self.columns.iter().flatten().copied().map(u32::from).sum();
        rows == columns
    }

    /// These clues as bytes, one way only.
    ///
    /// The same role [`gs_core::state::State::canonical_bytes`] plays for a board: a
    /// single definition of "this thing as bytes", so that two layers asking whether
    /// two puzzles are the same cannot answer differently. Two clue sets give equal
    /// bytes exactly when they are equal.
    ///
    /// Little-endian throughout, lengths before contents, so the encoding cannot be
    /// ambiguous: without the lengths, `rows = [[1,2]]` and `rows = [[1],[2]]` would
    /// produce the same bytes and two different puzzles would look like one.
    ///
    /// Used today to tell duplicates apart while filling a pool. It is also what a
    /// content id should be taken over in M4, which is the reason it is one function
    /// here rather than a detail of the worker.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.dims.width.to_le_bytes());
        out.extend_from_slice(&self.dims.height.to_le_bytes());
        for lines in [&self.rows, &self.columns] {
            out.extend_from_slice(&u32::try_from(lines.len()).unwrap_or(u32::MAX).to_le_bytes());
            for line in lines {
                out.extend_from_slice(&u32::try_from(line.len()).unwrap_or(u32::MAX).to_le_bytes());
                for run in line {
                    out.extend_from_slice(&run.to_le_bytes());
                }
            }
        }
        out
    }
}

/// Why a byte string is not a clue set.
///
/// Every variant is a refusal. There is no variant meaning "decoded, with a guess
/// filled in": a clue set that had to be guessed at is not the clue set that was
/// signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes end in the middle of something.
    Truncated { field: &'static str },
    /// Bytes left over after a complete clue set. Refused rather than ignored,
    /// because a reader that ignored them would accept two different byte strings as
    /// the same puzzle — and then [`Clues::canonical_bytes`] would no longer be the
    /// single definition of these clues as bytes.
    TrailingBytes(usize),
    /// The number of lines does not match the dimensions: a board 8 wide must have
    /// exactly 8 columns of clues. `Clues::of` can never produce such a thing, so
    /// accepting one here would let a decoded puzzle be in a state no generated
    /// puzzle can reach.
    LineCountDisagreesWithDims {
        axis: &'static str,
        expected: usize,
        found: u32,
    },
}

impl Clues {
    /// Read back what [`Clues::canonical_bytes`] wrote.
    ///
    /// This is what makes a package playable: a device receives bytes and needs the
    /// puzzle. The two functions are each other's inverse, which
    /// `tests/packaging.rs::clue_bytes_round_trip` holds them to — including for
    /// clue sets with empty lines, which are the case an encoding without lengths
    /// gets wrong.
    ///
    /// Strict on purpose. Anything it cannot read exactly is an error rather than a
    /// best effort, because the bytes it is given have already been vouched for by a
    /// signature: if they do not decode, something is wrong that guessing would hide.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = ByteReader { bytes, at: 0 };
        let width = r.u16("width")?;
        let height = r.u16("height")?;
        let dims = Dims::new(width, height);

        // Rows first, then columns, in the order `canonical_bytes` writes them. The
        // expected count comes from the dimensions rather than from the file, so a
        // stated count that disagrees is a refusal and not a new shape.
        let rows = r.lines("rows", usize::from(height))?;
        let columns = r.lines("columns", usize::from(width))?;

        let remaining = r.bytes.len().saturating_sub(r.at);
        if remaining != 0 {
            return Err(DecodeError::TrailingBytes(remaining));
        }

        Ok(Clues {
            dims,
            rows,
            columns,
        })
    }
}

/// A cursor that cannot read past the end of its slice, naming the field it was
/// reading when it ran out.
struct ByteReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl ByteReader<'_> {
    fn take(&mut self, n: usize, field: &'static str) -> Result<&[u8], DecodeError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(DecodeError::Truncated { field })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(DecodeError::Truncated { field })?;
        self.at = end;
        Ok(slice)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, DecodeError> {
        let b: [u8; 2] = self.take(2, field)?.try_into().expect("two bytes");
        Ok(u16::from_le_bytes(b))
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, DecodeError> {
        let b: [u8; 4] = self.take(4, field)?.try_into().expect("four bytes");
        Ok(u32::from_le_bytes(b))
    }

    /// One axis: a line count that must match the dimensions, then each line's runs.
    fn lines(&mut self, axis: &'static str, expected: usize) -> Result<Vec<Runs>, DecodeError> {
        let found = self.u32(axis)?;
        if usize::try_from(found) != Ok(expected) {
            return Err(DecodeError::LineCountDisagreesWithDims {
                axis,
                expected,
                found,
            });
        }
        let mut lines = Vec::with_capacity(expected);
        for _ in 0..expected {
            let run_count = usize::try_from(self.u32(axis)?)
                .map_err(|_| DecodeError::Truncated { field: axis })?;
            let mut runs = Runs::with_capacity(run_count);
            for _ in 0..run_count {
                runs.push(self.u16(axis)?);
            }
            lines.push(runs);
        }
        Ok(lines)
    }
}
