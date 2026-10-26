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

use crate::play::nonogram::Mark;
use crate::play::slitherlink::{Line, Which};
use crate::play::{Place, Session};
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

impl Page {
    /// The key list this page checks against.
    ///
    /// Not exposed to a web page: `#[wasm_bindgen]` is deliberately absent, so this is
    /// reachable from the rest of this crate and from nowhere a page can call. The
    /// saved-puzzle calls in [`crate::saved`] need the same list this page opens
    /// puzzles with, and giving them a second copy to be configured separately is how
    /// a device ends up trusting one set of keys for new puzzles and another for saved
    /// ones.
    pub(crate) fn keys(&self) -> &TrustedKeys {
        &self.keys
    }

    /// The digest this page checks with. There is one, and this is it.
    pub(crate) fn digest(&self) -> &Sha256 {
        &Sha256
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
    /// Which game this is: the first thing a page needs, because it decides what the
    /// page draws and which moves it may make.
    #[must_use]
    pub fn family(&self) -> String {
        self.playable.family().to_owned()
    }

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

/// What a page marks on the screen: one square of a Nonogram.
///
/// Numbered away from [`Side::Across`] and [`Side::Down`], so one list can carry both
/// games' places without a page having to ask which game it is holding.
///
/// Public so that a check can read the same number the page is given, rather than its
/// own copy of it.
pub const SQUARE: i32 = 2;

/// What a page marks on the screen: one white square of a Kakuro.
///
/// Numbered away from [`Side::Across`], [`Side::Down`] and [`SQUARE`] for the same
/// reason they are numbered away from each other, and kept distinct from `SQUARE`
/// although both carry a column and a row. The two do not mean the same thing: a
/// Nonogram's pair indexes the only squares there are, while a Kakuro's indexes the
/// whole grid with the black squares in it. A page that treated them alike would ring
/// the wrong square.
pub const DIGIT: i32 = 3;

/// The number a page is given for a black square where a digit would be.
///
/// Not zero. Zero means "a white square with nothing written in it yet", which is the
/// commonest state on the board, and a page that confused the two would draw an empty
/// grid as a solid wall or the other way round.
pub const BLACK: i32 = -1;

/// The number a page is given for a total that is not there.
///
/// Most black squares head no run at all, and a run starting against the rim has no
/// black square in front of it.
pub const NO_TOTAL: i32 = -1;

/// Flatten places into the three-numbers-each list a page reads.
///
/// One copy, used by both [`Game::hint`] and [`Game::check`]. Two copies of this would
/// be two numberings that agree until the day one of them is changed, and the page
/// would then ring the wrong square for one of the two.
fn flatten(places: &[Place]) -> Vec<i32> {
    let mut out = Vec::new();
    for place in places {
        match place {
            Place::Square { row, column } => {
                out.push(SQUARE);
                out.push(i32::from(*row));
                out.push(i32::from(*column));
            }
            Place::Line {
                across,
                column,
                row,
            } => {
                out.push(if *across {
                    Side::Across as i32
                } else {
                    Side::Down as i32
                });
                out.push(i32::from(*row));
                out.push(i32::from(*column));
            }
            Place::Digit { column, row } => {
                out.push(DIGIT);
                out.push(i32::from(*row));
                out.push(i32::from(*column));
            }
        }
    }
    out
}

/// How a board is doing, ready for a page to show.
///
/// Flat for the same reason [`Hinted`] is: a sentence, and a list of numbers in threes.
#[wasm_bindgen]
pub struct Checked {
    says: String,
    clear: bool,
    places: Vec<i32>,
}

#[wasm_bindgen]
impl Checked {
    /// What to tell the player. The game's own sentence.
    #[must_use]
    pub fn says(&self) -> String {
        self.says.clone()
    }

    /// Whether nothing is wrong. Not the same as finished: a blank board is clear.
    #[must_use]
    pub fn clear(&self) -> bool {
        self.clear
    }

    /// Everywhere to mark, three numbers at a time, numbered exactly as
    /// [`Hinted::places`] numbers them. Empty when nothing is wrong.
    #[must_use]
    pub fn places(&self) -> Vec<i32> {
        self.places.clone()
    }
}

/// One hint, ready for a page to show.
///
/// Flat on purpose: a sentence, and a list of numbers in threes. Making this a shape a
/// page has to take apart would be asking the page to understand the reasoning, which
/// is the thing it must not do.
#[wasm_bindgen]
pub struct Hinted {
    says: String,
    places: Vec<i32>,
    hints_left: u32,
}

#[wasm_bindgen]
impl Hinted {
    /// What to tell the player. The game's own sentence, with the reason in it.
    #[must_use]
    pub fn says(&self) -> String {
        self.says.clone()
    }

    /// Everywhere this hint marked, three numbers at a time: what kind of place, then
    /// its row, then its column.
    ///
    /// The first number of each three is [`SQUARE`] for a square of a Nonogram,
    /// [`DIGIT`] for a white square of a Kakuro, or a [`Side`] — `0` across, `1`
    /// down — for a line of a Slitherlink, in which case the row and column name the
    /// dot the line starts from.
    ///
    /// Empty when the hint marked nothing, which happens when it says a row cannot
    /// work: which square in it is wrong is the player's to decide.
    #[must_use]
    pub fn places(&self) -> Vec<i32> {
        self.places.clone()
    }

    /// How many places this hint marked.
    #[must_use]
    pub fn filled(&self) -> usize {
        self.places.len() / 3
    }

    /// How many hints are left on this puzzle after this one.
    #[must_use]
    pub fn hints_left(&self) -> u32 {
        self.hints_left
    }
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
        self.session
            .as_nonogram()
            .is_some_and(|game| game.mark_at(row, column, square.into()).is_ok())
    }

    /// Why the last `mark` did nothing, or an empty string if it did something.
    ///
    /// Separate from `mark` so the common path stays a plain yes or no. A page showing
    /// a message needs this; a page just drawing a board does not.
    pub fn mark_and_say(&mut self, row: u16, column: u16, square: Square) -> String {
        let game = self.session.puzzle().family();
        match self.session.as_nonogram() {
            Some(session) => match session.mark_at(row, column, square.into()) {
                Ok(()) => String::new(),
                Err(why) => why.to_string(),
            },
            // Not silence and not `false`. A page making a move the game does not have
            // has a bug in it, and the quietest possible answer is the one that takes
            // longest to find.
            None => format!("marking a square is not a move in {game}"),
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
        match &self.session {
            Session::Nonogram(game) => game.mark_at_of(row, column).into(),
            _ => Square::Unknown,
        }
    }

    /// The whole board in one call, row by row from the top left.
    ///
    /// Each byte is a [`Square`] value. This is for drawing: asking square by square
    /// across the boundary is one call per square, and a 20 by 20 board redrawn on every
    /// tap is 400 of them. It is derived fresh from the same board each time, so it
    /// cannot drift from what `square` would say.
    #[must_use]
    pub fn board(&self) -> Vec<u8> {
        let Session::Nonogram(game) = &self.session else {
            return Vec::new();
        };
        let dims = self.session.puzzle().dims();
        (0..dims.height)
            .flat_map(|row| {
                (0..dims.width).map(move |column| {
                    // `as` on a fieldless enum is its discriminant, which is the value
                    // the page reads. The numbers live on `Square` and nowhere else.
                    Square::from(game.mark_at_of(row, column)) as u8
                })
            })
            .collect()
    }

    /// Ask for a hint: the next part of the puzzle, worked out and marked in.
    ///
    /// Works out nothing here. The reasoning is in the game's own crate, the allowance
    /// and the growing sizes are in [`Session`], and this turns the answer into
    /// something a page can read.
    ///
    /// The marks it makes go through the ordinary rules, so they count as moves and
    /// undo takes them back one at a time.
    ///
    /// Throws when there is no hint to give — the three are spent, or nothing could be
    /// worked out — with the sentence to show the player. A thrown error rather than an
    /// empty hint, because those are the two cases where a page should *not* highlight
    /// anything, and an empty hint object is the kind of thing a page forgets to check.
    pub fn hint(&mut self) -> Result<Hinted, JsError> {
        match self.session.hint() {
            Ok(advice) => {
                // Flattened into one list of numbers, three per place: what kind, then
                // two coordinates. A page reads it in threes, the way it already reads
                // `line_places`. Carrying a list of objects across would mean a page
                // holding a shape from in here.
                let places = flatten(&advice.filled);
                Ok(Hinted {
                    says: advice.says,
                    places,
                    hints_left: u32::from(advice.hints_left),
                })
            }
            Err(why) => Err(JsError::new(&why.to_string())),
        }
    }

    /// Check the board as the player has it: what is already wrong, and how far along
    /// it is.
    ///
    /// **Nothing changes.** No square is marked, no allowance is spent, and it can be
    /// asked for as often as a player likes. That is the difference between this and
    /// [`Game::hint`]: a hint works part of the puzzle out and tells the player
    /// something new, so there are three of them; a check only counts what is already
    /// in front of them, so there is no reason to ration it.
    ///
    /// Not a thrown error when something is wrong: being wrong part-way through is the
    /// ordinary state of a puzzle being played, and a page should show the sentence
    /// either way.
    #[must_use]
    pub fn check(&self) -> Checked {
        let found = self.session.check();
        Checked {
            says: found.says,
            clear: found.clear,
            places: flatten(&found.places),
        }
    }

    /// How many hints are left on this puzzle.
    #[must_use]
    pub fn hints_left(&self) -> u32 {
        u32::from(self.session.hints_left())
    }

    /// How many hints a puzzle starts with.
    ///
    /// Read from the core rather than written into a page, so "3 hints" in front of a
    /// player and the number actually enforced cannot drift apart.
    #[must_use]
    pub fn hints_per_puzzle(&self) -> u32 {
        u32::from(crate::play::HINTS_PER_PUZZLE)
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

    /// Which game is being played, so a page can draw the right board and offer the
    /// right moves.
    #[must_use]
    pub fn family(&self) -> String {
        self.session.puzzle().family().to_owned()
    }

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

// ---------------------------------------------------------------------------
// The Slitherlink half
// ---------------------------------------------------------------------------

/// Which way a line runs, in the shape a web page can switch on.
///
/// A separate type from [`crate::play::slitherlink::Which`] for the same reason
/// [`Square`] is separate from `Mark`: `#[wasm_bindgen]` cannot go in a module that also
/// compiles for an ordinary machine, so this is the translation and the conversion below
/// is the only place the mapping exists.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Left to right.
    Across = 0,
    /// Top to bottom.
    Down = 1,
}

impl From<Side> for Which {
    fn from(side: Side) -> Which {
        match side {
            Side::Across => Which::Across,
            Side::Down => Which::Down,
        }
    }
}

/// What one line holds, in the shape a web page can switch on.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    /// Part of the loop.
    Yes = 0,
    /// Definitely not part of the loop.
    No = 1,
    /// Not decided yet.
    Unknown = 2,
}

impl From<Drawn> for Line {
    fn from(drawn: Drawn) -> Line {
        match drawn {
            Drawn::Yes => Line::Drawn,
            Drawn::No => Line::RuledOut,
            Drawn::Unknown => Line::Unknown,
        }
    }
}

impl From<Line> for Drawn {
    fn from(line: Line) -> Drawn {
        match line {
            Line::Drawn => Drawn::Yes,
            Line::RuledOut => Drawn::No,
            Line::Unknown => Drawn::Unknown,
        }
    }
}

/// The Slitherlink moves and readings.
///
/// Every one of these is nothing on a game of some other family: `false`, an empty list,
/// or a sentence saying so. None of them guesses at an equivalent move in the game it
/// actually has.
#[wasm_bindgen]
impl Game {
    /// How many dots there are across the board. One more than the squares.
    #[must_use]
    pub fn dots_across(&self) -> u16 {
        match &self.session {
            Session::Slitherlink(game) => game.geometry().dots_across(),
            _ => 0,
        }
    }

    /// How many dots there are down the board. One more than the squares.
    #[must_use]
    pub fn dots_down(&self) -> u16 {
        match &self.session {
            Session::Slitherlink(game) => game.geometry().dots_down(),
            _ => 0,
        }
    }

    /// Where every line on the board is, in the same order [`Game::lines`] reports
    /// them: three numbers each — `0` for across or `1` for down, then the column and
    /// row of the dot the line starts at.
    ///
    /// This exists so a page never works out the numbering for itself. A page that did
    /// would hold a second opinion about which line is which, and the day the two
    /// disagreed it would draw one line and mark another. Instead it builds its lines
    /// from this list, in this order, and reads their marks from `lines()` at the same
    /// positions.
    #[must_use]
    pub fn line_places(&self) -> Vec<u16> {
        let Session::Slitherlink(game) = &self.session else {
            return Vec::new();
        };
        let geometry = game.geometry();
        geometry
            .edges()
            .filter_map(|edge| geometry.edge_at(edge))
            .flat_map(|(along, column, row)| {
                let side = match along {
                    gs_slitherlink::board::Along::Across => 0,
                    gs_slitherlink::board::Along::Down => 1,
                };
                [side, column, row]
            })
            .collect()
    }

    /// Draw a line, rule one out, or clear either.
    ///
    /// `column` and `row` name the dot the line starts at, counting from the top-left
    /// corner. Returns `true` if the board changed. A tap that changes nothing — a line
    /// already marked that way, or one off the edge of the board — is `false` rather
    /// than an error, because those are ordinary during play.
    pub fn draw(&mut self, side: Side, column: u16, row: u16, drawn: Drawn) -> bool {
        self.session
            .as_slitherlink()
            .is_some_and(|game| game.mark(side.into(), column, row, drawn.into()).is_ok())
    }

    /// Why the last `draw` did nothing, or an empty string if it did something.
    pub fn draw_and_say(&mut self, side: Side, column: u16, row: u16, drawn: Drawn) -> String {
        let game = self.session.puzzle().family();
        match self.session.as_slitherlink() {
            Some(session) => match session.mark(side.into(), column, row, drawn.into()) {
                Ok(()) => String::new(),
                Err(why) => why.to_string(),
            },
            None => format!("drawing a line is not a move in {game}"),
        }
    }

    /// How one line is marked. A line off the board reads as undecided.
    #[must_use]
    pub fn line(&self, side: Side, column: u16, row: u16) -> Drawn {
        match &self.session {
            Session::Slitherlink(game) => game.line_at(side.into(), column, row).into(),
            _ => Drawn::Unknown,
        }
    }

    /// Every line on the board in one call: the ones running across first, row by row
    /// from the top, then the ones running down, column by column from the left.
    ///
    /// Each byte is a [`Drawn`] value. This is for drawing: asking line by line across
    /// the boundary is one call per line, and a 10 by 10 board has 220 of them. It is
    /// derived fresh from the same board each time, so it cannot drift from what `line`
    /// would say.
    #[must_use]
    pub fn lines(&self) -> Vec<u8> {
        let Session::Slitherlink(game) = &self.session else {
            return Vec::new();
        };
        game.geometry()
            .edges()
            .map(|edge| Drawn::from(game.line_of(edge)) as u8)
            .collect()
    }

    /// The number in one square, or `-1` where there is none.
    ///
    /// Most squares have no number, and that is the puzzle. `-1` rather than `0`,
    /// because `0` is a real number a square can carry and means something quite
    /// different: no sides of this square are used at all.
    #[must_use]
    pub fn number(&self, column: u16, row: u16) -> i16 {
        match &self.session {
            Session::Slitherlink(game) => game.number_at(column, row).map_or(-1, i16::from),
            _ => -1,
        }
    }

    /// How many sides of one square the player has drawn so far. `-1` on another game.
    ///
    /// Comes from the rules, so the count shown beside a number and the count the rules
    /// judge are the same count.
    #[must_use]
    pub fn sides_drawn(&self, column: u16, row: u16) -> i16 {
        match &self.session {
            Session::Slitherlink(game) => game.sides_drawn(column, row).map_or(-1, i16::from),
            _ => -1,
        }
    }

    /// What the drawn lines add up to at the moment, in a sentence a player can read:
    /// one loop, nothing yet, a loose end, a crossing, or several separate loops.
    ///
    /// The sentence comes from `gs-slitherlink`, not from here. A page writing its own
    /// would be a second opinion about the state of the board.
    #[must_use]
    pub fn shape(&self) -> String {
        match &self.session {
            Session::Slitherlink(game) => game.shape().plainly().to_owned(),
            _ => String::new(),
        }
    }

    /// The squares whose numbers are not yet met, as pairs of column and row.
    ///
    /// Not "wrong squares": a number not yet met is the ordinary state of a puzzle being
    /// played. A page may use this to show a player where to look.
    #[must_use]
    pub fn numbers_not_met(&self) -> Vec<u16> {
        let Session::Slitherlink(game) = &self.session else {
            return Vec::new();
        };
        game.numbers_not_met()
            .into_iter()
            .flat_map(|(column, row)| [column, row])
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Kakuro
// ---------------------------------------------------------------------------
//
// The third game's half of the translation layer. Written as its own block for the
// same reason the Slitherlink block is: these methods are meaningless on the other
// games, and a page calling one on the wrong game gets a flat, drawable answer rather
// than an exception.
//
// What this block had to add that neither other game needed is one thing, and it is
// the thing the family was chosen to test: **a move carries a value.** Everything
// else here is the same two patterns as before — one call that returns the whole board
// for drawing, and one call per move that returns the sentence to show.

#[wasm_bindgen]
impl Game {
    /// The board's shape, three numbers per grid square in reading order.
    ///
    /// For each square: whether it is black (1) or white (0), then the total of the run
    /// going right from it, then the total of the run going down from it, with
    /// [`NO_TOTAL`] where there is none.
    ///
    /// Called once, when the board is drawn. The shape of a Kakuro never changes while
    /// it is being played — black squares are part of the puzzle, not a state a player
    /// can reach — so a page that called this on every tap would be redrawing a wall
    /// that cannot move.
    ///
    /// A run starting hard against the left or top rim has no black square in front of
    /// it to carry its total. Those totals come back on the rim squares' own entries as
    /// well, through [`Game::rim_totals`], because a page cannot draw a number in a
    /// square that is not there.
    #[must_use]
    pub fn kakuro_shape(&self) -> Vec<i32> {
        let Session::Kakuro(game) = &self.session else {
            return Vec::new();
        };
        let layout = game.layout();
        let mut out = Vec::with_capacity(usize::from(layout.width()) * 3);
        for row in 0..layout.height() {
            for column in 0..layout.width() {
                let black = layout
                    .square(column, row)
                    .is_some_and(|square| layout.is_block(square));
                out.push(i32::from(black));
                if black {
                    let heading = game.heading_at(column, row);
                    out.push(heading.across.map_or(NO_TOTAL, i32::from));
                    out.push(heading.down.map_or(NO_TOTAL, i32::from));
                } else {
                    out.push(NO_TOTAL);
                    out.push(NO_TOTAL);
                }
            }
        }
        out
    }

    /// The totals of runs that start against the left or top rim, with no black square
    /// in front of them to be written in.
    ///
    /// Four numbers each: the column and row of the run's first white square, then the
    /// across total and the down total, with [`NO_TOTAL`] where there is none. A page
    /// draws these in the margin.
    ///
    /// This exists because the rim is not a black square and the board's shape is not
    /// the page's to invent. A page that drew a run's total by looking at the square
    /// before it would silently drop every run that starts at column 0 or row 0, which
    /// on a sparse board is most of them.
    #[must_use]
    pub fn rim_totals(&self) -> Vec<i32> {
        let Session::Kakuro(game) = &self.session else {
            return Vec::new();
        };
        let layout = game.layout();
        let mut out = Vec::new();
        for row in 0..layout.height() {
            for column in 0..layout.width() {
                let white = layout
                    .square(column, row)
                    .is_some_and(|square| layout.white_at(square).is_some());
                if !white {
                    continue;
                }
                // Only the squares on the rim: anywhere else, the square in front is
                // either black and carries the total, or white and the run continues
                // through it.
                let across = if column == 0 {
                    game.heading_for_rim(column, row, true)
                } else {
                    None
                };
                let down = if row == 0 {
                    game.heading_for_rim(column, row, false)
                } else {
                    None
                };
                if across.is_none() && down.is_none() {
                    continue;
                }
                out.push(i32::from(column));
                out.push(i32::from(row));
                out.push(across.map_or(NO_TOTAL, i32::from));
                out.push(down.map_or(NO_TOTAL, i32::from));
            }
        }
        out
    }

    /// Every digit on the board, one number per grid square in reading order.
    ///
    /// A digit from 1 to 9, `0` for a white square still empty, or [`BLACK`] for a
    /// black one. This is the call a page makes after every move.
    ///
    /// One call for the whole board rather than one per square, for the reason the
    /// Nonogram board gives: a 9 by 9 redrawn on every keystroke is 81 calls across the
    /// boundary. It is derived fresh from the same board each time, so it cannot drift
    /// from what [`Game::digit`] would say.
    #[must_use]
    pub fn digits(&self) -> Vec<i32> {
        let Session::Kakuro(game) = &self.session else {
            return Vec::new();
        };
        let layout = game.layout();
        let mut out = Vec::with_capacity(usize::from(layout.width()));
        for row in 0..layout.height() {
            for column in 0..layout.width() {
                let is_white = layout
                    .square(column, row)
                    .is_some_and(|square| layout.white_at(square).is_some());
                out.push(if is_white {
                    game.digit_at(column, row).map_or(0, i32::from)
                } else {
                    BLACK
                });
            }
        }
        out
    }

    /// The digit in one white square: 1 to 9, `0` for empty, [`BLACK`] for a black
    /// square or one off the board.
    #[must_use]
    pub fn digit(&self, column: u16, row: u16) -> i32 {
        let Session::Kakuro(game) = &self.session else {
            return BLACK;
        };
        let layout = game.layout();
        let is_white = layout
            .square(column, row)
            .is_some_and(|square| layout.white_at(square).is_some());
        if !is_white {
            return BLACK;
        }
        game.digit_at(column, row).map_or(0, i32::from)
    }

    /// Write a digit in one white square, and say what happened.
    ///
    /// `digit` is 1 to 9. **Zero empties the square**, which is what a page gets when a
    /// player presses Backspace or Delete, and it goes to `clear` rather than being
    /// written: zero is not a Kakuro digit and the rules refuse it. That translation is
    /// here rather than in a page because "what the Delete key means" is a question
    /// about this game, and a page should not have to know that nought is not a digit.
    ///
    /// The sentence is for a player to read. A refused move says why and changes
    /// nothing.
    pub fn write_and_say(&mut self, column: u16, row: u16, digit: i32) -> String {
        let Some(game) = self.session.as_kakuro() else {
            return "this is not a Kakuro".to_owned();
        };
        let outcome = if digit == 0 {
            game.clear(column, row)
        } else {
            match i16::try_from(digit) {
                Ok(digit) => game.write(column, row, digit),
                // A keystroke that is not a digit at all. Refused with the same words
                // as any other refused move: a page passes on what it read, and this
                // is where what counts as a digit is decided.
                Err(_) => Err(crate::play::NoChange::TheRulesRefused),
            }
        };
        match outcome {
            Ok(()) => {
                if digit == 0 {
                    format!(
                        "the square at {} across, {} down has been emptied",
                        u32::from(column) + 1,
                        u32::from(row) + 1
                    )
                } else {
                    format!(
                        "{digit} written at {} across, {} down",
                        u32::from(column) + 1,
                        u32::from(row) + 1
                    )
                }
            }
            Err(why) => why.to_string(),
        }
    }

    /// How many runs do not yet add up to their totals.
    ///
    /// Every run on a blank board is in this count, so it falls towards zero as the
    /// puzzle is filled in. Not a mistake count: a run that is not yet met is the
    /// ordinary state of a puzzle being played.
    #[must_use]
    pub fn runs_to_go(&self) -> usize {
        let Session::Kakuro(game) = &self.session else {
            return 0;
        };
        game.runs_not_met().len()
    }

    /// How many runs have the same digit in them twice.
    ///
    /// This one *is* a mistake, and it is the one Kakuro mistake a player can be shown
    /// without being told anything they could not work out. Separate from
    /// [`Game::runs_to_go`] so a page can say it differently.
    #[must_use]
    pub fn runs_with_a_repeat(&self) -> usize {
        let Session::Kakuro(game) = &self.session else {
            return 0;
        };
        game.runs_with_a_repeat().len()
    }

    /// Every white square that sits in a run holding the same digit twice, as a column
    /// and a row each.
    ///
    /// A page marks these. It exists rather than letting a page work it out from a
    /// count, because "somewhere on this board a digit repeats" is not help, and
    /// working out which squares those are means knowing where the runs are — which is
    /// the board's shape, and not the page's to re-derive. The core says which runs
    /// have the fault; the page paints the squares it is given.
    #[must_use]
    pub fn squares_with_a_repeat(&self) -> Vec<u16> {
        let Session::Kakuro(game) = &self.session else {
            return Vec::new();
        };
        let layout = game.layout();
        let mut out = Vec::new();
        for id in game.runs_with_a_repeat() {
            let Some(run) = layout.run(id) else { continue };
            for white in &run.squares {
                let Some((column, row)) = layout
                    .square_of(*white)
                    .and_then(|square| layout.square_at(square))
                else {
                    continue;
                };
                out.push(column);
                out.push(row);
            }
        }
        out
    }

    /// How the board is going, in a sentence: how many runs are still to go, and
    /// whether any has a digit twice.
    ///
    /// The Kakuro answer to Slitherlink's `shape`. Both exist because each game has one
    /// useful thing to say about a part-finished board that the other does not.
    #[must_use]
    pub fn how_it_is_going(&self) -> String {
        if !matches!(&self.session, Session::Kakuro(_)) {
            return String::new();
        }
        let repeats = self.runs_with_a_repeat();
        let left = self.runs_to_go();
        let runs = match left {
            0 => "every run adds up".to_owned(),
            1 => "1 run still to go".to_owned(),
            many => format!("{many} runs still to go"),
        };
        match repeats {
            0 => runs,
            1 => format!("{runs}, and 1 run has a digit in it twice"),
            many => format!("{runs}, and {many} runs have a digit in them twice"),
        }
    }
}
