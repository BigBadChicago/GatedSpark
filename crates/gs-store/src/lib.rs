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
//! It mostly closes itself. The validation report already records the **state hash of
//! the witness**, as `Replay::Reproduced(hash)`, and that field is inside the package, so
//! it is covered by the signature. [`StateHash::of`] is defined as `of_bytes` over
//! [`State::canonical_bytes`](gs_core::state::State::canonical_bytes) — exactly the
//! bytes stored here — so a stored witness can be checked against a number that was
//! signed at the moment the puzzle was published. No new format and no second digest.
//!
//! # What that check does and does not cover
//!
//! Stated precisely, because the first version of this paragraph claimed more than the
//! check delivers.
//!
//! `StateHash` is FNV-1a over 64 bits, and `gs_core::hash` is explicit that the strength
//! needed of it is "none; an accidental collision is what is being guarded against". It
//! is a replay-divergence check, not a cryptographic one. So:
//!
//! - **Covered:** an answer file that was corrupted, truncated, half-written, or edited
//!   by someone not trying to defeat the check. A change made without aiming at the hash
//!   is caught with overwhelming probability, and the failure names both numbers.
//! - **Not covered:** someone who can write into this store *and* wants to substitute a
//!   different answer. Finding a second byte string with the same 64-bit FNV hash is
//!   cheap, so a determined writer can pass this check.
//!
//! What bounds the damage is that the package itself is signed. The puzzle, its clues,
//! its report and the questions the solver answered cannot be changed at all. Only the
//! cached answer can, and an answer is recomputable from the clues — the store keeps it
//! so a puzzle need not be re-solved, not because it is the only copy.
//!
//! Making this adversarial would mean recording the witness under a cryptographic digest
//! as well, which changes `Manifest` and so changes what every existing signature
//! covers. That is a decision rather than a tidy-up, and it is recorded in ADR 0008 and
//! ADR 0003's not-enforced list rather than taken quietly here.
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
use gs_core::withdrawal::{open_list, OpenedList};

/// The file inside a puzzle's directory that holds the package.
pub const PACKAGE_FILE: &str = "package.gspk";

/// The file inside a puzzle's directory that holds the answer.
pub const WITNESS_FILE: &str = "witness.bin";

/// The file at the store's root that holds the signed "do not play these" list.
///
/// At the root rather than in a puzzle's directory, because it is about the store as a
/// whole and there is exactly one current one. [`Store::list`] skips it, because it only
/// reports directories.
pub const WITHDRAWALS_FILE: &str = "withdrawals.gspk";

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
    /// The withdrawal list did not survive being opened. Its own variant rather than
    /// sharing the one above, because "the list of withdrawn puzzles is unreadable" and
    /// "a puzzle is unreadable" need very different responses from whoever reads this.
    NotAValidList(gs_core::package::OpenError),
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
            StoreError::NotAValidList(why) => {
                write!(f, "not a valid list of withdrawn puzzles: {why:?}")
            }
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

    /// Where the current withdrawal list is kept.
    #[must_use]
    pub fn withdrawals_path(&self) -> PathBuf {
        self.root.join(WITHDRAWALS_FILE)
    }

    /// Write a signed withdrawal list, checking it first.
    ///
    /// The same rule as [`Store::put`]: nothing enters this store unverified. A list
    /// that cannot be opened here would be a list a device refuses, discovered by the
    /// devices rather than by the publisher.
    ///
    /// It does **not** check that the serial number is higher than the one already
    /// there. That is a decision about what to publish, not about what a store may hold,
    /// and the caller is the one that knows whether it is replacing a list or recovering
    /// from a mistake. [`Store::withdrawals`] is how the caller finds out what is there.
    pub fn put_withdrawals<D: Digest, V: SignatureVerifier>(
        &self,
        bytes: &[u8],
        digest: &D,
        verifier: &V,
    ) -> Result<OpenedList, StoreError> {
        let opened = open_list(bytes, digest, verifier).map_err(StoreError::NotAValidList)?;
        let path = self.withdrawals_path();
        // Written beside the target and renamed, so an interrupted write leaves the old
        // list in place rather than half of a new one. A half-written list would be
        // refused by every device, which is safe, but it would also mean no withdrawal
        // at all.
        let staging = self.root.join(".withdrawals.gspk.part");
        io(
            format!("writing {}", staging.display()),
            std::fs::write(&staging, bytes),
        )?;
        io(
            format!("putting {} in place", path.display()),
            std::fs::rename(&staging, &path),
        )?;
        Ok(opened)
    }

    /// The current withdrawal list, read back through its signature.
    ///
    /// `Ok(None)` means there is no list, which is a different fact from a list naming
    /// nothing: the second one is a publisher saying so, and the first is a publisher
    /// who has not said anything yet.
    pub fn withdrawals<D: Digest, V: SignatureVerifier>(
        &self,
        digest: &D,
        verifier: &V,
    ) -> Result<Option<OpenedList>, StoreError> {
        let path = self.withdrawals_path();
        match std::fs::read(&path) {
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(cause) => Err(StoreError::Io {
                doing: format!("reading {}", path.display()),
                cause,
            }),
            Ok(bytes) => open_list(&bytes, digest, verifier)
                .map(Some)
                .map_err(StoreError::NotAValidList),
        }
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
