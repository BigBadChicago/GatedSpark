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
//!
//! # Why one page reads three games
//!
//! This page knows Nonogram, Slitherlink and Kakuro. The alternative was a crate per
//! game that opened packages, checked fingerprints and verified signatures all over
//! again, and the trouble with that is not the duplication. It is that there would then
//! be three answers to "is this package trustworthy", they would drift apart, and the
//! one in front of a player would be whichever page they happened to open.
//!
//! So [`load`] is written once. What it does per game is exactly three things: match the
//! family name, match that game's rules version, and hand the payload to that game's own
//! reader. Everything before those three lines — the fingerprint, the signature, the key
//! list — happens once, for every game, in the same code.
//!
//! Adding the third game changed four lines here and nothing else, which is the claim
//! this arrangement was making all along.

use gs_core::content::{ContentId, Digest};
use gs_core::package::{open, OpenError, SignatureVerifier};
use gs_core::state::Dims;
use gs_nonogram::clues::{Clues, Runs};

#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(target_arch = "wasm32")]
pub mod calendar;
pub mod play;
#[cfg(target_arch = "wasm32")]
pub mod saved;

/// Why a package cannot be played here.
///
/// Every variant is a refusal. There is deliberately no variant meaning "played anyway,
/// with reservations".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPlayable {
    /// The bytes are not a valid, correctly signed package. Carries the reason, which
    /// distinguishes a corrupted download from a signature this page will not accept.
    NotAValidPackage(OpenError),
    /// A game this page does not know. Refused rather than attempted: this build has
    /// two families' rules in it and cannot judge a third's.
    WrongFamily { package_says: String },
    /// A version of the rules this build does not implement, in either direction. A
    /// newer one may rely on semantics this build lacks; an older one was verified
    /// under semantics this build no longer has.
    WrongRulesVersion { package_says: u32, we_have: u32 },
    /// The package is sound and for a game this page knows, but its puzzle bytes do
    /// not decode. That means a signed package disagrees with this build's own reader,
    /// which is a bug somewhere rather than a damaged download — the signature already
    /// passed.
    UnreadablePuzzle(BoardProblem),
}

/// A board that would not read, from whichever game's reader refused it.
///
/// Three games, three readers, three error types, and no attempt to flatten them into
/// one. A flattened message would have had to be written for whichever game it came
/// from anyway, and the flattening would only have thrown away the detail needed to
/// write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardProblem {
    Nonogram(gs_nonogram::clues::DecodeError),
    Slitherlink(gs_slitherlink::clues::DecodeError),
    Kakuro(gs_kakuro::clues::DecodeError),
}

impl std::fmt::Display for NotPlayable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotPlayable::NotAValidPackage(why) => {
                write!(f, "{}", plainly(why))
            }
            NotPlayable::WrongFamily { package_says } => write!(
                f,
                "this puzzle is a {package_says}, and this page knows how to play {}",
                KNOWN
                    .iter()
                    .map(|game| game.family)
                    .collect::<Vec<_>>()
                    .join(" and ")
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
                "this puzzle is correctly signed, but its board does not read: {}. \
                 That is a fault in this program, not a damaged download.",
                board_plainly(why)
            ),
        }
    }
}

/// Say in a sentence what went wrong with a package.
///
/// This exists because `{why:?}` prints the name of a variant, and a name like
/// `SignatureRejected` is not a sentence. A player reads these words, so every one of
/// them is written for a player. The compiler holds this to being complete: a new
/// reason added upstream stops this matching, rather than silently printing a name.
fn plainly(why: &OpenError) -> String {
    match why {
        OpenError::NotAPackage => "this file is not a GatedSpark puzzle at all.".to_string(),
        OpenError::UnsupportedFormat(version) => format!(
            "this puzzle was written in a newer file layout (number {version}) than this \
             page knows how to read. Refusing it rather than guessing at it."
        ),
        OpenError::Truncated { field } => format!(
            "this puzzle file stops part way through, while reading its {field}. The \
             download did not finish."
        ),
        OpenError::TrailingBytes(extra) => format!(
            "there are {extra} extra bytes after the end of this puzzle. Something has \
             been added to the file, so it is not the puzzle it was signed as."
        ),
        OpenError::ContentIdMismatch { .. } => {
            "this puzzle is not the puzzle it says it is: its contents do not match its \
             own fingerprint. Some of the bytes changed after it was made."
                .to_string()
        }
        OpenError::SignatureRejected => {
            "this puzzle is not signed by anyone this page trusts. Either it was changed \
             after it was signed, or it came from somewhere else."
                .to_string()
        }
        OpenError::Malformed { field } => {
            format!("this puzzle file is damaged: its {field} does not make sense.")
        }
    }
}

/// The same, for the board inside a package that was otherwise fine.
fn board_plainly(why: &BoardProblem) -> String {
    match why {
        BoardProblem::Nonogram(why) => nonogram_board_plainly(why),
        BoardProblem::Slitherlink(why) => slitherlink_board_plainly(why),
        // Kakuro writes its own sentences, in its own crate beside the format they
        // describe. The other two are spelled out here because their decoders predate
        // the convention; a fourth game should do what this one does.
        BoardProblem::Kakuro(why) => why.to_string(),
    }
}

fn nonogram_board_plainly(why: &gs_nonogram::clues::DecodeError) -> String {
    use gs_nonogram::clues::DecodeError as E;
    match why {
        E::Truncated { field } => {
            format!("the clue numbers stop part way through, at the {field}")
        }
        E::TrailingBytes(extra) => {
            format!("there are {extra} bytes left over after the clue numbers")
        }
        E::LineCountDisagreesWithDims {
            axis,
            expected,
            found,
        } => {
            format!("the board says it is {expected} {axis} across, and then gives {found} of them")
        }
    }
}

fn slitherlink_board_plainly(why: &gs_slitherlink::clues::DecodeError) -> String {
    use gs_slitherlink::board::SizeError;
    use gs_slitherlink::clues::{CluesError, DecodeError as E};
    match why {
        E::Truncated { field } => {
            format!("the numbers stop part way through, at the {field}")
        }
        E::TrailingBytes(extra) => {
            format!("there are {extra} bytes left over after the numbers")
        }
        E::CountDisagreesWithSize { expected, found } => format!(
            "the board is {expected} squares in size, and then gives {found} numbers for it"
        ),
        E::NotAClue { cell, byte } => format!(
            "square {cell} holds {byte}, which is neither a number a square can have nor \
             the mark for no number at all"
        ),
        E::Size(SizeError::TooSmall { width, height }) => {
            format!("the board says it is {width} by {height}, which is not a board")
        }
        E::Size(SizeError::TooBig { edges }) => {
            format!("the board says it has {edges} lines on it, which is more than can be played")
        }
        E::Clues(CluesError::WrongLength { expected, found }) => {
            format!("the board wants {expected} numbers and gives {found}")
        }
        E::Clues(CluesError::TooLarge { cell, clue }) => {
            format!("square {cell} says {clue} of its sides are used, and a square has only four")
        }
    }
}

impl std::error::Error for NotPlayable {}

/// One game this page knows, and the one version of its rules this build implements.
///
/// A list rather than a pair of constants, so the refusal message can name the games
/// without a second list to keep in step with this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownGame {
    pub family: &'static str,
    pub rules_version: u32,
}

/// Every game this page can play. Nothing outside this list is attempted.
pub const KNOWN: [KnownGame; 3] = [
    KnownGame {
        family: gs_nonogram::rules::FAMILY.0,
        rules_version: gs_nonogram::rules::RULESET_VERSION,
    },
    KnownGame {
        family: gs_slitherlink::rules::FAMILY.0,
        rules_version: gs_slitherlink::rules::RULESET_VERSION,
    },
    KnownGame {
        family: gs_kakuro::rules::FAMILY.0,
        rules_version: gs_kakuro::rules::RULESET_VERSION,
    },
];

/// The board inside a package, in whichever game's own terms.
///
/// This is the only place the games differ in this module. Everything else about a
/// package — fingerprint, signature, key, level, encoding — is the same for all three
/// and is handled once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Board {
    /// Numbers down the side and along the top, for a picture.
    Nonogram(Clues),
    /// A number in some squares, for one closed loop.
    Slitherlink(gs_slitherlink::clues::Clues),
    /// Black squares, and a total for each run of white ones.
    ///
    /// The whole puzzle, not a clue list beside one: a Kakuro's black squares are the
    /// board's shape and the shape is part of the puzzle, so `gs-kakuro` has nothing
    /// smaller than this to hand over.
    Kakuro(gs_kakuro::Kakuro),
}

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
    /// The level the game assigned, if it assigned one. Slitherlink assigns none, on
    /// purpose, so this is `None` for every puzzle of it.
    pub band: Option<String>,
    /// What the uniqueness claim was made relative to. Carried because uniqueness is
    /// only ever true relative to an encoding, and a page that showed "one answer only"
    /// without it would be claiming more than was proved.
    pub encoding: String,
    pub board: Board,
}

impl Playable {
    /// Which game this is, in the same spelling the package uses.
    #[must_use]
    pub fn family(&self) -> &'static str {
        match self.board {
            Board::Nonogram(_) => gs_nonogram::rules::FAMILY.0,
            Board::Slitherlink(_) => gs_slitherlink::rules::FAMILY.0,
            Board::Kakuro(_) => gs_kakuro::rules::FAMILY.0,
        }
    }

    /// The Nonogram board, or nothing if this is not a Nonogram.
    ///
    /// Returning nothing rather than a blank board of the wrong game: a page that drew
    /// an empty grid for a Slitherlink would look like a Nonogram with no clues, which
    /// is a puzzle, rather than like a mistake, which is what it is.
    #[must_use]
    pub fn nonogram(&self) -> Option<&Clues> {
        match &self.board {
            Board::Nonogram(clues) => Some(clues),
            _ => None,
        }
    }

    /// The Slitherlink board, or nothing if this is not a Slitherlink.
    #[must_use]
    pub fn slitherlink(&self) -> Option<&gs_slitherlink::clues::Clues> {
        match &self.board {
            Board::Slitherlink(clues) => Some(clues),
            _ => None,
        }
    }

    /// The Kakuro board, or nothing if this is not a Kakuro.
    #[must_use]
    pub fn kakuro(&self) -> Option<&gs_kakuro::Kakuro> {
        match &self.board {
            Board::Kakuro(puzzle) => Some(puzzle),
            _ => None,
        }
    }

    /// The size of the grid a page draws: squares across and down, for either game.
    #[must_use]
    pub fn dims(&self) -> Dims {
        match &self.board {
            Board::Nonogram(clues) => clues.dims,
            Board::Slitherlink(clues) => {
                Dims::new(clues.geometry().width(), clues.geometry().height())
            }
            // The grid a page draws, black squares included — not the state's shape,
            // which has one entry per white square and is one row long.
            Board::Kakuro(puzzle) => Dims::new(puzzle.layout.width(), puzzle.layout.height()),
        }
    }

    /// The clue numbers down the left-hand side. Empty for a game that has none.
    #[must_use]
    pub fn row_clues(&self) -> &[Runs] {
        match &self.board {
            Board::Nonogram(clues) => &clues.rows,
            _ => &[],
        }
    }

    /// The clue numbers along the top. Empty for a game that has none.
    #[must_use]
    pub fn column_clues(&self) -> &[Runs] {
        match &self.board {
            Board::Nonogram(clues) => &clues.columns,
            _ => &[],
        }
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
    // over the fingerprint. Everything below runs only because it returned Ok. It is
    // the same call for every game, which is the point of there being one loader.
    let opened = open(bytes, digest, keys).map_err(NotPlayable::NotAValidPackage)?;

    let Some(game) = KNOWN
        .iter()
        .find(|game| game.family == opened.manifest.family)
    else {
        return Err(NotPlayable::WrongFamily {
            package_says: opened.manifest.family.clone(),
        });
    };
    if opened.manifest.ruleset_version != game.rules_version {
        return Err(NotPlayable::WrongRulesVersion {
            package_says: opened.manifest.ruleset_version,
            we_have: game.rules_version,
        });
    }

    // The one game-specific line. Each game's own reader decides whether its own bytes
    // read, because a reader written here would be a second opinion about a format this
    // page does not own.
    let board = if game.family == gs_nonogram::rules::FAMILY.0 {
        Board::Nonogram(
            Clues::from_canonical_bytes(&opened.manifest.payload)
                .map_err(|why| NotPlayable::UnreadablePuzzle(BoardProblem::Nonogram(why)))?,
        )
    } else if game.family == gs_slitherlink::rules::FAMILY.0 {
        Board::Slitherlink(
            gs_slitherlink::clues::Clues::from_canonical_bytes(&opened.manifest.payload)
                .map_err(|why| NotPlayable::UnreadablePuzzle(BoardProblem::Slitherlink(why)))?,
        )
    } else if game.family == gs_kakuro::rules::FAMILY.0 {
        Board::Kakuro(
            gs_kakuro::clues::from_canonical_bytes(&opened.manifest.payload)
                .map_err(|why| NotPlayable::UnreadablePuzzle(BoardProblem::Kakuro(why)))?,
        )
    } else {
        // Unreachable while `KNOWN` and this chain name the same games, and written
        // out rather than folded into the last branch for exactly that reason. With a
        // bare `else` the third game read every unknown family's bytes as its own: a
        // fourth game added to `KNOWN` and forgotten here would have been handed to
        // Kakuro's reader, which would usually refuse it and occasionally not.
        return Err(NotPlayable::WrongFamily {
            package_says: opened.manifest.family.clone(),
        });
    };

    Ok(Playable {
        content_id: opened.content_id,
        signed_by: opened.key_id,
        band: opened.manifest.band,
        encoding: opened.manifest.encoding,
        board,
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
