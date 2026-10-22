# ADR 0001 — The shared deterministic core is written in Rust

- **Status:** accepted, pending the user's confirmation (raised in the project thread 2026-10-22)
- **Date:** 2026-10-22

## Context

The development plan §3 specifies "shared deterministic core: Rust or C++ for predictable
performance, memory control and native portability", with Swift and Kotlin integrations over a
native core bridge. The choice between the two was left open.

Two of the architecture plan's design principles constrain it. *Reproducible releases* requires
that a build can be re-created from a recorded revision. And correctness must not depend on
hardware floating-point behaviour, which means the logical state has to be integer-only — a rule
the foundation review noted was asserted everywhere and enforced nowhere.

## Decision

Rust.

## Consequences

- The integer-only rule is enforceable cheaply. `[workspace.lints.clippy] float_arithmetic = "deny"`
  makes floating-point arithmetic in the core a build failure today; verified by planting an `f64`
  function and observing the build fail. M1 extends this to a check over state *types*, which the
  lint does not cover.
- One toolchain covers the core, the generation worker and the CLI, and targets
  `aarch64-apple-ios`, Android via the NDK, and `wasm32` for the M5 review harness, from the same
  source.
- `Cargo.lock` plus `rust-toolchain.toml` pin the build inputs, which is what "reproducible" needs
  in practice.
- `unsafe_code = "forbid"` at the workspace level. Any future FFI bridge crate must opt out
  explicitly and locally, which makes the exception reviewable.
- Cost accepted: Rust's compile times, and the fact that the iOS and Android bridges are more work
  than they would be from C++. The bridge is out of the vertical slice either way.

## Alternatives

**C++.** Equally permitted, with the better-established mobile story. Rejected because the
determinism and reproducibility commitments are the expensive ones here, and they are where C++
costs more: no equivalent of a lockfile, and no cheap way to make floating-point in the logical
core a build error.
