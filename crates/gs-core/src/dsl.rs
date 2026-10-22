//! The puzzle DSL: the formal representation of a family's rules.
//!
//! One decision shapes this module. The options review named it as the single choice
//! here that would be expensive to reverse: **do not let the DSL assume every
//! constraint is local.** Nonogram is Tier A — every constraint is a run-length
//! sequence over one line — but Slitherlink, family two, needs "the edges form
//! exactly one closed loop", which has no small local encoding.
//!
//! So [`Global`] exists now, with no encoder behind it. What makes that reservation
//! real rather than decorative is [`Constraint::tier`]: it matches exhaustively with
//! no wildcard arm, so adding a constraint variant forces whoever adds it to say
//! which tier it belongs to, and [`Tier::B`] constraints are refusable by name. A
//! compiler that cannot encode a constraint must refuse the puzzle, never drop the
//! constraint and verify what is left — that is how PuzzleGen published a board whose
//! answer had been fabricated.

/// How hard a constraint is to encode, in the terms of the family options review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Local: over a row, a column, a region or a cell's neighbours.
    A,
    /// Global: over the whole board at once. Needs reachability, flow, or a lazy
    /// solve-and-block loop.
    B,
}

/// A cell's permitted values. Integer bounds only; see [`crate::state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    pub min: i16,
    pub max: i16,
}

/// Which cells a constraint is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    Row(u16),
    Column(u16),
    /// An explicit, ordered list of cell indices. Ordered because run-length
    /// constraints are about sequence, not just membership.
    Cells(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    AtMost,
    AtLeast,
}

/// A global constraint. **Reserved for family two; nothing encodes these yet.**
///
/// They are named rather than left as a placeholder so that the shape of the
/// reservation is visible: each of these needs a reachability, flow or
/// solve-and-block encoding, and none of them can be approximated by a local rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Global {
    /// The cells holding `value` form one orthogonally connected region.
    /// Hitori's unshaded cells, Nurikabe's sea.
    Connected { value: i16 },
    /// The selected edges form exactly one closed loop. Slitherlink, Masyu, Yajilin.
    SingleClosedLoop { value: i16 },
    /// No 2x2 block is entirely `value`. Nurikabe.
    NoSolidSquare { value: i16 },
}

/// A constraint in the DSL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// The filled runs along an ordered region, in order. Nonogram's line clue:
    /// `runs = [3, 1]` means a run of three, then at least one gap, then a run of one.
    RunLengths {
        region: Region,
        value: i16,
        runs: Vec<u16>,
    },
    /// How many cells in the region hold `value`.
    Count {
        region: Region,
        value: i16,
        cmp: Cmp,
        count: u32,
    },
    /// Every cell in the region holds a different value.
    Distinct {
        region: Region,
    },
    /// The values in the region sum to a bound.
    Sum {
        region: Region,
        cmp: Cmp,
        total: i64,
    },
    /// A global constraint. See [`Global`].
    Global(Global),
    All(Vec<Constraint>),
}

impl Constraint {
    /// Which tier this constraint belongs to.
    ///
    /// Matched exhaustively on purpose: there is no `_` arm, so a new variant does
    /// not compile until it is classified. Enforced by the exhaustiveness check in
    /// `tests/dsl_tiers.rs`, which also asserts that a nested global is not lost.
    #[must_use]
    pub fn tier(&self) -> Tier {
        match self {
            Constraint::RunLengths { .. }
            | Constraint::Count { .. }
            | Constraint::Distinct { .. }
            | Constraint::Sum { .. } => Tier::A,
            Constraint::Global(_) => Tier::B,
            // A conjunction is as global as its most global member. Taking the
            // maximum rather than looking only at the top level is what stops a
            // global constraint hiding inside an `All` and being treated as local.
            Constraint::All(inner) => inner.iter().map(Constraint::tier).max().unwrap_or(Tier::A),
        }
    }

    /// Every global constraint anywhere in this tree, so a compiler that cannot
    /// encode them can name what it is refusing.
    #[must_use]
    pub fn globals(&self) -> Vec<&Global> {
        match self {
            Constraint::Global(g) => vec![g],
            Constraint::All(inner) => inner.iter().flat_map(Constraint::globals).collect(),
            _ => Vec::new(),
        }
    }
}

/// A family's rules: what the board looks like and what must hold of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    pub domain: Domain,
    pub constraints: Vec<Constraint>,
}

impl Rules {
    /// The highest tier any constraint needs.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.constraints
            .iter()
            .map(Constraint::tier)
            .max()
            .unwrap_or(Tier::A)
    }
}
