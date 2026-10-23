//! The browser's half of GatedSpark: open a signed puzzle, and refuse everything else.
//!
//! # What this crate is allowed to do
//!
//! It plays puzzles and checks signatures. It does not generate, prove, or sign
//! anything, and it cannot: nothing it depends on can run a solver, write a file, or
//! produce a random number.
//!
//! That is not a convention. `gs-solver` and `gs-store` both compile for the browser's
//! target without a complaint, and then do nothing at run time — starting a program and
//! reading a folder are things a browser cannot do. A browser carrying the solver runner
//! would not crash. It would answer "don't know" to every question, which the pipeline
//! correctly refuses to treat as a proof, and so it would quietly become a browser in
//! which no puzzle is ever provably unique. Every puzzle would be rejected, nothing
//! would look broken, and the cause would be three levels down in a dependency list.
//!
//! So the dependency list is the boundary, and `tests/boundary.rs` is what holds it:
//! it walks this crate's workspace dependencies and fails if either crate is reachable.
//!
//! # Why the trust chain ends here rather than at the server
//!
//! A puzzle arrives as the bytes `gatedspark fill` wrote: a package carrying its own
//! fingerprint, a signature over that fingerprint, and the validation report describing
//! what a solver was asked. [`load`] recomputes the fingerprint from the bytes and
//! checks the signature against a key list the caller supplies. Nothing it believes
//! comes from the web server it was fetched from.
//!
//! The key list has to come from the page, not from beside the content. A reader that
//! trusted a key list it found next to the puzzle would trust whatever anybody put
//! there, which is the same reasoning that makes `gatedspark show` take `--key`.
//!
//! # Why the family and the rules version are checked
//!
//! A package names which game it is and which version of that game's rules it was
//! verified under. A reader that has a different version does not have "nearly the same
//! puzzle": a replay against different semantics is not a replay, and every claim in the
//! report was made about rules this build may no longer implement. So both are checked,
//! and a mismatch is a refusal rather than an attempt.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{open, OpenError, SignatureVerifier};
use gs_core::state::Dims;
use gs_nonogram::clues::{Clues, DecodeError, Runs};
use gs_nonogram::rules::{FAMILY, RULESET_VERSION};

#[cfg(target_arch = "wasm32")]
pub mod browser;
pub mod play;

/// Why a package cannot be played here.
///
/// Every variant is a refusal. There is deliberately no variant meaning "played anyway,
/// with reservations".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPlayable {
    /// The bytes are not a valid, correctly signed package. Carries the reason, which
    /// distinguishes a corrupted download from a signature this page will not accept.
    NotAValidPackage(OpenError),
    /// A different game. Refused rather than attempted: this build has one family's
    /// rules in it and cannot judge another's.
    WrongFamily { package_says: String },
    /// A version of the rules this build does not implement, in either direction. A
    /// newer one may rely on semantics this build lacks; an older one was verified
    /// under semantics this build no longer has.
    WrongRulesVersion { package_says: u32, we_have: u32 },
    /// The package is sound and for this game, but its puzzle bytes do not decode.
    /// That means a signed package disagrees with this build's own reader, which is a
    /// bug somewhere rather than a damaged download — the signature already passed.
    UnreadablePuzzle(DecodeError),
}

impl std::fmt::Display for NotPlayable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotPlayable::NotAValidPackage(why) => {
                write!(f, "this is not a puzzle this page will accept: {why:?}")
            }
            NotPlayable::WrongFamily { package_says } => write!(
                f,
                "this puzzle is a {package_says}, and this page only knows how to play {}",
                FAMILY.0
            ),
            NotPlayable::WrongRulesVersion {
                package_says,
                we_have,
            } => write!(
                f,
                "this puzzle was checked under rules version {package_says}, and this page \
                 implements version {we_have}. Refusing it rather than guessing."
            ),
            NotPlayable::UnreadablePuzzle(why) => write!(
                f,
                "this puzzle is correctly signed but its board does not read: {why:?}. \
                 That is a bug here, not a damaged download."
            ),
        }
    }
}

impl std::error::Error for NotPlayable {}

/// A puzzle that passed every check and can be drawn and played.
///
/// There is no way to build one of these except through [`load`], which is what makes
/// its existence mean "the signature checked out". The fields are the ones a page needs
/// to draw a board and tell a person what they are looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playable {
    /// The fingerprint, recomputed from the bytes rather than read out of them.
    pub content_id: ContentId,
    /// Which key vouched for it. Already accepted by the key list.
    pub signed_by: String,
    /// The level the game assigned, if it assigned one.
    pub band: Option<String>,
    /// What the uniqueness claim was made relative to. Carried because uniqueness is
    /// only ever true relative to an encoding, and a page that showed "one answer only"
    /// without it would be claiming more than was proved.
    pub encoding: String,
    pub clues: Clues,
}

impl Playable {
    #[must_use]
    pub fn dims(&self) -> Dims {
        self.clues.dims
    }

    #[must_use]
    pub fn row_clues(&self) -> &[Runs] {
        &self.clues.rows
    }

    #[must_use]
    pub fn column_clues(&self) -> &[Runs] {
        &self.clues.columns
    }
}

/// Open a package and give back something playable, or say why not.
///
/// The caller supplies the digest and the key list. Both come from the page rather than
/// from the package, which is the whole point: a package cannot nominate the key that
/// vouches for it.
pub fn load<D: Digest, V: SignatureVerifier>(
    bytes: &[u8],
    digest: &D,
    keys: &V,
) -> Result<Playable, NotPlayable> {
    // This one call recomputes the fingerprint over the bytes and checks the signature
    // over the fingerprint. Everything below runs only because it returned Ok.
    let opened = open(bytes, digest, keys).map_err(NotPlayable::NotAValidPackage)?;

    if opened.manifest.family != FAMILY.0 {
        return Err(NotPlayable::WrongFamily {
            package_says: opened.manifest.family.clone(),
        });
    }
    if opened.manifest.ruleset_version != RULESET_VERSION {
        return Err(NotPlayable::WrongRulesVersion {
            package_says: opened.manifest.ruleset_version,
            we_have: RULESET_VERSION,
        });
    }

    let clues = Clues::from_canonical_bytes(&opened.manifest.payload)
        .map_err(NotPlayable::UnreadablePuzzle)?;

    Ok(Playable {
        content_id: opened.content_id,
        signed_by: opened.key_id,
        band: opened.manifest.band,
        encoding: opened.manifest.encoding,
        clues,
    })
}

// ---------------------------------------------------------------------------
// The dependency boundary, as code rather than as a comment
// ---------------------------------------------------------------------------

/// Crates that must never be reachable from this one.
///
/// Public so the boundary test names them from here rather than keeping its own list,
/// which would be two lists that could disagree.
pub const MUST_NOT_REACH: [&str; 2] = ["gs-solver", "gs-store"];

/// One workspace crate another one names, and whether it only names it for tests.
///
/// The distinction is load-bearing. A crate's `[dev-dependencies]` are compiled into
/// its own test binaries and nothing else, so `gs-nonogram` depending on `gs-solver`
/// for its tests puts nothing into a browser. Treating the two the same made the first
/// run of the boundary test fail on exactly that, which is how this type came to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    /// True for `[dev-dependencies]` and `[build-dependencies]`: present when this
    /// crate is tested or built, absent from anything that depends on it.
    pub only_for_tests: bool,
}

/// The workspace crates one manifest's text names, and in which section.
///
/// Deliberately a string scan rather than a call to Cargo: the test using it runs in
/// the ordinary suite, on every platform, with no network and no lockfile. It reads the
/// `path = "../name"` entries a workspace crate uses for its siblings, in whichever
/// section they appear, including a target-gated one.
///
/// It can only see what a manifest says, which is exactly the claim being made: that no
/// manifest in the reachable set names a forbidden crate. If Cargo ever grew a way to
/// depend on a sibling without naming it, this would not see it — and nothing in this
/// workspace does that.
#[must_use]
pub fn workspace_dependencies_of(manifest: &str) -> Vec<Dependency> {
    let mut found: Vec<Dependency> = Vec::new();
    // Until a section header says otherwise, we are not in a dependency section at all.
    let mut section: Option<bool> = None;
    for line in manifest.lines() {
        let line = line.trim();
        // A comment naming a crate is not a dependency on it. This matters here more
        // than usual: the manifest this runs against explains at length why two crates
        // are absent, and naming them is the explanation.
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // `[target.'cfg(...)'.dependencies]` counts as `[dependencies]`, which is
            // what makes the browser-only `wasm-bindgen` entry visible to this scan.
            section =
                if line.ends_with("dev-dependencies]") || line.ends_with("build-dependencies]") {
                    Some(true)
                } else if line.ends_with("dependencies]") {
                    Some(false)
                } else {
                    None
                };
            continue;
        }
        let Some(only_for_tests) = section else {
            continue;
        };
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !rest.contains("path") || !name.starts_with("gs-") {
            continue;
        }
        // A crate named in two sections is recorded by its strongest claim: a real
        // dependency wins over a test-only one.
        if let Some(already) = found.iter_mut().find(|d| d.name == name) {
            already.only_for_tests &= only_for_tests;
        } else {
            found.push(Dependency {
                name: name.to_owned(),
                only_for_tests,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(manifest: &str) -> Vec<String> {
        workspace_dependencies_of(manifest)
            .into_iter()
            .map(|d| d.name)
            .collect()
    }

    // The scanner the boundary test leans on, checked against manifests written here.
    // Without these, a scanner that always returned an empty list would make the
    // boundary test pass while checking nothing.

    #[test]
    fn the_scanner_finds_a_path_dependency() {
        let manifest = r#"
[dependencies]
gs-core = { path = "../gs-core" }
gs-nonogram = { path = "../gs-nonogram" }
"#;
        assert_eq!(names(manifest), vec!["gs-core", "gs-nonogram"]);
    }

    #[test]
    fn the_scanner_finds_a_forbidden_one() {
        // The case the boundary test exists to catch, proved to be catchable.
        let manifest = "[dependencies]\ngs-solver = { path = \"../gs-solver\" }\n";
        assert_eq!(names(manifest), vec!["gs-solver"]);
    }

    #[test]
    fn a_test_only_dependency_is_marked_as_one() {
        // The distinction the first run of the boundary test forced into existence.
        // `gs-nonogram` names `gs-solver` for its own tests, which puts nothing into a
        // browser, and a scanner that could not tell made the boundary unholdable.
        let manifest = "[dev-dependencies]\ngs-store = { path = \"../gs-store\" }\n";
        assert_eq!(
            workspace_dependencies_of(manifest),
            vec![Dependency {
                name: "gs-store".to_owned(),
                only_for_tests: true
            }]
        );
    }

    #[test]
    fn a_real_dependency_wins_over_a_test_only_one() {
        let manifest = "[dependencies]\ngs-core = { path = \"../gs-core\" }\n\
                        [dev-dependencies]\ngs-core = { path = \"../gs-core\" }\n";
        assert_eq!(
            workspace_dependencies_of(manifest),
            vec![Dependency {
                name: "gs-core".to_owned(),
                only_for_tests: false
            }]
        );
    }

    #[test]
    fn a_target_gated_section_is_a_real_dependency_section() {
        // This is how the browser-only translation layer is declared, so a scanner that
        // skipped target-gated sections would not see it.
        let manifest = "[target.'cfg(target_arch = \"wasm32\")'.dependencies]\n\
                        gs-core = { path = \"../gs-core\" }\n";
        assert_eq!(
            workspace_dependencies_of(manifest),
            vec![Dependency {
                name: "gs-core".to_owned(),
                only_for_tests: false
            }]
        );
    }

    #[test]
    fn a_crate_named_outside_a_dependency_section_is_not_a_dependency() {
        let manifest = "[package]\nname = \"gs-web\"\n\
                        gs-solver = { path = \"../gs-solver\" }\n";
        assert!(workspace_dependencies_of(manifest).is_empty());
    }

    #[test]
    fn a_crate_named_in_a_comment_is_not_a_dependency() {
        // This crate's own manifest spends a paragraph explaining which crates are
        // absent and why. A scanner that counted those would make the boundary
        // unholdable, and the natural fix would be to delete the explanation.
        let manifest = "[dependencies]\n# gs-solver is absent on purpose; see ADR 0009\n\
                        gs-core = { path = \"../gs-core\" }\n";
        assert_eq!(names(manifest), vec!["gs-core"]);
    }

    #[test]
    fn a_published_dependency_without_a_path_is_not_a_workspace_crate() {
        let manifest = "[dependencies]\nsha2 = \"0.11.1\"\n";
        assert!(workspace_dependencies_of(manifest).is_empty());
    }
}
