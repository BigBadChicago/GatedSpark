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
use gs_core::schedule::{seal_schedule, Date, OpenedSchedule, Schedule, Scheduled};
use gs_core::withdrawal::{seal_list, OpenedList, Reason, WithdrawalList, Withdrawn};
use gs_crypto::{Ed25519Signer, Sha256, TrustedKeys};
use gs_kakuro::construct as ka_construct;
use gs_kakuro::generate as ka_generate;
use gs_kakuro::publish as ka_publish;
use gs_kakuro::solve as ka_solve;
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

      --family NAME  which game: nonogram (the default), slitherlink or kakuro

      Nonogram only:
      --level NAME   one difficulty level, instead of all of them

      Slitherlink only:
      --size N       board size, N by N cells. Defaults to 7.
      --enclosed N   how much of the board the loop should go round, in tenths of
                     a percent. Defaults to 400. This game has no difficulty
                     levels yet, on purpose: they wait for measured play data.

      Kakuro only:
      --size N       board size, N by N squares. Defaults to 7.
      --boards WHICH built or random. Defaults to built: the board is worked out
                     so that no run is one square long, so nothing is handed to
                     the player. random is the older method, which gives about
                     380/1000 of the board away; it is slower up to 7x7 and much
                     faster above it.
      --longest-run N the longest run of white squares, in squares. Defaults to
                     5, as a printed Kakuro does. Shorter runs mean more totals
                     over the same squares, which is what makes a board pin down
                     to one answer. Built boards only.
      --black N      how much of the board is black, in tenths of a percent.
                     Defaults to 350 for built boards and 200 for random ones.
      --given N      the most white squares that may sit alone in a run of one,
                     in tenths of a percent. A square alone in a run has its
                     digit named by the total, so this is a cap on how much of
                     the board is handed to the player. Defaults to 0 for built
                     boards, which have none, and 500 for random ones. This game
                     has no difficulty levels yet either, and for the same
                     reason.

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

  gatedspark schedule --store <dir> --key <file>
                      [--start YYYY-MM-DD] [--days N] [--family NAME]
                      [--add NAME --on YYYY-MM-DD] [--serial N]
      Publish the signed schedule: which puzzle is each day's. With no --days and
      no --add, it prints the current schedule and changes nothing.

      --start DATE   the day the count begins from. Needed for the first
                     schedule only, and fixed after that: moving it would
                     renumber every day already published.
      --days N       give each game N more days, taking puzzles the schedule does
                     not already name. Run it again to extend further.
      --family NAME  only this game, instead of every game in the store
      --add NAME     schedule one named puzzle, with --on for the day
      --on DATE      which day the --add puzzle is for
      --serial N     the schedule's number, which must go up. Defaults to one
                     more than the current schedule's.

      A device works out today from its own clock and looks it up here. A wrong
      clock shows the wrong day's puzzle; it cannot make a bad puzzle look good.

  gatedspark publish  --store <dir> --key <file> --out <dir> [--page <dir>]
      Write a folder an ordinary web host can serve: the signed packages, the
      schedule, the withdrawal list, and optionally the page itself.

      --out <dir>    where to write it
      --page <dir>   also copy the page from here, usually crates/gs-web/www

      The answers are deliberately NOT published. A store keeps the filled-in
      grid beside each puzzle so nothing has to solve it twice; uploading that as
      it stands would publish every answer. Only the signed package is copied.
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
        "schedule" => schedule(rest),
        "publish" => publish_site(rest),
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
        "black",
        "given",
        "longest-run",
        "boards",
    ])?;

    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (signer, trusted) = key_and_trust(args.required("key")?)?;
    let wanted: usize = args.number("count", 1)?;
    let from_seed: u64 = args.number("from-seed", 1)?;
    let max_seeds: u64 = args.number("max-seeds", 20_000)?;
    let seconds: u32 = args.number("timeout", 60)?;
    let family = args.get("family").unwrap_or("nonogram");
    // Every flag is checked here, before anything slow starts, so that a flag spelt
    // wrong is reported as a flag spelt wrong rather than as whatever the run does
    // next. The order matters: a missing solver is a fair thing to be told about, but
    // not instead of being told that `--level impossible` is not a level.
    //
    // A flag that belongs to the other game is a mistake about what is being made, not
    // a detail to ignore: `--level easy` on a Slitherlink run would otherwise quietly
    // make puzzles of no particular difficulty, which is exactly what the flag was
    // asking not to happen.
    let mut bands: Vec<&'static Band> = Vec::new();
    match family {
        "nonogram" => {
            for flag in [
                "size",
                "enclosed",
                "black",
                "given",
                "longest-run",
                "boards",
            ] {
                if args.get(flag).is_some() {
                    return Err(format!("--{flag} is not a nonogram flag"));
                }
            }
            bands = chosen_bands(args.get("level"))?;
        }
        "slitherlink" => {
            if args.get("level").is_some() {
                return Err(
                    "--level is for nonogram. Slitherlink has no difficulty levels yet, \
                     on purpose: they wait for measured play data rather than a guess."
                        .to_owned(),
                );
            }
            for flag in ["black", "given", "longest-run", "boards"] {
                if args.get(flag).is_some() {
                    return Err(format!("--{flag} is for kakuro, not slitherlink"));
                }
            }
        }
        "kakuro" => {
            if args.get("level").is_some() {
                return Err(
                    "--level is for nonogram. Kakuro has no difficulty levels yet, on \
                     purpose, and it has a particular reason to wait: the number most \
                     likely to move is how many of its digits are ones and nines."
                        .to_owned(),
                );
            }
            if args.get("enclosed").is_some() {
                return Err("--enclosed is for slitherlink, not kakuro".to_owned());
            }
        }
        other => {
            return Err(format!(
                "no such game: {other}\ngames: nonogram, slitherlink, kakuro"
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

    if family == "kakuro" {
        // Built boards by default, which is what makes `--given 0` the honest default:
        // a built board has no run of one square, so it gives nothing away. See ADR
        // 0021. `--longest-run` is the lever that matters, because each run is one
        // adding-up rule.
        let longest_run: usize = args.number("longest-run", 5usize)?;
        // Built boards unless asked otherwise. The older random method is still reachable
        // because it is still the better trade above 7 by 7: ADR 0021 measured the built
        // method keeping 2 of 8 seeds at 9 by 9 against the random one's 34 of 40. It is
        // asked for by name rather than fallen back to, because a method that quietly
        // started handing a third of the board away would be the worst of both.
        let (layout_from, given_default) = match args.get("boards").unwrap_or("built") {
            "built" => (
                ka_generate::LayoutFrom::Construction(ka_construct::Weave {
                    longest_run,
                    block_first_per_mille: args.number("black", 350u16)?,
                    avoid_swaps: true,
                }),
                0u32,
            ),
            "random" => (ka_generate::LayoutFrom::RandomPattern, 500u32),
            other => {
                return Err(format!(
                    "--boards takes built or random, not {other}.\n\
                     built: no run of one square, so nothing is given away. Better up to \
                     7x7.\n\
                     random: the older method, which gives about 380/1000 of the board \
                     away and is faster above 7x7."
                ))
            }
        };
        let shape = ka_generate::Shape {
            width: args.number("size", 7u16)?,
            height: args.number("size", 7u16)?,
            layout_from,
            block_per_mille: args.number("black", 200u16)?,
            max_given_away_per_mille: args.number("given", given_default)?,
        };
        return fill_kakuro(
            &store,
            &signer,
            &trusted,
            &solver,
            shape,
            wanted,
            from_seed..from_seed.saturating_add(max_seeds),
        );
    }

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

/// Fill the store with Kakuro puzzles.
///
/// The same shape as both loops below and above it — generate, ask a real solver,
/// store only what the solver vouched for — with one line that is this game's own:
/// the number of squares it gave away is printed for every puzzle stored. That is
/// the measurement this family is least proud of, and a run that printed only the
/// cheerful numbers would be a run nobody fixed it from.
fn fill_kakuro(
    store: &Store,
    signer: &Ed25519Signer,
    trusted: &TrustedKeys,
    solver: &ExternalSolver,
    shape: ka_generate::Shape,
    wanted: usize,
    seeds: std::ops::Range<u64>,
) -> Result<(), String> {
    // Printed for every run, including the cheerful numbers, because a run that showed
    // only the good ones is a run nobody fixes anything from. `given away` is zero by
    // construction now, and `ones and nines` is the number that took its place as the
    // one to bring down.
    match shape.layout_from {
        ka_generate::LayoutFrom::Construction(weave) => println!(
            "board:  {}x{} squares, built, runs of 2 to {} squares, {}/1000 black, \
             at most {}/1000 given away",
            shape.width,
            shape.height,
            weave.longest_run,
            weave.block_first_per_mille,
            shape.max_given_away_per_mille
        ),
        ka_generate::LayoutFrom::RandomPattern => println!(
            "board:  {}x{} squares, random pattern, {}/1000 black to start, \
             at most {}/1000 given away",
            shape.width, shape.height, shape.block_per_mille, shape.max_given_away_per_mille
        ),
    }
    println!();

    let mut stored = 0usize;
    let mut rejected = 0usize;
    let mut refused = 0usize;
    let mut undecided = 0usize;
    let mut seed = seeds.start;

    while stored < wanted && seed < seeds.end {
        let this_seed = seed;
        seed += 1;
        let candidate = match ka_generate::propose(this_seed, shape, ka_solve::Budget::default()) {
            Ok(c) => c,
            // A size this family cannot lay out is a bad flag, not a bad seed, and no
            // later seed will fix it. Everything else is this seed's luck.
            Err(ka_generate::Rejected::Unlayoutable(e)) => {
                return Err(format!("--size is not a board this game can make: {e:?}"))
            }
            Err(_) => {
                rejected += 1;
                continue;
            }
        };
        match ka_publish::publish(
            &candidate,
            solver,
            ka_solve::Budget::default(),
            &Sha256,
            signer,
        ) {
            Ok(published) => {
                let witness =
                    match ka_solve::solve(&candidate.puzzle, ka_solve::Budget::default()).witness {
                        Some(w) => w.canonical_bytes(),
                        // A published puzzle has a replayed witness by definition, so
                        // this cannot happen without a bug. Reported rather than
                        // unwrapped: a batch run should not die on one puzzle.
                        None => {
                            eprintln!("  seed {this_seed}: published but has no witness; skipped");
                            continue;
                        }
                    };
                match store.put(&published.package, &witness, &Sha256, trusted) {
                    Ok(id) => {
                        stored += 1;
                        println!(
                            "  {} seed {this_seed}, {} white squares, {} rounds of repair, \
                             {}/1000 given away, {}/1000 ones and nines, {} swap rectangles",
                            id.to_hex(),
                            candidate.puzzle.layout.white_count(),
                            candidate.repairs,
                            ka_generate::given_away_per_mille(&candidate.puzzle.layout),
                            candidate.features.digits_at_the_ends_per_mille,
                            candidate.features.swap_rectangles,
                        );
                    }
                    // Two seeds can produce the same board. Not an error.
                    Err(gs_store::StoreError::AlreadyStored(_)) => continue,
                    Err(e) => return Err(format!("storing seed {this_seed}: {e}")),
                }
            }
            Err(ka_publish::PublishError::Refused { report, .. }) => {
                if report.has_undecided_query() {
                    undecided += 1;
                } else {
                    refused += 1;
                }
            }
            Err(ka_publish::PublishError::Unaskable(_)) => refused += 1,
            Err(ka_publish::PublishError::Encoding(e)) => {
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
        println!("wanted {wanted} — short by {}", wanted - stored);
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

// ---------------------------------------------------------------------------
// schedule
// ---------------------------------------------------------------------------

/// Parse a date the way a person writes one.
fn date_from(text: &str) -> Result<Date, String> {
    let parts: Vec<&str> = text.split('-').collect();
    let bad = || {
        format!(
            "--{text} is not a date; write it as YYYY-MM-DD",
            text = text
        )
    };
    if parts.len() != 3 {
        return Err(bad());
    }
    let year: u16 = parts[0].parse().map_err(|_| bad())?;
    let month: u8 = parts[1].parse().map_err(|_| bad())?;
    let day: u8 = parts[2].parse().map_err(|_| bad())?;
    Date::new(year, month, day).ok_or_else(|| format!("{text} is not a real day"))
}

/// Publish the signed schedule: which puzzle is each day's.
///
/// The command that makes a daily puzzle a daily puzzle. Everything it refuses is a day
/// that would otherwise break for every player at once, which is why there are four
/// refusals rather than one.
fn schedule(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&[
        "store", "key", "start", "days", "family", "add", "on", "serial",
    ])?;
    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (signer, trusted) = key_and_trust(args.required("key")?)?;

    let current = store
        .schedule(&Sha256, &trusted)
        .map_err(|e| e.to_string())?;
    let held = current.as_ref().map_or(0, |opened| opened.schedule.serial);

    let days: u32 = args.number("days", 0)?;
    let adding = args.get("add");

    if days == 0 && adding.is_none() {
        print_schedule(current.as_ref(), &store);
        return Ok(());
    }

    // The start day. Day numbers mean nothing without it, so it can only be chosen once:
    // moving it later would silently renumber every day already published.
    let start = match (current.as_ref(), args.get("start")) {
        (Some(opened), None) => opened.schedule.start,
        (Some(opened), Some(given)) => {
            let given = date_from(given)?;
            if given != opened.schedule.start {
                return Err(format!(
                    "--start {given} does not match the {} this schedule already counts \
                     from. Changing the start day would renumber every day already \
                     published, so it is fixed once the first schedule is signed.",
                    opened.schedule.start
                ));
            }
            given
        }
        (None, Some(given)) => date_from(given)?,
        (None, None) => {
            return Err(
                "--start YYYY-MM-DD is needed for the first schedule: it is the \
                        day the count begins from"
                    .to_owned(),
            )
        }
    };

    let mut entries = current
        .as_ref()
        .map(|opened| opened.schedule.entries.clone())
        .unwrap_or_default();

    // Never schedule a withdrawn puzzle. A withdrawn puzzle is one a device deletes the
    // moment it reads the list, so scheduling one is scheduling a day with nothing on it.
    let withdrawals = store
        .withdrawals(&Sha256, &trusted)
        .map_err(|e| e.to_string())?;
    let withdrawn = |id: &[u8; 32]| -> bool {
        withdrawals.as_ref().is_some_and(|opened| {
            opened
                .list
                .withdrawn
                .iter()
                .any(|entry| &entry.content_id == id)
        })
    };

    if let Some(name) = adding {
        let on = args
            .required("on")
            .map_err(|_| "--add also needs --on YYYY-MM-DD: which day it is for".to_owned())?;
        let date = date_from(on)?;
        let day = date.day_number_from(start).ok_or_else(|| {
            format!(
                "{date} is before this schedule's start day of {start}, so it has no day number"
            )
        })?;
        let family = args.get("family").unwrap_or("nonogram").to_owned();
        // Opened from the store rather than parsed from the name, the same rule
        // `withdraw --add` follows: a name typed on a command line is a claim about a
        // puzzle, not a digest of one. It also means a typo is an error here instead of
        // a day that fetches nothing.
        let stored = store
            .get(name, &Sha256, &trusted)
            .map_err(|e| format!("--add {name}: {e}"))?;
        if stored.package.manifest.family != family {
            return Err(format!(
                "--add {name} is a {} puzzle but --family says {family}",
                stored.package.manifest.family
            ));
        }
        let id = *stored.package.content_id.as_bytes();
        if withdrawn(&id) {
            return Err(format!(
                "--add {name}: that puzzle has been withdrawn, so no device would play it"
            ));
        }
        if entries.iter().any(|e| e.day == day && e.family == family) {
            return Err(format!(
                "day {day} ({date}) already has a {family} puzzle. A day with two \
                 puzzles has no answer, so this is refused rather than one of them \
                 being picked."
            ));
        }
        entries.push(Scheduled {
            day,
            family,
            content_id: id,
        });
    }

    if days > 0 {
        extend_calendar(&store, &trusted, &mut entries, days, &withdrawn, &args)?;
    }

    // Sorted here, once, rather than relied on from the order things were pushed. The
    // format requires it and `seal_schedule` refuses anything else.
    entries.sort_by(|a, b| (a.day, &a.family).cmp(&(b.day, &b.family)));

    let serial: u64 = args.number("serial", held + 1)?;
    if serial <= held {
        return Err(format!(
            "--serial {serial} is not higher than the {held} already published. A \
             schedule numbered at or below the one a device already has is one the \
             device will refuse, which is how an old schedule is stopped from putting \
             back a day that was replaced."
        ));
    }

    let plan = Schedule {
        serial,
        start,
        entries,
    };
    let bytes = seal_schedule(&plan, &Sha256, &signer).map_err(|e| e.to_string())?;
    let written = store
        .put_schedule(&bytes, &Sha256, &trusted)
        .map_err(|e| e.to_string())?;

    println!(
        "wrote {} ({} bytes)",
        store.schedule_path().display(),
        bytes.len()
    );
    print_schedule(Some(&written), &store);
    Ok(())
}

/// Give each game the next `days` days, out of puzzles not already scheduled.
///
/// This is the daily workflow: run it, and every game has another week of days without
/// anybody choosing which puzzle goes where. Puzzles are taken in the store's own order,
/// which is sorted by fingerprint, so the same store and the same request produce the
/// same calendar.
fn extend_calendar(
    store: &Store,
    trusted: &TrustedKeys,
    entries: &mut Vec<Scheduled>,
    days: u32,
    withdrawn: &dyn Fn(&[u8; 32]) -> bool,
    args: &Args,
) -> Result<(), String> {
    let names = store.list().map_err(|e| e.to_string())?;

    // Everything in the store, by family, minus what is already scheduled and minus
    // anything withdrawn.
    let mut free: Vec<(String, [u8; 32])> = Vec::new();
    for name in &names {
        let stored = store
            .get(name, &Sha256, trusted)
            .map_err(|e| format!("{name}: {e}"))?;
        let id = *stored.package.content_id.as_bytes();
        if withdrawn(&id) || entries.iter().any(|e| e.content_id == id) {
            continue;
        }
        free.push((stored.package.manifest.family.clone(), id));
    }

    // Which games to schedule. Named, or every game the store holds, so that a store
    // with two games gets two games' worth of days.
    let wanted: Vec<String> = match args.get("family") {
        Some(one) => vec![one.to_owned()],
        None => {
            let mut families: Vec<String> = Vec::new();
            for (family, _) in &free {
                if !families.contains(family) {
                    families.push(family.clone());
                }
            }
            for entry in entries.iter() {
                if !families.contains(&entry.family) {
                    families.push(entry.family.clone());
                }
            }
            families.sort();
            families
        }
    };
    if wanted.is_empty() {
        return Err("there are no puzzles in the store to schedule".to_owned());
    }

    for family in &wanted {
        // Carry on from the last day this game already has, so running the command twice
        // extends the calendar rather than fighting over the same days.
        let next_day = entries
            .iter()
            .filter(|e| &e.family == family)
            .map(|e| e.day)
            .max()
            .map_or(0, |last| last + 1);

        let mut available: Vec<[u8; 32]> = free
            .iter()
            .filter(|(f, _)| f == family)
            .map(|(_, id)| *id)
            .collect();

        if available.len() < days as usize {
            // Says when it came up short, exactly as `fill` does. Silently scheduling
            // four days when seven were asked for is a Thursday with no puzzle,
            // discovered by the players.
            return Err(format!(
                "{family}: asked for {days} more days but only {} unscheduled puzzles \
                 are in the store. Run `gatedspark fill` for more, then this again. \
                 Nothing has been written.",
                available.len()
            ));
        }
        available.truncate(days as usize);
        for (offset, id) in available.into_iter().enumerate() {
            entries.push(Scheduled {
                day: next_day + u32::try_from(offset).expect("a day count fits"),
                family: family.clone(),
                content_id: id,
            });
        }
    }
    Ok(())
}

fn print_schedule(current: Option<&OpenedSchedule>, store: &Store) {
    let Some(opened) = current else {
        println!("no schedule in {} yet", store.root().display());
        return;
    };
    let plan = &opened.schedule;
    println!(
        "schedule number {}, signed by {}, counting from {}",
        plan.serial, opened.key_id, plan.start
    );
    if plan.is_empty() {
        println!("it names no days");
        return;
    }
    for family in plan.families() {
        let last = plan.last_day(family).unwrap_or(0);
        println!(
            "  {family}: {} days, up to {} ({})",
            plan.entries.iter().filter(|e| e.family == family).count(),
            plan.start.plus_days(last),
            format_args!("day {last}")
        );
    }
    println!();
    println!("A device works out today's day number from its own clock and looks it up");
    println!("here. A wrong clock shows the wrong day's puzzle; it cannot make a bad");
    println!("puzzle look good, because what a puzzle is worth is still its own signature.");
}

// ---------------------------------------------------------------------------
// publish
// ---------------------------------------------------------------------------

/// Write a folder that an ordinary web host can serve.
///
/// # Why there is no program on the server
///
/// Every check that matters already happens on the device: the page checks each
/// package's signature itself against a key built into the page, and it checks the
/// schedule and the withdrawal list the same way. A server that did nothing but hand
/// over files would be a server to run, pay for, patch and have go down, in exchange
/// for nothing. So this writes files.
///
/// # What it deliberately leaves out
///
/// **The answers.** A store keeps `witness.bin` beside each puzzle — the filled-in grid
/// — so that nothing has to solve the puzzle again. Uploading that folder as it stands
/// would publish every answer next to every puzzle. This command copies the signed
/// package and nothing else, and a test holds it.
///
/// The page never needs an answer: it asks the game's own rules whether a board is
/// finished, every time, which is the same reason the page is not allowed to work it out
/// for itself.
fn publish_site(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args)?;
    args.reject_unknown(&["store", "key", "out", "page"])?;
    let store = Store::open(args.required("store")?).map_err(|e| e.to_string())?;
    let (_signer, trusted) = key_and_trust(args.required("key")?)?;
    let out = PathBuf::from(args.required("out")?);

    // The schedule is what makes the folder a daily puzzle rather than a heap of files.
    // Without one, a visitor's page has nothing to ask for.
    let schedule = store
        .schedule(&Sha256, &trusted)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "there is no schedule in this store yet, so a visitor's page would have \
             nothing to open. Run `gatedspark schedule --days 7` first."
                .to_owned()
        })?;

    let withdrawals = store
        .withdrawals(&Sha256, &trusted)
        .map_err(|e| e.to_string())?;

    // Every puzzle the schedule names must be in the store. A day naming a puzzle
    // nobody can fetch is a day that breaks for every player at once, and it is far
    // better to find that here than from a player.
    let names = store.list().map_err(|e| e.to_string())?;
    let mut in_store: Vec<(String, [u8; 32])> = Vec::new();
    for name in &names {
        let stored = store
            .get(name, &Sha256, &trusted)
            .map_err(|e| format!("{name}: {e}"))?;
        in_store.push((name.clone(), *stored.package.content_id.as_bytes()));
    }

    let mut missing = Vec::new();
    for entry in &schedule.schedule.entries {
        if !in_store.iter().any(|(_, id)| id == &entry.content_id) {
            missing.push(format!(
                "day {} ({}) of {}: {}",
                entry.day,
                schedule.schedule.start.plus_days(entry.day),
                entry.family,
                hex(&entry.content_id)
            ));
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "the schedule names {} puzzle(s) that are not in this store, so those days \
             would break for everybody:\n  {}\nNothing has been written.",
            missing.len(),
            missing.join("\n  ")
        ));
    }

    let puzzles = out.join("store");
    io_err(format!("making {}", puzzles.display()), || {
        std::fs::create_dir_all(&puzzles)
    })?;

    // Only the signed package. Never the answer beside it; see this function's header.
    let mut copied = 0usize;
    for (name, _) in &in_store {
        let directory = puzzles.join(name);
        io_err(format!("making {}", directory.display()), || {
            std::fs::create_dir_all(&directory)
        })?;
        // A puzzle's directory is named by its own fingerprint, which is what `list`
        // returns, so no lookup is needed to find it.
        let from = store.root().join(name).join(gs_store::PACKAGE_FILE);
        let to = directory.join(gs_store::PACKAGE_FILE);
        io_err(format!("copying {}", to.display()), || {
            std::fs::copy(&from, &to).map(|_| ())
        })?;
        copied += 1;
    }

    let schedule_to = puzzles.join(gs_store::SCHEDULE_FILE);
    io_err(format!("copying {}", schedule_to.display()), || {
        std::fs::copy(store.schedule_path(), &schedule_to).map(|_| ())
    })?;

    if withdrawals.is_some() {
        let to = puzzles.join(gs_store::WITHDRAWALS_FILE);
        io_err(format!("copying {}", to.display()), || {
            std::fs::copy(store.withdrawals_path(), &to).map(|_| ())
        })?;
    }

    // The page itself, when asked for. Optional because the compiled core is built by
    // `scripts/build-web.sh` and this command does not run a compiler.
    //
    // **The page folder's own `store` is skipped**, and that is not a tidiness rule. A
    // developer's page folder normally has a working store sitting in it, put there by
    // `scripts/demo-web.sh`, and a working store has the answer beside every puzzle.
    // Copying the page folder whole would carry all of them into the published folder
    // and undo the one thing publishing is careful about. The published `store` comes
    // from the real store and from nowhere else.
    let mut page_files = 0usize;
    let mut skipped_a_store = false;
    if let Some(page) = args.get("page") {
        let from = PathBuf::from(page);
        skipped_a_store = from.join("store").is_dir();
        page_files = copy_tree(&from, &out, &["store"])?;
    }

    println!("wrote {}", out.display());
    println!("  {copied} puzzle(s), signed packages only — no answers");
    println!(
        "  the schedule: {} days from {}",
        schedule.schedule.len(),
        schedule.schedule.start
    );
    match &withdrawals {
        Some(opened) => println!(
            "  the withdrawal list: number {}, naming {}",
            opened.list.serial,
            opened.list.len()
        ),
        None => println!("  no withdrawal list (none has been published)"),
    }
    if page_files > 0 {
        println!("  {page_files} page file(s)");
    } else {
        println!("  no page: pass --page crates/gs-web/www to include it");
    }
    if skipped_a_store {
        // Said out loud rather than skipped quietly. Somebody who has been looking at
        // the page locally has a store in that folder, and they should know which of
        // the two stores ended up published.
        println!();
        println!("  note: the page folder has a store of its own and it was NOT copied.");
        println!("  That folder keeps the answer beside each puzzle; the published");
        println!("  puzzles came from --store, without them.");
    }
    println!();
    println!("Upload that folder to any web host. Nothing in it needs a program to serve");
    println!("it, because every check happens on the device that opens it.");
    Ok(())
}

/// Copy a directory and everything under it, returning how many files were written.
///
/// `skip` names entries at the top level that are not copied. It is one entry today —
/// `store` — and the reason it exists is in [`publish_site`].
fn copy_tree(from: &PathBuf, to: &PathBuf, skip: &[&str]) -> Result<usize, String> {
    io_err(format!("making {}", to.display()), || {
        std::fs::create_dir_all(to)
    })?;
    let mut written = 0usize;
    let entries =
        std::fs::read_dir(from).map_err(|cause| format!("reading {}: {cause}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|cause| format!("reading {}: {cause}", from.display()))?;
        let kind = entry
            .file_type()
            .map_err(|cause| format!("reading {}: {cause}", entry.path().display()))?;
        if skip.iter().any(|name| entry.file_name() == *name) {
            continue;
        }
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            // Only the top level is filtered: a folder named `store` deeper inside the
            // page is somebody's own and not the one this command writes.
            written += copy_tree(&entry.path(), &target, &[])?;
        } else {
            io_err(format!("copying {}", target.display()), || {
                std::fs::copy(entry.path(), &target).map(|_| ())
            })?;
            written += 1;
        }
    }
    Ok(written)
}

fn io_err(doing: String, mut act: impl FnMut() -> std::io::Result<()>) -> Result<(), String> {
    act().map_err(|cause| format!("{doing}: {cause}"))
}
