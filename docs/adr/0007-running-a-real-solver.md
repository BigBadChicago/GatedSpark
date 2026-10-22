# ADR 0007 — The solver runs as a separate program, and every failure is UNKNOWN

- **Status:** accepted
- **Date:** 2026-10-22

## Context

Until now the only code in this repository that could run a solver lived in a test file,
`gs-nonogram/tests/solver_agreement.rs`. That was fine while nothing could be published: M4's
pipeline took a `Verifier` and the only implementations were test scaffolding. M5 needs a real one,
because "one command takes an empty store to a signed, published, playable puzzle" means something
in the shipped code has to ask a solver.

Two things had to be decided: how the solver is reached, and what happens when it does not answer.

## Decision

### The solver is a subprocess speaking SMT-LIB2, not a linked library

`gs-solver::ExternalSolver` launches a program, hands it a `.smt2` file, and reads its verdict.
Z3 has a C API and maintained Rust bindings, and linking them would be faster per query. It was not
chosen, for three reasons:

- **ADR 0002 requires the solver to stay replaceable, and ADR 0002's cross-check requires two of
  them.** A subprocess speaking a standard text format makes a second solver a configuration change.
  Two sets of native bindings would be two build problems.
- **A linked solver shares our address space.** A segfault or an out-of-memory kill inside the
  solver takes down a batch worker that may have been filling a pool for hours. A subprocess that
  dies is an `Unknown` on one query.
- **Per-query process overhead does not matter here.** Generation runs ahead of demand
  (`content-delivery.md` section 3), so the budget is in seconds, not in the 100 ms interaction
  path. If measurement ever says otherwise, a linked backend goes behind the same trait — as an
  optimisation, never as a second trust standard.

### Every failure is `Outcome::Unknown`, and never `Unsat`

A solver that will not start, dies on a signal, exceeds its time limit, prints nothing, prints
something unparseable, or prints two contradictory verdicts: all are "nobody knows".

This is the whole point of the crate. `Unsat` on the uniqueness query is the single claim the
platform exists to make — it is how a puzzle is declared to have exactly one answer. A runner that
produced `Unsat` from a missing binary would turn an unconfigured machine into a proof, and the
puzzles it waved through would be indistinguishable in the stored record from ones a solver had
actually examined. Asserted over every failure mode the crate has by
`external_solver::every_failure_in_this_crate_is_unknown_and_none_is_unsat`.

Consequences that follow from it:

- **Nothing in `gs-solver` panics.** It runs unattended inside a worker, where a panic either kills
  the run or gets caught and turned into something. The *reason* for an `Unknown` is returned in
  `NoVerdict` instead, so an operator can tell a missing solver from a hard puzzle — a distinction
  the gate does not need and a human badly does.
- **The test runner still panics, and now drives this crate to do it.**
  `solver_agreement.rs` asks through `ExternalSolver` and panics when the reason is anything but
  the solver's own `unknown`. A malformed encoding should stop the build with the query in the log.
  Since that test no longer carries its own copy of the subprocess logic, CI's one
  outside-the-building job now checks the code that actually ships.

### A verdict is a whole line

`unsat` contains `sat`. Substring matching would read a refutation as a confirmation, which is the
one mistake in this crate that would invert the platform's central claim while looking like working
code. Verified by mutation: replacing the whole-line match with `contains` fails three tests. And
verified against reality rather than assumed —
`real_solver::a_real_solvers_error_output_is_never_mistaken_for_a_verdict` shows that Z3 4.8.12,
given a query it cannot parse, prints on **stdout**:

```
unsupported
(error "line 2 column 19: invalid check-sat-assuming command, ...")
```

The echoed command name contains `sat`. A fake solver cannot establish that a real one behaves this
way, which is why that test is in CI's solver job.

### Two different verdicts is no verdict

Output containing both `sat` and `unsat` is output nobody can read one way. Taking the first or the
last would be picking one of two answers at random. The *same* verdict repeated is fine; some
solvers echo it.

### A time limit is required and cannot be zero

`TimeLimit` has no "unlimited" and no way to spell one, because a worker left running overnight
against a query the solver cannot finish would hang, and a hang is harder to notice than a wrong
answer. Zero is rejected at construction, since a zero limit would make every query `Unknown` while
looking like a setting rather than a mistake.

### Output goes to a file, not a pipe

This one is subtle enough to record. Enforcing the time limit means polling for the child to exit
rather than blocking on its output. A child writing to a pipe nobody is reading blocks when the pipe
fills — and that deadlock would present as a solver taking a long time on a hard query, so it would
be diagnosed as a timeout and never found. A file cannot fill.
`external_solver::a_very_talkative_solver_does_not_deadlock` holds it, with 20,000 lines before the
verdict.

### `Verifier` moved from `gs_nonogram::publish` to `gs_core::verification`

An SMT-LIB2 query is a string, and the trait says nothing about Nonogram. Leaving it beside one
puzzle family would have made a family-agnostic solver runner depend on Nonogram, which is backwards.
`gs_nonogram::publish` re-exports it, so nothing that used it had to change.

## Consequences

- `gs-solver` has no third-party dependencies, which keeps ADR 0004's rule satisfied without needing
  a reason recorded.
- It ships a small `fake-solver` binary. That is test scaffolding in a production crate, which is
  not ideal; the alternative was a shell script, which would not run on Windows and would therefore
  leave this crate unchecked on one of the two platforms the project supports. It solves nothing, is
  told what to do by a directive in the query file so that parallel tests cannot race, and is named
  so that nobody could mistake it for a solver.
- **Not decided here:** which solver, and whether the ADR 0002 cross-check runs in CI. This crate
  makes the cross-check nearly free — two `ExternalSolver`s and a comparison — but building it is
  still a separate step, and the small-team plan says to do it after the slice works end to end.
