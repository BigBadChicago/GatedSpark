//! The board's shape: which squares are blocks, which are white, and what the runs
//! are.
//!
//! Nonogram's board and its state are the same shape, so that family needed no
//! module like this one. Slitherlink needed one because three index spaces are in
//! play at once. Kakuro needs one for a third reason, and it is the reason this
//! family was chosen: **the shape is not implied by the size.** A 7 by 7 Nonogram is
//! one board. A 7 by 7 Kakuro is one of a vast number of boards, because where the
//! blocks go is a free choice, and every choice gives different runs.
//!
//! So the layout is one module with no arithmetic rules in it, and the rules layer
//! depends on it rather than counting for itself.
//!
//! # The two index spaces
//!
//! * A **square** is a position on the grid, numbered in reading order over
//!   `width * height`. Every position has one, block or white.
//! * A **white square** is numbered in reading order over the white ones only.
//!
//! Logical state is white-shaped, not square-shaped, for the reason Slitherlink's
//! state is edge-shaped: the player only ever writes in white squares, and a
//! square-shaped state would have somewhere to put a digit that cannot legally hold
//! one. Making that unrepresentable is cheaper than checking for it everywhere.
//!
//! # Runs
//!
//! A **run** is a maximal straight line of white squares, going across or going
//! down. "Maximal" means it stops at a block or at the board's rim, so the runs of a
//! layout are decided by the layout alone and are not a choice. Every white square
//! belongs to exactly one across run and exactly one down run, and a run may be a
//! single square long.
//!
//! Runs are numbered across-first, each block in reading order of its first square.
//! That order is part of the stored format and part of every emitted query, so it is
//! fixed here once and nothing else counts it.

use gs_core::state::Dims;

/// A grid position's number, in reading order over `width * height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SquareId(pub u32);

/// A white square's number, in reading order over the white squares only.
///
/// A thin wrapper rather than a bare `u32`, because a square number and a white
/// square number are both small integers over the same board and nothing but a type
/// stops one being passed where the other belongs. On a board with blocks they are
/// never equal past the first block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WhiteId(pub u32);

/// A run's number, across-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub u32);

/// Which way a run goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Along {
    /// Left to right.
    Across,
    /// Top to bottom.
    Down,
}

/// The longest a run may be.
///
/// Nine, because a run holds distinct digits from one to nine and there are only
/// nine of those. A tenth square could not be filled by any assignment, so a layout
/// containing one is not a hard puzzle, it is an impossible one — and refusing it
/// here means the solver is never asked a question whose answer was decided by
/// counting.
pub const LONGEST_RUN: usize = 9;

/// One run: where it starts, which way it goes, and the white squares in it, in
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub along: Along,
    /// The run's first square, so a page can draw the total next to it.
    pub start: SquareId,
    /// The white squares in the run, in reading order along it.
    pub squares: Vec<WhiteId>,
}

impl Run {
    #[must_use]
    pub fn len(&self) -> usize {
        self.squares.len()
    }

    /// Never true: a run is built from at least one white square. Present because
    /// clippy asks for it beside `len`, and answering honestly is cheaper than
    /// silencing it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.squares.is_empty()
    }

    /// The smallest and largest total this run could possibly have.
    ///
    /// A run of `n` distinct digits from one to nine sums to at least `1 + 2 + ... +
    /// n` and at most `9 + 8 + ... `. Used to refuse a total no assignment could
    /// reach, before any solver is asked.
    #[must_use]
    pub fn total_bounds(&self) -> (u16, u16) {
        let n = u16::try_from(self.len()).unwrap_or(u16::MAX);
        let least = n * (n + 1) / 2;
        let most = n * (19 - n) / 2;
        (least, most)
    }
}

/// Why a layout was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// A board needs at least one square in each direction.
    TooSmall { width: u16, height: u16 },
    /// The white squares would not fit in one row of logical state.
    ///
    /// State is indexed by a `u16` width, so the white count has to fit in one. It
    /// is a refusal rather than a wrap-around, because a silently wrapped index
    /// would put two different squares at one address.
    TooBig { whites: usize },
    /// The block pattern does not have one entry per square.
    WrongLength { expected: usize, found: usize },
    /// A run is longer than nine squares, so no assignment of distinct digits one to
    /// nine could fill it. See [`LONGEST_RUN`].
    RunTooLong {
        start: u32,
        along: Along,
        len: usize,
    },
    /// Every square is a block, so there is nothing to fill in. Refused at the door
    /// rather than producing an empty puzzle that every later check passes for the
    /// wrong reason.
    NoWhiteSquares,
}

/// A board's shape, and every lookup between its two index spaces.
///
/// Holds no totals and no digits: it is the shape only, so the totals layer and the
/// rules layer cannot each grow their own copy of the run-finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    width: u16,
    height: u16,
    /// One entry per square: true where the square is a block.
    blocks: Vec<bool>,
    /// One entry per square: the white number, or `None` for a block.
    white_of_square: Vec<Option<WhiteId>>,
    /// One entry per white square: which square it sits on.
    square_of_white: Vec<SquareId>,
    runs: Vec<Run>,
    /// One entry per white square: the across run it is in, then the down run.
    runs_of_white: Vec<(RunId, RunId)>,
}

impl Layout {
    /// Build a layout from a block pattern, in reading order.
    ///
    /// Everything derived — the white numbering, the runs, which run a square is in
    /// — is worked out once here, so no later code re-derives it and gets a
    /// different answer.
    pub fn new(width: u16, height: u16, blocks: Vec<bool>) -> Result<Self, LayoutError> {
        if width == 0 || height == 0 {
            return Err(LayoutError::TooSmall { width, height });
        }
        let square_count = usize::from(width) * usize::from(height);
        if blocks.len() != square_count {
            return Err(LayoutError::WrongLength {
                expected: square_count,
                found: blocks.len(),
            });
        }

        let mut white_of_square = vec![None; square_count];
        let mut square_of_white = Vec::new();
        for (index, is_block) in blocks.iter().enumerate() {
            if *is_block {
                continue;
            }
            let white = WhiteId(u32::try_from(square_of_white.len()).unwrap_or(u32::MAX));
            white_of_square[index] = Some(white);
            square_of_white.push(SquareId(u32::try_from(index).unwrap_or(u32::MAX)));
        }
        if square_of_white.is_empty() {
            return Err(LayoutError::NoWhiteSquares);
        }
        if u16::try_from(square_of_white.len()).is_err() {
            return Err(LayoutError::TooBig {
                whites: square_of_white.len(),
            });
        }

        let mut partial = Layout {
            width,
            height,
            blocks,
            white_of_square,
            square_of_white,
            runs: Vec::new(),
            runs_of_white: Vec::new(),
        };
        let runs = partial.find_runs()?;
        let runs_of_white = index_runs(partial.white_count(), &runs);
        partial.runs = runs;
        partial.runs_of_white = runs_of_white;
        Ok(partial)
    }

    /// A board with no blocks at all: one across run per row, one down run per
    /// column.
    ///
    /// Legal up to nine by nine and refused above it, which is the size rule doing
    /// its job rather than a special case.
    pub fn open(width: u16, height: u16) -> Result<Self, LayoutError> {
        Layout::new(
            width,
            height,
            vec![false; usize::from(width) * usize::from(height)],
        )
    }

    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }

    #[must_use]
    pub fn square_count(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    #[must_use]
    pub fn white_count(&self) -> usize {
        self.square_of_white.len()
    }

    #[must_use]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// The shape of the logical state that holds this board's digits.
    ///
    /// One row, one entry per **white** square. See the module note on why the state
    /// is white-shaped.
    #[must_use]
    pub fn state_dims(&self) -> Dims {
        Dims::new(u16::try_from(self.white_count()).unwrap_or(u16::MAX), 1)
    }

    #[must_use]
    pub fn square(&self, x: u16, y: u16) -> Option<SquareId> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(SquareId(
            u32::from(y) * u32::from(self.width) + u32::from(x),
        ))
    }

    #[must_use]
    pub fn square_at(&self, square: SquareId) -> Option<(u16, u16)> {
        let width = u32::from(self.width);
        let x = u16::try_from(square.0 % width).ok()?;
        let y = u16::try_from(square.0 / width).ok()?;
        if y >= self.height {
            return None;
        }
        Some((x, y))
    }

    #[must_use]
    pub fn is_block(&self, square: SquareId) -> bool {
        usize::try_from(square.0)
            .ok()
            .and_then(|index| self.blocks.get(index))
            .copied()
            .unwrap_or(true)
    }

    /// The white number of a square, or `None` if it is a block or off the board.
    #[must_use]
    pub fn white_at(&self, square: SquareId) -> Option<WhiteId> {
        usize::try_from(square.0)
            .ok()
            .and_then(|index| self.white_of_square.get(index))
            .copied()
            .flatten()
    }

    /// Where a white square sits on the grid.
    #[must_use]
    pub fn square_of(&self, white: WhiteId) -> Option<SquareId> {
        usize::try_from(white.0)
            .ok()
            .and_then(|index| self.square_of_white.get(index))
            .copied()
    }

    #[must_use]
    pub fn run(&self, run: RunId) -> Option<&Run> {
        usize::try_from(run.0)
            .ok()
            .and_then(|index| self.runs.get(index))
    }

    /// The across run and the down run a white square is in, in that order.
    ///
    /// Always both, because every white square sits in exactly one of each. That is
    /// what makes a total mean the same thing everywhere on the board.
    #[must_use]
    pub fn runs_of(&self, white: WhiteId) -> Option<(RunId, RunId)> {
        usize::try_from(white.0)
            .ok()
            .and_then(|index| self.runs_of_white.get(index))
            .copied()
    }

    pub fn runs(&self) -> impl Iterator<Item = (RunId, &Run)> {
        self.runs
            .iter()
            .enumerate()
            .map(|(index, run)| (RunId(u32::try_from(index).unwrap_or(u32::MAX)), run))
    }

    pub fn squares(&self) -> impl Iterator<Item = SquareId> + '_ {
        (0..u32::try_from(self.square_count()).unwrap_or(u32::MAX)).map(SquareId)
    }

    pub fn whites(&self) -> impl Iterator<Item = WhiteId> + '_ {
        (0..u32::try_from(self.white_count()).unwrap_or(u32::MAX)).map(WhiteId)
    }

    /// Find every maximal run, across first then down.
    ///
    /// The one place runs are worked out. A run ends at a block or at the rim, and
    /// those two cases are the same case here on purpose: treating the rim as a
    /// block is what makes a corner run mean the same thing as a middle one.
    fn find_runs(&self) -> Result<Vec<Run>, LayoutError> {
        let mut runs = Vec::new();
        for y in 0..self.height {
            let mut current: Vec<WhiteId> = Vec::new();
            let mut start = None;
            for x in 0..=self.width {
                let white = if x == self.width {
                    None
                } else {
                    self.square(x, y).and_then(|s| self.white_at(s))
                };
                match white {
                    Some(white) => {
                        if current.is_empty() {
                            start = self.square(x, y);
                        }
                        current.push(white);
                    }
                    None => {
                        Self::close(&mut runs, Along::Across, &mut start, &mut current)?;
                    }
                }
            }
        }
        for x in 0..self.width {
            let mut current: Vec<WhiteId> = Vec::new();
            let mut start = None;
            for y in 0..=self.height {
                let white = if y == self.height {
                    None
                } else {
                    self.square(x, y).and_then(|s| self.white_at(s))
                };
                match white {
                    Some(white) => {
                        if current.is_empty() {
                            start = self.square(x, y);
                        }
                        current.push(white);
                    }
                    None => {
                        Self::close(&mut runs, Along::Down, &mut start, &mut current)?;
                    }
                }
            }
        }
        Ok(runs)
    }

    /// Finish the run being walked, if there is one.
    fn close(
        runs: &mut Vec<Run>,
        along: Along,
        start: &mut Option<SquareId>,
        current: &mut Vec<WhiteId>,
    ) -> Result<(), LayoutError> {
        if current.is_empty() {
            return Ok(());
        }
        let squares = core::mem::take(current);
        let start = start.take().unwrap_or(SquareId(0));
        if squares.len() > LONGEST_RUN {
            return Err(LayoutError::RunTooLong {
                start: start.0,
                along,
                len: squares.len(),
            });
        }
        runs.push(Run {
            along,
            start,
            squares,
        });
        Ok(())
    }
}

/// For each white square, the across run and the down run it belongs to.
///
/// Built from the runs rather than searched for later, so there is one answer to
/// "which run is this square in" and not two that might disagree.
fn index_runs(white_count: usize, runs: &[Run]) -> Vec<(RunId, RunId)> {
    let mut across = vec![RunId(0); white_count];
    let mut down = vec![RunId(0); white_count];
    for (index, run) in runs.iter().enumerate() {
        let id = RunId(u32::try_from(index).unwrap_or(u32::MAX));
        for white in &run.squares {
            let Ok(slot) = usize::try_from(white.0) else {
                continue;
            };
            match run.along {
                Along::Across => {
                    if let Some(entry) = across.get_mut(slot) {
                        *entry = id;
                    }
                }
                Along::Down => {
                    if let Some(entry) = down.get_mut(slot) {
                        *entry = id;
                    }
                }
            }
        }
    }
    across.into_iter().zip(down).collect()
}
