//! The one spelling of a Kakuro puzzle as bytes.
//!
//! A package carries a puzzle as a payload, and a browser reads it back. These two
//! functions are the whole of that format, and they are each other's inverse.
//!
//! # Why this family needed a module the other two did not
//!
//! For Nonogram and Slitherlink a puzzle is a list of numbers over a board whose
//! shape is implied by its size. A Kakuro's shape is not implied by anything: the
//! same 7 by 7 can be cut into runs a thousand ways, and which way it was cut is
//! part of the puzzle rather than part of its presentation. So the bytes have to
//! carry the black squares as well as the totals, and the order of the totals only
//! means anything relative to the runs the black squares produce.
//!
//! That coupling is the reason this is strict in a way the other two formats are not.
//! Reading the blocks gives a [`Layout`], the layout finds its own runs, and the
//! totals are then read against **the run count that layout found** rather than
//! against a number in the file. A file claiming eleven totals for a layout with ten
//! runs is refused. The alternative — line the totals up with whatever runs happen to
//! exist and ignore the rest — would turn a damaged file into a different puzzle that
//! still verified, because the signature is over these bytes and not over their
//! meaning.
//!
//! # Why a block is not a bit
//!
//! One byte per square, not one bit. A packed bit field is eight times smaller and a
//! Kakuro board is at most a few hundred squares, so the saving is a few dozen bytes
//! on a file that already carries a signature. What a byte per square buys is that
//! [`BLOCK`] and [`WHITE`] are both values somebody wrote on purpose, and **every
//! other byte is a refusal**. A run of zeroes in a damaged download decodes as a board
//! with no black squares at all, which is a legal Kakuro, so the format has to be able
//! to tell "all white" from "nothing was written here".

use crate::layout::{Layout, LayoutError};
use crate::rules::{Kakuro, PuzzleError};
use crate::totals::{Totals, TotalsError};

/// The byte that marks a black square.
///
/// Not zero, and not one. A damaged or zero-filled file must not decode as a legal
/// board, which is the same reasoning that keeps `NO_CLUE` away from zero in
/// Slitherlink's format and every tag constant away from zero in `gs_core::package`.
pub const BLOCK: u8 = 0xB1;

/// The byte that marks a white square, where a digit goes.
pub const WHITE: u8 = 0x1F;

/// How many bytes come before the block pattern.
const HEADER: usize = 8;

/// Why a byte string is not a Kakuro puzzle.
///
/// Every variant is a refusal. The bytes reaching here have already been vouched for
/// by a signature, so bytes that do not decode mean something is wrong that a best
/// effort would hide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes stop part way through, while reading the named field.
    Truncated { field: &'static str },
    /// Bytes left over after a complete puzzle. Refused rather than ignored, so that
    /// [`canonical_bytes`] stays the single spelling of a puzzle.
    TrailingBytes(usize),
    /// The stated square count does not match the stated board size.
    CountDisagreesWithSize { expected: usize, found: u32 },
    /// A byte in the block pattern is neither [`BLOCK`] nor [`WHITE`]. Never read as
    /// white: a byte nobody wrote on purpose is a damaged file, not an empty square.
    NotABlockOrWhite { square: u32, byte: u8 },
    /// The file carries a number of totals that is not the number of runs the block
    /// pattern produces. See this module's notes: this is refused rather than
    /// reconciled, because reconciling it would silently give a different puzzle.
    TotalCountDisagreesWithRuns { runs: usize, totals: usize },
    /// The block pattern does not describe a board this family can play.
    Layout(LayoutError),
    /// A total cannot be made by the run it belongs to.
    Totals(TotalsError),
    /// The layout and the totals are each fine and do not go together.
    Puzzle(PuzzleError),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Truncated { field } => {
                write!(f, "the puzzle stops part way through, at the {field}")
            }
            DecodeError::TrailingBytes(extra) => {
                write!(f, "there are {extra} bytes left over after the puzzle")
            }
            DecodeError::CountDisagreesWithSize { expected, found } => write!(
                f,
                "the board is {expected} squares in size, and then gives {found} of them"
            ),
            DecodeError::NotABlockOrWhite { square, byte } => write!(
                f,
                "square {square} holds {byte}, which marks neither a black square nor a \
                 white one"
            ),
            DecodeError::TotalCountDisagreesWithRuns { runs, totals } => write!(
                f,
                "the black squares make {runs} runs, and the puzzle gives {totals} totals"
            ),
            DecodeError::Layout(why) => write!(f, "the black squares do not make a board: {why:?}"),
            DecodeError::Totals(why) => write!(f, "a total cannot be made: {why:?}"),
            DecodeError::Puzzle(why) => write!(f, "the board and its totals disagree: {why:?}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Write a puzzle as the bytes a package carries.
///
/// Layout: width, height and square count as little-endian numbers, then one byte per
/// square marking black or white in reading order, then one little-endian `u16` per
/// run in the order [`Layout::runs`] gives them.
///
/// The run count is **not** written. It is a fact about the block pattern, and a file
/// carrying its own copy of it would be a file that could disagree with itself. What
/// the reader checks instead is that the number of totals present matches the runs it
/// found, which is the same check without the second copy.
#[must_use]
pub fn canonical_bytes(puzzle: &Kakuro) -> Vec<u8> {
    let layout = &puzzle.layout;
    let squares = layout.square_count();
    let mut out = Vec::with_capacity(HEADER + squares + 2 * layout.run_count());
    out.extend_from_slice(&layout.width().to_le_bytes());
    out.extend_from_slice(&layout.height().to_le_bytes());
    out.extend_from_slice(&u32::try_from(squares).unwrap_or(u32::MAX).to_le_bytes());
    for square in layout.squares() {
        out.push(if layout.is_block(square) {
            BLOCK
        } else {
            WHITE
        });
    }
    for total in puzzle.totals.as_slice() {
        out.extend_from_slice(&total.to_le_bytes());
    }
    out
}

/// Read back what [`canonical_bytes`] wrote.
///
/// Strict throughout: anything it cannot read exactly is a refusal, never a best
/// effort.
pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Kakuro, DecodeError> {
    let width = u16_at(bytes, 0, "width")?;
    let height = u16_at(bytes, 2, "height")?;
    let stated = u32::from_le_bytes(bytes.get(4..HEADER).and_then(|s| s.try_into().ok()).ok_or(
        DecodeError::Truncated {
            field: "square count",
        },
    )?);

    // Multiplied in `usize` after widening, so a hostile width and height cannot wrap
    // to a small number and then agree with a small stated count.
    let expected = usize::from(width) * usize::from(height);
    if usize::try_from(stated) != Ok(expected) {
        return Err(DecodeError::CountDisagreesWithSize {
            expected,
            found: stated,
        });
    }

    let body = bytes.get(HEADER..).ok_or(DecodeError::Truncated {
        field: "black squares",
    })?;
    let pattern = body.get(..expected).ok_or(DecodeError::Truncated {
        field: "black squares",
    })?;
    let mut blocks = Vec::with_capacity(expected);
    for (index, byte) in pattern.iter().enumerate() {
        match *byte {
            BLOCK => blocks.push(true),
            WHITE => blocks.push(false),
            byte => {
                return Err(DecodeError::NotABlockOrWhite {
                    square: u32::try_from(index).unwrap_or(u32::MAX),
                    byte,
                })
            }
        }
    }

    // The layout finds its own runs. From here on, the number of runs is a fact about
    // the bytes already read, and the totals are checked against it.
    let layout = Layout::new(width, height, blocks).map_err(DecodeError::Layout)?;

    let rest = &body[expected..];
    if rest.len() % 2 != 0 {
        return Err(DecodeError::Truncated { field: "totals" });
    }
    let given = rest.len() / 2;
    if given != layout.run_count() {
        // Deliberately one error for both directions. Too few totals is as much a
        // disagreement between the pattern and the numbers as too many, and calling
        // the short case "truncated" would suggest the file is merely incomplete when
        // it may be a complete file for a different board.
        return Err(DecodeError::TotalCountDisagreesWithRuns {
            runs: layout.run_count(),
            totals: given,
        });
    }
    let numbers = rest
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    let totals = Totals::new(&layout, numbers).map_err(DecodeError::Totals)?;
    Kakuro::new(layout, totals).map_err(DecodeError::Puzzle)
}

fn u16_at(bytes: &[u8], at: usize, field: &'static str) -> Result<u16, DecodeError> {
    bytes
        .get(at..at + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(DecodeError::Truncated { field })
}

impl Kakuro {
    /// This puzzle as the bytes a package carries. See [`canonical_bytes`].
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_bytes(self)
    }
}
