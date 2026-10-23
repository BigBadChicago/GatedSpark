//! Where the bytes sit. The one thing this crate does not decide.
//!
//! A browser has no folders, and the crate in this workspace that does use folders is
//! the one a browser is forbidden from carrying ([`ADR 0009`]). So the split is: this
//! crate decides **policy** — how many puzzles to keep, which one is next, what counts
//! as played — and whatever it is running inside decides **place**.
//!
//! The interface is four operations because four is what the policy needs. Resisting a
//! fifth is the point: a shelf that could also, say, sort or search would be a place
//! where a second opinion about the buffer's order could grow.
//!
//! **A shelf is not trusted.** On a real device it is browser storage, which anyone
//! with the developer tools open can edit by hand, or a file anything else on the
//! machine can write to. Everything that comes off a shelf is checked again before it
//! is used; see [`crate::buffer`].
//!
//! [`ADR 0009`]: https://github.com/BigBadChicago/GatedSpark/blob/main/docs/adr/0009-the-browser-version.md

use std::collections::BTreeMap;

/// Something went wrong with the storage itself.
///
/// Deliberately not an enum of causes. A shelf can fail for reasons this crate has no
/// vocabulary for — a browser refusing to store anything in a private window, a disk
/// that is full, a key that is somehow too long — and inventing a category for each one
/// would be inventing facts. What the policy above needs to know is only that the
/// operation did not happen, and what to tell a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfError(pub String);

impl ShelfError {
    pub fn new(what_happened: impl Into<String>) -> Self {
        ShelfError(what_happened.into())
    }
}

impl std::fmt::Display for ShelfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ShelfError {}

/// Somewhere to keep bytes under names.
///
/// Implemented by whatever is hosting the buffer. [`Memory`] is the implementation this
/// crate ships, and it exists so that every test below runs without a device.
pub trait Shelf {
    /// Store `bytes` under `name`, replacing anything already there.
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), ShelfError>;

    /// Read back what was stored, or `None` if nothing is under that name.
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ShelfError>;

    /// Remove it. Removing something that is not there is not an error: the caller
    /// wanted it gone, and it is gone.
    fn remove(&mut self, name: &str) -> Result<(), ShelfError>;

    /// Every name on the shelf, in a stable order.
    ///
    /// Stable rather than arbitrary because the buffer's behaviour must not depend on
    /// what order a particular device happens to hand things back in. Two devices with
    /// the same five puzzles must agree about which is next.
    fn names(&self) -> Result<Vec<String>, ShelfError>;
}

/// A shelf in memory, for tests and for a first run.
///
/// `BTreeMap` rather than `HashMap` so [`Shelf::names`] is sorted without sorting it,
/// which makes the stable-order promise above structural rather than remembered.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    items: BTreeMap<String, Vec<u8>>,
}

impl Memory {
    #[must_use]
    pub fn new() -> Self {
        Memory::default()
    }

    /// How many things are on it. For tests; the policy never asks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Shelf for Memory {
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), ShelfError> {
        self.items.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ShelfError> {
        Ok(self.items.get(name).cloned())
    }

    fn remove(&mut self, name: &str) -> Result<(), ShelfError> {
        self.items.remove(name);
        Ok(())
    }

    fn names(&self) -> Result<Vec<String>, ShelfError> {
        Ok(self.items.keys().cloned().collect())
    }
}

/// A shelf that refuses everything, so the failure path has a test.
///
/// Every operation fails with the message it was built with. Without this, the
/// `Result`s above would be returned by code nothing ever exercises, which is the same
/// as not handling the failure at all.
#[derive(Debug, Clone)]
pub struct AlwaysFails(pub String);

impl Shelf for AlwaysFails {
    fn put(&mut self, _name: &str, _bytes: &[u8]) -> Result<(), ShelfError> {
        Err(ShelfError::new(self.0.clone()))
    }

    fn get(&self, _name: &str) -> Result<Option<Vec<u8>>, ShelfError> {
        Err(ShelfError::new(self.0.clone()))
    }

    fn remove(&mut self, _name: &str) -> Result<(), ShelfError> {
        Err(ShelfError::new(self.0.clone()))
    }

    fn names(&self) -> Result<Vec<String>, ShelfError> {
        Err(ShelfError::new(self.0.clone()))
    }
}
