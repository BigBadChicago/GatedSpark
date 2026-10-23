//! `gatedspark`: the command that takes an empty store to signed, playable puzzles.
//!
//! This is M5's exit test made runnable. Four subcommands:
//!
//! ```text
//! gatedspark keygen  --out <file>                       make a development signing key
//! gatedspark fill    --store <dir> --key <file> [...]    generate, verify, sign, store
//! gatedspark show    --store <dir> <name>               print one puzzle's report
//! gatedspark recheck --store <dir> --key <file>          re-check the whole store
//! gatedspark withdraw --store <dir> --key <file> [...]   publish "do not play these"
//! ```
//!
//! # Why `show` and `recheck` need a key
//!
//! They need the *public* key, to check signatures, and they get it from the private key
//! file because that is what exists in a development setup.
//!
//! The tempting alternative is for the store to keep a list of the keys it trusts. That
//! is worse than it looks: a reader that trusts whatever key list it finds next to the
//! content is a reader that trusts whatever anybody put there, so the list would be
//! decoration. The trusted-key list has to come from outside the content it vouches
//! for, which in a real deployment means the device ships with it. Here it means a flag.
//!
//! # No argument-parsing library
//!
//! ADR 0004's rule is no dependency without a recorded reason, and four subcommands with
//! a handful of flags do not need one. The parsing below is deliberately dull: unknown
//! flags are an error rather than ignored, because a typo'd `--cuont 50` that silently
//! generated one puzzle would be discovered as a nearly empty store.

mod key;

use std::path::PathBuf;
use std::process::ExitCode;

use gs_core::package::{RecordedReplay, Signer};
use gs_core::withdrawal::{seal_list, OpenedList, Reason, WithdrawalList, Withdrawn};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_nonogram::difficulty::{propose_in_band, Band, BANDS};
use gs_nonogram::publish::{publish, PublishError};
use gs_nonogram::solve::{solve, Budget};
use gs_slitherlink::generate::{propose, Shape, REMOVAL_BUDGET};
use gs_slitherlink::publish as sl_publish;
use gs_slitherlink::solve as sl_solve;
use gs_solver::{ExternalSolver, TimeLimit};
use gs_store::{Store, Stored};

const USAGE: &str = "\
gatedspark — make and keep verified puzzles

  gatedspark keygen  --out <file>
      Make a development signing key. Refuses to write inside a Git repository.

  gatedspark fill    --store <dir> --key <file> [--family NAME] [--count N]
                     [--level NAME] [--size N] [--enclosed N]
                     [--solver PATH] [--from-seed N] [--max-seeds N] [--timeout SECS]
      Generate puzzles, ask a real solver about each one, and store the ones it
      vouches for. Nothing is signed that the solver did not vouch for.

      --family NAME  which game: nonogram (the default) or slitherlink

      Nonogram only:
      --level NAME   one difficulty level, instead of all of them

      Slitherlink only:
      --size N       board size, N by N cells. Defaults to 7.
      --enclosed N   how much of the board the loop should go round, in tenths of
                     a percent. Defaults to 400. This game has no difficulty
                     levels yet, on purpose: they wait for measured play data.

  gatedspark show    --store <dir> <puzzle name>
      Print one puzzle's validation report, read out of its signed package.
      --key <file> is needed to check the signature first.

  gatedspark recheck --store <dir> --key <file>
      Re-open every puzzle: recompute its fingerprint, check its signature, and
      check its stored answer against the fingerprint in the signed report.

  gatedspark withdraw --store <dir> --key <file>
                      [--add NAME] [--keep NAME] [--reason WHY] [--serial N]
      Publish the signed list of puzzles that must not be played. With no --add
      or --keep, it prints the current list and changes nothing.

      --add NAME     withdraw this puzzle, as `list` prints its name
      --keep NAME    drop this puzzle from the list, undoing a withdrawal
      --reason WHY   why, for the --add puzzles: no-answer, many-answers,
                     unsuitable, or mistake. Needed with --add.
      --serial N     the list's number, which must go up. Defaults to one more
                     than the current list's.

      Withdrawing does NOT delete the puzzle from the store. The list is what
      devices read; the puzzle stays where it is, so a mistaken withdrawal can be
      undone with --keep.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gatedspark: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first() else {
        print!("{USAGE}");
        return Err("no command given".to_owned());
    };
    let rest = &args[1..];
    match command.as_str() {
        "keygen" => keygen(rest),
        "fill" => fill(rest),
        "show" => show(rest),
        "recheck" => recheck(rest),
        "withdraw" => withdraw(rest),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => {
            print!("{USAGE}");
            Err(format!("no such command: {other}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// Flags and positional arguments, parsed once so each subcommand reads what it needs.
struct Args {
    flags: Vec<(String, String)>,
    positional: Vec<String>,
}

impl Args {
    /// Parse `--name value` pairs and bare words.
    ///
    /// A flag with no value is an error rather than a boolean, because every flag this
    /// command has takes one, and guessing would turn `--store` with a forgotten path
    /// into a store in the current directory.
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut flags = Vec::new();
        let mut positional = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if let Some(name) = arg.strip_prefix("--") {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                if value.starts_with("--") {
                    return Err(format!("--{name} needs a value, got {value}"));
                }
                flags.push((name.to_owned(), value.clone()));
                i += 2;
            } else {
                positional.push(arg.clone());
                i += 1;
            }
        }
        Ok(Args { flags, positional })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value given for a flag, in the order they were written.
    ///
    /// `--add` can appear more than once, because withdrawing three puzzles is one
    /// decision and should be one list with one serial number, not three.
    fn all(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.get(name)
            .ok_or_else(|| format!("--{name} is required"))
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.get(name) {
            None => Ok(default),
            Some(text) => text
                .parse()
                .map_err(|_| format!("--{name} must be a number, got {text}")),
        }
    }

    /// Reject anything this subcommand does not understand.
    ///
    /// A silently ignored flag is the worst kind of argument bug: `--cuont 50` would
    /// quietly generate the default number of puzzles, and the mistake would be found
    /// as a store with the wrong contents days later.
    fn reject_unknown(&self, known: &[&str]) -> Result<(), String> {
        for (name, _) in &self.flags {
            if !known.contains(&name.as_str()) {
                return Err(format!(
                    "no such option: --{name}\nknown options: {}",
                    known
                        .iter()
                        .map(|k| format!("--{k}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        }
        Ok(())
    }
}

/// Load a key file and build the signer and the matching trusted-key list.
fn key_and_trust(path: &str) -> Result<(Ed25519Signer, TrustedKeys), String> {
    let signer = key::load(&PathBuf::from(path)).map_err(|e| e.to_string())?;
    let mut keys = TrustedKeys::none();
    keys.trust(signer.key_id(), &signer.public_key())
        .map_err(|e| format!("the key in {path} cannot be trusted: {e:?}"))?;
    Ok((signer, keys))
}

// ---------------------------------------------------------------------------
// keygen
// ---------------------------------------------------------------------------

fn keygen(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&["out"])?;
    let out = PathBuf::from(args.required("out")?);
    let key_id = key::generate(&out).map_err(|e| e.to_string())?;
    println!("wrote a development signing key to {}", out.display());
    println!("key id: {key_id}");
    println!();
    println!("This key is for development only. Anything signed with it is signed by");
    println!("nobody. Keep it out of any repository: writing one inside a Git working");
    println!("tree is refused, which is why the path above is outside this project.");
    Ok(())
}

// ---------------------------------------------------------------------------
// fill
// ---------------------------------------------------------------------------

fn fill(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&[
        "store",
        "key",
        "family",
        "count",
        "level",
        "size",
        "enclosed",
        "solver",
        "from-seed",
        "max-seeds",
        "timeout",
    ])?;

    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (signer, trusted) = key_and_trust(args.required("key")?)?;
    let wanted: usize = args.number("count", 1)?;
    let from_seed: u64 = args.number("from-seed", 1)?;
    let max_seeds: u64 = args.number("max-seeds", 20_000)?;
    let seconds: u32 = args.number("timeout", 60)?;
    let family = args.get("family").unwrap_or("nonogram");
    // Checked before anything slow starts. A flag that belongs to the other game is a
    // mistake about what is being made, not a detail to ignore: `--level easy` on a
    // Slitherlink run would otherwise quietly make puzzles of no particular
    // difficulty, which is exactly what the flag was asking not to happen.
    match family {
        "nonogram" => {
            for flag in ["size", "enclosed"] {
                if args.get(flag).is_some() {
                    return Err(format!("--{flag} is for slitherlink, not nonogram"));
                }
            }
        }
        "slitherlink" => {
            if args.get("level").is_some() {
                return Err(
                    "--level is for nonogram. Slitherlink has no difficulty levels yet, \
                     on purpose: they wait for measured play data rather than a guess."
                        .to_owned(),
                );
            }
        }
        other => {
            return Err(format!(
                "no such game: {other}\ngames: nonogram, slitherlink"
            ))
        }
    }

    let program = args.get("solver").unwrap_or("z3");
    let limit = TimeLimit::seconds(seconds)
        .ok_or_else(|| "--timeout must be at least 1 second".to_owned())?;
    let solver = ExternalSolver::new(program, "external solver", limit);
    // Checked once, before a long run, so a missing solver is reported as a missing
    // solver. It would otherwise show up as every puzzle being refused for an
    // undecided query, which is correct but says nothing about why.
    if !solver.runnable() {
        return Err(format!(
            "the solver {program} will not run.\n\
             Install it, or pass --solver with a path to one. Nothing can be published \
             without a solver, and nothing here will pretend otherwise."
        ));
    }
    println!("game:   {family}");
    println!("solver: {program}, {seconds}s limit per query");
    println!("store:  {}", store.root().display());

    if family == "slitherlink" {
        let shape = Shape {
            width: args.number("size", 7u16)?,
            height: args.number("size", 7u16)?,
            enclosed_per_mille: args.number("enclosed", 400u32)?,
        };
        return fill_slitherlink(
            &store,
            &signer,
            &trusted,
            &solver,
            shape,
            wanted,
            from_seed..from_seed.saturating_add(max_seeds),
        );
    }

    let bands = chosen_bands(args.get("level"))?;
    let mut stored = 0usize;
    let mut refused = 0usize;
    let mut undecided = 0usize;

    for band in bands {
        let mut for_this_band = 0usize;
        let mut seed = from_seed;
        let last_seed = from_seed.saturating_add(max_seeds);
        while for_this_band < wanted && seed < last_seed {
            let this_seed = seed;
            seed += 1;
            let Some(candidate) = propose_in_band(band, this_seed, Budget::default()) else {
                continue;
            };
            match publish(&candidate, &solver, Budget::default(), &Sha256, &signer) {
                Ok(published) => {
                    let witness = match solve(&candidate.puzzle, Budget::default()).witness {
                        Some(w) => w.canonical_bytes(),
                        // A published puzzle has a replayed witness by definition, so
                        // this cannot happen without a bug. Reported rather than
                        // unwrapped: a batch run should not die on one puzzle.
                        None => {
                            eprintln!("  seed {this_seed}: published but has no witness; skipped");
                            continue;
                        }
                    };
                    match store.put(&published.package, &witness, &Sha256, &trusted) {
                        Ok(id) => {
                            stored += 1;
                            for_this_band += 1;
                            println!("  {} [{}] seed {this_seed}", id.to_hex(), band.name);
                        }
                        Err(gs_store::StoreError::AlreadyStored(_)) => {
                            // Two seeds can produce the same clues. Not an error.
                            continue;
                        }
                        Err(e) => return Err(format!("storing seed {this_seed}: {e}")),
                    }
                }
                Err(PublishError::Refused { report, .. }) => {
                    if report.has_undecided_query() {
                        undecided += 1;
                    } else {
                        refused += 1;
                    }
                }
                Err(PublishError::Unaskable(_)) => refused += 1,
                Err(PublishError::Encoding(e)) => {
                    return Err(format!("encoding seed {this_seed}: {e:?}"))
                }
            }
        }
        if for_this_band < wanted {
            // Said out loud, never hidden. A worker that quietly delivered fewer puzzles
            // than asked would be discovered as an empty level in front of a player.
            println!(
                "  {}: wanted {wanted}, stored {for_this_band} — short by {}",
                band.name,
                wanted - for_this_band
            );
        }
    }

    println!();
    println!("stored {stored}, refused by the solver {refused}, undecided {undecided}");
    if undecided > 0 {
        println!(
            "An undecided query means the solver did not answer — a timeout, or a crash. \
             Nothing undecided was published."
        );
    }
    if stored == 0 {
        return Err("nothing was stored".to_owned());
    }
    Ok(())
}

/// Fill the store with Slitherlink puzzles.
///
/// Shorter than the Nonogram loop above for one reason only: this game has no
/// difficulty levels yet, so there is no band to loop over. Everything else is the
/// same shape, and deliberately so — generate, ask a real solver, store only what the
/// solver vouched for.
fn fill_slitherlink(
    store: &Store,
    signer: &Ed25519Signer,
    trusted: &TrustedKeys,
    solver: &ExternalSolver,
    shape: Shape,
    wanted: usize,
    seeds: std::ops::Range<u64>,
) -> Result<(), String> {
    println!(
        "board:  {}x{} cells, loop round {}/1000 of it",
        shape.width, shape.height, shape.enclosed_per_mille
    );
    println!();

    let mut stored = 0usize;
    let mut rejected = 0usize;
    let mut refused = 0usize;
    let mut undecided = 0usize;
    let mut seed = seeds.start;

    while stored < wanted && seed < seeds.end {
        let this_seed = seed;
        seed += 1;
        let candidate = match propose(this_seed, shape, REMOVAL_BUDGET) {
            Ok(c) => c,
            // A size the board rules refuse is a bad flag, not a bad seed, and no later
            // seed will fix it. Everything else is this seed's luck.
            Err(gs_slitherlink::generate::Rejected::Size(e)) => {
                return Err(format!("--size is not a board this game can make: {e:?}"))
            }
            Err(_) => {
                rejected += 1;
                continue;
            }
        };
        match sl_publish::publish(
            &candidate,
            solver,
            sl_solve::Budget::default(),
            &Sha256,
            signer,
        ) {
            Ok(published) => {
                let witness =
                    match sl_solve::solve(&candidate.puzzle, sl_solve::Budget::default()).witness {
                        Some(w) => w.canonical_bytes(),
                        // A published puzzle has a replayed witness by definition, so this
                        // cannot happen without a bug. Reported rather than unwrapped: a
                        // batch run should not die on one puzzle.
                        None => {
                            eprintln!("  seed {this_seed}: published but has no witness; skipped");
                            continue;
                        }
                    };
                match store.put(&published.package, &witness, &Sha256, trusted) {
                    Ok(id) => {
                        stored += 1;
                        println!(
                            "  {} seed {this_seed}, {} numbers left, {} questions asked",
                            id.to_hex(),
                            candidate.puzzle.clues.given_count(),
                            candidate.questions_asked
                        );
                    }
                    // Two seeds can produce the same numbers. Not an error.
                    Err(gs_store::StoreError::AlreadyStored(_)) => continue,
                    Err(e) => return Err(format!("storing seed {this_seed}: {e}")),
                }
            }
            Err(sl_publish::PublishError::Refused { report, .. }) => {
                if report.has_undecided_query() {
                    undecided += 1;
                } else {
                    refused += 1;
                }
            }
            Err(sl_publish::PublishError::Unaskable(_)) => refused += 1,
            Err(sl_publish::PublishError::Encoding(e)) => {
                return Err(format!("encoding seed {this_seed}: {e:?}"))
            }
        }
    }

    println!();
    println!(
        "stored {stored}, dropped while generating {rejected}, \
         refused by the solver {refused}, undecided {undecided}"
    );
    if stored < wanted {
        // Said out loud, never hidden. A worker that quietly delivered fewer puzzles
        // than asked would be discovered as an empty shelf in front of a player.
        println!("wanted {wanted} — short by {}", wanted - stored);
    }
    if undecided > 0 {
        println!(
            "An undecided query means the solver did not answer — a timeout, or a crash. \
             Nothing undecided was published."
        );
    }
    if stored == 0 {
        return Err("nothing was stored".to_owned());
    }
    Ok(())
}

/// Which levels to fill: one named level, or all of them.
fn chosen_bands(name: Option<&str>) -> Result<Vec<&'static Band>, String> {
    match name {
        None => Ok(BANDS.iter().collect()),
        Some(wanted) => BANDS
            .iter()
            .find(|b| b.name == wanted)
            .map(|b| vec![b])
            .ok_or_else(|| {
                format!(
                    "no such level: {wanted}\nlevels: {}",
                    BANDS.iter().map(|b| b.name).collect::<Vec<_>>().join(", ")
                )
            }),
    }
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn show(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&["store", "key"])?;
    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (_, trusted) = key_and_trust(args.required("key")?)?;
    let name = args
        .positional
        .first()
        .ok_or_else(|| "which puzzle? give its name, as `list` prints it".to_owned())?;

    let stored = store
        .get(name, &Sha256, &trusted)
        .map_err(|e| e.to_string())?;
    print_report(&stored);
    Ok(())
}

/// Print a puzzle's report, read out of the signed package.
///
/// This is the whole reason the store keeps no second copy of the report as JSON. The
/// report exists once, inside the signed bytes, and this is a tool for reading it. Two
/// copies of one set of facts is how the two come to disagree, and the honest answer to
/// "which is right" would always have been the signed one.
fn print_report(stored: &Stored) {
    let m = &stored.package.manifest;
    println!("puzzle       {}", stored.package.content_id.to_hex());
    println!(
        "family       {} (rules version {})",
        m.family, m.ruleset_version
    );
    println!("generator    version {}", m.generator_version);
    println!("seed         {}", m.seed);
    println!(
        "level        {}",
        m.band.as_deref().unwrap_or("(none given)")
    );
    println!("signed by    {}", stored.package.key_id);
    println!("encoding     {}", m.encoding);
    println!(
        "answer       {} bytes, kept here only",
        stored.witness.len()
    );
    println!();
    println!("solver was asked:");
    for q in &m.queries {
        println!(
            "  {:<34} wanted {:?}, got {:?}{}",
            q.name,
            q.required,
            q.returned,
            if q.required == q.returned {
                ""
            } else {
                "   <-- not what was required"
            }
        );
    }
    println!();
    println!("difficulty measured:");
    for measurement in &m.measurements {
        println!("  {:<24} {}", measurement.name, measurement.value);
    }
    println!();
    match &m.replay {
        RecordedReplay::Reproduced(hash) => println!(
            "the real game accepted the solver's answer, which fingerprints to {}",
            hash.to_hex()
        ),
        RecordedReplay::Refused(why) => println!("the real game REFUSED the answer: {why}"),
        RecordedReplay::NotAttempted => println!("the answer was never replayed"),
    }
    if m.has_undecided_query() {
        println!();
        println!("NOTE: a query came back undecided. This puzzle should not have been published.");
    }
}

// ---------------------------------------------------------------------------
// recheck
// ---------------------------------------------------------------------------

fn recheck(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&["store", "key"])?;
    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (_, trusted) = key_and_trust(args.required("key")?)?;

    let report = store
        .recheck(&Sha256, &trusted)
        .map_err(|e| e.to_string())?;

    // `checked` is printed first and always. A store with nothing in it has no
    // failures, and a run that said only "all sound" would read as a clean bill of
    // health for an empty directory.
    println!(
        "checked {} puzzles in {}",
        report.checked,
        store.root().display()
    );
    if report.checked == 0 {
        println!("the store is empty, so nothing was verified");
        return Ok(());
    }
    if report.all_sound() {
        println!("every fingerprint, signature and stored answer checks out");
        return Ok(());
    }
    println!();
    for (name, why) in &report.failures {
        println!("FAILED {name}");
        println!("       {why}");
    }
    Err(format!(
        "{} of {} puzzles did not check out",
        report.failures.len(),
        report.checked
    ))
}

// ---------------------------------------------------------------------------
// withdraw
// ---------------------------------------------------------------------------

/// Publish the signed list of puzzles that must not be played.
///
/// # Why this does not delete anything
///
/// The list is what a device reads, and a device that already holds a puzzle is the
/// whole reason the list exists. Deleting the puzzle from the store would make the
/// withdrawal unprovable — nothing left to point at, and no way to undo a mistake. So
/// the puzzle stays and the list names it.
///
/// # Why the serial number is worked out rather than asked for
///
/// It has to go up, and the only number it has to go up from is the one already in the
/// store. Asking a person for it is asking them to read a file and add one, which they
/// will eventually get wrong on the day it matters. `--serial` is still there, for
/// publishing a list numbered higher than this store knows about.
fn withdraw(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&["store", "key", "add", "keep", "reason", "serial"])?;
    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (signer, trusted) = key_and_trust(args.required("key")?)?;

    let current = store
        .withdrawals(&Sha256, &trusted)
        .map_err(|e| e.to_string())?;
    let held = current.as_ref().map_or(0, |opened| opened.list.serial);

    let adding = args.all("add");
    let keeping = args.all("keep");

    if adding.is_empty() && keeping.is_empty() {
        print_withdrawals(current.as_ref(), &store);
        return Ok(());
    }

    let reason =
        match args.get("reason") {
            Some(word) => reason_from(word)?,
            None if adding.is_empty() => Reason::Mistake, // never used; nothing is being added
            None => return Err(
                "--reason is needed with --add: no-answer, many-answers, unsuitable, or mistake"
                    .to_owned(),
            ),
        };

    // Start from what is already withdrawn, minus anything being kept.
    let mut withdrawn: Vec<Withdrawn> = current
        .as_ref()
        .map(|opened| opened.list.withdrawn.clone())
        .unwrap_or_default();
    for name in &keeping {
        let before = withdrawn.len();
        withdrawn.retain(|entry| hex(&entry.content_id) != *name);
        if withdrawn.len() == before {
            return Err(format!("--keep {name}: that puzzle is not on the list"));
        }
    }

    for name in &adding {
        // Opened from the store rather than parsed from the name. A content id in this
        // codebase may only come from digesting something, and a name typed on a command
        // line is a claim about a puzzle rather than a digest of one. Opening it also
        // means a typo'd name is an error here instead of a withdrawal that names
        // nothing and quietly protects nobody.
        let stored = store
            .get(name, &Sha256, &trusted)
            .map_err(|e| format!("--add {name}: {e}"))?;
        let id = *stored.package.content_id.as_bytes();
        if withdrawn.iter().any(|entry| entry.content_id == id) {
            return Err(format!("--add {name}: that puzzle is already withdrawn"));
        }
        withdrawn.push(Withdrawn {
            content_id: id,
            reason,
        });
    }

    let serial: u64 = args.number("serial", held + 1)?;
    if serial <= held {
        return Err(format!(
            "--serial {serial} is not higher than the {held} already published. A list \
             numbered at or below the one a device already has is one the device will \
             refuse, which is how an old list is stopped from un-withdrawing a puzzle."
        ));
    }

    let list = WithdrawalList { serial, withdrawn };
    let bytes = seal_list(&list, &Sha256, &signer).map_err(|e| format!("{e:?}"))?;
    let written = store
        .put_withdrawals(&bytes, &Sha256, &trusted)
        .map_err(|e| e.to_string())?;

    println!(
        "wrote {} ({} bytes)",
        store.withdrawals_path().display(),
        bytes.len()
    );
    print_withdrawals(Some(&written), &store);
    println!();
    println!("The puzzles themselves are still in the store. This list is what devices");
    println!("read, and a device deletes what it names the first time it reads it.");
    Ok(())
}

fn reason_from(word: &str) -> Result<Reason, String> {
    match word {
        "no-answer" => Ok(Reason::NoAnswer),
        "many-answers" => Ok(Reason::MoreThanOneAnswer),
        "unsuitable" => Ok(Reason::Unsuitable),
        "mistake" => Ok(Reason::Mistake),
        other => Err(format!(
            "no such reason: {other}\nknown reasons: no-answer many-answers unsuitable mistake"
        )),
    }
}

fn print_withdrawals(current: Option<&OpenedList>, store: &Store) {
    let Some(opened) = current else {
        // Not the same as a list naming nothing. One is a publisher who has said
        // nothing; the other is a publisher saying nothing is withdrawn.
        println!(
            "no list of withdrawn puzzles in {} yet",
            store.root().display()
        );
        return;
    };
    println!(
        "list number {}, signed by {}",
        opened.list.serial, opened.key_id
    );
    if opened.list.is_empty() {
        println!("it names no puzzles: nothing is withdrawn");
        return;
    }
    for entry in &opened.list.withdrawn {
        println!("  {}", hex(&entry.content_id));
        println!("      {}", entry.reason);
    }
}

fn hex(id: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in id {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("a nibble is a hex digit"));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("a nibble is a hex digit"));
    }
    out
}
