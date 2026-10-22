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
| `scripts/windows/` | The Windows build wrapper. See *Building on Windows* below. |

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

Next is M3, generation: a pattern generator feeding mechanical clue derivation, with the solver
deciding what is accepted. The generator is never a correctness oracle.

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

### Building on Windows

Use the wrapper, from anywhere in the repository:

```bat
scripts\windows\cargo.cmd test --workspace
scripts\windows\cargo.cmd clippy --all-targets
```

Anything after the script name goes to cargo unchanged. The first run installs a second toolchain
and takes a few minutes; later runs are ordinary cargo builds.

Plain `cargo` on Windows fails with ``error: linker `link.exe` not found`` unless Visual Studio or
the Microsoft Build Tools are installed, because the default Windows toolchain has no linker of its
own. The wrapper uses the GNU toolchain, which brings one. Rustup cannot select a different
toolchain per operating system, which is why this is a script and not a line of configuration. See
[`docs/adr/0005-windows-gnu-toolchain.md`](docs/adr/0005-windows-gnu-toolchain.md).
