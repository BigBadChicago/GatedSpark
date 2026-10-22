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

**Amendment, 2026-10-22: the block is per environment, not a property of the project.** Both hosts
are reachable from the project's own Windows machine — `cargo search z3` works there, and the gnu
toolchain was installed from `static.rust-lang.org` on it the same day. What this ADR describes is
the cloud container's network policy. So the zero-dependency rule above is a constraint being
honoured, not a law of the project: the moment the container's allowed domains are widened, adding
a dependency becomes an ordinary decision again rather than an impossible one. Nothing here changes
until then, because a dependency that only some machines can fetch is worse than none.

## Amendment, 2026-10-22, second one: the rule becomes "no dependency without a recorded reason"

The container's network policy was widened and `index.crates.io`, `static.crates.io` and
`static.rust-lang.org` are reachable, confirmed by `cargo search sha2` and a full `cargo build` in a
fresh container. A running container keeps the rules it booted with, so picking up a changed policy
needs a new session; that is worth knowing but is not a property of the project.

So the constraint this ADR described is gone, and with it the reason for the zero-dependency rule as
written. The rule it is replaced by:

- **`gs-core` still has no third-party dependencies.** It is the crate the platform's determinism
  rests on, and keeping its audit surface at zero is worth something on its own. This is now a
  choice rather than a constraint.
- **Everywhere else, a dependency needs a reason written down.** Not a ban, and not a free hand. The
  first three arrive with ADR 0006: `sha2`, `ed25519-dalek` and `signature`, all in the new
  `gs-crypto` crate, with their licences and the alternatives weighed recorded there.
- **`content::Digest`, `package::Signer` and `package::SignatureVerifier` are implemented, in
  `gs-crypto`.** The paragraph above saying they have no production implementation is superseded.
  They stay as traits in `gs-core` for the reason ADR 0006 gives, which is not the registry: a crate
  that can sign is a crate that can forge, and a crate that verifies its own signatures proves
  nothing.

What has not changed: a dependency that only some machines can fetch is still worse than none, which
is why this amendment waited for the policy change rather than anticipating it.
