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

use crate::play::{Mark, Session};
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

/// What one square holds, in the shape a web page can switch on.
///
/// A separate type from [`crate::play::Mark`] on purpose: `Mark` lives in a module that
/// compiles for an ordinary machine too, and `#[wasm_bindgen]` cannot go there. This is
/// the translation, and the two conversions below are the only place the mapping exists.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Square {
    /// Part of the picture.
    Filled = 0,
    /// Definitely not part of the picture.
    Empty = 1,
    /// Not decided yet.
    Unknown = 2,
}

impl From<Square> for Mark {
    fn from(square: Square) -> Mark {
        match square {
            Square::Filled => Mark::Filled,
            Square::Empty => Mark::Empty,
            Square::Unknown => Mark::Unknown,
        }
    }
}

impl From<Mark> for Square {
    fn from(mark: Mark) -> Square {
        match mark {
            Mark::Filled => Square::Filled,
            Mark::Empty => Square::Empty,
            Mark::Unknown => Square::Unknown,
        }
    }
}

#[wasm_bindgen]
impl Puzzle {
    /// Start playing.
    ///
    /// Consumes the puzzle, so a page cannot hold a puzzle and a game of it at once and
    /// wonder which is current. There is one game of one puzzle.
    #[must_use]
    pub fn play(self) -> Game {
        Game {
            session: Session::start(self.playable),
        }
    }
}

/// One person playing one puzzle.
///
/// Every method here passes straight through to [`Session`], which passes straight
/// through to the rules. Nothing in this type decides anything — above all not whether
/// the board is finished, which is the one question a page must never answer for itself.
#[wasm_bindgen]
pub struct Game {
    session: Session,
}

#[wasm_bindgen]
impl Game {
    /// Mark one square, by row and column.
    ///
    /// Returns `true` if the board changed. A tap that changes nothing — a square
    /// already marked that way, or one off the edge of the board — is `false` rather
    /// than an error, because those are ordinary during play and a page should not have
    /// to catch an exception to find out somebody tapped twice.
    pub fn mark(&mut self, row: u16, column: u16, square: Square) -> bool {
        self.session.mark_at(row, column, square.into()).is_ok()
    }

    /// Why the last `mark` did nothing, or an empty string if it did something.
    ///
    /// Separate from `mark` so the common path stays a plain yes or no. A page showing
    /// a message needs this; a page just drawing a board does not.
    pub fn mark_and_say(&mut self, row: u16, column: u16, square: Square) -> String {
        match self.session.mark_at(row, column, square.into()) {
            Ok(()) => String::new(),
            Err(why) => why.to_string(),
        }
    }

    /// Take back the last move. `false` if there was nothing to take back.
    pub fn undo(&mut self) -> bool {
        self.session.undo().is_ok()
    }

    /// Put back a move that was taken back. `false` if there was none.
    pub fn redo(&mut self) -> bool {
        self.session.redo().is_ok()
    }

    /// What one square holds. A square off the board reads as `Unknown`, because a page
    /// draws a board in a loop and a thrown error there is a blank page.
    #[must_use]
    pub fn square(&self, row: u16, column: u16) -> Square {
        self.session.mark_at_of(row, column).into()
    }

    /// The whole board in one call, row by row from the top left.
    ///
    /// Each byte is a [`Square`] value. This is for drawing: asking square by square
    /// across the boundary is one call per square, and a 20 by 20 board redrawn on every
    /// tap is 400 of them. It is derived fresh from the same board each time, so it
    /// cannot drift from what `square` would say.
    #[must_use]
    pub fn board(&self) -> Vec<u8> {
        let dims = self.session.puzzle().clues.dims;
        (0..dims.height)
            .flat_map(|row| {
                (0..dims.width).map(move |column| {
                    // `as` on a fieldless enum is its discriminant, which is the value
                    // the page reads. The numbers live on `Square` and nowhere else.
                    Square::from(self.session.mark_at_of(row, column)) as u8
                })
            })
            .collect()
    }

    /// Whether the puzzle is solved.
    ///
    /// Asked of the rules every time it is called. No cached answer, and nothing in this
    /// file works it out.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        self.session.is_solved()
    }

    #[must_use]
    pub fn moves_made(&self) -> u32 {
        self.session.moves_made()
    }

    /// The fingerprint of the board as it stands: the same kind of number a published
    /// report binds a stored answer to.
    #[must_use]
    pub fn board_fingerprint(&self) -> String {
        self.session.board_fingerprint().to_hex()
    }

    // The puzzle's own details, so a page can show "signed by, level, one answer only"
    // while somebody plays. Reading them from the session rather than keeping a second
    // copy, for the usual reason.

    #[must_use]
    pub fn width(&self) -> u16 {
        self.session.puzzle().dims().width
    }

    #[must_use]
    pub fn height(&self) -> u16 {
        self.session.puzzle().dims().height
    }

    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.session.puzzle().content_id.to_hex()
    }

    #[must_use]
    pub fn signed_by(&self) -> String {
        self.session.puzzle().signed_by.clone()
    }

    #[must_use]
    pub fn band(&self) -> String {
        self.session.puzzle().band.clone().unwrap_or_default()
    }

    #[must_use]
    pub fn row_clue(&self, index: usize) -> Vec<u16> {
        self.session
            .puzzle()
            .row_clues()
            .get(index)
            .cloned()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn column_clue(&self, index: usize) -> Vec<u16> {
        self.session
            .puzzle()
            .column_clues()
            .get(index)
            .cloned()
            .unwrap_or_default()
    }
}
