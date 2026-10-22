//! The store: where a published puzzle lives after it is made.
//!
//! `design/content-delivery.md` section 6 requires every accepted puzzle to be kept
//! with its content id and a machine-readable validation report, for four jobs:
//! re-verifying the corpus when the rules or the solver change, reproducing a player's
//! bug report, retraining difficulty calibration, and audit.
//!
//! Nearly all of that is already inside the published package, which carries the seed,
//! the ruleset and generator versions, every solver query and its outcome, the encoding
//! the uniqueness claim is relative to, the difficulty measurements, the level, and the
//! signature over all of it. One required thing is not in there, deliberately.
//!
//! # The one thing the package cannot carry
//!
//! **The answer.** A package is what a player's device receives, so a package that
//! shipped the solution next to the puzzle would be shipping the solution. The
//! generator throws away the pattern it started from for the same reason.
//!
//! But section 6 requires the answer to be kept, because reproducing a bug report and
//! re-checking a puzzle years later both need it. So the store holds two files per
//! puzzle: the package, and the witness, which never leaves this side.
//!
//! # The layout
//!
//! ```text
//! <store>/
//!   <content id as 64 hex characters>/
//!      package.gspk     the signed file a player receives
//!      witness.bin      the answer, kept here only
//! ```
//!
//! One directory per puzzle, named by the puzzle's content id. That id is already the
//! one name a puzzle has; naming directories by a counter or a date would create a
//! second way to identify a puzzle, and two names for one thing is the failure
//! [`gs_core::content`] exists to prevent.
//!
//! # A stored answer cannot be swapped
//!
//! The obvious worry about keeping the answer in a plain file is that nothing stops
//! someone replacing it, and a tampered answer would make a future re-check produce the
//! wrong result while every signature still passed.
//!
//! It closes itself. The validation report already records the **state hash of the
//! witness**, as `Replay::Reproduced(hash)`, and that field is inside the package, so it
//! is covered by the signature. [`StateHash::of`] is defined as `of_bytes` over
//! [`State::canonical_bytes`](gs_core::state::State::canonical_bytes) — exactly the
//! bytes stored here — so a stored witness can be checked against a number that was
//! signed at the moment the puzzle was published. No new format and no second digest.
//!
//! # Nothing enters this store unverified
//!
//! [`Store::put`] does not take a content id and write bytes where it is told. It takes
//! the package bytes, opens them — recomputing the digest, checking the signature —
//! learns the content id from that, checks the witness against the signed hash, and only
//! then writes. So the store cannot hold a package it did not verify on the way in.
//!
//! On the way out, a puzzle is found by its directory name and the name is then checked
//! against the id the package actually digests to. A renamed directory is a refusal, not
//! a puzzle with a different id. That check exists because the name is the one part of
//! the layout no signature covers — a directory can be renamed by anyone who can rename
//! a directory — so the name is treated as a claim to verify rather than a fact.
//!
//! Note what [`ContentId`] does **not** have, and why this crate works the way it does:
//! there is no way to build one except by digesting bytes. That is deliberate (see
//! [`gs_core::content`]) and it is why a lookup takes the directory name as text rather
//! than an id parsed out of it. An id parsed from a directory name would be a number
//! wearing the type that means "this was digested".
//!
//! # What this crate does not know
//!
//! Anything about Nonogram. It stores packages and witnesses as bytes. Replaying a
//! witness through a real engine needs the family that owns it, and that belongs to the
//! caller.

use std::path::{Path, PathBuf};

use gs_core::content::{ContentId, Digest};
use gs_core::hash::StateHash;
use gs_core::package::{open, Opened, RecordedReplay, SignatureVerifier};

/// The file inside a puzzle's directory that holds the package.
pub const PACKAGE_FILE: &str = "package.gspk";

/// The file inside a puzzle's directory that holds the answer.
pub const WITNESS_FILE: &str = "witness.bin";

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    /// The filesystem said no. Carries what was being done, because "permission denied"
    /// on its own has sent many people looking in the wrong place.
    Io {
        doing: String,
        cause: std::io::Error,
    },
    /// The package did not survive being opened: a bad digest, a signature from a key
    /// this reader does not trust, or a malformed file. Carries the reason from
    /// [`gs_core::package::open`].
    ///
    /// On the way in, this means something handed the store a package it should not
    /// keep. On the way out, it means the stored file has changed since it was written.
    NotAValidPackage(gs_core::package::OpenError),
    /// The package says its witness replayed to one hash, and the stored witness hashes
    /// to another. Either the answer file was replaced, or it belongs to a different
    /// puzzle.
    WitnessDoesNotMatch {
        /// What the signed report says the witness hashes to.
        signed: StateHash,
        /// What the stored bytes actually hash to.
        stored: StateHash,
    },
    /// The package's report does not say the witness was reproduced, so there is no
    /// hash to bind an answer to.
    ///
    /// Such a package is not publishable in the first place — `publishable` refuses a
    /// report whose witness did not replay — so this is the store refusing to keep
    /// something that should never have reached it.
    NoReproducedWitness,
    /// A puzzle with this content id is already stored. Refused rather than overwritten:
    /// two packages with one content id would mean the digest had collided or something
    /// is confused about identity, and quietly replacing one with the other would
    /// destroy the evidence of which.
    AlreadyStored(ContentId),
    /// Nothing is stored under this name.
    NotStored(String),
    /// The directory name is not 64 hex characters, so it cannot be a content id.
    NotAPuzzleName(String),
    /// The package inside the directory digests to a different id than the directory
    /// name claims. The directory was renamed, or two puzzles were shuffled.
    ///
    /// The name is the one part of this layout no signature covers, so it is checked
    /// rather than believed.
    DirectoryNameDisagrees {
        /// The directory name.
        named: String,
        /// What the package actually digests to.
        actual: ContentId,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io { doing, cause } => write!(f, "{doing}: {cause}"),
            StoreError::NotAValidPackage(why) => write!(f, "not a valid package: {why:?}"),
            StoreError::WitnessDoesNotMatch { signed, stored } => write!(
                f,
                "the stored answer hashes to {} but the signed report says {}",
                stored.to_hex(),
                signed.to_hex()
            ),
            StoreError::NoReproducedWitness => write!(
                f,
                "the package's report does not say its witness was reproduced, \
                 so there is no hash to bind an answer to"
            ),
            StoreError::AlreadyStored(id) => write!(f, "already stored: {}", id.to_hex()),
            StoreError::NotStored(name) => write!(f, "not stored: {name}"),
            StoreError::NotAPuzzleName(name) => {
                write!(f, "not a puzzle name (expected 64 hex characters): {name}")
            }
            StoreError::DirectoryNameDisagrees { named, actual } => write!(
                f,
                "the directory is named {named} but the package in it digests to {}",
                actual.to_hex()
            ),
        }
    }
}

impl std::error::Error for StoreError {}

fn io<T>(doing: impl Into<String>, result: std::io::Result<T>) -> Result<T, StoreError> {
    result.map_err(|cause| StoreError::Io {
        doing: doing.into(),
        cause,
    })
}

/// One puzzle as it came out of the store, already re-verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// The opened package. Holding one of these means the digest and the signature were
    /// checked, because [`Opened`] cannot be built any other way.
    pub package: Opened,
    /// The raw package bytes, for handing on to a device unchanged.
    pub bytes: Vec<u8>,
    /// The answer, checked against the hash in the signed report.
    pub witness: Vec<u8>,
}

/// A directory holding published puzzles.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Open a store, creating the directory if it is not there.
    ///
    /// An empty directory is a valid empty store, which is what makes "one command takes
    /// an **empty store** to a signed, published, playable puzzle" expressible.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        io(
            format!("creating the store directory {}", root.display()),
            std::fs::create_dir_all(&root),
        )?;
        Ok(Store { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where one puzzle lives.
    #[must_use]
    pub fn directory_of(&self, id: &ContentId) -> PathBuf {
        self.root.join(id.to_hex())
    }

    /// Store a package and its answer, verifying both first.
    ///
    /// The content id is **not** a parameter. It is learned by opening the package, which
    /// is what makes the directory name and the file inside it impossible to disagree.
    /// See the note on this module about nothing entering unverified.
    ///
    /// The write is atomic: the files go into a temporary directory which is then renamed
    /// into place, so an interrupted run leaves either a complete puzzle or nothing. A
    /// half-written package would fail its digest on the way out, which is safe but is a
    /// confusing way to learn the machine lost power.
    pub fn put<D: Digest, V: SignatureVerifier>(
        &self,
        package: &[u8],
        witness: &[u8],
        digest: &D,
        verifier: &V,
    ) -> Result<ContentId, StoreError> {
        let opened = open(package, digest, verifier).map_err(StoreError::NotAValidPackage)?;
        check_witness(&opened, witness)?;

        let id = opened.content_id;
        let final_dir = self.directory_of(&id);
        if final_dir.exists() {
            return Err(StoreError::AlreadyStored(id));
        }

        // The temporary name sits beside the final one, so the rename stays on one
        // filesystem. A rename across filesystems is a copy, which is not atomic.
        let staging = self.root.join(format!(".writing-{}", id.to_hex()));
        if staging.exists() {
            io(
                format!(
                    "clearing a leftover staging directory {}",
                    staging.display()
                ),
                std::fs::remove_dir_all(&staging),
            )?;
        }
        io(
            format!("creating {}", staging.display()),
            std::fs::create_dir_all(&staging),
        )?;

        let write = |name: &str, bytes: &[u8]| -> Result<(), StoreError> {
            let path = staging.join(name);
            io(
                format!("writing {}", path.display()),
                std::fs::write(&path, bytes),
            )
        };
        if let Err(e) = write(PACKAGE_FILE, package).and_then(|()| write(WITNESS_FILE, witness)) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        if let Err(e) = std::fs::rename(&staging, &final_dir) {
            let _ = std::fs::remove_dir_all(&staging);
            // A rename that fails because the destination appeared in the meantime is
            // another writer having stored the same puzzle. Reported as what it is.
            if final_dir.exists() {
                return Err(StoreError::AlreadyStored(id));
            }
            return Err(StoreError::Io {
                doing: format!("moving {} into place", final_dir.display()),
                cause: e,
            });
        }
        Ok(id)
    }

    /// Read one puzzle back, re-verifying it.
    ///
    /// Every read re-checks the digest, the signature and the witness hash. That is not
    /// paranoia about our own disk: it is the first of section 6's four jobs. A store
    /// whose contents are only checked when they were written cannot answer "is the
    /// corpus still sound".
    pub fn get<D: Digest, V: SignatureVerifier>(
        &self,
        name: &str,
        digest: &D,
        verifier: &V,
    ) -> Result<Stored, StoreError> {
        if !is_puzzle_name(name) {
            return Err(StoreError::NotAPuzzleName(name.to_owned()));
        }
        let dir = self.root.join(name);
        if !dir.is_dir() {
            return Err(StoreError::NotStored(name.to_owned()));
        }
        let package_path = dir.join(PACKAGE_FILE);
        let witness_path = dir.join(WITNESS_FILE);
        let bytes = io(
            format!("reading {}", package_path.display()),
            std::fs::read(&package_path),
        )?;
        let witness = io(
            format!("reading {}", witness_path.display()),
            std::fs::read(&witness_path),
        )?;

        let opened = open(&bytes, digest, verifier).map_err(StoreError::NotAValidPackage)?;
        // The directory name is a claim. This is where it is checked.
        if opened.content_id.to_hex() != name {
            return Err(StoreError::DirectoryNameDisagrees {
                named: name.to_owned(),
                actual: opened.content_id,
            });
        }
        check_witness(&opened, &witness)?;
        Ok(Stored {
            package: opened,
            bytes,
            witness,
        })
    }

    /// Every puzzle's name in the store, sorted.
    ///
    /// Sorted because a caller that prints these, or walks them to re-check a corpus,
    /// should produce the same output twice. Directory order is whatever the filesystem
    /// feels like.
    ///
    /// Entries that are not a puzzle directory are skipped rather than reported: the
    /// staging directories [`Store::put`] uses begin with a dot and are transient, and a
    /// store a human has put a `README` in is still a store.
    pub fn list(&self) -> Result<Vec<String>, StoreError> {
        let entries = io(
            format!("listing {}", self.root.display()),
            std::fs::read_dir(&self.root),
        )?;
        let mut ids = Vec::new();
        for entry in entries {
            let entry = io("reading a directory entry", entry)?;
            if !entry.path().is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if is_puzzle_name(&name) {
                ids.push(name);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Re-check every puzzle in the store.
    ///
    /// This is section 6's first job made runnable. It returns what it found rather than
    /// stopping at the first problem, because "one puzzle in the corpus is broken" and
    /// "the corpus is broken" need very different responses, and a walk that stopped
    /// early could not tell them apart.
    pub fn recheck<D: Digest, V: SignatureVerifier>(
        &self,
        digest: &D,
        verifier: &V,
    ) -> Result<Recheck, StoreError> {
        let mut report = Recheck {
            checked: 0,
            failures: Vec::new(),
        };
        for name in self.list()? {
            report.checked += 1;
            if let Err(why) = self.get(&name, digest, verifier) {
                report.failures.push((name, why));
            }
        }
        Ok(report)
    }
}

/// What a re-check found.
#[derive(Debug)]
pub struct Recheck {
    /// How many puzzles were looked at.
    pub checked: usize,
    /// The ones that did not survive, with the reason.
    pub failures: Vec<(String, StoreError)>,
}

impl Recheck {
    /// Whether everything checked out.
    ///
    /// Note what this does **not** say: a store with nothing in it has no failures. A
    /// caller that treats this as "the corpus is sound" without also looking at
    /// [`Recheck::checked`] would read an empty store as a clean bill of health, which
    /// is the shape of a check that passes while verifying nothing.
    #[must_use]
    pub fn all_sound(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Check stored witness bytes against the hash in the package's signed report.
fn check_witness(opened: &Opened, witness: &[u8]) -> Result<(), StoreError> {
    let RecordedReplay::Reproduced(signed) = opened.manifest.replay else {
        return Err(StoreError::NoReproducedWitness);
    };
    let stored = StateHash::of_bytes(witness);
    if stored != signed {
        return Err(StoreError::WitnessDoesNotMatch { signed, stored });
    }
    Ok(())
}

/// Whether a directory name has the shape of a content id: 64 lowercase hex characters.
///
/// Only the shape. Nothing here turns it into a [`ContentId`], because a content id may
/// only come from digesting bytes, and a number parsed out of a directory name is a
/// claim rather than something this code digested. [`Store::get`] re-digests the package
/// and compares, which is what makes a wrong name a refusal instead of a lookup.
///
/// Lowercase only, because [`ContentId::to_hex`] emits lowercase and this is the inverse
/// of that. Accepting uppercase too would mean one puzzle could be stored under two
/// names on a case-sensitive filesystem.
fn is_puzzle_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
