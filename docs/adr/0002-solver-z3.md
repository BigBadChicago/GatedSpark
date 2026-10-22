# ADR 0002 — Z3 behind a replaceable `Verifier` interface, with a second solver in CI

- **Status:** accepted; the user chose Z3 plus the CVC5 cross-check on 2026-10-22
- **Date:** 2026-10-22

## Context

Every foundation document says the same thing: integrate a mature, established SMT solver behind an
engine-owned interface, and never write one. The engine owns the puzzle encoding, the solver
lifecycle and the verification policy.

The claim that carries the most weight is uniqueness. It is established by finding a solution,
asserting a blocking constraint that requires at least one relevant variable to differ, and getting
UNSAT. The architecture plan is explicit that this establishes uniqueness **only relative to the
same complete encoding**. If the encoding or the solver is wrong, a puzzle with two solutions is
published as unique — and under the identical-daily-board decision, every player meets it at once.

## Decision

1. **Z3 is the primary solver.** MIT licence, mature, stable C API, maintained Rust bindings, builds
   for mobile targets.
2. **The engine talks to it through a `Verifier` trait over SMT-LIB2 text.** The solver stays
   replaceable, which the architecture requires of every vendor dependency.
3. **CVC5 (BSD-3-Clause) re-checks UNSAT and uniqueness results in CI** over the regression corpus.
   Because the interface is SMT-LIB2, this is CI plumbing rather than a second encoder. Chosen by the
   user on 2026-10-22 over running Z3 alone, so a disagreement between two solvers is a build
   failure rather than something nobody would ever notice.

## Consequences

- Two independent solvers disagreeing on a uniqueness result is a CI failure, which is how an
  encoding bug is caught before it becomes a false claim about a published puzzle.
- CVC5 is built plain and runs only in CI: some optional CVC5 build configurations link
  GPL-licensed components, which is not acceptable in a shipped client and is irrelevant in CI.
- Nonogram's encoding is Boolean plus cardinality constraints, so a dedicated SAT solver such as
  CaDiCaL will very likely be faster than either SMT solver. That goes behind the same `Verifier`
  trait later as a performance choice. It is never a second trust standard: content-delivery.md D4
  allows one verification standard, not a cheaper one for some channel.
- Not yet implemented. This environment's network policy blocks the crate registry, so no solver
  binding is vendored yet; see ADR 0004.

## Alternatives

**Z3 alone.** Cheaper, and loses the independent check on exactly the claim that matters most.
**CVC5 as primary.** Stronger proof production for UNSAT, weaker mobile story. If proof objects
later become a hard requirement, this ADR gets revisited rather than worked around.
