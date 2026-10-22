# ADR 0006 — SHA-256 for the content id, Ed25519 for the signature, both in one crate

- **Status:** accepted
- **Date:** 2026-10-22
- **Supersedes:** the "no production implementation" parts of ADR 0004

## Context

A package's content id is the only thing standing between a report and publication:
`ValidationReport::publishable` refuses with `NoContentId` while it is absent, and a test asserts
that this is the *only* remaining objection. Until now there was no implementation, because the
environment could not reach the package registry and because hand-writing a cryptographic hash was
rejected: a subtly wrong one looks exactly like a right one until it matters, and nothing in this
repository's test discipline could tell them apart.

The registry is now reachable. The decision recorded here is which primitives, and where they live.

## Decision

### The content id is SHA-256

Not because it is the fastest or the newest. Because a content id is the name an artifact is known
by for as long as the artifact exists, and the property worth more than speed is that *anything
else* can recompute it: another language, another team, a shell one-liner, a court. SHA-256 has
published test vectors, is implemented everywhere, and is what `sha256sum` computes. BLAKE3 is
faster and perfectly respectable, and loses on exactly this point.

The implementation is the `sha2` crate (RustCrypto, MIT OR Apache-2.0).

One wrinkle, written down because it would otherwise be a surprise: a content id is **not**
`sha256sum` of the package body. It is SHA-256 over the container version, two bytes little endian,
followed by the body. The reason is in `package::digest_input` — the signature covers the content id
and nothing else, so anything outside the content id is unprotected, and the container version says
which rules the body is to be read under. Without it, the day a version 2 exists, a signed body
could be relabelled and read under whichever version suited whoever changed the byte.

### The signature is Ed25519

- **Deterministic.** The same key over the same message gives the same bytes, with no random nonce.
  That fits a platform whose whole argument is reproducibility, and it removes the failure that has
  broken ECDSA deployments repeatedly, where a repeated or predictable nonce leaks the private key.
  `publishing_the_same_candidate_twice_gives_byte_identical_files` is the test that depends on it.
- **Fixed sizes.** 64-byte signatures, 32-byte public keys, which keeps the package format simple.
- **Reviewed implementation with published vectors.** `ed25519-dalek` (BSD-3-Clause), checked here
  against RFC 8032 section 7.1.

Two choices inside it, both narrowing what is accepted:

1. **Verification is `verify_strict`, not `verify`.** The permissive check is the one RFC 8032
   describes, and under it a signature is not unique: more than one byte string can verify for the
   same key and message. In a format whose premise is that a file has exactly one identity, that is
   the wrong property. The strict check also refuses small-order points inside the signature.
2. **A domain string is prefixed to what is signed** (`gatedspark/package-content-id/v1\0`). A bare
   Ed25519 signature is a signature over 32 arbitrary bytes, and a content id is 32 bytes; without
   something naming the purpose, a signature over one kind of 32-byte value could be presented as a
   signature over another. The trailing version means a future change to what is signed is a new
   domain rather than a quiet reinterpretation of old signatures.

`TrustedKeys::trust` also refuses small-order public keys. These decode perfectly well, and anybody
can forge signatures that verify under them, so a reader that trusted one would accept forged
packages with every check passing.

### Both live in `gs-crypto`, not in `gs-core`

`gs-core` keeps zero dependencies and keeps the three traits — `content::Digest`, `package::Signer`,
`package::SignatureVerifier` — with no implementation. The registry is no longer the reason. The
reason is that a crate which can sign is a crate which can forge, and a crate that verifies its own
signatures proves nothing about the real thing. Because the real signer is unreachable from
`gs-core`, every signature in that crate's own test tree is visibly a stand-in, named so that using
one to publish would be obvious in review. Put a real signer within reach of those tests and that
stops being true.

The second reason is smaller and still real: `gs-core` is the crate the platform's determinism rests
on, and every dependency in it is one more thing to pin, audit and reproduce.

## Consequences

- Building the workspace now needs `index.crates.io`. Three direct dependencies, 24 crates in the
  tree; `Cargo.lock` is committed.
- `ValidationReport::publishable` can return `Ok` for the first time in this project's history.
  `gs_nonogram::publish::publish` is the closed loop that gets there: report, gate, seal, bind, gate
  again.
- The package tests become claims rather than wiring checks. Every bit of a published file is
  flipped one at a time and the file must be refused; a genuine signature from an untrusted key must
  be refused.
- **Still not decided: where a production signing key comes from.** This crate has no key generation
  outside tests and no key storage, deliberately — a convenient generator is how a private key ends
  up committed to a repository. Key custody, rotation and the trusted-key list a device ships with
  are operational decisions, and nothing in the slice needs them. The test key is printed in the
  clear in `crates/gs-nonogram/tests/packaging.rs` under a name that says so.
- **Still not decided: the second solver (ADR 0002).** The CVC5 cross-check in CI is unaffected by
  this and remains open.
