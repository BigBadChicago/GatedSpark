//! What the browser must refuse, and what it must accept.
//!
//! These use real SHA-256 and real Ed25519 from `gs-crypto`, and real packages built by
//! the same `publish` the pipeline uses. A test that built a package by hand would be
//! checking the browser against a file format of its own invention.
//!
//! The point being proved is narrow and worth stating plainly: **a browser decides for
//! itself whether to believe a puzzle.** Nothing here asks a server anything.

use gs_core::content::ContentId;
use gs_core::package::{seal, OpenError, RecordedReplay};
use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::clues::DecodeError;
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::generate::Candidate;
use gs_nonogram::publish::{manifest_for, publish, report_for, Published};
use gs_nonogram::rules::{FAMILY, RULESET_VERSION};
use gs_nonogram::solve::Budget;
use gs_web::{load, NotPlayable};

/// A private key printed in a public repository, which is all anybody needs to know
/// about it. Here so these tests use real Ed25519 rather than a stand-in.
const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
/// A second one, standing in for somebody else entirely.
const SOMEBODY_ELSES_PRIVATE_KEY: [u8; 32] = [
    0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e, 0x0f,
    0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed, 0x4f, 0xb8, 0xa6, 0xfb,
];
const KEY_ID: &str = "the-page-trusts-this-one";

fn our_signer() -> Ed25519Signer {
    Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY)
}

/// The key list a page ships with: ours, and nobody else's.
fn the_page_trusts_us() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &our_signer().public_key())
        .expect("our test key is valid and the list starts empty");
    keys
}

/// A solver that answers the way one would for a puzzle with exactly one answer.
///
/// Scripted by call order rather than by reading the query, exactly as `gs-store`'s
/// tests are: the query text is the encoder's business, and a test that guessed at its
/// shape would break whenever the encoding changed for reasons having nothing to do
/// with the browser.
struct AgreesWithAUniquePuzzle {
    asked: std::cell::RefCell<usize>,
}

impl Verifier for AgreesWithAUniquePuzzle {
    fn name(&self) -> &'static str {
        "a solver that agrees the puzzle is unique"
    }

    fn ask(&self, _query: &str) -> Outcome {
        let mut asked = self.asked.borrow_mut();
        let answer = if *asked == 0 {
            Outcome::Sat
        } else {
            Outcome::Unsat
        };
        *asked += 1;
        answer
    }
}

fn agreeing_solver() -> AgreesWithAUniquePuzzle {
    AgreesWithAUniquePuzzle {
        asked: std::cell::RefCell::new(0),
    }
}

/// The first seed that produces a candidate at the gentlest level.
///
/// Searching rather than naming a seed, because not every seed yields a puzzle a given
/// level claims, and a hard-coded seed would fail whenever generation changed.
fn a_candidate() -> Candidate {
    (1u64..3_000)
        .find_map(|seed| propose_in_band(&BANDS[0], seed, Budget::default()))
        .expect("some seed in the first few thousand produces a gentle candidate")
}

/// A real published package, signed by the key the page trusts.
fn a_real_puzzle() -> Published {
    publish(
        &a_candidate(),
        &agreeing_solver(),
        Budget::default(),
        &Sha256,
        &our_signer(),
    )
    .expect("a uniquely solvable candidate with an agreeing solver publishes")
}

// ---------------------------------------------------------------------------
// The control
// ---------------------------------------------------------------------------

#[test]
fn a_real_puzzle_loads_and_knows_what_it_is() {
    // Without this, every refusal below could pass because nothing is ever accepted.
    let published = a_real_puzzle();
    let playable = load(&published.package, &Sha256, &the_page_trusts_us())
        .expect("a real, correctly signed puzzle loads");

    assert_eq!(playable.signed_by, KEY_ID);
    assert_eq!(playable.band.as_deref(), Some(BANDS[0].name));
    assert!(
        !playable.encoding.is_empty(),
        "the encoding the uniqueness claim is relative to must come through, because a \
         page that said \"one answer only\" without it would claim more than was proved"
    );

    // The board is real and the two sets of clues match its shape.
    let dims = playable.dims();
    assert!(dims.width > 0 && dims.height > 0);
    assert_eq!(playable.row_clues().len(), dims.height as usize);
    assert_eq!(playable.column_clues().len(), dims.width as usize);

    // Loading the same bytes twice gives the same thing. The browser has no state that
    // could make a second reading differ from the first.
    let again = load(&published.package, &Sha256, &the_page_trusts_us()).expect("loads again");
    assert_eq!(playable, again);
}

#[test]
fn the_fingerprint_is_recomputed_rather_than_believed() {
    // The page's copy of the fingerprint has to come from digesting the bytes, not from
    // reading the number the file states. Checked by comparing against the one `publish`
    // computed on the other side of the pipeline, which never travelled in the file.
    let published = a_real_puzzle();
    let playable = load(&published.package, &Sha256, &the_page_trusts_us()).expect("loads");
    assert_eq!(
        Some(playable.content_id),
        published.report.content_id,
        "the fingerprint the browser computed must equal the one the publisher did"
    );
}

// ---------------------------------------------------------------------------
// What the browser refuses
// ---------------------------------------------------------------------------

#[test]
fn a_puzzle_signed_by_somebody_else_is_refused() {
    // The claim the whole design rests on. Anyone can sign their own file with their
    // own key and the signature will be perfectly valid; it is the key list, shipped
    // with the page, that keeps a stranger's puzzle out.
    let theirs = publish(
        &a_candidate(),
        &agreeing_solver(),
        Budget::default(),
        &Sha256,
        &Ed25519Signer::new("somebody-elses-key", &SOMEBODY_ELSES_PRIVATE_KEY),
    )
    .expect("they can publish perfectly well, with their own key");

    match load(&theirs.package, &Sha256, &the_page_trusts_us()) {
        Err(NotPlayable::NotAValidPackage(_)) => {}
        other => panic!("a stranger's puzzle must be refused, got {other:?}"),
    }
}

#[test]
fn a_page_that_trusts_nothing_accepts_nothing() {
    // The starting state of a page. If an empty key list accepted anything, a page that
    // forgot to install its key would be a page that accepted every puzzle on earth
    // while appearing to work.
    let published = a_real_puzzle();
    match load(&published.package, &Sha256, &TrustedKeys::none()) {
        Err(NotPlayable::NotAValidPackage(_)) => {}
        other => panic!("an empty key list must accept nothing, got {other:?}"),
    }
}

#[test]
fn every_single_changed_byte_is_refused() {
    // Not a sample. Every byte of a real package is replaced in turn, and the browser
    // must refuse all of them. This is what makes "the browser checks for itself" a
    // measurement rather than a description.
    let published = a_real_puzzle();
    let keys = the_page_trusts_us();
    let mut refused = 0usize;
    for i in 0..published.package.len() {
        let mut damaged = published.package.clone();
        damaged[i] = damaged[i].wrapping_add(1);
        assert!(
            load(&damaged, &Sha256, &keys).is_err(),
            "byte {i} was changed and the puzzle still loaded"
        );
        refused += 1;
    }
    assert_eq!(
        refused,
        published.package.len(),
        "every byte must have been tried"
    );
    assert!(refused > 100, "a package this small is suspicious");
}

#[test]
fn a_truncated_download_is_refused_rather_than_half_drawn() {
    let published = a_real_puzzle();
    let keys = the_page_trusts_us();
    for cut in [
        0,
        1,
        8,
        published.package.len() / 2,
        published.package.len() - 1,
    ] {
        assert!(
            load(&published.package[..cut], &Sha256, &keys).is_err(),
            "a package cut to {cut} bytes must be refused"
        );
    }
}

#[test]
fn another_games_puzzle_is_refused_rather_than_attempted() {
    // A reader cannot judge the rules of a game it does not have. The package is
    // genuinely and correctly signed, which is the point: the signature is not what is
    // wrong with it.
    //
    // The name used to be "slitherlink", until the page learnt to play Slitherlink and
    // this test started passing for the wrong reason. A game nobody has written is the
    // only name that stays a stranger.
    let candidate = a_candidate();
    let report =
        report_for(&candidate, &agreeing_solver(), Budget::default()).expect("can be asked about");
    let mut manifest = manifest_for(&candidate, &report);
    manifest.family = "a-game-nobody-has-written".to_owned();
    let sealed = seal(&manifest, &Sha256, &our_signer()).expect("seals");

    match load(&sealed.bytes, &Sha256, &the_page_trusts_us()) {
        Err(NotPlayable::WrongFamily { package_says }) => {
            assert_eq!(package_says, "a-game-nobody-has-written");
        }
        other => panic!("another game's puzzle must be refused, got {other:?}"),
    }
}

#[test]
fn a_puzzle_from_different_rules_is_refused_in_either_direction() {
    // A replay against different semantics is not a replay, and every claim in the
    // report was made about rules this build may no longer have. Both directions,
    // because "newer" and "older" are equally unjudgeable.
    for their_version in [RULESET_VERSION - 1, RULESET_VERSION + 1] {
        let candidate = a_candidate();
        let report = report_for(&candidate, &agreeing_solver(), Budget::default())
            .expect("can be asked about");
        let mut manifest = manifest_for(&candidate, &report);
        manifest.ruleset_version = their_version;
        let sealed = seal(&manifest, &Sha256, &our_signer()).expect("seals");

        match load(&sealed.bytes, &Sha256, &the_page_trusts_us()) {
            Err(NotPlayable::WrongRulesVersion {
                package_says,
                we_have,
            }) => {
                assert_eq!(package_says, their_version);
                assert_eq!(we_have, RULESET_VERSION);
            }
            other => panic!("rules version {their_version} must be refused, got {other:?}"),
        }
    }
}

#[test]
fn the_version_this_build_implements_is_the_one_it_accepts() {
    // Anti-vacuity for the test above. If `RULESET_VERSION` ever stopped matching what
    // `publish` writes into a package, the two refusals above would still pass while
    // the accepted case had quietly become impossible.
    let published = a_real_puzzle();
    let playable = load(&published.package, &Sha256, &the_page_trusts_us())
        .expect("a package this build published is one this build accepts");
    assert_eq!(
        playable
            .nonogram()
            .expect("a Nonogram package loads as a Nonogram")
            .dims,
        playable.dims()
    );
    assert_eq!(FAMILY.0, "nonogram");
}

#[test]
fn a_signed_package_whose_board_does_not_read_is_a_refusal_not_a_guess() {
    // Reaching this means a correctly signed package disagrees with this build's own
    // reader, which is a bug rather than a damaged download — the signature already
    // passed. It still must not be drawn.
    let candidate = a_candidate();
    let report =
        report_for(&candidate, &agreeing_solver(), Budget::default()).expect("can be asked about");
    let mut manifest = manifest_for(&candidate, &report);
    manifest.payload = vec![0xff; 7];
    let sealed = seal(&manifest, &Sha256, &our_signer()).expect("seals");

    match load(&sealed.bytes, &Sha256, &the_page_trusts_us()) {
        Err(NotPlayable::UnreadablePuzzle(_)) => {}
        other => panic!("an unreadable board must be refused, got {other:?}"),
    }
}

#[test]
fn nothing_that_is_not_a_package_loads() {
    let keys = the_page_trusts_us();
    for (what, bytes) in [
        ("nothing at all", vec![]),
        ("a web page", b"<!doctype html>".to_vec()),
        ("some JSON", br#"{"family":"nonogram"}"#.to_vec()),
        ("zeroes", vec![0u8; 512]),
    ] {
        assert!(
            load(&bytes, &Sha256, &keys).is_err(),
            "{what} must not load as a puzzle"
        );
    }
}

// ---------------------------------------------------------------------------
// What a refusal says
// ---------------------------------------------------------------------------

#[test]
fn every_refusal_explains_itself_in_words_a_person_can_read() {
    // These strings reach a browser console, and sometimes a person. A refusal nobody
    // can act on is most of the way to a refusal nobody notices.
    let candidate = a_candidate();
    let report =
        report_for(&candidate, &agreeing_solver(), Budget::default()).expect("can be asked about");

    let mut wrong_family = manifest_for(&candidate, &report);
    wrong_family.family = "a-game-nobody-has-written".to_owned();
    let mut wrong_version = manifest_for(&candidate, &report);
    wrong_version.ruleset_version = RULESET_VERSION + 1;
    let mut unreadable = manifest_for(&candidate, &report);
    unreadable.payload = vec![0xff; 7];

    let cases = [
        (
            load(b"not a package", &Sha256, &the_page_trusts_us()),
            "is not a GatedSpark puzzle",
        ),
        (
            load(
                &seal(&wrong_family, &Sha256, &our_signer())
                    .expect("seals")
                    .bytes,
                &Sha256,
                &the_page_trusts_us(),
            ),
            "a-game-nobody-has-written",
        ),
        (
            load(
                &seal(&wrong_version, &Sha256, &our_signer())
                    .expect("seals")
                    .bytes,
                &Sha256,
                &the_page_trusts_us(),
            ),
            "Refusing it rather than guessing",
        ),
        (
            load(
                &seal(&unreadable, &Sha256, &our_signer())
                    .expect("seals")
                    .bytes,
                &Sha256,
                &the_page_trusts_us(),
            ),
            "a fault in this program",
        ),
    ];

    for (result, expected) in cases {
        let why = result.expect_err("this case must be a refusal").to_string();
        assert!(
            why.contains(expected),
            "a refusal should mention {expected:?}, got: {why}"
        );
        assert!(
            reads_as_words(&why),
            "a person should not be shown code: {why}"
        );
    }
}

/// True when a string looks like a sentence rather than like source code.
///
/// The test this serves used to check only that the words "NotPlayable" were absent,
/// and it passed on the message `this is not a puzzle this page will accept:
/// SignatureRejected` — which is exactly the fault it was written to catch. Two words
/// run together with a capital in the middle is what a type name looks like and what
/// English does not, so that is what this looks for.
fn reads_as_words(message: &str) -> bool {
    // The project's own name has a capital in the middle of it, which is the one case
    // where that is a real word and not a type name. It is spelt out here so that the
    // check below stays as blunt as it is.
    let message = &message.replace("GatedSpark", "the project");
    let mut previous = ' ';
    for letter in message.chars() {
        if previous.is_ascii_lowercase() && letter.is_ascii_uppercase() {
            return false;
        }
        previous = letter;
    }
    !message.is_empty() && !message.contains("::") && !message.contains('{')
}

#[test]
fn every_single_reason_a_package_can_be_refused_is_written_in_words() {
    // The loop above covers the refusals a test can actually provoke. This covers the
    // rest, including the ones that need a damaged file nobody can easily build, by
    // asking each reason for its words directly. The match inside `plainly` is
    // exhaustive, so a reason added upstream will not compile until it has a sentence;
    // this is the half that checks the sentences are sentences.
    let reasons = [
        OpenError::NotAPackage,
        OpenError::UnsupportedFormat(9),
        OpenError::Truncated { field: "manifest" },
        OpenError::TrailingBytes(4),
        OpenError::ContentIdMismatch {
            stated: [0; 32],
            computed: ContentId::of(&Sha256, b"something else"),
        },
        OpenError::SignatureRejected,
        OpenError::Malformed { field: "family" },
    ];

    for reason in reasons {
        let shown = NotPlayable::NotAValidPackage(reason).to_string();
        assert!(reads_as_words(&shown), "not words a person reads: {shown}");
        assert!(
            shown.ends_with('.') || shown.ends_with("it."),
            "a sentence ends with a full stop: {shown}"
        );
        assert!(
            shown.len() > 30,
            "a refusal should say something: {shown:?}"
        );
    }

    // The same for an unreadable board, which is the other reason with a nested cause.
    let shown = NotPlayable::UnreadablePuzzle(gs_web::BoardProblem::Nonogram(
        DecodeError::TrailingBytes(3),
    ))
    .to_string();
    assert!(reads_as_words(&shown), "not words a person reads: {shown}");
}

#[test]
fn the_browser_has_no_way_to_build_a_playable_puzzle_without_loading_one() {
    // Structural, and stated here because it is the reason `Playable` existing means
    // anything: the type's only constructor is `load`, so holding one *is* the proof
    // that a signature was checked. There is no `Playable::new`, and this test is the
    // note that says so on purpose rather than by accident.
    //
    // Enforced by the compiler: `Playable`'s fields are public but the only function in
    // the crate that returns one is `load`. If a constructor were added, this comment
    // would be wrong and the reviewer of that change is the one who should see it.
    let published = a_real_puzzle();
    let playable = load(&published.package, &Sha256, &the_page_trusts_us()).expect("loads");
    assert_eq!(playable.signed_by, KEY_ID);
}

#[test]
fn a_replay_that_was_never_reproduced_still_loads_but_is_visible() {
    // Deliberate and worth recording: the browser does not refuse a package whose
    // answer was never replayed. The store does, on the way in, which is the right
    // place — by the time bytes reach a browser that judgement has been made and the
    // report carries it. A page that wants to show it can.
    let candidate = a_candidate();
    let report =
        report_for(&candidate, &agreeing_solver(), Budget::default()).expect("can be asked about");
    let mut manifest = manifest_for(&candidate, &report);
    manifest.replay = RecordedReplay::NotAttempted;
    let sealed = seal(&manifest, &Sha256, &our_signer()).expect("seals");

    assert!(
        load(&sealed.bytes, &Sha256, &the_page_trusts_us()).is_ok(),
        "the browser's job is the signature and the rules, not re-deciding publication"
    );
}
