//! Logical game state, and the rule that it never contains a float.
//!
//! The architecture plan forbids making correctness depend on hardware
//! floating-point behaviour, and the foundation review noted that nothing made that
//! enforceable. Two mechanisms do here:
//!
//! * `float_arithmetic = "deny"` at the workspace level makes floating-point
//!   *arithmetic* anywhere in the core a build error. Verified by planting an `f64`
//!   function and observing the failure.
//! * [`IntegerOnly`] is a sealed trait with no float implementation, and
//!   [`LogicalValue`] requires it. A cell type backed by `f64` therefore does not
//!   compile.
//!
//! **Unenforced:** there is no runtime test that `f64: !IntegerOnly`, because Rust
//! cannot assert a negative implementation without an extra dependency. The sealing
//! is what carries that half, and the lint covers arithmetic regardless.

mod sealed {
    pub trait Sealed {}
    impl Sealed for i8 {}
    impl Sealed for i16 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
}

/// Marks a type as an exact integer. Sealed: only this module may implement it, and
/// it is implemented for no floating-point type.
pub trait IntegerOnly: sealed::Sealed {}
impl IntegerOnly for i8 {}
impl IntegerOnly for i16 {}
impl IntegerOnly for i32 {}
impl IntegerOnly for i64 {}

/// What a cell may hold. Exact, ordered, and cheap to copy.
pub trait LogicalValue: IntegerOnly + Copy + Eq + Ord {
    fn to_i32(self) -> i32;
}

impl LogicalValue for i16 {
    fn to_i32(self) -> i32 {
        i32::from(self)
    }
}

/// Binds `Cell`'s backing type to [`IntegerOnly`] at compile time.
///
/// This is the mechanism, not a decoration: changing `Cell` to wrap `f64` makes this
/// line fail to compile, because [`IntegerOnly`] is sealed and has no float
/// implementation.
const fn assert_integer_only<T: IntegerOnly>() {}
const _: () = assert_integer_only::<CellRepr>();

/// The integer type a cell's value is stored as.
pub type CellRepr = i16;

/// One cell's value.
///
/// `i16` is wide enough for every grid family in the plan (three states for
/// Nonogram, nine digits for Sudoku, edge states for Slitherlink) and narrow enough
/// that a board's canonical encoding stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell(pub CellRepr);

impl Cell {
    /// Not yet decided by the player or the solver. Distinct from every value a
    /// family assigns, so "empty" and "unknown" can never be confused — a
    /// distinction Nonogram needs, since marking a cell known-empty is a real move.
    pub const UNKNOWN: Cell = Cell(CellRepr::MIN);

    #[must_use]
    pub fn is_unknown(self) -> bool {
        self == Cell::UNKNOWN
    }
}

/// Board dimensions. `u16` each, so an index fits in `u32` with room to spare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    pub width: u16,
    pub height: u16,
}

impl Dims {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Dims { width, height }
    }

    #[must_use]
    pub fn cell_count(self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    #[must_use]
    pub fn index_of(self, x: u16, y: u16) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(u32::from(y) * u32::from(self.width) + u32::from(x))
    }
}

/// The logical state of a board.
///
/// Logical only: no animation, no selection, no camera, nothing ephemeral. The
/// architecture plan requires logical and visual state to be separable, and the
/// cheapest way to keep that true is for this type to have nowhere to put the
/// visual half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    dims: Dims,
    cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    OutOfBounds { index: u32, cell_count: usize },
}

impl State {
    /// A board with every cell unknown.
    #[must_use]
    pub fn blank(dims: Dims) -> Self {
        State {
            dims,
            cells: vec![Cell::UNKNOWN; dims.cell_count()],
        }
    }

    #[must_use]
    pub fn dims(&self) -> Dims {
        self.dims
    }

    pub fn get(&self, index: u32) -> Result<Cell, StateError> {
        self.cells
            .get(index as usize)
            .copied()
            .ok_or(StateError::OutOfBounds {
                index,
                cell_count: self.cells.len(),
            })
    }

    pub fn set(&mut self, index: u32, value: Cell) -> Result<Cell, StateError> {
        let cell_count = self.cells.len();
        let slot = self
            .cells
            .get_mut(index as usize)
            .ok_or(StateError::OutOfBounds { index, cell_count })?;
        let previous = *slot;
        *slot = value;
        Ok(previous)
    }

    /// The canonical byte encoding, and the only one.
    ///
    /// Everything that needs to compare, hash or persist a state goes through this,
    /// so that no two layers can disagree about what a state *is*. Little-endian and
    /// explicit, because the deterministic core must agree across architectures —
    /// checked on x86_64 and aarch64 in CI.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.cells.len() * 2);
        out.extend_from_slice(&self.dims.width.to_le_bytes());
        out.extend_from_slice(&self.dims.height.to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(self.cells.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for cell in &self.cells {
            out.extend_from_slice(&cell.0.to_le_bytes());
        }
        out
    }
}

/// One cell's change, carrying its own inverse.
///
/// The architecture plan's memory design wants "compact inverse deltas". Storing
/// `from` alongside `to` is what makes undo an application of the same machinery
/// rather than a second code path that can disagree with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellChange {
    pub index: u32,
    pub from: Cell,
    pub to: Cell,
}

impl CellChange {
    #[must_use]
    pub fn inverse(self) -> Self {
        CellChange {
            index: self.index,
            from: self.to,
            to: self.from,
        }
    }
}

/// The complete effect of one action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    pub changes: Vec<CellChange>,
}

impl Delta {
    #[must_use]
    pub fn inverse(&self) -> Delta {
        Delta {
            // Reversed, so that two changes to the same cell undo in the right order.
            changes: self.changes.iter().rev().map(|c| c.inverse()).collect(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Apply to `state`, checking that each change's `from` is what is actually there.
    ///
    /// The check is the point: a delta that no longer matches the state it is being
    /// applied to means the history and the state have diverged, and the only safe
    /// response is to refuse rather than to write a value nobody can explain.
    pub fn apply(&self, state: &mut State) -> Result<(), DeltaError> {
        for change in &self.changes {
            let actual = state.get(change.index).map_err(DeltaError::State)?;
            if actual != change.from {
                return Err(DeltaError::Diverged {
                    index: change.index,
                    expected: change.from,
                    actual,
                });
            }
            state
                .set(change.index, change.to)
                .map_err(DeltaError::State)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaError {
    State(StateError),
    /// History and state disagree. Never silently repaired.
    Diverged {
        index: u32,
        expected: Cell,
        actual: Cell,
    },
}
