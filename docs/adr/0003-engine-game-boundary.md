# ADR 0003 — The platform suite runs with no puzzle family installed

- **Status:** accepted and implemented
- **Date:** 2026-10-22

## Context

The one thing worth copying from the PuzzleGen retrospective is that its engine/game boundary held
while three unrelated games were added, because CI ran the platform suite with no game installed,
with guards against that suite passing vacuously. The retrospective's advice was to build this
check on day one rather than after the boundary has already leaked.

Nonogram is the launch family, and Slitherlink is family two. The boundary will be tested for real
when Slitherlink needs a global connectivity constraint that Nonogram never exercises.

## Decision

`crates/gs-core/tests/platform_with_no_family.rs` runs the platform's own invariants against
`Registry::empty()`. `crates/gs-core/tests/anti_vacuity_guard.rs` tests the guard that stops it
passing for nothing.

## What is enforced, and by what

| Claim | Enforcing test |
|---|---|
| The platform works with zero families installed | `platform_holds_with_no_family_installed` |
| A family lookup never falls back to a default | same test, via `LookupError::NotInstalled` |
| A timeout is not a proof of unsatisfiability | same test, `Outcome::proves_unsatisfiable` |
| An undecided query cannot publish, and is reported as undecided rather than failed | same test, `decide` returning `Refusal::Undecided` |
| A gate with nothing asserted against it refuses | same test, `decide(&[])` |
| The publication decision reads no clock and no configuration | the signature of `verification::decide`, plus the purity check in the same test |
| Content identity comes from bytes, with one definition | same test, `ContentId::of` |
| The suite cannot pass by running no checks | `a_ledger_that_ran_nothing_fails` |
| The suite cannot pass by running some checks | `a_ledger_missing_one_check_fails` |
| A check cannot drift away from its declared name | `an_undeclared_check_name_is_rejected` |
| One check cannot stand in for the others | `one_check_cannot_stand_in_for_the_others` |
| Floating-point arithmetic in the core fails the build | `float_arithmetic = "deny"`; verified by planting an `f64` function and observing the failure |

Explicitly **not yet enforced**, and labelled as such in the code: digest strength
(`content::Digest` has no cryptographic implementation yet — ADR 0004), and the no-float rule over
state *types* rather than arithmetic, which arrives with the state representation in M1.

## Consequences

Adding a platform check means adding its name to `platform_suite::PLATFORM_CHECKS`, the single
definition. Forgetting to run it is then a failure rather than a silence. The list is the only
place the count of checks exists, so no document or report re-derives it.
