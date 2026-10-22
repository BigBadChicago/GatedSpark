//! The only part of this crate that knows a web page exists.
//!
//! Everything above this module is plain Rust, testable on an ordinary machine. This
//! file is the translation layer and nothing else: it holds no rules, decides no
//! question, and checks nothing that [`crate::load`] does not already check. That
//! division is what keeps a desktop version from being a rewrite — and it is also the
//! only way most of this crate can be tested at all, since a test that needs a browser
//! is a test that will eventually be skipped.
//!
//! # The one rule for anything added here
//!
//! If it decides something, it belongs upstairs. The specific mistake to avoid is a page
//! that works out for itself whether a board is finished, "just for the display". There
//! would then be two answers to that question, they would disagree one day, and the
//! wrong one would be the one the player saw.

use wasm_bindgen::prelude::*;

use gs_crypto::{Sha256, TrustedKeys};

use crate::{load, Playable};

/// The page, holding the keys it will accept puzzles from.
///
/// Starts trusting nothing. A page that shipped with an empty key list and never called
/// [`Page::trust`] accepts no puzzles at all, which is the right way round: the failure
/// is visible immediately rather than being a page that accepts anything.
#[wasm_bindgen]
pub struct Page {
    keys: TrustedKeys,
}

#[wasm_bindgen]
impl Page {
    /// A page that trusts nothing yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Page {
        Page {
            keys: TrustedKeys::none(),
        }
    }

    /// Add a key this page will accept puzzles from.
    ///
    /// The key comes from the page's own code, never from alongside a puzzle. A reader
    /// that trusted a key list it found next to the content would trust whatever anybody
    /// put there, so the list would be decoration.
    pub fn trust(&mut self, key_id: &str, public_key: &[u8]) -> Result<(), JsError> {
        // A page hands over whatever bytes it has. A wrong length is caught here with
        // the length named, rather than reaching the key library as something it has to
        // guess about.
        let key: &[u8; 32] = public_key.try_into().map_err(|_| {
            JsError::new(&format!(
                "a public key is 32 bytes, and this one is {}",
                public_key.len()
            ))
        })?;
        self.keys
            .trust(key_id, key)
            .map_err(|why| JsError::new(&format!("this key cannot be trusted: {why:?}")))
    }

    /// Open the bytes of a package, or throw with the reason.
    ///
    /// The reason is the one [`crate::NotPlayable`] gives, in the words it gives, so a
    /// person looking at a browser console sees the same explanation a person looking at
    /// `gatedspark show` would.
    pub fn open(&self, bytes: &[u8]) -> Result<Puzzle, JsError> {
        load(bytes, &Sha256, &self.keys)
            .map(|playable| Puzzle { playable })
            .map_err(|why| JsError::new(&why.to_string()))
    }
}

impl Default for Page {
    fn default() -> Self {
        Page::new()
    }
}

/// A puzzle the page has accepted, in the shape a web page can read.
///
/// Every method here reads a field that [`load`] already filled in. None of them
/// computes anything.
#[wasm_bindgen]
pub struct Puzzle {
    playable: Playable,
}

#[wasm_bindgen]
impl Puzzle {
    #[must_use]
    pub fn width(&self) -> u16 {
        self.playable.dims().width
    }

    #[must_use]
    pub fn height(&self) -> u16 {
        self.playable.dims().height
    }

    /// The puzzle's fingerprint, as the 64 characters that also name its folder.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.playable.content_id.to_hex()
    }

    /// Which key vouched for this puzzle. Worth showing: it is the answer to "why
    /// should I believe this came from you".
    #[must_use]
    pub fn signed_by(&self) -> String {
        self.playable.signed_by.clone()
    }

    /// The level, or an empty string if the game assigned none. `None` is a real
    /// answer, and an empty string is how a web page receives it.
    #[must_use]
    pub fn band(&self) -> String {
        self.playable.band.clone().unwrap_or_default()
    }

    /// What the uniqueness claim is relative to. A page that said "only one answer"
    /// without this would be claiming more than was proved.
    #[must_use]
    pub fn encoding(&self) -> String {
        self.playable.encoding.clone()
    }

    /// One row's clue numbers. Out of range gives an empty list rather than throwing,
    /// because a page drawing a board asks in a loop and a throw there is a blank page.
    #[must_use]
    pub fn row_clue(&self, index: usize) -> Vec<u16> {
        self.playable
            .row_clues()
            .get(index)
            .cloned()
            .unwrap_or_default()
    }

    /// One column's clue numbers. Out of range gives an empty list, as above.
    #[must_use]
    pub fn column_clue(&self, index: usize) -> Vec<u16> {
        self.playable
            .column_clues()
            .get(index)
            .cloned()
            .unwrap_or_default()
    }
}
