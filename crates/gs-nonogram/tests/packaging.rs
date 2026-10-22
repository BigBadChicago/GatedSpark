//! From a generated puzzle to a file and back to a playable puzzle.
//!
//! The other tests in this crate stop at the validation report. These go one step
//! further and close the loop the whole slice exists to demonstrate: generate, ask a
//! solver, write a file, read the file, and check that what comes out is the puzzle
//! that went in and that the engine still accepts its answer.
//!
//! The stand-ins here are a fake digest and a fake signature, because no real ones
//! exist yet (ADR 0004). What they let these tests prove is that the *wiring* is
//! right — the clues survive the trip, nothing is silently dropped, and the pattern
//! the generator started from is not in the file. What they cannot prove is that the
//! file is tamper-proof, which waits on a reviewed implementation.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{open, seal, Manifest, SignatureVerifier, Signer};
use gs_core::ruleset::Ruleset;
use gs_core::verification::Outcome;
use gs_nonogram::clues::{Clues, DecodeError, Pattern};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::generate::Candidate;
use gs_nonogram::publish::{manifest_for, report_for, Verifier};
use gs_nonogram::rules::Nonogram;
use gs_nonogram::solve::{solve, Budget};

/// Not a hash. See `gs-core/tests/package.rs` for why a test digest has to be
/// sensitive to every byte, and why using one of these to publish would be a
/// security hole.
struct NotAHash;

impl Digest for NotAHash {
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

/// Not a signature. Anyone can produce it, which is exactly what a real scheme must
/// make impossible.
struct NotASignature;

impl Signer for NotASignature {
    fn key_id(&self) -> &str {
        "test-key"
    }
    fn sign(&self, content_id: &ContentId) -> Vec<u8> {
        content_id.as_bytes().to_vec()
    }
}

impl SignatureVerifier for NotASignature {
    fn verify(&self, content_id: &ContentId, key_id: &str, signature: &[u8]) -> bool {
        key_id == "test-key" && signature == content_id.as_bytes()
    }
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
    let bytes = seal(&manifest, &NotAHash, &NotASignature).expect("seals");

    let got = open(&bytes, &NotAHash, &NotASignature).expect("opens");
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
    let bytes = seal(
        &manifest_for(&candidate, &report),
        &NotAHash,
        &NotASignature,
    )
    .expect("seals");
    let got = open(&bytes, &NotAHash, &NotASignature).expect("opens");

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
    let sealed = seal(&manifest, &NotAHash, &NotASignature).expect("seals");

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
