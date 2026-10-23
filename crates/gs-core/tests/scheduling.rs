//! What a schedule must refuse, and what the clock may and may not decide.
//!
//! A schedule says which puzzle every player gets today. If it can be forged, swapped
//! or replayed, then so can today. So as with the withdrawal list, most of these tests
//! are about what must *not* work.
//!
//! The container is written out by hand in [`wrap`] rather than by `seal_schedule`.
//! Using the module for both sides of a test only proves the module agrees with itself,
//! and the layout written down in `schedule.rs` could drift away from the layout
//! actually written. `wrap` and [`digest_input`] here are independent encoders of what
//! that file documents.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{OpenError, SignatureVerifier, Signer};
use gs_core::schedule::{
    open_schedule, seal_schedule, Date, HighestSeen, NotACalendar, Schedule, Scheduled, SealError,
    FORMAT_VERSION, MAGIC,
};

/// A stand-in digest, for tests only, and **not** a cryptographic hash.
///
/// FNV-1a over every byte, widened to 32, so that a change deep inside a long body is
/// noticed. It is trivially invertible and offers no collision resistance. What these
/// tests prove with it is the shape and the order of the checks, never the strength of
/// the hash. The real digest is SHA-256 in `gs-crypto`.
struct NotAHashButSensitiveToEveryByte;

impl Digest for NotAHashButSensitiveToEveryByte {
    fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for (i, chunk) in out.chunks_mut(8).enumerate() {
            let mixed = h
                .wrapping_add(u64::try_from(i).expect("small"))
                .wrapping_mul(0x9e37_79b9_7f4a_7c15);
            chunk.copy_from_slice(&mixed.to_le_bytes());
        }
        out
    }
}

/// A stand-in signer, for tests only, and **not** a signature scheme.
struct NotASignature {
    key_id: &'static str,
}

impl Signer for NotASignature {
    fn key_id(&self) -> &str {
        self.key_id
    }

    fn sign(&self, content_id: &ContentId) -> Vec<u8> {
        let mut out = b"not-a-signature:".to_vec();
        out.extend_from_slice(content_id.as_bytes());
        out
    }
}

struct TrustingOnly(&'static [&'static str]);

impl SignatureVerifier for TrustingOnly {
    fn verify(&self, content_id: &ContentId, key_id: &str, signature: &[u8]) -> bool {
        if !self.0.contains(&key_id) {
            return false;
        }
        let mut expected = b"not-a-signature:".to_vec();
        expected.extend_from_slice(content_id.as_bytes());
        signature == expected.as_slice()
    }
}

const KEY: &str = "test-key-1";

fn signer() -> NotASignature {
    NotASignature { key_id: KEY }
}

fn verifier() -> TrustingOnly {
    TrustingOnly(&["test-key-1"])
}

fn digest() -> NotAHashButSensitiveToEveryByte {
    NotAHashButSensitiveToEveryByte
}

fn an_id(n: u8) -> [u8; 32] {
    let mut id = [n; 32];
    id[0] = n.wrapping_add(1);
    id
}

fn start() -> Date {
    Date::new(2026, 10, 24).expect("a real day")
}

fn entry(day: u32, family: &str, n: u8) -> Scheduled {
    Scheduled {
        day,
        family: family.to_owned(),
        content_id: an_id(n),
    }
}

/// Two games over three days, in the order the format requires.
fn a_schedule() -> Schedule {
    Schedule {
        serial: 4,
        start: start(),
        entries: vec![
            entry(0, "nonogram", 1),
            entry(0, "slitherlink", 2),
            entry(1, "nonogram", 3),
            entry(1, "slitherlink", 4),
            entry(2, "nonogram", 5),
        ],
    }
}

/// What the signature covers, written out by hand from what `schedule.rs` documents.
fn digest_input(version: u16, body: &[u8]) -> Vec<u8> {
    let mut input = Vec::new();
    input.extend_from_slice(&MAGIC);
    input.extend_from_slice(&version.to_le_bytes());
    input.extend_from_slice(body);
    input
}

/// The container, written out by hand from what `schedule.rs` documents.
fn wrap(body: &[u8], version: u16, content_id: &[u8; 32], key_id: &str, sig: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&u32::try_from(body.len()).expect("short body").to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(content_id);
    out.extend_from_slice(
        &u16::try_from(key_id.len())
            .expect("short key id")
            .to_le_bytes(),
    );
    out.extend_from_slice(key_id.as_bytes());
    out.extend_from_slice(
        &u32::try_from(sig.len())
            .expect("short signature")
            .to_le_bytes(),
    );
    out.extend_from_slice(sig);
    out
}

/// Seal a body by hand, so a test can seal something `seal_schedule` would refuse.
fn wrap_signed(body: &[u8]) -> Vec<u8> {
    let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION, body));
    let sig = signer().sign(&id);
    wrap(body, FORMAT_VERSION, id.as_bytes(), KEY, &sig)
}

#[test]
fn a_sealed_schedule_opens_to_what_went_in() {
    let schedule = a_schedule();
    let bytes = seal_schedule(&schedule, &digest(), &signer()).expect("sealed");
    let opened = open_schedule(&bytes, &digest(), &verifier()).expect("opened");
    assert_eq!(opened.schedule, schedule);
    assert_eq!(opened.key_id, KEY);
}

/// The test that makes the signature mean something. Every single bit of the file is
/// flipped, one at a time, and every one of them must be refused.
#[test]
fn every_single_bit_matters() {
    let bytes = seal_schedule(&a_schedule(), &digest(), &signer()).expect("sealed");
    for byte in 0..bytes.len() {
        for bit in 0..8u32 {
            let mut altered = bytes.clone();
            altered[byte] ^= 1 << bit;
            assert!(
                open_schedule(&altered, &digest(), &verifier()).is_err(),
                "a schedule with bit {bit} of byte {byte} flipped was accepted"
            );
        }
    }
}

/// The reason the magic bytes are inside what the signature covers.
///
/// A publisher's signature over a withdrawal list is a real signature from a trusted
/// key. If the two files digested the same way, that signature could be lifted onto a
/// schedule — and whoever lifted it would choose what every player plays today, with no
/// key at all.
#[test]
fn a_signature_over_a_withdrawal_list_can_never_stand_for_a_schedule() {
    let body = a_schedule().canonical_bytes().expect("encodes");

    // The id a withdrawal list would have computed over these same body bytes: its own
    // magic, then the version, then the body.
    let mut as_a_list = Vec::new();
    as_a_list.extend_from_slice(&gs_core::withdrawal::MAGIC);
    as_a_list.extend_from_slice(&gs_core::withdrawal::FORMAT_VERSION.to_le_bytes());
    as_a_list.extend_from_slice(&body);
    let list_id = ContentId::of(&digest(), &as_a_list);
    let lifted = signer().sign(&list_id);

    let bytes = wrap(&body, FORMAT_VERSION, list_id.as_bytes(), KEY, &lifted);
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::ContentIdMismatch {
            stated: *list_id.as_bytes(),
            computed: ContentId::of(&digest(), &digest_input(FORMAT_VERSION, &body)),
        }),
        "a signature made over a withdrawal list was accepted as a schedule"
    );
}

/// The same, the other way round: a package's signature is not a schedule's either.
#[test]
fn a_signature_over_a_puzzle_can_never_stand_for_a_schedule() {
    let body = a_schedule().canonical_bytes().expect("encodes");

    // A package digests two version bytes and then its body, with no magic.
    let mut as_a_package = Vec::new();
    as_a_package.extend_from_slice(&1u16.to_le_bytes());
    as_a_package.extend_from_slice(&body);
    let package_id = ContentId::of(&digest(), &as_a_package);
    let lifted = signer().sign(&package_id);

    let bytes = wrap(&body, FORMAT_VERSION, package_id.as_bytes(), KEY, &lifted);
    assert!(
        matches!(
            open_schedule(&bytes, &digest(), &verifier()),
            Err(OpenError::ContentIdMismatch { .. })
        ),
        "a signature made over a package was accepted as a schedule"
    );
}

#[test]
fn a_schedule_from_an_untrusted_key_is_refused() {
    let other = NotASignature {
        key_id: "somebody-elses-key",
    };
    let bytes = seal_schedule(&a_schedule(), &digest(), &other).expect("sealed");
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::SignatureRejected)
    );
}

#[test]
fn a_version_this_reader_does_not_know_is_refused_rather_than_guessed_at() {
    let body = a_schedule().canonical_bytes().expect("encodes");
    let id = ContentId::of(&digest(), &digest_input(FORMAT_VERSION + 1, &body));
    let sig = signer().sign(&id);
    let bytes = wrap(&body, FORMAT_VERSION + 1, id.as_bytes(), KEY, &sig);
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::UnsupportedFormat(FORMAT_VERSION + 1))
    );
}

#[test]
fn bytes_after_the_end_are_refused() {
    let mut bytes = seal_schedule(&a_schedule(), &digest(), &signer()).expect("sealed");
    bytes.push(0);
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::TrailingBytes(1))
    );
}

/// Two puzzles for one game on one day has no answer, so it is refused on the way in.
#[test]
fn sealing_two_puzzles_for_one_game_on_one_day_is_refused() {
    let schedule = Schedule {
        serial: 1,
        start: start(),
        entries: vec![entry(3, "nonogram", 1), entry(3, "nonogram", 2)],
    };
    assert_eq!(
        seal_schedule(&schedule, &digest(), &signer()),
        Err(SealError::NotACalendar(NotACalendar::TwoForOneDay {
            day: 3
        }))
    );
}

#[test]
fn sealing_days_out_of_order_is_refused() {
    let schedule = Schedule {
        serial: 1,
        start: start(),
        entries: vec![entry(4, "nonogram", 1), entry(2, "nonogram", 2)],
    };
    assert_eq!(
        seal_schedule(&schedule, &digest(), &signer()),
        Err(SealError::NotACalendar(NotACalendar::OutOfOrder))
    );
}

/// And the same two rules on the way *out*, which is the half that matters.
///
/// `seal_schedule` refusing them only binds this code. A file written by anything else
/// — a future tool, a different version, somebody by hand — must be refused too, or a
/// device could end up holding a calendar with two answers for one day and silently
/// pick whichever came first.
#[test]
fn a_file_naming_a_day_twice_is_refused_even_when_properly_signed() {
    let unsorted = Schedule {
        serial: 1,
        start: start(),
        entries: vec![entry(3, "nonogram", 1), entry(3, "nonogram", 2)],
    };
    let bytes = wrap_signed(&unsorted.canonical_bytes().expect("encodes"));
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::Malformed {
            field: "schedule order"
        })
    );
}

#[test]
fn a_file_with_days_out_of_order_is_refused_even_when_properly_signed() {
    let unsorted = Schedule {
        serial: 1,
        start: start(),
        entries: vec![entry(9, "nonogram", 1), entry(1, "nonogram", 2)],
    };
    let bytes = wrap_signed(&unsorted.canonical_bytes().expect("encodes"));
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::Malformed {
            field: "schedule order"
        })
    );
}

#[test]
fn a_start_day_that_is_not_a_real_day_is_refused() {
    // The 30th of February, written straight into the body, bypassing `Date::new`.
    let mut body = Vec::new();
    body.extend_from_slice(&1u64.to_le_bytes());
    body.extend_from_slice(&2026u16.to_le_bytes());
    body.push(2);
    body.push(30);
    body.extend_from_slice(&0u32.to_le_bytes());
    let bytes = wrap_signed(&body);
    assert_eq!(
        open_schedule(&bytes, &digest(), &verifier()),
        Err(OpenError::Malformed { field: "start day" })
    );
}

#[test]
fn the_same_calendar_always_encodes_to_the_same_bytes() {
    let once = seal_schedule(&a_schedule(), &digest(), &signer()).expect("sealed");
    let twice = seal_schedule(&a_schedule(), &digest(), &signer()).expect("sealed");
    assert_eq!(once, twice);
}

// ---------------------------------------------------------------------------
// Looking things up
// ---------------------------------------------------------------------------

#[test]
fn a_day_names_one_puzzle_per_game() {
    let schedule = a_schedule();
    assert_eq!(
        schedule.on_day(1, "nonogram").map(|e| e.content_id),
        Some(an_id(3))
    );
    assert_eq!(
        schedule.on_day(1, "slitherlink").map(|e| e.content_id),
        Some(an_id(4))
    );
    assert_eq!(schedule.on_day(2, "slitherlink"), None);
    assert_eq!(schedule.on_day(99, "nonogram"), None);
}

/// The archive is today and the days before it, and never tomorrow.
///
/// Days are published ahead, so a schedule almost always contains days that have not
/// arrived. An archive that listed them would hand a player tomorrow's puzzle today,
/// which for a daily puzzle is the whole thing spoiled.
#[test]
fn the_archive_never_includes_a_day_that_has_not_arrived() {
    let schedule = a_schedule();
    let days: Vec<u32> = schedule.archive(1, "nonogram").map(|e| e.day).collect();
    assert_eq!(
        days,
        vec![1, 0],
        "most recent first, and nothing after day 1"
    );

    let all: Vec<u32> = schedule.archive(99, "nonogram").map(|e| e.day).collect();
    assert_eq!(all, vec![2, 1, 0]);
}

#[test]
fn what_to_download_ahead_is_the_days_after_today() {
    let schedule = a_schedule();
    let days: Vec<u32> = schedule.after(0, "nonogram").map(|e| e.day).collect();
    assert_eq!(days, vec![1, 2], "soonest first");
    assert!(schedule.after(2, "nonogram").next().is_none());
}

#[test]
fn the_last_day_and_the_families_are_reported_per_game() {
    let schedule = a_schedule();
    assert_eq!(schedule.last_day("nonogram"), Some(2));
    assert_eq!(schedule.last_day("slitherlink"), Some(1));
    assert_eq!(schedule.last_day("sudoku"), None);
    assert_eq!(schedule.families(), vec!["nonogram", "slitherlink"]);
}

// ---------------------------------------------------------------------------
// The serial number
// ---------------------------------------------------------------------------

/// The attack the serial number exists to stop, and it needs no key.
///
/// Keep serving last month's genuine, correctly signed schedule. Every signature checks
/// out, and the player never sees a new day — and a day whose puzzle was replaced comes
/// back.
#[test]
fn an_old_schedule_served_again_is_refused() {
    let mut seen = HighestSeen::nothing_yet();
    assert!(seen.accepts(4));
    seen.accept(4);

    assert!(!seen.accepts(3), "an older schedule was accepted");
    assert!(!seen.accepts(4), "the same serial again was accepted");
    assert!(seen.accepts(5));
}

/// At or below, not just below.
///
/// An equal serial carrying different entries is exactly the substitution the number
/// exists to stop: it is the one case where two files both look current.
#[test]
fn the_same_serial_with_different_days_is_still_refused() {
    let mut seen = HighestSeen::of(7);
    assert!(!seen.accepts(7));
    seen.accept(7);
    assert_eq!(seen.serial(), Some(7), "the remembered serial moved");
}

#[test]
fn remembering_never_moves_backwards() {
    let mut seen = HighestSeen::of(10);
    seen.accept(2);
    assert_eq!(seen.serial(), Some(10));
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

#[test]
fn a_day_that_does_not_exist_is_not_a_date() {
    assert!(Date::new(2026, 2, 30).is_none());
    assert!(Date::new(2026, 13, 1).is_none());
    assert!(Date::new(2026, 0, 1).is_none());
    assert!(Date::new(2026, 1, 0).is_none());
    assert!(Date::new(2026, 1, 32).is_none());
    assert!(Date::new(2026, 4, 31).is_none());
}

#[test]
fn leap_days_exist_in_the_years_they_should() {
    assert!(Date::new(2024, 2, 29).is_some(), "2024 is a leap year");
    assert!(Date::new(2026, 2, 29).is_none(), "2026 is not");
    assert!(Date::new(2000, 2, 29).is_some(), "2000 is, being 400s");
    assert!(Date::new(1900, 2, 29).is_none(), "1900 is not, being 100s");
}

#[test]
fn day_numbers_count_forwards_from_the_start_day() {
    let s = Date::new(2026, 10, 24).expect("a real day");
    assert_eq!(s.day_number_from(s), Some(0), "the start day is day zero");
    assert_eq!(
        Date::new(2026, 10, 25).expect("real").day_number_from(s),
        Some(1)
    );
    assert_eq!(
        Date::new(2026, 11, 1).expect("real").day_number_from(s),
        Some(8),
        "across the end of a month"
    );
    assert_eq!(
        Date::new(2027, 10, 24).expect("real").day_number_from(s),
        Some(365),
        "a whole year, with no leap day in it"
    );
}

/// A clock set before the start day gets no puzzle, rather than day zero's.
#[test]
fn a_date_before_the_start_day_has_no_day_number() {
    let s = start();
    assert_eq!(
        Date::new(2026, 10, 23).expect("real").day_number_from(s),
        None
    );
    assert_eq!(
        Date::new(1970, 1, 1).expect("real").day_number_from(s),
        None
    );
}

/// Counting forward and counting back must agree, every day for eight years.
///
/// Eight years covers two leap years and a century's worth of month lengths, and a
/// calendar that disagrees with itself anywhere in that range would show the wrong
/// day's puzzle on that date for everybody at once.
#[test]
fn counting_forwards_and_back_agree_on_every_day_for_eight_years() {
    let s = Date::new(2024, 1, 1).expect("a real day");
    for n in 0..(8 * 366) {
        let date = s.plus_days(n);
        assert_eq!(
            date.day_number_from(s),
            Some(n),
            "{date} came back as the wrong day number"
        );
    }
}

/// And across the leap-year rule's two awkward cases.
#[test]
fn counting_agrees_across_the_century_rules() {
    for s in [
        Date::new(1899, 12, 31).expect("real"),
        Date::new(1999, 12, 31).expect("real"),
    ] {
        for n in 0..800 {
            let date = s.plus_days(n);
            assert_eq!(date.day_number_from(s), Some(n), "{date}");
        }
    }
}

#[test]
fn a_date_is_written_the_way_people_write_one() {
    assert_eq!(
        Date::new(2026, 10, 24).expect("real").to_string(),
        "2026-10-24"
    );
    assert_eq!(
        Date::new(2026, 1, 2).expect("real").to_string(),
        "2026-01-02"
    );
}
