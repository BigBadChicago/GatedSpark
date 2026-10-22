//! A stand-in solver, for this crate's tests. It solves nothing.
//!
//! `ExternalSolver` launches a program, so testing it needs a program. A shell script
//! would not run on Windows, and the Windows CI job runs the whole test suite, so a
//! script would quietly leave this crate's behaviour unchecked on one of the two
//! platforms the project supports.
//!
//! # How it is told what to do
//!
//! From the query file, not from the environment. The test writes a directive line
//! into the query text it hands over:
//!
//! ```text
//! ; FAKE: unsat
//! (check-sat)
//! ```
//!
//! The reason is that tests run in parallel threads of one process, so an environment
//! variable would be shared between them and two tests wanting different behaviour
//! would race. The query file belongs to exactly one query.
//!
//! It answers `--version` so that `ExternalSolver::runnable` can be tested too.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("fake-solver 0 (solves nothing)");
        return;
    }

    let Some(path) = args.first() else {
        eprintln!("fake-solver: expected a query file");
        std::process::exit(2);
    };
    let query = std::fs::read_to_string(path).unwrap_or_default();
    let mode = query
        .lines()
        .find_map(|line| line.trim().strip_prefix("; FAKE:"))
        .map(str::trim)
        .unwrap_or("silent");

    match mode {
        "sat" => println!("sat"),
        "unsat" => println!("unsat"),
        "unknown" => println!("unknown"),
        // The verdict repeated. Some real solvers echo it, and repeating one answer is
        // not the same as giving two.
        "repeat" => {
            println!("unsat");
            println!("unsat");
        }
        // Two different verdicts. Must be read as no verdict at all.
        "contradictory" => {
            println!("sat");
            println!("unsat");
        }
        // The word, but not on a line of its own. A substring search would read this
        // as a refutation; whole-line parsing must not.
        "prose" => println!("after some thought the answer is unsat, probably"),
        // Something that is not a verdict, so the output is not empty but says nothing.
        "chatty" => {
            println!("(error \"line 3: unknown function\")");
            println!("; done");
        }
        "stderr" => {
            eprintln!("fake-solver: something went wrong on line 7");
            std::process::exit(1);
        }
        // A verdict on stdout and a non-zero exit. The verdict still counts: real
        // solvers exit non-zero for reasons that have nothing to do with the answer.
        "unsat-then-fail" => {
            println!("unsat");
            std::process::exit(3);
        }
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
        },
        // Fills its output with a verdict buried in a lot of text, to show that the
        // output path cannot deadlock the way a pipe would.
        "noisy" => {
            let mut out = std::io::stdout().lock();
            for i in 0..20_000 {
                let _ = writeln!(out, "; working, step {i}");
            }
            let _ = writeln!(out, "unsat");
        }
        other => {
            eprintln!("fake-solver: no such mode {other}");
            std::process::exit(2);
        }
    }
}
