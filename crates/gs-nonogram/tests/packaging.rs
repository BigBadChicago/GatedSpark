//! From a generated puzzle to a signed file and back to a playable puzzle.
//!
//! The other tests in this crate stop at the validation report. These go one step
//! further and close the loop the whole slice exists to demonstrate: generate, ask a
//! solver, write a signed file, read the file, and check that what comes out is the
//! puzzle that went in and that the engine still accepts its answer.
//!
//! The digest and the signature here are the real ones, from `gs-crypto`: SHA-256 and
//! Ed25519. So these tests prove more than the wiring — a changed byte anywhere in the
//! file is genuinely detected, and a file signed by a key this reader does not trust is
//! genuinely refused. The one thing that is still a stand-in is the *key*, which is
//! written down below in the clear and could not be more plainly unfit to publish with.

use gs_core::package::{open, seal, Manifest, OpenError};
use gs_core::ruleset::Ruleset;
use gs_core::verification::Outcome;
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::clues::{Clues, DecodeError, Pattern};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::generate::Candidate;
use gs_nonogram::publish::{manifest_for, publish, report_for, PublishError, Verifier};
use gs_nonogram::rules::Nonogram;
use gs_nonogram::solve::{solve, Budget};

/// A private key that is printed here in a public repository, which is the only thing
/// anybody needs to know about it. It exists so these tests exercise real Ed25519
/// rather than a stand-in. Anything signed with it is signed by everyone.
const TEST_ONLY_PRIVATE_KEY_DO_NOT_PUBLISH_WITH_THIS: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

const TEST_KEY_ID: &str = "test-only-key";

fn signer() -> Ed25519Signer {
    Ed25519Signer::new(TEST_KEY_ID, &TEST_ONLY_PRIVATE_KEY_DO_NOT_PUBLISH_WITH_THIS)
}

/// A reader that trusts exactly the test key above and nothing else.
fn trusted() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(TEST_KEY_ID, &signer().public_key())
        .expect("the test key is valid and the list starts empty");
    keys
}

/// Answers the two questions the way a solver would for a uniquely solvable puzzle.
///
/// These tests are about the file, not about the solver; `solver_agreement` is where
/// a real solver is asked. Scripting the answers here keeps a missing Z3 from
/// turning a packaging failure into a skipped test.
struct AgreesWithAUniquePuzzle {
    asked: std::cell::RefCell<usize>,
}

impl Verifier for AgreesWithAUniquePuzzle {
    fn name(&self) -> &'static str {
        "scripted-unique"
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

fn verifier() -> AgreesWithAUniquePuzzle {
    AgreesWithAUniquePuzzle {
        asked: std::cell::RefCell::new(0),
    }
}

fn a_candidate_in(level: usize) -> Candidate {
    (0..4_000u64)
        .find_map(|seed| propose_in_band(&BANDS[level], seed, Budget::default()))
        .expect("every level produces something within 4000 seeds")
}

fn manifest_of(candidate: &Candidate) -> Manifest {
    let report =
        report_for(candidate, &verifier(), Budget::default()).expect("the candidate is askable");
    manifest_for(candidate, &report)
}

#[test]
fn clue_bytes_round_trip() {
    // The cases that matter are the ones an encoding without lengths gets wrong: a
    // line with no runs at all, and two lines whose runs would concatenate alike.
    let empty_line = Clues::of(&Pattern::from_rows(&["..#", "...", "#.#"]).expect("3x3"));
    let ambiguous_split = Clues::of(&Pattern::from_rows(&["#.##", "##.#"]).expect("4x2"));

    for clues in [empty_line, ambiguous_split] {
        let bytes = clues.canonical_bytes();
        let back = Clues::from_canonical_bytes(&bytes).expect("its own bytes must decode");
        assert_eq!(back, clues);
        assert_eq!(back.canonical_bytes(), bytes, "the encoding has slack");
    }
}

#[test]
fn clue_bytes_round_trip_for_every_difficulty_level() {
    for (level, band) in BANDS.iter().enumerate() {
        let candidate = a_candidate_in(level);
        let bytes = candidate.puzzle.clues.canonical_bytes();
        let back = Clues::from_canonical_bytes(&bytes).expect("decodes");
        assert_eq!(
            back, candidate.puzzle.clues,
            "level {} ({}) did not survive the trip",
            level, band.name
        );
    }
}

#[test]
fn a_sealed_package_becomes_a_playable_puzzle() {
    // The end of the loop. Everything before this has been our code talking to our
    // code; this is the shape of what a device actually does.
    let candidate = a_candidate_in(1);
    let manifest = manifest_of(&candidate);
    let bytes = seal(&manifest, &Sha256, &signer()).expect("seals").bytes;

    let got = open(&bytes, &Sha256, &trusted()).expect("opens");
    let clues = Clues::from_canonical_bytes(&got.manifest.payload).expect("the payload is clues");
    assert_eq!(clues, candidate.puzzle.clues);

    // And the puzzle rebuilt from the file is the puzzle that was verified: it still
    // has exactly one answer, and the engine accepts that answer. Reached through
    // the ruleset's own goal test rather than by re-reading the clues here.
    let rebuilt = Nonogram::new(clues);
    let analysis = solve(&rebuilt, Budget::default());
    let witness = analysis.witness.expect("a unique puzzle has a witness");
    assert!(rebuilt.is_solved(&witness));
    assert_eq!(Ruleset::id(&rebuilt), Ruleset::id(&candidate.puzzle));
}

#[test]
fn the_package_carries_the_evidence_the_report_recorded() {
    let candidate = a_candidate_in(0);
    let report =
        report_for(&candidate, &verifier(), Budget::default()).expect("the candidate is askable");
    let bytes = seal(&manifest_for(&candidate, &report), &Sha256, &signer())
        .expect("seals")
        .bytes;
    let got = open(&bytes, &Sha256, &trusted()).expect("opens");

    assert_eq!(got.manifest.family, "nonogram");
    assert_eq!(got.manifest.seed, candidate.seed);
    assert_eq!(got.manifest.encoding, report.encoding);
    assert_eq!(got.manifest.band.as_deref(), report.band);
    assert_eq!(got.manifest.queries.len(), report.queries.len());
    for (recorded, original) in got.manifest.queries.iter().zip(&report.queries) {
        assert_eq!(recorded.name, original.name);
        assert_eq!(recorded.required, original.required);
        assert_eq!(recorded.returned, original.returned);
    }
    for original in &report.measurements {
        assert_eq!(
            got.manifest.measurement(original.name),
            Some(original.value),
            "measurement {} was lost",
            original.name
        );
    }
    assert!(got.manifest.replay.reproduced());
}

#[test]
fn the_package_does_not_carry_the_answer() {
    // The generator throws the pattern away on purpose, and the witness stays in the
    // server-side report. A package that shipped either would ship the solution.
    let candidate = a_candidate_in(1);
    let manifest = manifest_of(&candidate);
    assert_eq!(
        manifest.payload,
        candidate.puzzle.clues.canonical_bytes(),
        "the payload must be the clues and nothing else"
    );

    let witness = solve(&candidate.puzzle, Budget::default())
        .witness
        .expect("a unique puzzle has a witness");
    let sealed = seal(&manifest, &Sha256, &signer()).expect("seals").bytes;

    // The positive half first, so the negative half below means something: a search
    // that found nothing because it cannot find anything would pass whatever the
    // file held.
    assert!(
        contains(&sealed, &candidate.puzzle.clues.canonical_bytes()),
        "the clues should be in the file; if they are not, the search below proves nothing"
    );

    let answer = witness.canonical_bytes();
    assert!(
        !contains(&sealed, &answer),
        "the sealed package contains the witness's bytes"
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn a_payload_that_is_not_clues_is_refused_rather_than_guessed_at() {
    assert!(matches!(
        Clues::from_canonical_bytes(b"not clues at all"),
        Err(DecodeError::TrailingBytes(_)) | Err(DecodeError::LineCountDisagreesWithDims { .. })
    ));
    assert!(matches!(
        Clues::from_canonical_bytes(&[]),
        Err(DecodeError::Truncated { field: "width" })
    ));
}

#[test]
fn a_line_count_that_disagrees_with_the_board_is_refused() {
    // A board 3 wide must carry exactly 3 columns of clues. Accepting fewer would let
    // a decoded puzzle be in a state no generated puzzle can reach, and the solver
    // would then be asked about a board that never existed.
    let clues = Clues::of(&Pattern::from_rows(&["#.#", ".#.", "#.#"]).expect("3x3"));
    let mut bytes = clues.canonical_bytes();
    // The row count is the u32 at offset 4, after the two u16 dimensions.
    bytes[4] = 2;
    assert_eq!(
        Clues::from_canonical_bytes(&bytes),
        Err(DecodeError::LineCountDisagreesWithDims {
            axis: "rows",
            expected: 3,
            found: 2
        })
    );
}

#[test]
fn every_truncation_of_a_clue_payload_is_refused_and_none_panics() {
    let clues = Clues::of(&Pattern::from_rows(&["##.", ".#.", "..#"]).expect("3x3"));
    let bytes = clues.canonical_bytes();
    for n in 0..bytes.len() {
        assert!(
            Clues::from_canonical_bytes(&bytes[..n]).is_err(),
            "a {n}-byte prefix was accepted"
        );
    }
    assert!(Clues::from_canonical_bytes(&bytes).is_ok());
}

// ---------------------------------------------------------------------------
// With a real digest and a real signature, these are claims about the file rather
// than about the wiring.
// ---------------------------------------------------------------------------

#[test]
fn publish_hands_back_a_report_bound_to_the_package_it_sealed() {
    // The closed loop, in one call: generate, ask, decide, seal, bind. The report that
    // comes back must be publishable — which is the first time in this project's
    // history that has been true of anything — and the id it carries must be the id of
    // the file next to it, not a second one worked out separately.
    let candidate = a_candidate_in(1);
    let published = publish(
        &candidate,
        &verifier(),
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect("a uniquely solvable candidate with an agreeing solver is publishable");

    assert_eq!(
        published.report.publishable(),
        Ok(()),
        "the whole point of binding the content id is that this now passes"
    );

    let got = open(&published.package, &Sha256, &trusted()).expect("the sealed package opens");
    assert_eq!(
        published.report.content_id,
        Some(got.content_id),
        "the report must name the fingerprint of the package beside it"
    );
    assert_eq!(got.key_id, TEST_KEY_ID);
}

#[test]
fn a_candidate_the_solver_will_not_vouch_for_is_never_sealed() {
    // A solver that says the puzzle has more than one answer. Nothing may be signed:
    // the refusal arrives instead of a package, and it carries the report, because why
    // a puzzle was dropped is worth keeping.
    struct SaysThereIsAnotherAnswer;
    impl Verifier for SaysThereIsAnotherAnswer {
        fn name(&self) -> &'static str {
            "a solver that found a second answer"
        }
        fn ask(&self, _query: &str) -> Outcome {
            Outcome::Sat
        }
    }

    let candidate = a_candidate_in(1);
    let err = publish(
        &candidate,
        &SaysThereIsAnotherAnswer,
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect_err("a puzzle with more than one answer must not be published");

    match err {
        PublishError::Refused { report, .. } => {
            assert!(
                report.content_id.is_none(),
                "nothing was sealed, so there is no fingerprint"
            );
            assert_eq!(
                report.queries.len(),
                2,
                "the record of what was asked is kept"
            );
        }
        other => panic!("expected a refusal from the gate, got {other:?}"),
    }
}

#[test]
fn a_solver_that_does_not_know_is_not_treated_as_a_solver_that_said_no() {
    // An UNKNOWN is carried into the report as UNKNOWN and refuses publication. The
    // puzzle is not published and is also not refuted; those are different facts and
    // this is where the difference has to survive.
    struct DoesNotKnow;
    impl Verifier for DoesNotKnow {
        fn name(&self) -> &'static str {
            "a solver that timed out"
        }
        fn ask(&self, _query: &str) -> Outcome {
            Outcome::Unknown
        }
    }

    let candidate = a_candidate_in(1);
    let err = publish(
        &candidate,
        &DoesNotKnow,
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect_err("an undecided solver is not grounds to publish");

    match err {
        PublishError::Refused { report, .. } => {
            assert!(report.has_undecided_query());
            assert!(report
                .queries
                .iter()
                .all(|q| q.returned == Outcome::Unknown));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn publishing_the_same_candidate_twice_gives_byte_identical_files() {
    // Reproducibility, all the way through the signature. Ed25519 is deterministic and
    // nothing in the manifest comes from a clock or a counter, so the same puzzle and
    // the same key must give the same file. If this ever fails, something in the
    // pipeline has started reading something it should not.
    let candidate = a_candidate_in(2);
    let once = publish(
        &candidate,
        &verifier(),
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect("publishes");
    let twice = publish(
        &candidate,
        &verifier(),
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect("publishes");
    assert_eq!(once.package, twice.package);
    assert_eq!(once.report.content_id, twice.report.content_id);
}

#[test]
fn a_single_flipped_bit_anywhere_in_a_published_file_is_detected() {
    // This is the test the stand-in digest could never support. Every bit of the body
    // and of the stated fingerprint, one at a time: the file must be refused, and the
    // refusal must be the fingerprint check or the signature, never a successful open.
    let candidate = a_candidate_in(0);
    let published = publish(
        &candidate,
        &verifier(),
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect("publishes");
    let keys = trusted();

    for byte in 0..published.package.len() {
        for bit in 0..8u8 {
            let mut broken = published.package.clone();
            broken[byte] ^= 1 << bit;
            match open(&broken, &Sha256, &keys) {
                Ok(_) => panic!("bit {bit} of byte {byte} was changed and the file still opened"),
                Err(OpenError::ContentIdMismatch { .. } | OpenError::SignatureRejected) => {}
                // Framing damage — a changed magic byte, a length that now runs off the
                // end — is refused earlier and just as firmly. What must not happen is
                // a successful open.
                Err(_) => {}
            }
        }
    }
}

#[test]
fn a_file_signed_by_a_key_this_reader_does_not_trust_is_refused() {
    // The package is well-formed and its signature is genuine. It is simply not signed
    // by anybody this reader was told to accept, which is the only thing standing
    // between a device and content from a stranger.
    let stranger = Ed25519Signer::new(
        "somebody-elses-key",
        &[
            0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11,
            0x4e, 0x0f, 0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed,
            0x4f, 0xb8, 0xa6, 0xfb,
        ],
    );
    let candidate = a_candidate_in(1);
    let report =
        report_for(&candidate, &verifier(), Budget::default()).expect("the candidate is askable");
    let bytes = seal(&manifest_for(&candidate, &report), &Sha256, &stranger)
        .expect("seals")
        .bytes;

    assert_eq!(
        open(&bytes, &Sha256, &trusted()),
        Err(OpenError::SignatureRejected),
        "a genuine signature from an unknown key must be refused"
    );

    // And it is the key that is wrong, not the file: a reader that does trust that key
    // opens the very same bytes.
    let mut wider = trusted();
    wider
        .trust("somebody-elses-key", &stranger.public_key())
        .expect("a second key under a second id");
    assert!(open(&bytes, &Sha256, &wider).is_ok());
}
