//! The schedule: which puzzle is today's, and which were the days before it.
//!
//! A daily puzzle needs a notion of "today", and this project has a firm rule that
//! nothing deciding whether content can be trusted may depend on the reader's clock.
//! A device's clock is the easiest thing on it to change, and a record whose validity
//! depends on the reader's clock cannot be re-checked later.
//!
//! # How both things stay true
//!
//! A schedule maps a **day number** to a puzzle. Day numbers count from a start day
//! written inside the signed file. The device's clock turns today's date into a day
//! number — [`Date::day_number_from`] — and that is the only thing it is used for.
//!
//! So a wrong clock shows somebody the wrong day's puzzle. It cannot make a bad puzzle
//! look good, cannot revive a withdrawn one, and cannot forge a day that was never
//! signed. **The clock chooses which puzzle; it never decides whether a puzzle is
//! trustworthy.** Everything that decides trust is still the package's own signature
//! and the withdrawal list.
//!
//! # Built like the withdrawal list, for the same reasons
//!
//! This file is [`crate::withdrawal`]'s shape, because it has the same problems:
//!
//! * **Its own magic bytes, covered by the signature.** A signature over a package, or
//!   over a withdrawal list, can never be made to stand as a signature over a schedule.
//!   Without that, somebody holding one signed file could rearrange the calendar.
//! * **A serial number that only goes up.** A device remembers the highest it accepted
//!   and refuses anything at or below it. Without it the attack needs no key at all:
//!   keep serving last month's genuine, correctly signed schedule and the player never
//!   sees a new day — and a day whose puzzle was later replaced comes back.
//! * **No timestamps.** Day numbers are arithmetic. Nothing here reads a clock; the
//!   caller supplies a date and this module does sums on it.
//!
//! # One schedule for every game
//!
//! Each entry names its own family, so one file carries both games, the same way there
//! is one withdrawal list and one offline buffer for both (ADR 0015). A schedule per
//! game would be a new file, a new serial number and a new fetch for every family
//! added.
//!
//! # Exactly one byte string per calendar
//!
//! Entries are sorted by day and then by family, and no day may name the same family
//! twice. [`seal_schedule`] refuses to write a list that breaks either rule and
//! [`open_schedule`] refuses to read one, which together mean a given calendar has
//! exactly one possible encoding. Two publishers with the same content produce the same
//! bytes, and a reader never has to decide which of two orderings was meant.

use crate::content::{ContentId, Digest};
use crate::package::{
    put_bytes32, put_str, EncodeError, OpenError, Reader, SignatureVerifier, Signer,
};

/// The first bytes of every schedule. Not the package's magic, and not the withdrawal
/// list's.
pub const MAGIC: [u8; 8] = *b"GATEDSCD";

/// The layout version of a schedule.
///
/// Its own number, separate from the package's and the withdrawal list's. A reader that
/// does not know a version refuses the file rather than guessing.
pub const FORMAT_VERSION: u16 = 1;

/// How many bytes a content id takes up in a file.
const ID_LEN: usize = 32;

/// A day on the calendar, as a person writes it.
///
/// Carries no time of day and no zone, because a daily puzzle is a day and not a
/// moment. Which zone's date a device uses is the device's business; the consequence of
/// getting it wrong is seeing yesterday's puzzle, which is the whole blast radius of
/// the clock in this design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// A date, or nothing if those three numbers are not a real day.
    ///
    /// Checked rather than trusted: a schedule's start day is inside the signed file and
    /// a publisher typing 2026-02-30 should be told so, not have it quietly become
    /// March.
    #[must_use]
    pub fn new(year: u16, month: u8, day: u8) -> Option<Date> {
        let date = Date { year, month, day };
        if month == 0 || month > 12 || day == 0 || day > date.days_in_month() {
            return None;
        }
        Some(date)
    }

    fn leap_year(self) -> bool {
        let y = u32::from(self.year);
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }

    fn days_in_month(self) -> u8 {
        match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if self.leap_year() => 29,
            2 => 28,
            _ => 0,
        }
    }

    /// Days since the start of the proleptic Gregorian calendar's era, as a count with
    /// no meaning of its own.
    ///
    /// Integer arithmetic only, like everything else that decides anything here: the
    /// workspace denies decimal arithmetic in a build, and a calendar computed in
    /// decimals is exactly the kind of thing that would disagree between two devices.
    ///
    /// This is the shift-the-year-to-March trick, which makes the leap day the last day
    /// of the year and so removes every special case from the month lengths.
    fn days_from_era(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let year_of_era = y.rem_euclid(400);
        let m = i64::from(self.month);
        let shifted = if m > 2 { m - 3 } else { m + 9 };
        let day_of_year = (153 * shifted + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era
    }

    /// How many days this date is after `start`, or nothing if it is before it.
    ///
    /// Nothing before the start day has a day number, which is deliberate: a device
    /// whose clock says 1970 gets no puzzle rather than day zero's.
    #[must_use]
    pub fn day_number_from(self, start: Date) -> Option<u32> {
        let days = self.days_from_era() - start.days_from_era();
        u32::try_from(days).ok()
    }

    /// The date a day number lands on, counting from `self` as day zero.
    ///
    /// The inverse of [`Date::day_number_from`], for showing a player which day an
    /// archive entry was.
    #[must_use]
    pub fn plus_days(self, days: u32) -> Date {
        // The reverse of `days_from_era`, by the same shifted-year trick.
        let z = self.days_from_era() + i64::from(days);
        let era = z.div_euclid(146_097);
        let day_of_era = z.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let y = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted + 2) / 5 + 1;
        let month = if shifted < 10 {
            shifted + 3
        } else {
            shifted - 9
        };
        Date {
            year: u16::try_from(y + i64::from(month <= 2)).unwrap_or(u16::MAX),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }
}

impl core::fmt::Display for Date {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// One day's puzzle for one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    /// Days after the schedule's start day. Day 0 is the start day itself.
    pub day: u32,
    pub family: String,
    /// The id named in the file, as plain bytes and **not** as a [`ContentId`].
    ///
    /// The same rule the withdrawal list follows: a `ContentId` in this codebase can
    /// only be made by digesting something, so holding one means a digest was computed.
    /// A number read out of a file is somebody's claim about a puzzle, so it stays
    /// bytes until it is compared with a real one.
    pub content_id: [u8; ID_LEN],
}

/// The calendar. The thing that gets signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Goes up with every schedule ever published. See this module's header: this is
    /// what makes replaying an old schedule useless.
    pub serial: u64,
    /// Day 0. Day numbers mean nothing without it, which is why it is inside the signed
    /// body rather than agreed by convention.
    pub start: Date,
    /// Sorted by day, then by family, with no family appearing twice on a day.
    pub entries: Vec<Scheduled>,
}

/// Why a set of entries is not a calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotACalendar {
    /// Two entries for the same game on the same day. There is no rule for picking one.
    TwoForOneDay { day: u32 },
    /// The entries are not in order. Refused rather than sorted, so that the bytes a
    /// publisher signs are the bytes they described.
    OutOfOrder,
}

impl core::fmt::Display for NotACalendar {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NotACalendar::TwoForOneDay { day } => {
                write!(f, "two puzzles for the same game on day {day}")
            }
            NotACalendar::OutOfOrder => write!(f, "the days are not in order"),
        }
    }
}

impl Schedule {
    /// Check the one structural rule: sorted, and one puzzle per game per day.
    ///
    /// Called by [`seal_schedule`] before signing and by [`open_schedule`] after
    /// decoding, so neither a publisher nor a device can end up holding a calendar with
    /// two answers for one day.
    pub fn check(&self) -> Result<(), NotACalendar> {
        for pair in self.entries.windows(2) {
            let (before, after) = (&pair[0], &pair[1]);
            let key = |e: &Scheduled| (e.day, e.family.clone());
            if key(before) == key(after) {
                return Err(NotACalendar::TwoForOneDay { day: after.day });
            }
            if key(before) > key(after) {
                return Err(NotACalendar::OutOfOrder);
            }
        }
        Ok(())
    }

    /// The puzzle for one game on one day, if there is one.
    #[must_use]
    pub fn on_day(&self, day: u32, family: &str) -> Option<&Scheduled> {
        self.entries
            .iter()
            .find(|e| e.day == day && e.family == family)
    }

    /// Every day for one game at or before `day`, most recent first: the archive.
    ///
    /// Bounded by the day the caller asks about rather than by the end of the file, so
    /// a schedule published with days in it that have not arrived yet does not hand a
    /// player tomorrow's puzzle. Which is the normal case: days are published ahead.
    pub fn archive<'a>(&'a self, day: u32, family: &'a str) -> impl Iterator<Item = &'a Scheduled> {
        self.entries
            .iter()
            .rev()
            .filter(move |e| e.family == family && e.day <= day)
    }

    /// The days for one game after `day`, soonest first: what to download ahead.
    ///
    /// This is what the offline buffer fills from. It is the one place the schedule
    /// deliberately looks at days that have not arrived, because downloading ahead is
    /// the entire point of holding puzzles on a device.
    pub fn after<'a>(&'a self, day: u32, family: &'a str) -> impl Iterator<Item = &'a Scheduled> {
        self.entries
            .iter()
            .filter(move |e| e.family == family && e.day > day)
    }

    /// The last day the schedule covers for a game, if any.
    #[must_use]
    pub fn last_day(&self, family: &str) -> Option<u32> {
        self.entries
            .iter()
            .filter(|e| e.family == family)
            .map(|e| e.day)
            .max()
    }

    /// Every family this schedule mentions, in the order they first appear.
    #[must_use]
    pub fn families(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for entry in &self.entries {
            if !out.contains(&entry.family.as_str()) {
                out.push(&entry.family);
            }
        }
        out
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The canonical byte encoding, and the only one.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.serial.to_le_bytes());
        out.extend_from_slice(&self.start.year.to_le_bytes());
        out.push(self.start.month);
        out.push(self.start.day);
        let count = u32::try_from(self.entries.len()).map_err(|_| EncodeError::TooLong {
            field: "scheduled count",
            len: self.entries.len(),
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entry in &self.entries {
            out.extend_from_slice(&entry.day.to_le_bytes());
            put_str(&mut out, &entry.family, "scheduled family")?;
            out.extend_from_slice(&entry.content_id);
        }
        Ok(out)
    }
}

/// A schedule whose bytes have been vouched for.
///
/// The private field is load-bearing in the same way [`crate::package::Opened`]'s and
/// [`crate::withdrawal::OpenedList`]'s are: no code outside this module can build one,
/// so holding one *is* the proof that the signature was checked. A forged
/// `OpenedSchedule` would let anybody decide what every player plays today.
///
/// ```compile_fail
/// use gs_core::content::Digest;
/// use gs_core::schedule::{Date, OpenedSchedule, Schedule};
///
/// struct D;
/// impl Digest for D {
///     fn digest(&self, _bytes: &[u8]) -> [u8; 32] { [0; 32] }
/// }
///
/// // Every field is named, including the private one, and every value is well-typed.
/// // The error is that the field is private, which is the point.
/// let _forged = OpenedSchedule {
///     content_id: gs_core::content::ContentId::of(&D, b"anything"),
///     schedule: Schedule {
///         serial: u64::MAX,
///         start: Date::new(2026, 1, 1).unwrap(),
///         entries: Vec::new(),
///     },
///     key_id: "a key I chose".to_owned(),
///     checked_by_open: (),
/// };
/// ```
///
/// The same code without the final struct literal does compile, which is what says the
/// failure above is the private field and not a mistake in the setup:
///
/// ```
/// use gs_core::schedule::{Date, Schedule};
/// let _schedule = Schedule {
///     serial: u64::MAX,
///     start: Date::new(2026, 1, 1).unwrap(),
///     entries: Vec::new(),
/// };
/// ```
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedSchedule {
    /// The fingerprint of the body, recomputed here rather than taken on trust.
    pub content_id: ContentId,
    pub schedule: Schedule,
    /// Who signed it.
    pub key_id: String,
    checked_by_open: (),
}

/// What is digested, and so what the signature covers.
///
/// The magic is in it, which is what stops a signature over a package or a withdrawal
/// list from standing as a signature over a schedule. See this module's header.
fn digest_input(version: u16, body: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(MAGIC.len() + 2 + body.len());
    input.extend_from_slice(&MAGIC);
    input.extend_from_slice(&version.to_le_bytes());
    input.extend_from_slice(body);
    input
}

/// Why a schedule could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    Encode(EncodeError),
    /// The entries are not a calendar. Refused before signing, because a signature over
    /// a calendar with two answers for one day would be a valid signature over a
    /// question nobody can answer.
    NotACalendar(NotACalendar),
}

impl core::fmt::Display for SealError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SealError::Encode(e) => write!(f, "{e:?}"),
            SealError::NotACalendar(e) => write!(f, "{e}"),
        }
    }
}

/// Write a signed schedule.
pub fn seal_schedule<D: Digest, S: Signer>(
    schedule: &Schedule,
    digest: &D,
    signer: &S,
) -> Result<Vec<u8>, SealError> {
    schedule.check().map_err(SealError::NotACalendar)?;
    let body = schedule.canonical_bytes().map_err(SealError::Encode)?;
    let id = ContentId::of(digest, &digest_input(FORMAT_VERSION, &body));
    let signature = signer.sign(&id);

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    put_bytes32(&mut out, &body, "body").map_err(SealError::Encode)?;
    out.extend_from_slice(id.as_bytes());
    put_str(&mut out, signer.key_id(), "key id").map_err(SealError::Encode)?;
    put_bytes32(&mut out, &signature, "signature").map_err(SealError::Encode)?;
    Ok(out)
}

/// Read a schedule, checking before decoding.
///
/// The same order of operations [`crate::package::open`] and
/// [`crate::withdrawal::open_list`] follow, and for the same reason: framing, then the
/// digest, then the signature, and only then the contents.
pub fn open_schedule<D: Digest, V: SignatureVerifier>(
    bytes: &[u8],
    digest: &D,
    verifier: &V,
) -> Result<OpenedSchedule, OpenError> {
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
        .take(ID_LEN, "schedule id")?
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

    Ok(OpenedSchedule {
        content_id: computed,
        schedule: decode_schedule(body)?,
        key_id,
        checked_by_open: (),
    })
}

fn decode_schedule(body: &[u8]) -> Result<Schedule, OpenError> {
    let mut r = Reader::new(body);
    let serial = r.u64("serial")?;
    let year = r.u16("start year")?;
    let month = r.u8("start month")?;
    let day = r.u8("start day")?;
    let start = Date::new(year, month, day).ok_or(OpenError::Malformed { field: "start day" })?;
    let count = r.u32("scheduled count")?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let day = r.u32("scheduled day")?;
        let family = r.string("scheduled family")?;
        let content_id: [u8; ID_LEN] = r
            .take(ID_LEN, "scheduled content id")?
            .try_into()
            .expect("slice of exactly ID_LEN bytes");
        entries.push(Scheduled {
            day,
            family,
            content_id,
        });
    }
    let remaining = r.remaining();
    if remaining != 0 {
        return Err(OpenError::TrailingBytes(remaining));
    }

    let schedule = Schedule {
        serial,
        start,
        entries,
    };
    // The structural rule, checked on the way in as well as on the way out. A file
    // whose entries are out of order or name a day twice is refused rather than
    // repaired, so there is exactly one byte string per calendar.
    schedule.check().map_err(|_| OpenError::Malformed {
        field: "schedule order",
    })?;
    Ok(schedule)
}

/// What a device remembers so that an old schedule cannot be replayed at it.
///
/// The withdrawal list's `Accepted` has the same job. Kept separate from it because the
/// two files have separate serial numbers: a publisher who has withdrawn nothing this
/// month still publishes schedules, and tying the two numbers together would mean one
/// could hold the other back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighestSeen(Option<u64>);

impl HighestSeen {
    #[must_use]
    pub fn nothing_yet() -> HighestSeen {
        HighestSeen(None)
    }

    /// Remembering a serial this device has already accepted, read back from storage.
    #[must_use]
    pub fn of(serial: u64) -> HighestSeen {
        HighestSeen(Some(serial))
    }

    #[must_use]
    pub fn serial(self) -> Option<u64> {
        self.0
    }

    /// Whether a schedule is newer than what this device already has.
    ///
    /// At or below is refused, not just below. An equal serial carrying different
    /// entries is exactly the substitution the number exists to stop.
    #[must_use]
    pub fn accepts(self, serial: u64) -> bool {
        match self.0 {
            None => true,
            Some(seen) => serial > seen,
        }
    }

    /// Record a serial as accepted. Never moves backwards, whatever it is handed.
    pub fn accept(&mut self, serial: u64) {
        if self.accepts(serial) {
            self.0 = Some(serial);
        }
    }
}
