//! The development signing key file, and the guards around it.
//!
//! # What this is for, and what it is not
//!
//! M5's exit test needs *a* key, because nothing can be published unsigned. Where a
//! **production** key comes from is deliberately undecided — key custody, rotation and
//! the trusted-key list a device ships with are operational questions, recorded as open
//! in ADR 0006 — and nothing in the slice needs them answered.
//!
//! So this makes a key that says, in the file itself, that it must not be used to
//! publish anything real.
//!
//! # The guards, and why each one is here
//!
//! A convenient key generator is the usual way a private key ends up committed to a
//! public repository, so the convenience has to be paid for:
//!
//! * **It refuses to write inside a Git working tree.** Not a lint, a refusal. This is
//!   the guard that matters, because it stops the mistake at the only moment anybody is
//!   paying attention.
//! * **It refuses to overwrite an existing file.** A key that silently replaced another
//!   would invalidate every signature made with the old one, and the packages would
//!   still be sitting in a store looking fine until something tried to read them.
//! * **On Unix the file is owner-read-only**, so it is not world-readable by default.
//! * **The first line of the file says what it is**, in words, so anybody who opens it
//!   knows before they read the key.

use std::io::Write;
use std::path::{Path, PathBuf};

use gs_crypto::Ed25519Signer;

/// The first line of every key file this writes.
pub const WARNING: &str = "# GATEDSPARK DEVELOPMENT SIGNING KEY - NOT FOR PUBLISHING ANYTHING REAL";

const SECOND_LINE: &str = "# Anything signed with this key is signed by nobody. Do not commit it.";

/// Why a key could not be made or read.
#[derive(Debug)]
pub enum KeyFileError {
    /// The path is inside a Git working tree. Carries the directory where `.git` was
    /// found, so the message can say which repository was about to be polluted.
    InsideAGitRepository { found_at: PathBuf },
    /// Something is already there. Never overwritten.
    AlreadyExists(PathBuf),
    Io {
        doing: String,
        cause: std::io::Error,
    },
    /// The OS would not give us random bytes. A key from a broken source of randomness
    /// is worse than no key, so this is an error and never a fallback.
    NoRandomness(String),
    /// The file is not a key file this understands.
    Malformed { path: PathBuf, why: String },
}

impl std::fmt::Display for KeyFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyFileError::InsideAGitRepository { found_at } => write!(
                f,
                "refusing to write a signing key inside the Git repository at {}.\n\
                 A private key in a repository is a published private key. Put it \
                 somewhere outside, such as your home directory.",
                found_at.display()
            ),
            KeyFileError::AlreadyExists(path) => write!(
                f,
                "{} already exists, and a key is never overwritten.\n\
                 Replacing it would invalidate every signature made with the old one.",
                path.display()
            ),
            KeyFileError::Io { doing, cause } => write!(f, "{doing}: {cause}"),
            KeyFileError::NoRandomness(why) => write!(
                f,
                "could not get random bytes from the operating system: {why}.\n\
                 No key was written; a key from a broken source of randomness is worse \
                 than none."
            ),
            KeyFileError::Malformed { path, why } => {
                write!(f, "{} is not a key file: {why}", path.display())
            }
        }
    }
}

impl std::error::Error for KeyFileError {}

fn io<T>(doing: impl Into<String>, result: std::io::Result<T>) -> Result<T, KeyFileError> {
    result.map_err(|cause| KeyFileError::Io {
        doing: doing.into(),
        cause,
    })
}

/// Walk upwards looking for a `.git` entry.
///
/// Both a directory (an ordinary clone) and a file (a worktree or submodule, where
/// `.git` is a file pointing elsewhere) count, because a key committed from either is
/// just as committed.
fn git_repository_above(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut at = absolute.parent()?.to_path_buf();
    loop {
        if at.join(".git").exists() {
            return Some(at);
        }
        if !at.pop() {
            return None;
        }
    }
}

/// Make a new development key and write it to `path`.
///
/// Returns the key id it chose, which is derived from the public key so that two
/// development keys cannot collide under one name.
pub fn generate(path: &Path) -> Result<String, KeyFileError> {
    if let Some(found_at) = git_repository_above(path) {
        return Err(KeyFileError::InsideAGitRepository { found_at });
    }
    if path.exists() {
        return Err(KeyFileError::AlreadyExists(path.to_path_buf()));
    }

    let mut private = [0u8; 32];
    getrandom::fill(&mut private).map_err(|e| KeyFileError::NoRandomness(e.to_string()))?;

    // The id is the first bytes of the public key, so it names this key and nothing
    // else. A counter or a date would eventually name two different keys the same.
    let signer = Ed25519Signer::new("placeholder", &private);
    let public = signer.public_key();
    let key_id = format!("dev-{}", hex(&public[..6]));

    let contents = format!(
        "{WARNING}\n{SECOND_LINE}\nkey-id: {key_id}\nprivate-key: {}\npublic-key: {}\n",
        hex(&private),
        hex(&public)
    );

    // Created with the restrictive mode from the start on Unix, rather than written and
    // then chmod'ed: between those two steps the key would be readable by anyone.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = io(format!("creating {}", path.display()), options.open(path))?;
    io(
        format!("writing {}", path.display()),
        file.write_all(contents.as_bytes()),
    )?;
    Ok(key_id)
}

/// Read a key file and build a signer from it.
pub fn load(path: &Path) -> Result<Ed25519Signer, KeyFileError> {
    let text = io(
        format!("reading {}", path.display()),
        std::fs::read_to_string(path),
    )?;
    let field = |name: &str| -> Result<String, KeyFileError> {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{name}:")))
            .map(|v| v.trim().to_owned())
            .ok_or_else(|| KeyFileError::Malformed {
                path: path.to_path_buf(),
                why: format!("no `{name}:` line"),
            })
    };
    let key_id = field("key-id")?;
    let private = unhex(&field("private-key")?).ok_or_else(|| KeyFileError::Malformed {
        path: path.to_path_buf(),
        why: "the private key is not 32 bytes of hex".to_owned(),
    })?;
    Ok(Ed25519Signer::new(key_id, &private))
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).expect("nibble"));
        out.push(char::from_digit(u32::from(b & 0x0f), 16).expect("nibble"));
    }
    out
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}
