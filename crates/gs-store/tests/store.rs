//! What the store must refuse to keep, and what it must refuse to hand back.
//!
//! A store is a place things are trusted to have stayed the same. So most of these tests
//! damage something — a byte of a package, the answer file, the directory name — and
//! require the store to notice. The digest and signature here are the real ones from
//! `gs-crypto`, so "notices" means what it sounds like rather than meaning the wiring
//! was exercised.
//!
//! The puzzles are real too: generated, put to a scripted solver, and sealed through the
//! same `publish` the pipeline uses. Building a package by hand would make these tests
//! agree with a layout of their own invention.

use gs_core::content::ContentId;
use gs_core::package::{seal, RecordedReplay};
use gs_core::verification::{Outcome, Verifier};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::difficulty::{propose_in_band, BANDS};
use gs_nonogram::generate::Candidate;
use gs_nonogram::publish::{manifest_for, publish, report_for, Published};
use gs_nonogram::solve::{solve, Budget};
use gs_store::{Store, StoreError, PACKAGE_FILE, WITNESS_FILE};

/// A private key printed in a public repository, which is all anybody needs to know
/// about it. It is here so these tests use real Ed25519 rather than a stand-in.
const TEST_ONLY_PRIVATE_KEY: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const KEY_ID: &str = "test-only-key";

fn signer() -> Ed25519Signer {
    Ed25519Signer::new(KEY_ID, &TEST_ONLY_PRIVATE_KEY)
}

fn trusted() -> TrustedKeys {
    let mut keys = TrustedKeys::none();
    keys.trust(KEY_ID, &signer().public_key())
        .expect("the test key is valid and the list starts empty");
    keys
}

/// A solver that answers the way one would for a puzzle with exactly one answer.
///
/// These tests are about the store, not the solver; `gs-solver` and `solver_agreement`
/// are where a real one is asked. Scripting it keeps a missing Z3 from turning a store
/// failure into a skipped test.
struct AgreesWithAUniquePuzzle {
    asked: std::cell::RefCell<usize>,
}

impl Verifier for AgreesWithAUniquePuzzle {
    fn name(&self) -> &'static str {
        "a solver that agrees the puzzle is unique"
    }

    /// Answers by position: the pipeline asks solvability first, then uniqueness with
    /// the witness blocked. Counting rather than inspecting the query text, because the
    /// text is the encoder's business and a test that guessed at its shape would break
    /// whenever the encoding changed, for a reason having nothing to do with the store.
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

/// The first seed that produces a candidate for this level.
///
/// Searching rather than naming a seed, because not every seed yields a puzzle a given
/// level claims — that is what the generator's measured yield means — and a hard-coded
/// seed would make these tests fail whenever generation changed.
fn a_candidate(level: usize) -> Candidate {
    (1u64..3_000)
        .find_map(|seed| propose_in_band(&BANDS[level], seed, Budget::default()))
        .expect("some seed in the first few thousand produces a candidate for this level")
}

fn a_published(level: usize) -> Published {
    publish(
        &a_candidate(level),
        &agreeing_solver(),
        Budget::default(),
        &Sha256,
        &signer(),
    )
    .expect("a uniquely solvable candidate with an agreeing solver publishes")
}

/// The answer, as the store keeps it.
fn witness_of(candidate: &Candidate) -> Vec<u8> {
    solve(&candidate.puzzle, Budget::default())
        .witness
        .expect("a unique puzzle has a witness")
        .canonical_bytes()
}

/// A store in a directory of its own, removed when the test ends.
struct TempStore {
    store: Store,
    path: std::path::PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        // Process id plus the label: unique between concurrent runs and between the
        // tests of one run, without reading a clock.
        let path = std::env::temp_dir().join(format!(
            "gatedspark-store-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        TempStore {
            store: Store::open(&path).expect("a fresh store"),
            path,
        }
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Store one real puzzle and give back everything about it.
fn store_one(temp: &TempStore, level: usize) -> (ContentId, Vec<u8>, Vec<u8>) {
    let candidate = a_candidate(level);
    let published = a_published(level);
    let witness = witness_of(&candidate);
    let id = temp
        .store
        .put(&published.package, &witness, &Sha256, &trusted())
        .expect("a freshly published puzzle goes in");
    (id, published.package, witness)
}

#[test]
fn a_published_puzzle_goes_in_and_comes_back_unchanged() {
    // The control. Without it every refusal below could pass because nothing is ever
    // accepted.
    let temp = TempStore::new("roundtrip");
    let (id, package, witness) = store_one(&temp, 1);

    let got = temp
        .store
        .get(&id.to_hex(), &Sha256, &trusted())
        .expect("what was just stored must come back");
    assert_eq!(got.bytes, package, "the package bytes are kept exactly");
    assert_eq!(got.witness, witness, "the answer is kept exactly");
    assert_eq!(got.package.content_id, id);
    assert_eq!(got.package.key_id, KEY_ID);
    assert_eq!(temp.store.list().expect("lists"), vec![id.to_hex()]);
}

#[test]
fn an_empty_directory_is_a_valid_empty_store() {
    // "One command takes an empty store to a signed, published, playable puzzle" has to
    // start somewhere, and this is that somewhere.
    let temp = TempStore::new("empty");
    assert_eq!(temp.store.list().expect("lists"), Vec::<String>::new());
    let report = temp.store.recheck(&Sha256, &trusted()).expect("rechecks");
    assert_eq!(report.checked, 0);
    assert!(
        report.all_sound(),
        "an empty store has no broken puzzles — which is exactly why a caller must look \
         at `checked` too, and the doc comment says so"
    );
}

#[test]
fn the_directory_is_named_by_the_content_id_and_holds_the_two_files() {
    // The layout is the part of this design that is expensive to change, so it is
    // asserted rather than left to the module's doc comment.
    let temp = TempStore::new("layout");
    let (id, _, _) = store_one(&temp, 0);
    let dir = temp.store.root().join(id.to_hex());
    assert!(dir.join(PACKAGE_FILE).is_file());
    assert!(dir.join(WITNESS_FILE).is_file());
}

#[test]
fn a_package_the_store_cannot_verify_never_reaches_the_disk() {
    // The store is not a place to put bytes and check later. If this wrote the file and
    // failed on the way out instead, a corpus could hold packages nothing ever vouched
    // for, which is the state this whole design exists to make impossible.
    let temp = TempStore::new("unverifiable");
    let (_, mut package, witness) = {
        let candidate = a_candidate(1);
        let published = a_published(1);
        (0, published.package, witness_of(&candidate))
    };
    let last = package.len() - 1;
    package[last] ^= 0x01;

    let err = temp
        .store
        .put(&package, &witness, &Sha256, &trusted())
        .expect_err("a package with a broken signature must be refused");
    assert!(
        matches!(err, StoreError::NotAValidPackage(_)),
        "got {err:?}"
    );
    assert_eq!(
        temp.store.list().expect("lists"),
        Vec::<String>::new(),
        "nothing may have been written"
    );
}

#[test]
fn a_package_signed_by_an_untrusted_key_is_refused() {
    let temp = TempStore::new("stranger");
    let stranger = Ed25519Signer::new(
        "somebody-elses-key",
        &[
            0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11,
            0x4e, 0x0f, 0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed,
            0x4f, 0xb8, 0xa6, 0xfb,
        ],
    );
    let candidate = a_candidate(1);
    let report = report_for(&candidate, &agreeing_solver(), Budget::default())
        .expect("the candidate is askable");
    let sealed = seal(&manifest_for(&candidate, &report), &Sha256, &stranger).expect("seals");

    let err = temp
        .store
        .put(&sealed.bytes, &witness_of(&candidate), &Sha256, &trusted())
        .expect_err("a key this store was not told to trust must be refused");
    assert!(
        matches!(err, StoreError::NotAValidPackage(_)),
        "got {err:?}"
    );
}

#[test]
fn an_answer_that_does_not_match_the_signed_report_is_refused() {
    // The whole reason the answer can live in a plain file. The report records the
    // state hash of the witness and the report is signed, so a wrong answer is caught
    // by a number nobody can change without breaking the signature.
    let temp = TempStore::new("wrong-answer");
    let candidate = a_candidate(1);
    let published = a_published(1);

    let mut witness = witness_of(&candidate);
    let last = witness.len() - 1;
    witness[last] ^= 0x01;

    let err = temp
        .store
        .put(&published.package, &witness, &Sha256, &trusted())
        .expect_err("an answer that is not the one that was replayed must be refused");
    match err {
        StoreError::WitnessDoesNotMatch { signed, stored } => {
            assert_ne!(signed, stored, "the two hashes must actually differ");
        }
        other => panic!("expected WitnessDoesNotMatch, got {other:?}"),
    }
}

#[test]
fn an_answer_corrupted_on_disk_after_the_fact_is_caught_on_the_way_out() {
    // The case the design is really for: the store was written correctly and the answer
    // file changed afterwards. Every read re-checks, so this is found.
    //
    // "Corrupted" rather than "swapped", which is what this was first called. The check
    // is a 64-bit FNV hash recorded in the signed report, and `gs_core::hash` is explicit
    // that it guards against *accidental* collision. It catches damage; it does not stop
    // somebody who can write here and is willing to craft a colliding answer. See the
    // module documentation on `gs-store` for what that does and does not cover.
    let temp = TempStore::new("swapped");
    let (id, _, witness) = store_one(&temp, 1);

    let mut tampered = witness.clone();
    tampered[0] ^= 0x80;
    std::fs::write(
        temp.store.root().join(id.to_hex()).join(WITNESS_FILE),
        &tampered,
    )
    .expect("writing the tampered answer");

    let err = temp
        .store
        .get(&id.to_hex(), &Sha256, &trusted())
        .expect_err("a replaced answer must be caught");
    assert!(
        matches!(err, StoreError::WitnessDoesNotMatch { .. }),
        "got {err:?}"
    );

    // And a re-check finds it as one failure among however many puzzles there are.
    let report = temp.store.recheck(&Sha256, &trusted()).expect("rechecks");
    assert_eq!(report.checked, 1);
    assert!(!report.all_sound());
    assert_eq!(report.failures.len(), 1);
}

#[test]
fn a_renamed_directory_is_a_refusal_rather_than_a_different_puzzle() {
    // The directory name is the one part of the layout no signature covers, so it is a
    // claim to check. Without this test the store would happily serve a puzzle under
    // whatever name somebody typed, and the id a caller used to ask would not be the id
    // of what they got.
    let temp = TempStore::new("renamed");
    let (id, _, _) = store_one(&temp, 1);

    let wrong_name = "0".repeat(64);
    std::fs::rename(
        temp.store.root().join(id.to_hex()),
        temp.store.root().join(&wrong_name),
    )
    .expect("renaming the directory");

    let err = temp
        .store
        .get(&wrong_name, &Sha256, &trusted())
        .expect_err("a directory whose name is not its package's id must be refused");
    match err {
        StoreError::DirectoryNameDisagrees { named, actual } => {
            assert_eq!(named, wrong_name);
            assert_eq!(actual, id, "the refusal names what the package really is");
        }
        other => panic!("expected DirectoryNameDisagrees, got {other:?}"),
    }
}

#[test]
fn storing_the_same_puzzle_twice_is_refused_rather_than_overwritten() {
    // Two packages under one id means either the digest collided or something is
    // confused about identity. Replacing one with the other would destroy the evidence
    // of which.
    let temp = TempStore::new("duplicate");
    let (id, package, witness) = store_one(&temp, 1);
    let err = temp
        .store
        .put(&package, &witness, &Sha256, &trusted())
        .expect_err("the second put must be refused");
    match err {
        StoreError::AlreadyStored(got) => assert_eq!(got, id),
        other => panic!("expected AlreadyStored, got {other:?}"),
    }
    assert_eq!(temp.store.list().expect("lists").len(), 1);
}

#[test]
fn a_name_that_is_not_a_content_id_is_refused_without_touching_the_disk() {
    let temp = TempStore::new("bad-name");
    for name in [
        "",
        "not-hex",
        "../escape",
        &"g".repeat(64),
        &"A".repeat(64), // uppercase: `to_hex` emits lowercase, and accepting both would
        // let one puzzle be stored under two names.
        &"a".repeat(63),
        &"a".repeat(65),
    ] {
        let err = temp
            .store
            .get(name, &Sha256, &trusted())
            .expect_err("a name that cannot be a content id must be refused");
        assert!(
            matches!(err, StoreError::NotAPuzzleName(_)),
            "name {name:?} gave {err:?}"
        );
    }
}

#[test]
fn a_file_or_stray_directory_in_the_store_is_not_a_puzzle() {
    // A store a human has left a README in is still a store, and the staging directory
    // a half-finished write uses must not be listed as a puzzle.
    let temp = TempStore::new("strays");
    let (id, _, _) = store_one(&temp, 1);
    std::fs::write(temp.store.root().join("README.md"), b"notes").expect("writing a stray file");
    std::fs::create_dir(temp.store.root().join(".writing-something")).expect("a stray directory");
    std::fs::create_dir(temp.store.root().join("old-backup")).expect("another stray directory");

    assert_eq!(
        temp.store.list().expect("lists"),
        vec![id.to_hex()],
        "only directories named like a content id are puzzles"
    );
}

#[test]
fn asking_for_a_puzzle_that_is_not_there_says_so() {
    let temp = TempStore::new("missing");
    let err = temp
        .store
        .get(&"ab".repeat(32), &Sha256, &trusted())
        .expect_err("nothing is stored");
    assert!(matches!(err, StoreError::NotStored(_)), "got {err:?}");
}

#[test]
fn many_puzzles_are_listed_in_a_stable_order() {
    // A caller that prints the store, or walks it to re-check a corpus, must get the
    // same answer twice. Directory order is whatever the filesystem feels like.
    let temp = TempStore::new("many");
    let mut stored = Vec::new();
    for level in 0..BANDS.len().min(4) {
        let (id, _, _) = store_one(&temp, level);
        stored.push(id.to_hex());
    }
    stored.sort();

    let listed = temp.store.list().expect("lists");
    assert_eq!(listed, stored);
    assert_eq!(temp.store.list().expect("lists again"), listed);

    let report = temp.store.recheck(&Sha256, &trusted()).expect("rechecks");
    assert_eq!(report.checked, stored.len());
    assert!(report.all_sound(), "failures: {:?}", report.failures);
}

#[test]
fn a_truncated_package_on_disk_is_caught_rather_than_served() {
    let temp = TempStore::new("truncated");
    let (id, package, _) = store_one(&temp, 1);
    let path = temp.store.root().join(id.to_hex()).join(PACKAGE_FILE);
    std::fs::write(&path, &package[..package.len() / 2]).expect("truncating");

    let err = temp
        .store
        .get(&id.to_hex(), &Sha256, &trusted())
        .expect_err("half a package is not a package");
    assert!(
        matches!(err, StoreError::NotAValidPackage(_)),
        "got {err:?}"
    );
}

#[test]
fn a_package_with_no_replayed_answer_is_refused() {
    // The store checks a stored answer against the fingerprint in the signed report, so
    // a report with no such fingerprint leaves nothing to check against. Keeping it
    // would mean keeping an answer file nobody could ever disagree with, which is the
    // same as keeping no answer at all.
    //
    // `publish` cannot produce one of these — it refuses a candidate whose answer the
    // real game did not accept — so the manifest is built and resealed by hand. This is
    // the one test here that does that, and it does it to reach a path the pipeline
    // cannot.
    let temp = TempStore::new("noreplay");
    let candidate = a_candidate(0);
    let report = report_for(&candidate, &agreeing_solver(), Budget::default())
        .expect("the candidate can be asked about");
    let mut manifest = manifest_for(&candidate, &report);
    manifest.replay = RecordedReplay::NotAttempted;
    let sealed = seal(&manifest, &Sha256, &signer()).expect("a hand-built manifest seals");

    let witness = witness_of(&candidate);
    match temp.store.put(&sealed.bytes, &witness, &Sha256, &trusted()) {
        Err(StoreError::NoReproducedWitness) => {}
        other => panic!("a package with no replayed answer must be refused, got {other:?}"),
    }
    assert!(
        temp.store.list().expect("listing works").is_empty(),
        "nothing may have reached the disk"
    );
}

#[test]
fn a_half_written_puzzle_is_not_a_puzzle() {
    // `put` stages into a directory whose name starts with a dot and renames it into
    // place, so a process killed halfway through leaves one behind. Those must not be
    // listed, counted or served: a re-check that reported a half-written puzzle as a
    // failure would cry wolf, and one that reported it as sound would be worse.
    let temp = TempStore::new("halfwritten");
    let (id, package, witness) = store_one(&temp, 0);

    // A staging directory with a complete, valid puzzle in it — the most convincing
    // leftover available, so passing cannot be an accident of the contents.
    let staging = temp.path.join(format!(".writing-{}", id.to_hex()));
    std::fs::create_dir_all(&staging).expect("a staging directory");
    std::fs::write(staging.join(PACKAGE_FILE), &package).expect("write the package");
    std::fs::write(staging.join(WITNESS_FILE), &witness).expect("write the answer");

    let listed = temp.store.list().expect("listing works");
    assert_eq!(
        listed,
        vec![id.to_hex()],
        "only the finished puzzle is listed"
    );

    let recheck = temp
        .store
        .recheck(&Sha256, &trusted())
        .expect("the re-check runs");
    assert_eq!(recheck.checked, 1, "the leftover is not counted");
    assert!(
        recheck.all_sound(),
        "the leftover is not a failure either: {:?}",
        recheck.failures
    );
}
