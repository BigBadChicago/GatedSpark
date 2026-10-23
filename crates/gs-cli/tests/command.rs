//! The command, driven the way a person drives it.
//!
//! The key file handling and the argument parsing live inside the binary, so they are
//! tested by running it rather than by calling into it. That is the better test anyway:
//! what matters about `keygen` refusing to write into a repository is that the *command*
//! refuses and writes nothing, not that some function returned an error.
//!
//! The whole-loop test at the bottom needs a real solver, and says so loudly when there
//! is none rather than passing quietly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GATEDSPARK: &str = env!("CARGO_BIN_EXE_gatedspark");

/// A directory of its own, removed when the test ends.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("gatedspark-cli-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Temp(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A path as the command wants it: a string.
    fn text(&self, name: &str) -> String {
        self.join(name)
            .to_str()
            .expect("the temporary path is text")
            .to_owned()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(GATEDSPARK)
        .args(args)
        .output()
        .expect("the command runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Make a key in `temp`, and hand back its path.
fn a_key(temp: &Temp) -> String {
    let key = temp.text("dev.key");
    let out = run(&["keygen", "--out", &key]);
    assert!(out.status.success(), "keygen failed: {}", stderr(&out));
    key
}

// ---------------------------------------------------------------------------
// The key file and its guards
// ---------------------------------------------------------------------------

#[test]
fn keygen_writes_a_key_that_says_what_it_is() {
    let temp = Temp::new("keygen");
    let key = a_key(&temp);

    let text = std::fs::read_to_string(&key).expect("the key file exists");
    assert!(
        text.lines()
            .next()
            .expect("a first line")
            .contains("NOT FOR PUBLISHING"),
        "the first line must say what this key is, got:\n{text}"
    );
    assert!(text.contains("key-id: dev-"), "{text}");
    assert!(text.contains("private-key: "), "{text}");
    // The public half is written alongside, so a person can see which key signed a store
    // without handling the private half.
    assert!(text.contains("public-key: "), "{text}");
}

#[cfg(unix)]
#[test]
fn a_key_file_is_not_readable_by_anybody_else() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new("perms");
    let key = a_key(&temp);
    let mode = std::fs::metadata(&key)
        .expect("the key file exists")
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o077,
        0,
        "the key must not be readable by the group or the world, mode is {mode:o}"
    );
}

#[test]
fn keygen_refuses_to_write_inside_a_git_repository_and_writes_nothing() {
    // The guard that matters most. A convenient key generator is the usual way a private
    // key ends up committed, and this is the only moment anybody is paying attention. A
    // bare `.git` directory is enough, because that is what the check looks for.
    let temp = Temp::new("gitrepo");
    std::fs::create_dir(temp.join(".git")).expect("a .git directory");
    let nested = temp.join("deep/inside");
    std::fs::create_dir_all(&nested).expect("nested directories");
    let key = nested.join("dev.key");

    let out = run(&["keygen", "--out", key.to_str().expect("text")]);
    assert!(!out.status.success(), "it must refuse");
    assert!(
        stderr(&out).contains("refusing to write a signing key inside the Git repository"),
        "the refusal must explain itself, got: {}",
        stderr(&out)
    );
    assert!(
        !key.exists(),
        "no key may be written, not even an empty or partial one"
    );
}

#[test]
fn keygen_never_overwrites_an_existing_key() {
    // Replacing a key invalidates every signature made with the old one, and the
    // packages would sit in the store looking fine until something tried to read them.
    let temp = Temp::new("overwrite");
    let key = a_key(&temp);
    let first = std::fs::read_to_string(&key).expect("read");

    let out = run(&["keygen", "--out", &key]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("already exists"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(&key).expect("read"),
        first,
        "the original key must be untouched"
    );
}

#[test]
fn two_keys_are_different_keys() {
    // A generator that produced the same key twice would be the worst failure available
    // here, and it would look exactly like one that worked.
    let temp = Temp::new("distinct");
    let mut seen = Vec::new();
    for n in 0..3 {
        let key = temp.text(&format!("k{n}.key"));
        assert!(run(&["keygen", "--out", &key]).status.success());
        seen.push(std::fs::read_to_string(&key).expect("read"));
    }
    assert_ne!(seen[0], seen[1]);
    assert_ne!(seen[1], seen[2]);
    assert_ne!(seen[0], seen[2]);
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

#[test]
fn a_misspelled_option_is_an_error_rather_than_being_ignored() {
    // The worst kind of argument bug: `--cuont 50` quietly generating the default
    // number of puzzles, found days later as a store with the wrong contents.
    let temp = Temp::new("typo");
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &temp.text("dev.key"),
        "--cuont",
        "50",
    ]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("no such option: --cuont"),
        "{}",
        stderr(&out)
    );
    // And it names what it does understand, so the message carries the fix.
    assert!(stderr(&out).contains("--count"), "{}", stderr(&out));
    // Rejected before anything was touched.
    assert!(
        !temp.join("store").exists(),
        "no store may be created by a run that was rejected"
    );
}

#[test]
fn an_option_with_no_value_is_an_error_rather_than_a_guess() {
    // `--store` with a forgotten path must not become a store in the current directory.
    let out = run(&["fill", "--store"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("--store needs a value"),
        "{}",
        stderr(&out)
    );

    // And a flag swallowing the next flag is the same mistake wearing a disguise.
    let out = run(&["fill", "--store", "--key", "/tmp/nothing-here"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("--store needs a value, got --key"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_required_option_says_which_one() {
    let temp = Temp::new("required");
    let out = run(&["fill", "--store", &temp.text("store")]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("--key is required"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_level_that_does_not_exist_lists_the_ones_that_do() {
    let temp = Temp::new("level");
    let key = a_key(&temp);
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--level",
        "impossible",
        // A solver that cannot exist, on purpose. A bad flag must be reported as a bad
        // flag whether or not this machine has a solver installed, and without this the
        // test only checked the machines that have one. It is here because the real
        // build caught exactly that.
        "--solver",
        &temp.text("no-solver-here"),
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        !message.contains("will not run"),
        "a bad flag must be reported before a missing solver is: {message}"
    );
    assert!(message.contains("no such level: impossible"), "{message}");
    assert!(
        message.contains("gentle"),
        "the real levels belong in the message: {message}"
    );
}

#[test]
fn a_game_that_does_not_exist_lists_the_ones_that_do() {
    let temp = Temp::new("game");
    let key = a_key(&temp);
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--family",
        "sudoku",
        "--solver",
        &temp.text("no-solver-here"),
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(message.contains("no such game: sudoku"), "{message}");
    assert!(message.contains("nonogram"), "{message}");
    assert!(message.contains("slitherlink"), "{message}");
}

#[test]
fn a_flag_that_belongs_to_the_other_game_is_refused_rather_than_ignored() {
    // Both halves matter. A flag silently ignored is worse than a flag refused: the
    // run would finish, look like it worked, and have made something other than what
    // was asked for.
    let temp = Temp::new("crossedflags");
    let key = a_key(&temp);

    let level = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--family",
        "slitherlink",
        "--level",
        "gentle",
        "--solver",
        &temp.text("no-solver-here"),
    ]);
    assert!(!level.status.success());
    let message = stderr(&level);
    assert!(message.contains("--level is for nonogram"), "{message}");
    assert!(
        message.contains("no difficulty levels yet"),
        "the refusal should say why, not just no: {message}"
    );

    let size = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--size",
        "7",
        "--solver",
        &temp.text("no-solver-here"),
    ]);
    assert!(!size.status.success());
    assert!(
        stderr(&size).contains("--size is for slitherlink"),
        "{}",
        stderr(&size)
    );
}

#[test]
fn a_time_limit_of_zero_is_refused() {
    // A zero time limit would mean every query is out of time before it starts, so
    // every puzzle would be refused for an undecided query. Refusing the flag says why.
    let temp = Temp::new("zerotimeout");
    let key = a_key(&temp);
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--timeout",
        "0",
    ]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("at least 1 second"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_count_that_is_not_a_number_is_refused() {
    let temp = Temp::new("notanumber");
    let key = a_key(&temp);
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--count",
        "lots",
    ]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("--count must be a number, got lots"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn no_command_and_a_wrong_command_both_print_the_usage() {
    let out = run(&[]);
    assert!(!out.status.success());
    assert!(stdout(&out).contains("gatedspark"), "{}", stdout(&out));

    let out = run(&["frobnicate"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("no such command: frobnicate"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn asking_for_help_succeeds() {
    for flag in ["help", "--help", "-h"] {
        let out = run(&[flag]);
        assert!(out.status.success(), "{flag} should succeed");
        assert!(stdout(&out).contains("gatedspark keygen"), "{flag}");
    }
}

#[test]
fn a_missing_solver_is_reported_as_a_missing_solver() {
    // Without this check, a machine with no solver would show every puzzle refused for
    // an undecided query: correct, and saying nothing about why. A pipeline that cannot
    // verify anything has to say so in those words.
    let temp = Temp::new("nosolver");
    let key = a_key(&temp);
    let out = run(&[
        "fill",
        "--store",
        &temp.text("store"),
        "--key",
        &key,
        "--solver",
        "gatedspark-no-such-solver-exists",
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("will not run") && message.contains("without a solver"),
        "{message}"
    );
    let stored = std::fs::read_dir(temp.join("store"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(stored, 0, "nothing may have been stored");
}

#[test]
fn show_without_a_name_asks_for_one() {
    let temp = Temp::new("noname");
    let key = a_key(&temp);
    let out = run(&["show", "--store", &temp.text("store"), "--key", &key]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("which puzzle?"), "{}", stderr(&out));
}

#[test]
fn recheck_on_an_empty_store_says_it_verified_nothing() {
    // A store with nothing in it has no failures. A run that printed only "all sound"
    // would read as a clean bill of health for an empty directory, which is the shape of
    // a check that passes while checking nothing.
    let temp = Temp::new("emptyrecheck");
    let key = a_key(&temp);
    let out = run(&["recheck", "--store", &temp.text("store"), "--key", &key]);
    assert!(out.status.success(), "{}", stderr(&out));
    let printed = stdout(&out);
    assert!(printed.contains("checked 0 puzzles"), "{printed}");
    assert!(
        printed.contains("the store is empty, so nothing was verified"),
        "{printed}"
    );
    assert!(
        !printed.contains("checks out"),
        "an empty store must not be told it checks out: {printed}"
    );
}

// ---------------------------------------------------------------------------
// The whole loop, which needs a real solver
// ---------------------------------------------------------------------------

/// The solver to use, or why there is none.
fn solver_program() -> Result<String, String> {
    let program = std::env::var("GATEDSPARK_SMT_SOLVER").unwrap_or_else(|_| "z3".to_owned());
    let runnable = Command::new(&program)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if runnable {
        return Ok(program);
    }
    let message = format!("no SMT solver found (tried {program}; set GATEDSPARK_SMT_SOLVER)");
    if std::env::var("GATEDSPARK_REQUIRE_SOLVER").is_ok_and(|v| v == "1") {
        panic!("GATEDSPARK_REQUIRE_SOLVER=1 but {message}");
    }
    Err(message)
}

#[test]
fn an_empty_folder_becomes_signed_verified_puzzles_and_then_rechecks() {
    // M5's exit test, as one command and then another. Everything else in this
    // repository checks a part. This checks that the parts are connected.
    let program = match solver_program() {
        Ok(program) => program,
        Err(why) => {
            println!(
                "\n*** THE WHOLE LOOP WAS NOT VERIFIED: {why}.\n\
                 *** Unchecked: that an empty folder becomes signed, solver-verified\n\
                 *** puzzles that survive a re-check, and that a tampered answer does not.\n"
            );
            return;
        }
    };

    let temp = Temp::new("wholeloop");
    let key = a_key(&temp);
    let store_path = temp.text("store");
    let store = temp.join("store");
    assert!(!store.exists(), "the store starts out not existing at all");

    let fill = run(&[
        "fill",
        "--store",
        &store_path,
        "--key",
        &key,
        "--count",
        "1",
        "--level",
        "gentle",
        "--solver",
        &program,
    ]);
    assert!(fill.status.success(), "{}", stderr(&fill));
    let printed = stdout(&fill);
    assert!(printed.contains("stored 1,"), "{printed}");
    assert!(
        printed.contains("undecided 0"),
        "nothing undecided may be published: {printed}"
    );

    // One puzzle, one directory named by its fingerprint, two files in it.
    let dirs: Vec<_> = std::fs::read_dir(&store)
        .expect("the store exists now")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(dirs.len(), 1, "one puzzle was asked for");
    let name = dirs[0].file_name().into_string().expect("text");
    assert_eq!(
        name.len(),
        64,
        "the directory is named by the content fingerprint, got {name}"
    );
    assert!(dirs[0].path().join("package.gspk").is_file());
    assert!(dirs[0].path().join("witness.bin").is_file());

    // The report reads back out of the signed package.
    let shown = run(&["show", "--store", &store_path, "--key", &key, &name]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    let text = stdout(&shown);
    assert!(text.contains(&name), "{text}");
    assert!(text.contains("nonogram"), "{text}");
    assert!(text.contains("gentle"), "{text}");
    assert!(
        text.contains("solver was asked:"),
        "the queries belong in the report: {text}"
    );
    assert!(
        !text.contains("not what was required"),
        "nothing may be published whose queries did not answer as required: {text}"
    );
    assert!(
        text.contains("the real game accepted the solver's answer"),
        "{text}"
    );
    assert!(!text.contains("should not have been published"), "{text}");

    // And the store checks out.
    let ok = run(&["recheck", "--store", &store_path, "--key", &key]);
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(stdout(&ok).contains("checked 1 puzzles"), "{}", stdout(&ok));
    assert!(
        stdout(&ok).contains("every fingerprint, signature and stored answer checks out"),
        "{}",
        stdout(&ok)
    );

    // Now break one byte of the stored answer, and require the re-check to fail.
    // Without this half, "every answer checks out" could be printed by a re-check that
    // looked at nothing at all.
    let witness_path = dirs[0].path().join("witness.bin");
    let mut witness = std::fs::read(&witness_path).expect("read the answer");
    witness[0] ^= 0x80;
    std::fs::write(&witness_path, &witness).expect("write the tampered answer");

    let broken = run(&["recheck", "--store", &store_path, "--key", &key]);
    assert!(
        !broken.status.success(),
        "a store with a tampered answer must fail the re-check"
    );
    let message = stdout(&broken) + &stderr(&broken);
    assert!(message.contains("FAILED"), "{message}");
    assert!(
        message.contains("the stored answer hashes to"),
        "the failure must name what disagreed: {message}"
    );
    assert!(
        message.contains("1 of 1 puzzles did not check out"),
        "{message}"
    );
}

#[test]
fn a_store_signed_by_another_key_is_not_accepted() {
    // The signature has to be doing work. If `recheck` accepted a package signed by a
    // key it was not given, the signature would be decoration.
    let Ok(program) = solver_program() else {
        println!(
            "\n*** NOT VERIFIED: that a store signed by another key is rejected \
             (no solver).\n"
        );
        return;
    };

    let temp = Temp::new("strangerkey");
    let mine = a_key(&temp);
    let store_path = temp.text("store");
    assert!(run(&[
        "fill",
        "--store",
        &store_path,
        "--key",
        &mine,
        "--count",
        "1",
        "--level",
        "gentle",
        "--solver",
        &program,
    ])
    .status
    .success());

    let stranger = temp.text("stranger.key");
    assert!(run(&["keygen", "--out", &stranger]).status.success());

    let out = run(&["recheck", "--store", &store_path, "--key", &stranger]);
    assert!(
        !out.status.success(),
        "a stranger's key must not vouch for this store"
    );
    let message = stdout(&out) + &stderr(&out);
    assert!(
        message.contains("SignatureRejected") || message.contains("signature"),
        "the failure must be about the signature: {message}"
    );
}

/// The one thing in here that is about the repository rather than the command.
#[test]
fn the_repository_ignores_key_files() {
    // The Git guard in `keygen` is the real defence, but somebody will eventually write
    // a key with a text editor. Cheap belt and braces.
    let ignore = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".gitignore");
    let text = std::fs::read_to_string(&ignore).expect(".gitignore exists");
    assert!(
        text.lines().any(|line| line.trim() == "*.key"),
        ".gitignore must ignore *.key, got:\n{text}"
    );
}

#[test]
fn slitherlink_goes_from_an_empty_folder_to_a_signed_verified_puzzle_too() {
    // The same exit test as the Nonogram one above, for the second game. The point is
    // not that Slitherlink works — its own crate checks that — but that the one
    // command, the one store and the one re-check are not built around one game.
    let program = match solver_program() {
        Ok(program) => program,
        Err(why) => {
            println!(
                "\n*** SLITHERLINK'S WHOLE LOOP WAS NOT VERIFIED: {why}.\n\
                 *** Unchecked: that `fill --family slitherlink` stores a signed,\n\
                 *** solver-verified puzzle that survives a re-check.\n"
            );
            return;
        }
    };

    let temp = Temp::new("slitherloop");
    let key = a_key(&temp);
    let store_path = temp.text("store");

    // The smallest board this game makes, because this test is about the plumbing and
    // a bigger board would only cost time. The cost of real boards is measured in
    // `gs-slitherlink`, not here.
    let fill = run(&[
        "fill",
        "--store",
        &store_path,
        "--key",
        &key,
        "--family",
        "slitherlink",
        "--count",
        "1",
        "--size",
        "4",
        "--solver",
        &program,
    ]);
    assert!(fill.status.success(), "{}", stderr(&fill));
    let printed = stdout(&fill);
    assert!(printed.contains("game:   slitherlink"), "{printed}");
    assert!(printed.contains("stored 1,"), "{printed}");
    assert!(
        printed.contains("undecided 0"),
        "nothing undecided may be published: {printed}"
    );

    let dirs: Vec<_> = std::fs::read_dir(temp.join("store"))
        .expect("the store exists now")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(dirs.len(), 1, "one puzzle was asked for");
    let name = dirs[0].file_name().into_string().expect("text");

    let shown = run(&["show", "--store", &store_path, "--key", &key, &name]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    let text = stdout(&shown);
    assert!(text.contains("slitherlink"), "{text}");
    assert!(
        text.contains("level        (none given)"),
        "this game has no difficulty levels yet, and the report should say so: {text}"
    );
    assert!(text.contains("solver was asked:"), "{text}");
    assert!(
        text.contains("the real game accepted the solver's answer"),
        "{text}"
    );
    assert!(!text.contains("not what was required"), "{text}");

    let ok = run(&["recheck", "--store", &store_path, "--key", &key]);
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(
        stdout(&ok).contains("every fingerprint, signature and stored answer checks out"),
        "{}",
        stdout(&ok)
    );
}
