# ADR 0004 — The core carries no third-party dependencies yet

- **Status:** accepted, and partly forced by the environment
- **Date:** 2026-10-22

## Context

The cloud environment this repository is being built in has a network policy that denies
`index.crates.io`; `cargo add` fails with a 403 from the proxy gateway. No crate can be fetched
here until that policy is widened.

## Decision

`gs-core` depends on nothing but the standard library for M0. Where a real dependency is required,
the code defines the *interface* and labels the gap rather than shipping a weak stand-in that looks
finished.

## Consequences

- `content::Digest` is a trait with no production implementation. A deliberately weak digest lives
  in the test tree, where nothing that publishes can reach it, and is documented as exercising the
  binding discipline and not collision resistance. M4 binds the trait to a reviewed SHA-256 or
  BLAKE3 implementation before anything is signed.
- The Z3 binding (ADR 0002) cannot be added here yet, which bounds how far M2 can go in this
  environment.
- When the policy is widened, every dependency added arrives with its licence recorded, since the
  solver licence question is already an open item.

## What unblocks this

`index.crates.io` and `static.rust-lang.org` added under the environment's allowed domains, or a
broader network access level. The second host matters because `rust-toolchain.toml` pins the
toolchain, and `rustup` needs that host to honour the pin; local work here uses
`RUSTUP_TOOLCHAIN=stable` as a workaround, which CI does not need.
