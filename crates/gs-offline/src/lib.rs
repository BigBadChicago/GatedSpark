//! Puzzles kept on a device, so that losing signal does not end the game.
//!
//! Three rules, agreed before any of this was written:
//!
//! * **Five ahead.** The device holds a few unplayed puzzles, downloaded in advance.
//! * **Delivered is not played.** Bytes arriving is not a player playing. The bookmark
//!   moves only when somebody actually opens a puzzle.
//! * **One bookmark in a shuffled order.** Instead of a record of every puzzle a player
//!   has finished, one number saying how far along their own shuffled list they are.
//!
//! # What this crate does and does not do
//!
//! It decides **policy**: how many to hold, which is next, what counts as played, and
//! what a player is told when there are none left. It touches no files and no network,
//! which is what lets it run in a browser.
//!
//! **Where the bytes live is not its business.** That is [`shelf::Shelf`], four
//! operations implemented by whatever is hosting it. [`shelf::Memory`] is the one this
//! crate ships, and every test here runs on it.
//!
//! **It knows nothing about any game.** It holds packages as bytes and checks them
//! against a key. Which family a puzzle belongs to is the page's question: a buffer that
//! knew about Nonogram would have to learn about every family after it.
//!
//! # What is deliberately missing
//!
//! **Withdrawing a puzzle already downloaded.** If a bad puzzle ships, a device holding
//! it needs telling to drop it. [`buffer::Buffer::discard_spoiled`] throws away anything
//! that fails its *own* checks, which covers a damaged or tampered file, but it cannot
//! cover a puzzle that is perfectly signed and simply should not be played. That needs
//! an agreed way of publishing "do not play these", and it is the last hole in the
//! offline story. ADR 0011 records it as such rather than leaving it to be noticed.

pub mod buffer;
pub mod order;
pub mod shelf;

pub use buffer::{Buffer, Delivered, NotKept, NothingToPlay, Stock, HOW_MANY_AHEAD};
pub use order::{Bookmark, Order};
pub use shelf::{Shelf, ShelfError};
