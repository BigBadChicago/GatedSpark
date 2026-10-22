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
| `crates/gs-core` | The shared deterministic core. Today: the platform's vocabulary and the no-family-installed suite. Next: the DSL, transition semantics, state hashing and replay. |
| `docs/adr/` | Architecture decision records, including the language and solver choices. |

The project's foundation documents, the build plan for this slice, and the content-delivery design
live in the project's shared folder rather than in the repository.

## Current state: milestone M0

M0 is the repository and its test harness. The launch puzzle family is **Nonogram**, with
**Slitherlink** as family two, and no family is implemented yet.

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
