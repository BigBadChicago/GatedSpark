//! The board's geometry: cells, dots and the edges between them.
//!
//! Nonogram's board and its state are the same shape, so that family needed no
//! module like this one. Slitherlink's do not. A player marks **edges**, the clues
//! are in **cells**, and the loop rule is about **dots**, so three different index
//! spaces are in play at once and every one of them is a chance to be off by one.
//!
//! So the geometry is one module with no rules in it, and the rules depend on it
//! rather than counting for themselves.
//!
//! # The three spaces
//!
//! A board `width` cells across and `height` cells down has:
//!
//! * `width * height` **cells**. A cell may carry a clue.
//! * `(width + 1) * (height + 1)` **dots**, the corners of the cells.
//! * `width * (height + 1)` **horizontal edges** and `(width + 1) * height`
//!   **vertical edges**, each joining two neighbouring dots.
//!
//! Edges are numbered horizontal-first, each block in reading order. That order is
//! part of the stored format and part of every emitted query, so it is fixed here
//! once: [`Geometry::edge_count`] and the four lookups below are the only code that
//! knows it.

use gs_core::state::Dims;

/// An edge's number. A thin wrapper rather than a bare `u32` because an edge number
/// and a cell number are both small integers and nothing but a type stops one being
/// passed where the other belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(pub u32);

/// A dot's number, in reading order over `(width + 1) * (height + 1)` dots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DotId(pub u32);

/// A cell's number, in reading order over `width * height` cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(pub u32);

/// Which way an edge runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Along {
    /// Joins dot `(x, y)` to dot `(x + 1, y)`.
    Across,
    /// Joins dot `(x, y)` to dot `(x, y + 1)`.
    Down,
}

/// Why a board size was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeError {
    /// A board needs at least one cell in each direction. A board with no cells has
    /// no loop that could go round anything, so it is refused at the door rather
    /// than producing an empty puzzle that every check passes.
    TooSmall { width: u16, height: u16 },
    /// The edges would not fit in one row of logical state.
    ///
    /// State is indexed by a `u16` width, so the edge count has to fit in one. The
    /// limit is about 180 cells square, far above anything a person plays, and it is
    /// a refusal rather than a wrap-around because a silently wrapped index would
    /// put two different edges at one address.
    TooBig { edges: usize },
}

/// A board's size, and every lookup between its three index spaces.
///
/// Holds no puzzle and no marks: it is the shape only, so the clue layer and the
/// rules layer cannot each grow their own copy of the arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    width: u16,
    height: u16,
}

impl Geometry {
    /// A board `width` cells across and `height` cells down.
    pub fn new(width: u16, height: u16) -> Result<Self, SizeError> {
        if width == 0 || height == 0 {
            return Err(SizeError::TooSmall { width, height });
        }
        let geometry = Geometry { width, height };
        let edges = geometry.edge_count();
        if u16::try_from(edges).is_err() {
            return Err(SizeError::TooBig { edges });
        }
        Ok(geometry)
    }

    #[must_use]
    pub fn width(self) -> u16 {
        self.width
    }

    #[must_use]
    pub fn height(self) -> u16 {
        self.height
    }

    #[must_use]
    pub fn cell_count(self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    #[must_use]
    pub fn dots_across(self) -> u16 {
        self.width + 1
    }

    #[must_use]
    pub fn dots_down(self) -> u16 {
        self.height + 1
    }

    #[must_use]
    pub fn dot_count(self) -> usize {
        usize::from(self.dots_across()) * usize::from(self.dots_down())
    }

    /// How many horizontal edges there are. The first block of the numbering.
    #[must_use]
    pub fn across_count(self) -> usize {
        usize::from(self.width) * usize::from(self.dots_down())
    }

    /// How many vertical edges there are. The second block of the numbering.
    #[must_use]
    pub fn down_count(self) -> usize {
        usize::from(self.dots_across()) * usize::from(self.height)
    }

    #[must_use]
    pub fn edge_count(self) -> usize {
        self.across_count() + self.down_count()
    }

    /// The shape of the logical state that holds this board's marks.
    ///
    /// One row, one entry per edge. A player's marks live on the edges, so the state
    /// is edge-shaped and not cell-shaped; a cell-shaped state would have nowhere to
    /// put the thing the player is actually doing.
    #[must_use]
    pub fn state_dims(self) -> Dims {
        Dims::new(u16::try_from(self.edge_count()).unwrap_or(u16::MAX), 1)
    }

    /// The horizontal edge from dot `(x, y)` to dot `(x + 1, y)`.
    #[must_use]
    pub fn across(self, x: u16, y: u16) -> Option<EdgeId> {
        if x >= self.width || y >= self.dots_down() {
            return None;
        }
        Some(EdgeId(u32::from(y) * u32::from(self.width) + u32::from(x)))
    }

    /// The vertical edge from dot `(x, y)` to dot `(x, y + 1)`.
    #[must_use]
    pub fn down(self, x: u16, y: u16) -> Option<EdgeId> {
        if x >= self.dots_across() || y >= self.height {
            return None;
        }
        let offset = u32::try_from(self.across_count()).unwrap_or(u32::MAX);
        Some(EdgeId(
            offset + u32::from(y) * u32::from(self.dots_across()) + u32::from(x),
        ))
    }

    /// Where an edge is and which way it runs. The inverse of [`Geometry::across`]
    /// and [`Geometry::down`], so a query or a stored file can be read back.
    #[must_use]
    pub fn edge_at(self, edge: EdgeId) -> Option<(Along, u16, u16)> {
        let index = usize::try_from(edge.0).ok()?;
        if index < self.across_count() {
            let width = usize::from(self.width);
            let x = u16::try_from(index % width).ok()?;
            let y = u16::try_from(index / width).ok()?;
            Some((Along::Across, x, y))
        } else if index < self.edge_count() {
            let across = usize::from(self.dots_across());
            let rest = index - self.across_count();
            let x = u16::try_from(rest % across).ok()?;
            let y = u16::try_from(rest / across).ok()?;
            Some((Along::Down, x, y))
        } else {
            None
        }
    }

    /// The dot at `(x, y)`.
    #[must_use]
    pub fn dot(self, x: u16, y: u16) -> Option<DotId> {
        if x >= self.dots_across() || y >= self.dots_down() {
            return None;
        }
        Some(DotId(
            u32::from(y) * u32::from(self.dots_across()) + u32::from(x),
        ))
    }

    #[must_use]
    pub fn dot_at(self, dot: DotId) -> Option<(u16, u16)> {
        let across = u32::from(self.dots_across());
        let x = u16::try_from(dot.0 % across).ok()?;
        let y = u16::try_from(dot.0 / across).ok()?;
        if y >= self.dots_down() {
            return None;
        }
        Some((x, y))
    }

    #[must_use]
    pub fn cell(self, x: u16, y: u16) -> Option<CellId> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(CellId(u32::from(y) * u32::from(self.width) + u32::from(x)))
    }

    #[must_use]
    pub fn cell_at(self, cell: CellId) -> Option<(u16, u16)> {
        let width = u32::from(self.width);
        let x = u16::try_from(cell.0 % width).ok()?;
        let y = u16::try_from(cell.0 / width).ok()?;
        if y >= self.height {
            return None;
        }
        Some((x, y))
    }

    /// The four edges round a cell: top, bottom, left, right.
    ///
    /// Always four, because every cell has four sides whether or not it is at the
    /// board's rim. That is what makes a clue mean the same thing everywhere: a `3`
    /// in a corner cell is still three of four sides.
    #[must_use]
    pub fn edges_of_cell(self, cell: CellId) -> Option<[EdgeId; 4]> {
        let (x, y) = self.cell_at(cell)?;
        Some([
            self.across(x, y)?,
            self.across(x, y + 1)?,
            self.down(x, y)?,
            self.down(x + 1, y)?,
        ])
    }

    /// The edges meeting at a dot: two, three or four, depending where it sits.
    ///
    /// The count varies here and does not vary for a cell, which is the one place
    /// this geometry is not uniform. The loop rule is "nought or two", and a corner
    /// dot with only two edges is the case that makes that rule interesting.
    #[must_use]
    pub fn edges_of_dot(self, dot: DotId) -> Vec<EdgeId> {
        let Some((x, y)) = self.dot_at(dot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(4);
        if x > 0 {
            out.extend(self.across(x - 1, y));
        }
        out.extend(self.across(x, y));
        if y > 0 {
            out.extend(self.down(x, y - 1));
        }
        out.extend(self.down(x, y));
        out
    }

    /// The cells an edge borders: two in the middle of the board, one at the rim.
    ///
    /// The other direction of [`Geometry::edges_of_cell`], and the lookup a
    /// worklist-driven solver needs: when an edge is decided, these are the clues
    /// whose arithmetic may have changed.
    #[must_use]
    pub fn cells_of(self, edge: EdgeId) -> Vec<CellId> {
        let Some((along, x, y)) = self.edge_at(edge) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(2);
        match along {
            Along::Across => {
                if y > 0 {
                    out.extend(self.cell(x, y - 1));
                }
                out.extend(self.cell(x, y));
            }
            Along::Down => {
                if x > 0 {
                    out.extend(self.cell(x - 1, y));
                }
                out.extend(self.cell(x, y));
            }
        }
        out
    }

    /// The two dots an edge joins, in order.
    #[must_use]
    pub fn ends_of(self, edge: EdgeId) -> Option<(DotId, DotId)> {
        let (along, x, y) = self.edge_at(edge)?;
        match along {
            Along::Across => Some((self.dot(x, y)?, self.dot(x + 1, y)?)),
            Along::Down => Some((self.dot(x, y)?, self.dot(x, y + 1)?)),
        }
    }

    /// Every edge, in the numbering order. For walking a board without repeating the
    /// arithmetic.
    pub fn edges(self) -> impl Iterator<Item = EdgeId> {
        (0..u32::try_from(self.edge_count()).unwrap_or(u32::MAX)).map(EdgeId)
    }

    pub fn dots(self) -> impl Iterator<Item = DotId> {
        (0..u32::try_from(self.dot_count()).unwrap_or(u32::MAX)).map(DotId)
    }

    pub fn cells(self) -> impl Iterator<Item = CellId> {
        (0..u32::try_from(self.cell_count()).unwrap_or(u32::MAX)).map(CellId)
    }
}
