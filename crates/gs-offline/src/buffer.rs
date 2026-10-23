//! The five puzzles a device keeps so that losing signal does not end the game.
//!
//! The rules it implements, all agreed before it was written:
//!
//! * **Five ahead.** A target number of unplayed puzzles, held on the device.
//! * **Delivered is not played.** Bytes arriving counts as delivered. The bookmark
//!   moves only when the player actually opens one. Counting a download as a play would
//!   quietly burn puzzles nobody saw.
//! * **Running out is an answer.** When the buffer empties with no signal, the caller
//!   gets a sentence saying so, not an empty screen.
//!
//! # Everything is checked on the way out
//!
//! A shelf is not trusted. On a device it is browser storage, a few clicks from being
//! edited by hand. So every puzzle is checked against the trusted key *as it leaves*,
//! exactly as one arriving over the network would be — and checked on the way in too,
//! so a bad download fails where somebody can see it rather than three days later with
//! no signal.
//!
//! The on-the-way-out check is the one that matters. Dropping it would make the buffer
//! the one place in this system where bytes are believed because of where they were
//! found.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{open, OpenError, SignatureVerifier};

use crate::shelf::{Shelf, ShelfError};

/// The agreed number of unplayed puzzles to hold.
///
/// Five, from the delivery design. A package is a few kilobytes, so five is tens of
/// kilobytes and no separate size limit is worth enforcing — the count is already far
/// below any storage budget worth naming.
pub const HOW_MANY_AHEAD: usize = 5;

/// The prefix every buffered puzzle is stored under.
///
/// A prefix rather than the bare content id, so a shelf shared with anything else — a
/// browser's storage is shared with everything the page keeps — cannot have its other
/// entries mistaken for puzzles.
pub const PREFIX: &str = "ahead/";

/// Why a puzzle could not be handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NothingToPlay {
    /// The buffer is empty and there is no network to fill it from.
    Empty,
    /// The storage itself would not answer.
    ShelfBroke(ShelfError),
}

impl std::fmt::Display for NothingToPlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NothingToPlay::Empty => write!(
                f,
                "There are no more puzzles saved on this device. Connect to the \
                 internet to get some more."
            ),
            NothingToPlay::ShelfBroke(why) => write!(
                f,
                "This device would not let the app read its saved puzzles: {why}"
            ),
        }
    }
}

impl std::error::Error for NothingToPlay {}

/// Why a puzzle was not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotKept {
    /// It did not pass the same check a puzzle off the network gets.
    NotAcceptable(OpenError),
    /// The storage itself would not answer.
    ShelfBroke(ShelfError),
}

impl std::fmt::Display for NotKept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotKept::NotAcceptable(_) => write!(
                f,
                "This puzzle was not saved, because it did not pass the checks a puzzle \
                 has to pass. Nothing has been kept."
            ),
            NotKept::ShelfBroke(why) => {
                write!(f, "This device would not let the app save a puzzle: {why}")
            }
        }
    }
}

impl std::error::Error for NotKept {}

/// One puzzle, checked, on its way to a player.
///
/// The fields are what came out of the package's own signed manifest, not out of the
/// shelf. The only way to get one is [`Buffer::next`], which is what makes holding one
/// mean the signature was checked.
#[derive(Debug, Clone)]
pub struct Delivered {
    pub content_id: ContentId,
    pub signed_by: String,
    pub band: Option<String>,
    /// The package's own bytes, to be opened by whatever knows the family.
    pub package: Vec<u8>,
}

/// What the buffer looks like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stock {
    /// Unplayed puzzles on the device that pass their checks.
    pub sound: usize,
    /// Entries on the shelf that do not. Counted rather than hidden: a device quietly
    /// discarding things is a device nobody can debug.
    pub spoiled: usize,
}

impl Stock {
    /// How many more to ask for, to get back to the target.
    #[must_use]
    pub fn wanted(&self) -> usize {
        HOW_MANY_AHEAD.saturating_sub(self.sound)
    }
}

/// The puzzles held on this device.
///
/// Holds no bytes itself. Everything lives on the shelf, so a buffer rebuilt after the
/// app is closed and reopened is the same buffer — there is no state here to lose.
#[derive(Debug, Clone, Default)]
pub struct Buffer;

impl Buffer {
    #[must_use]
    pub fn new() -> Self {
        Buffer
    }

    /// Keep a puzzle that has just been downloaded.
    ///
    /// Checked before it is stored, under the same key and the same rules a puzzle off
    /// the network gets. The content id is learnt from the package rather than taken
    /// from the caller: a name supplied alongside the bytes is a claim, and the whole
    /// point of a content id is that it is not one.
    ///
    /// Storing one already held replaces it, which is a no-op in content: the name is
    /// the digest, so the bytes under it cannot differ.
    pub fn keep<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &mut S,
        package: &[u8],
        digest: &D,
        keys: &V,
    ) -> Result<ContentId, NotKept> {
        let opened = open(package, digest, keys).map_err(NotKept::NotAcceptable)?;
        let id = opened.content_id;
        shelf
            .put(&name_for(&id), package)
            .map_err(NotKept::ShelfBroke)?;
        Ok(id)
    }

    /// The next puzzle to play, checked on the way out.
    ///
    /// Does **not** mark anything as played. A player being handed a puzzle is not a
    /// player having played it; [`Buffer::played`] is the separate step, and the gap
    /// between them is the "delivered is not played" rule.
    ///
    /// Anything on the shelf that no longer passes is skipped, not returned. It is left
    /// where it is rather than deleted, so [`Buffer::stock`] can report it and so a
    /// device is never silently emptied by a storage fault that might be temporary.
    pub fn next<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &S,
        digest: &D,
        keys: &V,
    ) -> Result<Delivered, NothingToPlay> {
        for name in self.held(shelf).map_err(NothingToPlay::ShelfBroke)? {
            let Some(package) = shelf.get(&name).map_err(NothingToPlay::ShelfBroke)? else {
                continue;
            };
            let Ok(opened) = open(&package, digest, keys) else {
                continue;
            };
            return Ok(Delivered {
                content_id: opened.content_id,
                signed_by: opened.key_id,
                band: opened.manifest.band,
                package,
            });
        }
        Err(NothingToPlay::Empty)
    }

    /// Record that a puzzle has been played, and stop holding it.
    ///
    /// The caller moves the bookmark. This type does not hold one, because a bookmark
    /// belongs to a player and a level while a shelf belongs to a device, and tying the
    /// two together here would be deciding that a device has exactly one player.
    pub fn played<S: Shelf>(&self, shelf: &mut S, id: &ContentId) -> Result<(), ShelfError> {
        shelf.remove(&name_for(id))
    }

    /// What is on the device, and how much of it is still good.
    pub fn stock<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &S,
        digest: &D,
        keys: &V,
    ) -> Result<Stock, ShelfError> {
        let mut sound = 0;
        let mut spoiled = 0;
        for name in self.held(shelf)? {
            match shelf.get(&name)? {
                Some(package) if open(&package, digest, keys).is_ok() => sound += 1,
                _ => spoiled += 1,
            }
        }
        Ok(Stock { sound, spoiled })
    }

    /// Throw away everything on the shelf that no longer passes its checks.
    ///
    /// Separate from reading, and never automatic. A puzzle that fails today because
    /// the device's storage is misbehaving would be gone forever if reading deleted it,
    /// and a player with no signal would have nothing left. Deleting is something a
    /// caller does on purpose, when it can replace what it drops.
    pub fn discard_spoiled<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &mut S,
        digest: &D,
        keys: &V,
    ) -> Result<usize, ShelfError> {
        let mut thrown = Vec::new();
        for name in self.held(shelf)? {
            let keep =
                matches!(shelf.get(&name)?, Some(package) if open(&package, digest, keys).is_ok());
            if !keep {
                thrown.push(name);
            }
        }
        for name in &thrown {
            shelf.remove(name)?;
        }
        Ok(thrown.len())
    }

    /// The shelf names this buffer owns, in the shelf's stable order.
    fn held<S: Shelf>(&self, shelf: &S) -> Result<Vec<String>, ShelfError> {
        Ok(shelf
            .names()?
            .into_iter()
            .filter(|name| name.starts_with(PREFIX))
            .collect())
    }
}

/// Where a puzzle with this id is kept.
#[must_use]
pub fn name_for(id: &ContentId) -> String {
    format!("{PREFIX}{}", id.to_hex())
}
