# ADR 0005: Windows builds use the GNU toolchain

Status: accepted. The user chose this on 2026-10-22 over installing Microsoft's
Build Tools and over documenting the problem without fixing it.

## The problem

`rust-toolchain.toml` pins `1.97.0`. On Windows that resolves to
`1.97.0-x86_64-pc-windows-msvc`, and that toolchain has no linker of its own: it
calls `link.exe`, which arrives with Visual Studio or the standalone Microsoft Build
Tools. On a Windows machine that has rustup and nothing else, every build stops at:

```
error: linker `link.exe` not found
```

This was found on 2026-10-22 on the machine holding the project's working clone. It
is worth naming precisely: the pin gives a reproducible *compiler* and says nothing
about the *linker*, which is a separate program the compiler shells out to in order
to stitch compiled pieces into one executable. "Reproducible releases" was therefore
a weaker claim than it looked.

## The decision

Windows builds use `<pinned channel>-x86_64-pc-windows-gnu`, which bundles its own
linker, through the wrapper `scripts/windows/cargo.cmd`. The wrapper reads the
channel out of `rust-toolchain.toml` instead of repeating it, installs the toolchain
on first use, and passes everything else to cargo unchanged.

`rust-toolchain.toml` is unchanged, because it cannot express this: rustup has no way
to select a different toolchain per operating system. That is the whole reason a
wrapper exists rather than a line of configuration.

## Why not the alternatives

**Install the Microsoft Build Tools.** Works, and is what most Rust-on-Windows
projects do. Rejected because it makes a multi-gigabyte install from another vendor a
precondition for building this repository, which is a bad trade for a project whose
Windows-specific code is currently zero.

**Document it and move on.** Rejected: a known build failure with a known fix does
not belong in prose only. A contributor meeting `linker link.exe not found` has no
reason to connect it to a paragraph in a README.

**Switch the pin to the GNU channel outright.** Rejected: the channel line is shared
with Linux CI, where the GNU Windows triple means nothing. Per-operating-system
selection is exactly what rustup does not offer.

## What this costs

* Windows builds and Linux builds use different linkers, so a linker-specific problem
  can appear on one and not the other. Accepted: the CI matrix already builds on
  Linux x86-64 and Linux arm64, and the `windows` job adds a third combination.
* The GNU target's C runtime differs from the MSVC one. This matters when linking
  against C libraries. The workspace has no third-party dependencies at all today
  (ADR 0004), so it does not matter yet. It will matter when a Z3 binding arrives, and
  that is the point to revisit this decision rather than now.
* The first build on a Windows machine downloads a second toolchain.

## What is enforced

The `windows` job in `.github/workflows/ci.yml` runs the wrapper, checks that the
toolchain it selects reports a `*-windows-gnu` host, and builds and tests the whole
workspace with it.

The job does **not** prove the build works on a machine with no Visual Studio,
because GitHub's Windows runners ship with the Build Tools and a job cannot honestly
remove them. That the GNU target does not call `link.exe` follows from the target
triple, not from a test here. Recorded as such in ADR 0003.
