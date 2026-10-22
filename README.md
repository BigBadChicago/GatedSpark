# GatedSpark

A platform for building original digital puzzles: AI proposes candidates, formal verification
decides what can be trusted, and deterministic software delivers the result reliably, online or
offline.

The verification layer is the trust boundary. LLM output is untrusted input and is never evidence
of correctness, uniqueness, security or difficulty. `SAT`, `UNSAT` and `UNKNOWN`/timeout stay
distinct outcomes, and `UNKNOWN` never counts as a pass.

## Where things are

| | |
|---|---|
| `crates/gs-core` | The shared deterministic core: the puzzle DSL, logical state, transition semantics, the branching timeline, replay, and the platform's verification vocabulary. |
| `crates/gs-nonogram` | The launch puzzle family: clues, rules, the line solver, the difficulty feature vector, and the SMT-LIB2 encoding a real solver reads. |
| `docs/adr/` | Architecture decision records, including the language, solver and toolchain choices. |
| `scripts/` | One-time setup for Windows machines. See *On Windows, run the setup script once* below. |

The project's foundation documents, the build plan for this slice, and the content-delivery design
live in the project's shared folder rather than in the repository.

## Current state: milestones M0 to M2

M0 was the repository and its test harness. M1 is the deterministic core: the DSL, logical state,
`S(t+1) = F(S(t), A(t), R)`, state hashing, undo and redo with branching, and replay against
recorded hashes. M2 is Nonogram: the clue model, the line solver, the difficulty feature vector,
the SMT-LIB2 encoding, and a CI job that runs the encoding past a real solver and fails if the
solver and the line solver disagree.

The launch puzzle family is **Nonogram**, with **Slitherlink** as family two. The DSL already
reserves the Tier B global constraints Slitherlink needs, because freezing the DSL around local
constraints is the one decision here that would be expensive to reverse.

M3 (generation) and most of M4 (the validation report) now exist: a pattern generator feeds
mechanical clue derivation, the solver decides what is accepted, and every accepted puzzle carries a
report of the questions a solver was asked and what it answered. The generator is never a
correctness oracle. One piece of M4 is deliberately missing: there is no content fingerprint yet, so
nothing is publishable, and a test asserts that this is the only thing standing in the way.

What exists and is enforced by a test is listed in
[`docs/adr/0003-engine-game-boundary.md`](docs/adr/0003-engine-game-boundary.md). Nothing in this
repository claims to be enforced without naming the test that enforces it; where something is not
yet enforced, it says so.

## Building

```sh
cargo test --workspace     # everything
cargo clippy --all-targets # floating-point arithmetic in the core is a build error
```

The toolchain is pinned in `rust-toolchain.toml`. In an environment whose network policy blocks
`static.rust-lang.org`, prefix commands with `RUSTUP_TOOLCHAIN=stable`; see
[`docs/adr/0004-dependencies.md`](docs/adr/0004-dependencies.md).

### The solver check is skipped unless a solver is installed

`cargo test --workspace` compares this code against this code. Exactly one test compares it against
an outside program: `solver_agreement`, which asks a real SMT solver whether it agrees with our line
solver. With no solver on the PATH — the list of folders your system searches when you type a
command — that test prints a notice saying it verified nothing and returns. Rust hides whatever a
*passing* test prints, so what you see is a bare `ok` that checked nothing.

To see the notice, or a real verification:

```sh
cargo test -p gs-nonogram --test solver_agreement -- --nocapture
```

A real run prints `verifying against N solver(s)`. If that line is absent, the check did not happen.
CI does not depend on remembering any of this: the `solver` job installs Z3 and sets
`GATEDSPARK_REQUIRE_SOLVER=1`, under which a missing solver fails the test instead of skipping it.

### On Windows, run the setup script once

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-windows.ps1
```

After that, the commands above work as written, with no extra flags, and so do plain `cargo` runs
from an editor. It is safe to run the script more than once.

The one thing to know before running it: it changes this machine's **default** Rust host, so other
Rust projects on the same machine also start building with the `gnu` toolchain. That is reversible
with `rustup set default-host x86_64-pc-windows-msvc`.

Why it is needed: a *linker* is the last step of a build, the part that joins the compiled pieces
into one program you can run. Rust's default Windows setup borrows that linker from Visual Studio,
so on a machine without Visual Studio the build stops with ``linker `link.exe` not found`` before any
test runs. The script switches the machine to Rust's `gnu` toolchain, which brings its own linker,
and installs the version named in `rust-toolchain.toml`. The host is deliberately **not** pinned in
that file, because pinning it would break Linux and the Apple and Android targets; see
[`docs/adr/0005-windows-gnu-toolchain.md`](docs/adr/0005-windows-gnu-toolchain.md).
