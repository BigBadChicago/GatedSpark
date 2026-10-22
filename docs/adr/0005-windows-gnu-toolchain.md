# ADR 0005 — Windows developer builds use the GNU toolchain, not MSVC

- **Status:** accepted
- **Date:** 2026-10-22

## Context

Three plain definitions first, because the whole decision turns on them. A **linker** is the last
step of a build: it joins the compiled pieces into one program you can run. A **toolchain** is the
bundle of Rust programs that performs a build. Every toolchain has a **host**, meaning the kind of
machine it runs on, written as a triple such as `x86_64-pc-windows-msvc`.

Rust on Windows comes in two flavours. The default, `msvc`, borrows its linker and the system
libraries from Visual Studio. The other, `gnu`, brings its own linker and libraries, in a rustup
component called `rust-mingw`.

On a Windows machine with no Visual Studio, the build stops before a single test runs:

```
error: linker `link.exe` not found
note: the msvc targets depend on the msvc linker but `link.exe` was not found
```

This was observed on 2026-10-22 on a Windows 11 machine with no Visual Studio, no Build Tools and
no `gcc`. Two findings from that machine shaped the decision:

- Adding the gnu **target** to an msvc **host** toolchain does not fix it. The build then asks for
  `x86_64-w64-mingw32-gcc`, a cross-compiler which is also absent. The linker arrives with the gnu
  host toolchain, not with the target's standard library — so `targets = [...]` in
  `rust-toolchain.toml` would not have helped.
- With the gnu host toolchain the whole suite passes: 47 tests, `cargo fmt --all --check`,
  `cargo clippy --all-targets --all-features`, and `solver_agreement` against a real Z3 under
  `GATEDSPARK_REQUIRE_SOLVER=1`.

Worth naming the general lesson: the pin gives a reproducible **compiler** and says nothing about
the **linker**, which is a separate program the compiler shells out to. "Reproducible releases" was
a weaker claim than it looked.

`rust-toolchain.toml` pins `channel = "1.97.0"` and names no host. That is deliberate: rustup fills
the host in from the machine. Writing `channel = "1.97.0-x86_64-pc-windows-gnu"` would pin a
Windows host for *every* machine, breaking the Linux CI jobs and the Apple and Android targets
ADR 0001 commits to.

## Decision

Windows developer builds use the `gnu` host toolchain. The repository keeps control of the
version; the machine supplies the host:

```
rustup set default-host x86_64-pc-windows-gnu
rustup toolchain install <the channel pinned in rust-toolchain.toml>-x86_64-pc-windows-gnu
```

`scripts/setup-windows.ps1` does both, reading the version out of `rust-toolchain.toml` so it
cannot drift from the pin. After it, a plain `cargo test --workspace` works with no extra flags.

A per-directory override (`rustup override set`) was rejected for this. It names one exact
version, so a later bump of the pin would be silently ignored on exactly the machines that ran the
setup. Changing the default host leaves the pin authoritative.

## Consequences

- The pinned channel stays host-free. `rust-toolchain.toml` now carries a comment saying so, because
  adding a triple there is the obvious-looking "fix" that would break every non-Windows machine.
- A new Windows CI job builds and tests on the gnu toolchain, so the path cannot rot unnoticed. It
  selects the toolchain the same way the documented setup does, rather than naming a version of its
  own. What this job **cannot** prove is that the build works with no Visual Studio present:
  GitHub's Windows runners ship it, and we are not going to uninstall it to find out. The narrower
  claim it defends is worth stating plainly: the gnu toolchain builds this workspace and its tests
  pass. The job **asserts** the host triple rather than printing it for a human to read, so the
  check cannot stay green if the switch stops working. The limit is recorded in ADR 0003's
  not-enforced list as well, so it is visible from the one place that lists what is enforced.
- `gnu` and `msvc` use different **ABIs** — the rules for how compiled code calls other compiled
  code. For a workspace that is pure Rust with no third-party crates (ADR 0004), nothing here
  depends on the difference. It begins to matter if we later link a C library, or ship a Windows
  binary to players, where `msvc` is the normal choice. **This decision covers developer builds and
  does not settle what a release build uses.**
- `rustup set default-host` is a machine-wide setting rather than a per-project one. On a machine
  with no MSVC linker, that is an honest description of the machine. A developer who later installs
  Visual Studio can reverse it with `rustup set default-host x86_64-pc-windows-msvc`.
- Solver agreement on Windows was checked by hand, not by CI. The Linux `solver` job stays the one
  that gates it.

## Alternatives

**Install Visual Studio Build Tools.** Works, and keeps the default msvc ABI. Rejected as a
*requirement* for developer builds: it is a multi-gigabyte install for a workspace that needs no C
compiler at all. It remains the right answer for shipping Windows binaries, which this ADR leaves
open.

**Pin the gnu triple in `rust-toolchain.toml`.** One line and no setup step. Rejected: it breaks
every non-Windows machine, CI included.

**A wrapper script in the repository** that calls `cargo +<pinned channel>-x86_64-pc-windows-gnu`.
Written first, then dropped in favour of the setting above. It needs no one-time setup and touches
nothing outside the repository, which is genuinely better in one respect.

It was also *not* untested, and an earlier draft of this ADR said it was. It ran on a
`windows-latest` runner in CI and passed the full suite there. What it was never run on is a
machine with no Visual Studio — and so was nothing else in CI, which is the limit recorded below.
So the honest comparison is narrower than "tested versus untested": both approaches are proven on a
runner that has Visual Studio, and only the default-host setting is proven on a machine that does
not. What decides it is the remaining defect: every command has to go through the wrapper, so a
plain `cargo build` from an editor or rust-analyzer still fails with the original linker error, on
exactly the machines the change exists for.

**`RUSTUP_TOOLCHAIN=...-x86_64-pc-windows-gnu` on each command.** The same escape hatch the README
already documents for blocked networks. Works, but has to be remembered on every command, and
freezes the version the way a directory override does.
