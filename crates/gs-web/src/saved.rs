//! The buffer, as a web page sees it.
//!
//! Two things live here and nothing else: a [`Shelf`] over the browser's own storage,
//! and the handful of calls a page makes. The policy — how many to hold, which is next,
//! what counts as played — is all in `gs-offline`, which has no idea a browser exists.
//!
//! # Why `localStorage` and not something better
//!
//! It is the only storage a browser offers that can be read and written without waiting,
//! which matters because every other call in this file is immediate and a page that had
//! to wait for one of them would need a different shape throughout. It is small — a few
//! megabytes — and five packages are tens of kilobytes, so the limit is nowhere near.
//!
//! It is also **not private and not reliable**, which is why nothing here depends on it
//! being either. A private window may refuse it, clearing site data empties it, and
//! anyone with the developer tools open can edit it. Every puzzle that comes off it is
//! checked against the trusted key before a player sees it, exactly as one off the
//! network would be. The failures are reported rather than swallowed: a browser that
//! will not store anything is a different fact from a device with no puzzles left, and
//! a player can act on them differently.

use gs_offline::shelf::{Shelf, ShelfError};
use gs_offline::{Buffer, Stock};
use wasm_bindgen::prelude::*;

use crate::browser::Page;

/// Everything this page keeps is named with this in front of it.
///
/// `localStorage` for a site is one shared space. A prefix keeps the buffer's names out
/// of the way of anything else the page stores, and keeps anything else out of the way
/// of the buffer.
const OURS: &str = "gatedspark/";

/// The browser's own storage, behind the four operations the buffer needs.
struct LocalStorage {
    inner: web_sys::Storage,
}

impl LocalStorage {
    /// Fails when the browser will not give it out at all, which a private window does.
    fn open() -> Result<Self, ShelfError> {
        let window = web_sys::window()
            .ok_or_else(|| ShelfError::new("there is no browser window to store anything in"))?;
        let inner = window
            .local_storage()
            .map_err(|_| ShelfError::new("this browser will not allow saved data"))?
            .ok_or_else(|| ShelfError::new("this browser has saved data turned off"))?;
        Ok(LocalStorage { inner })
    }

    fn full_name(name: &str) -> String {
        format!("{OURS}{name}")
    }
}

impl Shelf for LocalStorage {
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), ShelfError> {
        // Storage holds text, so the bytes go in as hex. Not base64: hex needs no
        // dependency and no padding rules, and at tens of kilobytes the extra size is
        // not worth a second encoding to be wrong about.
        self.inner
            .set_item(&Self::full_name(name), &to_hex(bytes))
            .map_err(|_| {
                ShelfError::new(
                    "this device would not save the puzzle, which usually means its \
                     storage is full",
                )
            })
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ShelfError> {
        let text = self
            .inner
            .get_item(&Self::full_name(name))
            .map_err(|_| ShelfError::new("this device would not read its saved puzzles"))?;
        // Unreadable text is not an error here: it is something that is not one of ours,
        // and the buffer's own check is what decides whether bytes are acceptable. An
        // error would turn one corrupted entry into a device that cannot be read at all.
        Ok(text.as_deref().and_then(from_hex))
    }

    fn remove(&mut self, name: &str) -> Result<(), ShelfError> {
        self.inner
            .remove_item(&Self::full_name(name))
            .map_err(|_| ShelfError::new("this device would not remove a saved puzzle"))
    }

    fn names(&self) -> Result<Vec<String>, ShelfError> {
        let count = self
            .inner
            .length()
            .map_err(|_| ShelfError::new("this device would not list its saved puzzles"))?;
        let mut names = Vec::new();
        for i in 0..count {
            let Ok(Some(key)) = self.inner.key(i) else {
                continue;
            };
            if let Some(rest) = key.strip_prefix(OURS) {
                names.push(rest.to_owned());
            }
        }
        // Sorted, because the buffer promises two devices holding the same puzzles agree
        // about which is next, and `localStorage` makes no promise about its own order.
        names.sort();
        Ok(names)
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let digits = std::str::from_utf8(pair).ok()?;
        out.push(u8::from_str_radix(digits, 16).ok()?);
    }
    Some(out)
}

/// What a page is told about the puzzles saved on this device.
#[wasm_bindgen]
pub struct Saved {
    /// Unplayed puzzles that pass their checks.
    ready: usize,
    /// Entries that do not. Reported rather than hidden: a device quietly throwing
    /// things away is a device nobody can debug.
    spoiled: usize,
    wanted: usize,
}

#[wasm_bindgen]
impl Saved {
    #[must_use]
    pub fn ready(&self) -> usize {
        self.ready
    }

    #[must_use]
    pub fn spoiled(&self) -> usize {
        self.spoiled
    }

    /// How many more this device should download when it next has a connection.
    #[must_use]
    pub fn wanted(&self) -> usize {
        self.wanted
    }

    /// The same thing as a sentence, for showing to a person.
    #[must_use]
    pub fn in_words(&self) -> String {
        let saved = match self.ready {
            0 => "No puzzles saved on this device".to_owned(),
            1 => "1 puzzle saved on this device".to_owned(),
            n => format!("{n} puzzles saved on this device"),
        };
        match (self.wanted, self.spoiled) {
            (0, 0) => format!("{saved}."),
            (0, 1) => format!("{saved}, and 1 saved file that could not be used."),
            (0, n) => format!("{saved}, and {n} saved files that could not be used."),
            (want, 0) => format!("{saved}. Room for {want} more."),
            (want, 1) => {
                format!("{saved}. Room for {want} more, and 1 saved file that could not be used.")
            }
            (want, n) => {
                format!(
                    "{saved}. Room for {want} more, and {n} saved files that could not be used."
                )
            }
        }
    }
}

impl From<Stock> for Saved {
    fn from(stock: Stock) -> Saved {
        Saved {
            ready: stock.sound,
            spoiled: stock.spoiled,
            wanted: stock.wanted(),
        }
    }
}

#[wasm_bindgen]
impl Page {
    /// Save a puzzle for later, checking it first.
    ///
    /// Returns its fingerprint. Refuses anything the page itself would refuse, so a bad
    /// download fails here, where somebody can see it, rather than in three days with no
    /// signal.
    pub fn save_for_later(&self, package: &[u8]) -> Result<String, JsError> {
        let mut shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        Buffer::new()
            .keep(&mut shelf, package, self.digest(), self.keys())
            .map(|id| id.to_hex())
            .map_err(|why| JsError::new(&why.to_string()))
    }

    /// Open the next saved puzzle, without marking it played.
    ///
    /// Handing a player a puzzle is not a player playing it. [`Page::mark_played`] is
    /// the separate step, and the gap between them is why five downloaded puzzles are
    /// still five puzzles tomorrow.
    pub fn next_saved(&self) -> Result<crate::browser::Puzzle, JsError> {
        let shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        let delivered = Buffer::new()
            .next(&shelf, self.digest(), self.keys())
            .map_err(|why| JsError::new(&why.to_string()))?;
        // Opened again here, by the same path any other puzzle takes. The buffer checked
        // the signature; this is the page checking the family and the rules version, and
        // there is no way into a `Puzzle` that skips it.
        self.open(&delivered.package)
    }

    /// Record that a saved puzzle has been played, and stop holding it.
    pub fn mark_played(&self, fingerprint: &str) -> Result<(), JsError> {
        let mut shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        // The name is built from the fingerprint the page was given, which came out of a
        // package this code opened. A fingerprint from anywhere else simply names nothing.
        shelf
            .remove(&format!("{}{fingerprint}", gs_offline::buffer::PREFIX))
            .map_err(|why| JsError::new(&why.to_string()))
    }

    /// How many puzzles are saved, how many are unusable, and how many more to fetch.
    pub fn saved(&self) -> Result<Saved, JsError> {
        let shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        Buffer::new()
            .stock(&shelf, self.digest(), self.keys())
            .map(Saved::from)
            .map_err(|why| JsError::new(&why.to_string()))
    }

    /// Throw away saved files that no longer pass their checks.
    ///
    /// Never automatic. A puzzle that fails today because the device's storage is
    /// misbehaving would be gone forever if reading deleted it, and a player with no
    /// signal would have nothing left.
    pub fn discard_unusable(&self) -> Result<usize, JsError> {
        let mut shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        Buffer::new()
            .discard_spoiled(&mut shelf, self.digest(), self.keys())
            .map_err(|why| JsError::new(&why.to_string()))
    }
}
