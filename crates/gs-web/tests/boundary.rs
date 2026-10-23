//! The dependency boundary, held by a test rather than by a comment.
//!
//! `gs-solver` and `gs-store` both compile for the browser's target without complaint,
//! and then do nothing at run time. A browser carrying the solver runner would not
//! crash: it would answer "don't know" to every question, which the pipeline correctly
//! refuses to treat as a proof, and so it would become a browser in which no puzzle is
//! ever provably unique. Nothing would look broken. The cause would be a line in a
//! dependency list three crates away.
//!
//! That is why this is a test. A note in a document does not survive somebody needing
//! one function from one of those crates at eleven at night.

use std::path::{Path, PathBuf};

use gs_web::{workspace_dependencies_of, MUST_NOT_REACH};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate lives in crates/")
        .to_path_buf()
}

fn manifest_of(crate_name: &str) -> String {
    let path = crates_dir().join(crate_name).join("Cargo.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every workspace crate that ends up compiled into `start`, including `start` itself.
///
/// `start`'s own test-only dependencies count, because they are compiled into its test
/// binaries and because one appearing there means the design has drifted. A *dependency*
/// crate's test-only dependencies do not: `gs-nonogram` naming `gs-solver` for its own
/// tests puts nothing into anything that depends on `gs-nonogram`.
///
/// This distinction is not a refinement thought up in advance. The first run of this
/// test failed on exactly that case, which is the only reason the scanner can tell the
/// two sections apart.
fn everything_compiled_into(start: &str) -> Vec<String> {
    let mut found = vec![start.to_owned()];
    // The root's test-only dependencies are included; deeper ones are not, so the
    // queue carries only crates whose own test dependencies should be ignored.
    let mut to_visit = Vec::new();
    for dependency in workspace_dependencies_of(&manifest_of(start)) {
        found.push(dependency.name.clone());
        to_visit.push(dependency.name);
    }
    while let Some(name) = to_visit.pop() {
        for dependency in workspace_dependencies_of(&manifest_of(&name)) {
            if dependency.only_for_tests || found.contains(&dependency.name) {
                continue;
            }
            found.push(dependency.name.clone());
            to_visit.push(dependency.name);
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_solver_runner_and_the_store_are_not_compiled_into_the_browser() {
    let reachable = everything_compiled_into("gs-web");
    for forbidden in MUST_NOT_REACH {
        assert!(
            !reachable.contains(&forbidden.to_owned()),
            "`{forbidden}` is compiled into gs-web, through: {reachable:?}\n\n\
             It compiles for the browser and does nothing there. Read ADR 0009 before \
             deciding this is fine: a browser that contains the solver runner reports \
             \"don't know\" for every question and quietly becomes one where no puzzle \
             is ever provably unique."
        );
    }
}

#[test]
fn the_browser_contains_exactly_what_it_is_supposed_to() {
    // The other half. Without it, this file would still pass if somebody deleted
    // gs-web's dependencies entirely, or if the manifest scanner silently stopped
    // finding anything.
    assert_eq!(
        everything_compiled_into("gs-web"),
        vec![
            "gs-core".to_owned(),
            "gs-crypto".to_owned(),
            "gs-nonogram".to_owned(),
            "gs-offline".to_owned(),
            "gs-web".to_owned(),
        ],
        "what is compiled into the browser changed. That is a decision, not a detail."
    );
}

#[test]
fn the_walk_really_does_follow_dependencies() {
    // Proof that the walk is transitive rather than one level deep, using the crate
    // whose dependencies are known: gs-cli is the one that reaches everything. Without
    // this, a walk that returned only the crate it started from would pass every
    // assertion above while checking nothing.
    let reachable = everything_compiled_into("gs-cli");
    for expected in [
        "gs-core",
        "gs-crypto",
        "gs-nonogram",
        "gs-solver",
        "gs-store",
    ] {
        assert!(
            reachable.contains(&expected.to_owned()),
            "{expected} should be reachable from gs-cli, found: {reachable:?}"
        );
    }
}

#[test]
fn the_forbidden_crates_still_exist_under_those_names() {
    // If `gs-solver` were ever renamed, the guard above would keep passing while
    // guarding nothing: a name that matches no crate can never be found. This is the
    // anti-vacuity check for the boundary, in the same spirit as the one in gs-core. So the names
    // are checked against the directories they refer to.
    for forbidden in MUST_NOT_REACH {
        assert!(
            crates_dir().join(forbidden).join("Cargo.toml").is_file(),
            "MUST_NOT_REACH names `{forbidden}`, but there is no such crate. \
             Either it was renamed, in which case fix the name here, or it was \
             removed, in which case remove it here."
        );
    }
}
