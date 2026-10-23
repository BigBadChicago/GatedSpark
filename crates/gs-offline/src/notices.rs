//! The "do not play these" list, as a device keeps it.
//!
//! [`gs_core::withdrawal`] defines the file. This module is the part that lives on the
//! device: it remembers the newest list it has been shown, refuses to go backwards, and
//! deletes what the list names.
//!
//! # The three rules
//!
//! * **Newer only.** A list carries a serial number that only goes up. The device
//!   remembers the highest it has accepted and refuses any list numbered at or below
//!   it. Without that rule, anyone able to answer the device could keep handing it
//!   yesterday's genuine, correctly signed list — the one from before the bad puzzle
//!   was named — and the withdrawal would never arrive. No key needed, nothing forged.
//! * **Delete, do not hide.** Accepting a list throws the named puzzles off the shelf
//!   then and there. Keeping them and filtering them on the way out would mean a player
//!   who wiped the stored list got the bad puzzle back. Once the bytes are gone, no
//!   amount of tampering with this device's notes brings the puzzle back.
//! * **Checked on the way out, like everything else.** The stored list is read back
//!   through its signature every time. Storage on a device is a few clicks from being
//!   edited by hand, and a forged list would let anybody withdraw anything.
//!
//! # What this cannot do, said plainly
//!
//! A device that never connects never hears about a withdrawal. Nothing in this design
//! fixes that, and no design can: the decision is made somewhere else, and reaching the
//! device takes a connection. What the rules above buy is that the withdrawal arrives
//! the *first* time the device connects, and cannot be talked out of arriving.
//!
//! Wiping this device's own storage by hand also resets its memory of the highest
//! serial. That is why the second rule matters: the puzzles are already deleted, so the
//! worst a wipe does is make the device accept an old list again, and an old list names
//! fewer puzzles than it already acted on.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{OpenError, SignatureVerifier};
use gs_core::withdrawal::{open_list, Reason, WithdrawalList};

use crate::buffer::name_for_bytes;
use crate::shelf::{Shelf, ShelfError};

/// Where the newest accepted list is kept on the shelf.
///
/// Outside [`crate::buffer::PREFIX`], so the buffer's own listing never mistakes it for
/// a puzzle.
pub const WHERE: &str = "withdrawals/current";

/// Why a list was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotNoticed {
    /// It did not pass its checks: not a list, wrong signature, changed bytes.
    NotAcceptable(OpenError),
    /// It is not newer than the one this device already has.
    ///
    /// Carries both numbers, because this is the case a reader of a log most needs to
    /// tell apart from a broken download.
    NotNewer { held: u64, offered: u64 },
    /// The storage itself would not answer.
    ShelfBroke(ShelfError),
}

impl std::fmt::Display for NotNoticed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotNoticed::NotAcceptable(_) => write!(
                f,
                "This list of withdrawn puzzles was ignored, because it did not pass \
                 the checks a list has to pass."
            ),
            NotNoticed::NotNewer { .. } => write!(
                f,
                "This device already has a newer list of withdrawn puzzles, so the one \
                 offered was ignored."
            ),
            NotNoticed::ShelfBroke(why) => write!(
                f,
                "This device would not let the app save the list of withdrawn puzzles: \
                 {why}"
            ),
        }
    }
}

impl std::error::Error for NotNoticed {}

/// What accepting a list did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Noticed {
    /// The serial number this device now refuses to go below.
    pub serial: u64,
    /// How many puzzles the list names, in total.
    pub named: usize,
    /// How many of those this device was actually holding, and has now deleted.
    pub dropped: usize,
}

/// The withdrawals this device is going by.
///
/// The only way to get one is [`Notices::standing`], so holding one means the stored
/// list was read back through its signature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Standing {
    /// The highest serial number accepted, or zero if none ever was.
    ///
    /// Zero works as "none" because [`Notices::accept`] requires a strict increase, so
    /// a published list numbered zero could never be accepted anyway.
    pub serial: u64,
    withdrawn: Vec<([u8; 32], Reason)>,
    /// True when there was something stored and it did not pass its checks.
    ///
    /// Reported rather than hidden. It means somebody has been editing this device's
    /// storage, which is worth a person knowing even though the puzzles the list named
    /// are already gone.
    pub stored_list_was_unreadable: bool,
}

impl Standing {
    /// Nothing withdrawn, as far as this device knows.
    #[must_use]
    pub fn nothing() -> Self {
        Standing::default()
    }

    /// Why this puzzle must not be played, if it must not be.
    #[must_use]
    pub fn against(&self, id: &ContentId) -> Option<Reason> {
        self.withdrawn
            .iter()
            .find(|(bytes, _)| bytes == id.as_bytes())
            .map(|(_, reason)| *reason)
    }

    /// How many puzzles are named.
    #[must_use]
    pub fn len(&self) -> usize {
        self.withdrawn.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.withdrawn.is_empty()
    }

    /// Every withdrawn id as lower-case hex, with its reason.
    ///
    /// For callers that hold a fingerprint as text rather than as a [`ContentId`] — a
    /// web page does, because a `ContentId` in this codebase can only be made by
    /// digesting something and a string from a page is not that.
    #[must_use]
    pub fn named_in_hex(&self) -> Vec<(String, Reason)> {
        self.withdrawn
            .iter()
            .map(|(bytes, reason)| {
                let name = crate::buffer::name_for_bytes(bytes);
                (name[crate::buffer::PREFIX.len()..].to_owned(), *reason)
            })
            .collect()
    }

    fn of(list: &WithdrawalList) -> Self {
        Standing {
            serial: list.serial,
            withdrawn: list
                .withdrawn
                .iter()
                .map(|entry| (entry.content_id, entry.reason))
                .collect(),
            stored_list_was_unreadable: false,
        }
    }
}

/// The device's memory of what has been withdrawn.
///
/// Holds nothing itself; the list lives on the shelf, so a device that is closed and
/// reopened remembers exactly what it remembered before.
#[derive(Debug, Clone, Copy, Default)]
pub struct Notices;

impl Notices {
    #[must_use]
    pub fn new() -> Self {
        Notices
    }

    /// Take a newly downloaded list.
    ///
    /// Checks it, refuses it if it is not newer, stores it, and deletes every puzzle it
    /// names. The deletion is part of accepting rather than a separate call a caller
    /// could forget.
    pub fn accept<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &mut S,
        bytes: &[u8],
        digest: &D,
        keys: &V,
    ) -> Result<Noticed, NotNoticed> {
        let opened = open_list(bytes, digest, keys).map_err(NotNoticed::NotAcceptable)?;
        let held = self
            .standing(shelf, digest, keys)
            .map_err(NotNoticed::ShelfBroke)?;
        if opened.list.serial <= held.serial {
            return Err(NotNoticed::NotNewer {
                held: held.serial,
                offered: opened.list.serial,
            });
        }

        // Stored before anything is deleted. If the shelf fails partway, the worse
        // outcome is a device that knows about a withdrawal and still holds the puzzle,
        // not one that deleted puzzles and forgot why.
        shelf.put(WHERE, bytes).map_err(NotNoticed::ShelfBroke)?;

        let mut dropped = 0;
        for entry in &opened.list.withdrawn {
            let name = name_for_bytes(&entry.content_id);
            let there = shelf.get(&name).map_err(NotNoticed::ShelfBroke)?.is_some();
            if there {
                shelf.remove(&name).map_err(NotNoticed::ShelfBroke)?;
                dropped += 1;
            }
        }

        Ok(Noticed {
            serial: opened.list.serial,
            named: opened.list.withdrawn.len(),
            dropped,
        })
    }

    /// What this device is going by, read back through its signature.
    pub fn standing<S: Shelf, D: Digest, V: SignatureVerifier>(
        &self,
        shelf: &S,
        digest: &D,
        keys: &V,
    ) -> Result<Standing, ShelfError> {
        let Some(bytes) = shelf.get(WHERE)? else {
            return Ok(Standing::nothing());
        };
        match open_list(&bytes, digest, keys) {
            Ok(opened) => Ok(Standing::of(&opened.list)),
            Err(_) => Ok(Standing {
                stored_list_was_unreadable: true,
                ..Standing::nothing()
            }),
        }
    }

    /// Forget the stored list.
    ///
    /// Here for tests and for a deliberate reset, never as part of normal running.
    /// Forgetting is a step backwards in the one direction this module refuses to go,
    /// so it is its own named call.
    pub fn forget<S: Shelf>(&self, shelf: &mut S) -> Result<(), ShelfError> {
        shelf.remove(WHERE)
    }
}
