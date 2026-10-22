//! The engine/game boundary.
//!
//! The one thing worth copying from PuzzleGen is that its engine/game boundary held
//! while three unrelated games were added, because CI ran the platform suite with no
//! game installed. That only works if the platform refuses to invent a family: a
//! lookup that falls back to a default would make the empty-registry run pass for
//! the wrong reason.

use core::fmt;

/// A puzzle family's stable identifier, e.g. `nonogram`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FamilyId(pub &'static str);

impl fmt::Display for FamilyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What a puzzle family must supply to the platform.
///
/// Intentionally almost empty in M0. It grows in M1 (transition semantics, state
/// hashing) and M2 (constraint encoding). What matters now is that it exists, so the
/// platform depends on the trait and never on a concrete family.
pub trait Family {
    fn id(&self) -> FamilyId;
}

#[derive(Debug, PartialEq, Eq)]
pub enum LookupError {
    /// No family with this id is installed. Not a fallback, not a default.
    NotInstalled(FamilyId),
}

/// The set of installed families. Empty is a legitimate configuration, and the
/// platform suite runs against it.
#[derive(Default)]
pub struct Registry {
    families: Vec<Box<dyn Family>>,
}

impl Registry {
    #[must_use]
    pub fn empty() -> Self {
        Registry {
            families: Vec::new(),
        }
    }

    pub fn install(&mut self, family: Box<dyn Family>) {
        self.families.push(family);
    }

    #[must_use]
    pub fn installed_count(&self) -> usize {
        self.families.len()
    }

    /// Resolve a family, or say plainly that it is not installed.
    pub fn resolve(&self, id: FamilyId) -> Result<&dyn Family, LookupError> {
        self.families
            .iter()
            .find(|f| f.id() == id)
            .map(AsRef::as_ref)
            .ok_or(LookupError::NotInstalled(id))
    }
}
