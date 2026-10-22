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
| `docs/adr/` | Architecture decision records, including the language and solver choices. |

The project's foundation documents, the build plan for this slice, and the content-delivery design
live in the project's shared folder rather than in the repository.

## Current state: milestone M1

M0 was the repository and its test harness. M1 is the deterministic core: the DSL, logical state,
`S(t+1) = F(S(t), A(t), R)`, state hashing, undo and redo with branching, and replay against
recorded hashes.

The launch puzzle family is **Nonogram**, with **Slitherlink** as family two. Neither is
implemented yet: the constraint encoding and the solver integration are M2. The DSL already
reserves the Tier B global constraints Slitherlink needs, because freezing the DSL around local
constraints is the one decision here that would be expensive to reverse.

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

### On Windows, run the setup script once

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-windows.ps1
```

After that, the commands above work as written, with no extra flags.

Why it is needed: a *linker* is the last step of a build, the part that joins the compiled pieces
into one program you can run. Rust's default Windows setup borrows that linker from Visual Studio,
so on a machine without Visual Studio the build stops with ``linker `link.exe` not found`` before any
test runs. The script switches the machine to Rust's `gnu` toolchain, which brings its own linker,
and installs the version named in `rust-toolchain.toml`. The host is deliberately **not** pinned in
that file, because pinning it would break Linux and the Apple and Android targets; see
[`docs/adr/0005-windows-gnu-toolchain.md`](docs/adr/0005-windows-gnu-toolchain.md).
