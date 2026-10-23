//! The schedule, as a web page sees it.
//!
//! The page asks three questions and works nothing out for itself: what is today's
//! puzzle, what were the days before it, and what should be downloaded ahead. Every
//! answer comes from a signed schedule, and the clock is used for exactly one thing —
//! turning today's date into a day number.
//!
//! # What the clock can and cannot do
//!
//! A device's clock is the easiest thing on it to change, so it decides nothing about
//! trust. Set it wrong and a player sees the wrong day's puzzle, which is the whole
//! blast radius. It cannot make a bad puzzle look good, cannot revive a withdrawn one,
//! and cannot invent a day nobody signed, because the only thing it does is pick a
//! number to look up in a file that was signed before it was ever read.
//!
//! The page passes the date in rather than this code reading one. Nothing in this
//! workspace reads a clock, and that stays true here.
//!
//! # Where the schedule is kept, and why it is checked every time
//!
//! In the browser's own storage, so the daily puzzle and the archive work with no
//! signal. That storage is a few clicks from being edited by hand, so the schedule is
//! re-opened through its signature **every time it is read**, exactly as a saved puzzle
//! is. A schedule held in memory after one check would be a schedule that was checked
//! once; this one is checked on every question.

use gs_core::schedule::{open_schedule, Date, HighestSeen, OpenedSchedule};
use gs_offline::shelf::Shelf;
use wasm_bindgen::prelude::*;

use crate::browser::Page;
use crate::saved::LocalStorage;

/// The name the schedule is kept under.
pub(crate) const SCHEDULE: &str = "schedule";

/// One day on the calendar, for a page to show.
///
/// Carries the date already worked out, so the page never does calendar arithmetic. A
/// page that computed a date itself would be a second answer to "which day is this".
#[wasm_bindgen]
pub struct Day {
    date: String,
    name: String,
}

#[wasm_bindgen]
impl Day {
    /// The day, written the way a person writes one.
    #[must_use]
    pub fn date(&self) -> String {
        self.date.clone()
    }

    /// The puzzle's name, which is its own fingerprint: what to fetch.
    #[must_use]
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

/// What taking a schedule did.
#[wasm_bindgen]
pub struct Calendar {
    serial: u64,
    days: usize,
}

#[wasm_bindgen]
impl Calendar {
    /// The schedule's number.
    #[must_use]
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// How many days it names, across every game.
    #[must_use]
    pub fn days(&self) -> usize {
        self.days
    }

    #[must_use]
    pub fn in_words(&self) -> String {
        match self.days {
            0 => "The schedule names no days yet.".to_owned(),
            1 => "The schedule names 1 day.".to_owned(),
            n => format!("The schedule names {n} days."),
        }
    }
}

#[wasm_bindgen]
impl Page {
    /// Take a signed schedule.
    ///
    /// Checked against the same trusted keys a puzzle is, and refused unless it is newer
    /// than the one this device already has. That refusal is the point: replaying last
    /// month's genuine, correctly signed schedule needs no key, and it would hide every
    /// day since — including a day whose puzzle was replaced because the first one was
    /// wrong.
    pub fn accept_schedule(&self, bytes: &[u8]) -> Result<Calendar, JsError> {
        let mut shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        let opened = open_schedule(bytes, self.digest(), self.keys())
            .map_err(|why| JsError::new(&format!("that schedule was refused: {why:?}")))?;

        let held = self.schedule_seen()?;
        if !HighestSeen::of(held).accepts(opened.schedule.serial) && held != 0 {
            return Err(JsError::new(&format!(
                "this device already has schedule number {held}, so number {} is not \
                 newer and has been refused.",
                opened.schedule.serial
            )));
        }

        // Stored only after it has been opened and found newer, so a refused schedule
        // never replaces a good one.
        shelf
            .put(SCHEDULE, bytes)
            .map_err(|why| JsError::new(&why.to_string()))?;

        Ok(Calendar {
            serial: opened.schedule.serial,
            days: opened.schedule.len(),
        })
    }

    /// The schedule number this device is going by, or zero if it has none.
    pub fn schedule_seen(&self) -> Result<u64, JsError> {
        match self.calendar()? {
            None => Ok(0),
            Some(opened) => Ok(opened.schedule.serial),
        }
    }

    /// Whether this device has a schedule at all.
    ///
    /// A page with no schedule has nothing to open, which is a different thing from a
    /// schedule that names no days today.
    pub fn has_schedule(&self) -> Result<bool, JsError> {
        Ok(self.calendar()?.is_some())
    }

    /// The name of today's puzzle for one game, or nothing.
    ///
    /// Nothing means one of three things, and the page says so rather than guessing:
    /// there is no schedule, the date is before the schedule begins, or that day has no
    /// puzzle for that game.
    pub fn todays_puzzle(
        &self,
        year: u16,
        month: u8,
        day: u8,
        family: &str,
    ) -> Result<Option<String>, JsError> {
        let Some(opened) = self.calendar()? else {
            return Ok(None);
        };
        let Some(number) = Self::day_number(&opened, year, month, day)? else {
            return Ok(None);
        };
        Ok(opened
            .schedule
            .on_day(number, family)
            .map(|entry| to_hex(&entry.content_id)))
    }

    /// Today and the days before it for one game, most recent first.
    ///
    /// Never a day that has not arrived. Days are published ahead, so a schedule almost
    /// always holds days in the future; listing them would hand a player tomorrow's
    /// puzzle today, which for a daily puzzle spoils the only thing it has.
    pub fn archive(
        &self,
        year: u16,
        month: u8,
        day: u8,
        family: &str,
    ) -> Result<Vec<Day>, JsError> {
        let Some(opened) = self.calendar()? else {
            return Ok(Vec::new());
        };
        let Some(number) = Self::day_number(&opened, year, month, day)? else {
            return Ok(Vec::new());
        };
        Ok(opened
            .schedule
            .archive(number, family)
            .map(|entry| Day {
                date: opened.schedule.start.plus_days(entry.day).to_string(),
                name: to_hex(&entry.content_id),
            })
            .collect())
    }

    /// The days after today for one game, soonest first: what to download ahead.
    ///
    /// The one place a future day is deliberately handed out, because downloading ahead
    /// is the whole point of keeping puzzles on a device. They are saved, not shown: the
    /// buffer holds them and `archive` still refuses to list them until their day comes.
    pub fn to_download(
        &self,
        year: u16,
        month: u8,
        day: u8,
        family: &str,
        most: usize,
    ) -> Result<Vec<Day>, JsError> {
        let Some(opened) = self.calendar()? else {
            return Ok(Vec::new());
        };
        let Some(number) = Self::day_number(&opened, year, month, day)? else {
            return Ok(Vec::new());
        };
        Ok(opened
            .schedule
            .after(number, family)
            .take(most)
            .map(|entry| Day {
                date: opened.schedule.start.plus_days(entry.day).to_string(),
                name: to_hex(&entry.content_id),
            })
            .collect())
    }

    /// Every game this schedule covers.
    pub fn scheduled_games(&self) -> Result<Vec<String>, JsError> {
        let Some(opened) = self.calendar()? else {
            return Ok(Vec::new());
        };
        Ok(opened
            .schedule
            .families()
            .into_iter()
            .map(str::to_owned)
            .collect())
    }
}

impl Page {
    /// The schedule this device is going by, read back through its signature.
    ///
    /// Read fresh on every call. Browser storage can be edited by hand, so a copy kept
    /// after one check is a copy that was checked once.
    fn calendar(&self) -> Result<Option<OpenedSchedule>, JsError> {
        let shelf = LocalStorage::open().map_err(|why| JsError::new(&why.to_string()))?;
        let Some(bytes) = shelf
            .get(SCHEDULE)
            .map_err(|why| JsError::new(&why.to_string()))?
        else {
            return Ok(None);
        };
        // Anything that fails its checks is treated as no schedule rather than as an
        // error, and is left where it is. Reading never deletes: the same rule the
        // buffer follows, for the same reason — a device with no signal and a
        // misbehaving store would otherwise be left with nothing at all.
        Ok(open_schedule(&bytes, self.digest(), self.keys()).ok())
    }

    /// Today's day number, or nothing when the date is before the schedule starts.
    fn day_number(
        opened: &OpenedSchedule,
        year: u16,
        month: u8,
        day: u8,
    ) -> Result<Option<u32>, JsError> {
        let date = Date::new(year, month, day)
            .ok_or_else(|| JsError::new(&format!("{year}-{month}-{day} is not a real day")))?;
        Ok(date.day_number_from(opened.schedule.start))
    }
}

fn to_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("a nibble is a hex digit"));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("a nibble is a hex digit"));
    }
    out
}
