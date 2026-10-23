//! The "do not play these" list: how a device is told to stop offering a puzzle.
//!
//! Once a puzzle has been downloaded, nothing else in this system can take it back.
//! The package is signed, so it stays valid; the device holds it and will keep handing
//! it to the player. If a puzzle turns out to have no answer, or more than one, that is
//! a player stuck on a board that cannot be finished and no amount of checking the
//! signature will help, because the signature is perfectly good. This module is the
//! only mechanism that fixes that.
//!
//! # What a list is
//!
//! A signed file naming content ids that must not be played, each with a short reason.
//! Signed by the same kind of key that signs puzzles, so a device checks it exactly as
//! it checks a puzzle, and so a list from anybody else is worth nothing.
//!
//! # The serial number, which is the whole security of it
//!
//! Every list carries a serial number that only goes up. A device remembers the
//! highest it has accepted and **refuses any list numbered at or below it**.
//!
//! Without that, the attack is trivial and needs no key at all: keep serving yesterday's
//! genuine, correctly signed list, the one from before the bad puzzle was withdrawn.
//! Every signature checks out and the withdrawal never arrives. A number that only goes
//! up is what makes replaying an old list useless.
//!
//! **It is a number and not a date, deliberately.** This project's reports carry no
//! timestamp for the same reason: a record whose validity depends on the reader's clock
//! cannot be re-checked later, and a device's clock is the easiest thing on it to
//! change. A serial number needs no clock and works with no signal.
//!
//! What this does not fix, and cannot: a device that never connects never learns about
//! a withdrawal. No scheme fixes that one. It is named in ADR 0012 rather than implied
//! away.
//!
//! # Why this is not a package
//!
//! A withdrawal list has its own magic bytes, and — unlike a package — **its magic is
//! part of what the signature covers**. That is what stops a signature made over one
//! being replayed as a signature over the other. A publisher's signature on a puzzle
//! can never be made to stand as a signature on a list of withdrawals, which would
//! otherwise be a way to withdraw puzzles with no key.
//!
//! The package's own digest input is left exactly as it was, because changing it would
//! change what every signature already issued covers. The two can still never collide:
//! a package digests two version bytes followed by its body, `[0x01, 0x00, ...]`, and a
//! list digests its magic first, `[0x47, 0x41, ...]` — `GA` of `GATEDSWD`. Those first
//! bytes differ, so no byte string is a valid digest input for both. Held by
//! `tests/withdrawal.rs::a_signature_over_a_puzzle_can_never_stand_for_a_withdrawal`.

use crate::content::{ContentId, Digest};
use crate::package::{
    count16, put_bytes32, put_str, EncodeError, OpenError, Reader, SignatureVerifier, Signer,
};

/// The first bytes of every withdrawal list. Not the package magic.
pub const MAGIC: [u8; 8] = *b"GATEDSWD";

/// The layout version of a withdrawal list.
///
/// Its own number, separate from the package's. A reader that does not know a version
/// refuses the file rather than guessing at it — and for this file in particular,
/// guessing would mean possibly ignoring a withdrawal.
pub const FORMAT_VERSION: u16 = 1;

/// Why a puzzle must not be played.
///
/// A small closed set, because each one is shown to a player as a sentence and because
/// a free-text reason is a free-text reason nobody writes carefully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// It cannot be finished. The clues admit no board at all.
    NoAnswer,
    /// It has more than one answer, so the player can be right and still be told they
    /// are wrong.
    MoreThanOneAnswer,
    /// It works, but it should not be in front of players: wrong level, a picture
    /// nobody should see, anything else about its content.
    Unsuitable,
    /// It was published by mistake and should not have been.
    Mistake,
}

impl Reason {
    /// The byte this is written as. No tag is zero, so a run of zero bytes decodes as
    /// nothing rather than as a reason — the same rule the package format follows.
    #[must_use]
    pub fn tag(self) -> u8 {
        match self {
            Reason::NoAnswer => 1,
            Reason::MoreThanOneAnswer => 2,
            Reason::Unsuitable => 3,
            Reason::Mistake => 4,
        }
    }

    /// Read a tag back, or refuse the file.
    ///
    /// An unknown tag is a malformed file, never a default reason. A newer publisher
    /// withdrawing a puzzle for a reason this reader has no word for must still stop
    /// the puzzle, and the caller decides that; silently calling it `Mistake` would put
    /// a reason in front of a player that nobody stated.
    pub fn of_tag(tag: u8) -> Result<Reason, OpenError> {
        match tag {
            1 => Ok(Reason::NoAnswer),
            2 => Ok(Reason::MoreThanOneAnswer),
            3 => Ok(Reason::Unsuitable),
            4 => Ok(Reason::Mistake),
            _ => Err(OpenError::Malformed {
                field: "withdrawal reason",
            }),
        }
    }
}

impl std::fmt::Display for Reason {
    /// Written for a player, not for a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let words = match self {
            // Each of these completes a sentence that has already named the puzzle, so
            // none of them names it again: "This puzzle was not saved, because it
            // turned out to…". Written this way because the two places that show them
            // both have a puzzle in hand, and "this puzzle … this puzzle" is how the
            // first attempt read.
            Reason::NoAnswer => "it turned out to have no answer at all",
            Reason::MoreThanOneAnswer => {
                "it turned out to have more than one answer, so it could not be marked \
                 fairly"
            }
            Reason::Unsuitable => "it should not have been offered",
            Reason::Mistake => "it was published by mistake",
        };
        write!(f, "{words}")
    }
}

/// How many bytes a content id takes up in a file.
const ID_LEN: usize = 32;

/// One puzzle, withdrawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withdrawn {
    /// The id named in the file, as plain bytes and **not** as a [`ContentId`].
    ///
    /// This is the same rule [`crate::package::OpenError::ContentIdMismatch`] follows: a
    /// `ContentId` in this codebase can only be made by digesting something, so holding
    /// one means a digest was computed. A number read out of a file is somebody's claim
    /// about a puzzle, not a digest of anything this code has seen, so it stays bytes.
    /// Compare it with [`WithdrawalList::reason_for`], which does the comparison against
    /// a real `ContentId`'s bytes.
    pub content_id: [u8; ID_LEN],
    pub reason: Reason,
}

/// A list of puzzles that must not be played.
///
/// The thing that gets signed. [`seal_list`] writes it; [`open_list`] reads it back,
/// checking before decoding, exactly as a package is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalList {
    /// Goes up with every list ever published. See this module's header: this is what
    /// makes replaying an old list useless.
    pub serial: u64,
    pub withdrawn: Vec<Withdrawn>,
}

impl WithdrawalList {
    /// The canonical byte encoding, and the only one.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.serial.to_le_bytes());
        let count = count16(self.withdrawn.len(), "withdrawn count")?;
        out.extend_from_slice(&count.to_le_bytes());
        for entry in &self.withdrawn {
            out.extend_from_slice(&entry.content_id);
            out.push(entry.reason.tag());
        }
        Ok(out)
    }

    /// Whether this list names a puzzle, and why.
    #[must_use]
    pub fn reason_for(&self, id: &ContentId) -> Option<Reason> {
        self.withdrawn
            .iter()
            .find(|entry| &entry.content_id == id.as_bytes())
            .map(|entry| entry.reason)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.withdrawn.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.withdrawn.is_empty()
    }
}

/// A withdrawal list whose bytes have been vouched for.
///
/// The private field is load-bearing in the same way [`crate::package::Opened`]'s is:
/// no code outside this module can build one, so holding one *is* the proof that the
/// signature was checked. Here that matters more than it does for a package, because
/// a forged `OpenedList` would not just let a bad puzzle through, it would let anyone
/// withdraw any puzzle they liked.
///
/// ```compile_fail
/// use gs_core::content::Digest;
/// use gs_core::withdrawal::{OpenedList, WithdrawalList};
///
/// struct D;
/// impl Digest for D {
///     fn digest(&self, _bytes: &[u8]) -> [u8; 32] { [0; 32] }
/// }
///
/// // Every field is named, including the private one, and every value is well-typed.
/// // The error is that the field is private, which is the point. Naming the field is
/// // what makes this test mean something: leaving it out would fail as a missing
/// // field whether the field were private or not.
/// let _forged = OpenedList {
///     content_id: gs_core::content::ContentId::of(&D, b"anything"),
///     list: WithdrawalList { serial: u64::MAX, withdrawn: Vec::new() },
///     key_id: "a key I chose".to_owned(),
///     checked_by_open: (),
/// };
/// ```
///
/// The same code without the final struct literal does compile, which is what says the
/// failure above is the private field and not a mistake in the setup:
///
/// ```
/// use gs_core::withdrawal::WithdrawalList;
/// let _list = WithdrawalList { serial: u64::MAX, withdrawn: Vec::new() };
/// ```
// Clippy offers `#[non_exhaustive]` here and it is not the same thing: that stops the
// rest of the world, a private field stops the rest of this crate too. See
// `package::Opened` for the full reasoning.
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedList {
    /// The fingerprint of the body, recomputed here rather than taken on trust.
    pub content_id: ContentId,
    pub list: WithdrawalList,
    /// Who signed it.
    pub key_id: String,
    checked_by_open: (),
}

/// What is digested, and so what the signature covers.
///
/// The magic is in it, which is the difference from [`crate::package`] and the reason
/// a signature over one can never stand for the other. See this module's header.
fn digest_input(version: u16, body: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(MAGIC.len() + 2 + body.len());
    input.extend_from_slice(&MAGIC);
    input.extend_from_slice(&version.to_le_bytes());
    input.extend_from_slice(body);
    input
}

/// Write a signed withdrawal list.
pub fn seal_list<D: Digest, S: Signer>(
    list: &WithdrawalList,
    digest: &D,
    signer: &S,
) -> Result<Vec<u8>, EncodeError> {
    let body = list.canonical_bytes()?;
    let id = ContentId::of(digest, &digest_input(FORMAT_VERSION, &body));
    let signature = signer.sign(&id);

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    put_bytes32(&mut out, &body, "body")?;
    out.extend_from_slice(id.as_bytes());
    put_str(&mut out, signer.key_id(), "key id")?;
    put_bytes32(&mut out, &signature, "signature")?;
    Ok(out)
}

/// Read a withdrawal list, checking before decoding.
///
/// The same order of operations [`crate::package::open`] follows, and for the same
/// reason: framing, then the digest, then the signature, and only then the contents.
pub fn open_list<D: Digest, V: SignatureVerifier>(
    bytes: &[u8],
    digest: &D,
    verifier: &V,
) -> Result<OpenedList, OpenError> {
    let mut r = Reader::new(bytes);

    if r.take(MAGIC.len(), "magic")? != MAGIC {
        return Err(OpenError::NotAPackage);
    }
    let version = r.u16("format version")?;
    if version != FORMAT_VERSION {
        return Err(OpenError::UnsupportedFormat(version));
    }

    let body = r.bytes32("body")?;
    let stated: [u8; ID_LEN] = r
        .take(ID_LEN, "list id")?
        .try_into()
        .expect("slice of exactly ID_LEN bytes");
    let key_id = r.string("key id")?;
    let signature = r.bytes32("signature")?;
    let remaining = r.remaining();
    if remaining != 0 {
        return Err(OpenError::TrailingBytes(remaining));
    }

    let computed = ContentId::of(digest, &digest_input(version, body));
    if computed.as_bytes() != &stated {
        return Err(OpenError::ContentIdMismatch { stated, computed });
    }
    // Checked against the id this code computed, never the one the file stated.
    if !verifier.verify(&computed, &key_id, signature) {
        return Err(OpenError::SignatureRejected);
    }

    Ok(OpenedList {
        content_id: computed,
        list: decode_list(body)?,
        key_id,
        checked_by_open: (),
    })
}

fn decode_list(body: &[u8]) -> Result<WithdrawalList, OpenError> {
    let mut r = Reader::new(body);
    let serial = r.u64("serial")?;
    let count = r.u16("withdrawn count")?;
    let mut withdrawn = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let id: [u8; ID_LEN] = r
            .take(ID_LEN, "withdrawn content id")?
            .try_into()
            .expect("slice of exactly ID_LEN bytes");
        let reason = Reason::of_tag(r.u8("withdrawal reason")?)?;
        withdrawn.push(Withdrawn {
            content_id: id,
            reason,
        });
    }
    let remaining = r.remaining();
    if remaining != 0 {
        return Err(OpenError::TrailingBytes(remaining));
    }
    Ok(WithdrawalList { serial, withdrawn })
}
