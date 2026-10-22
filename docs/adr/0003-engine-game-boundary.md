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
| The DSL expresses a Tier B global constraint with no family installed | same test, `Constraint::tier` |
| The suite cannot pass by running no checks | `a_ledger_that_ran_nothing_fails` |
| The suite cannot pass by running some checks | `a_ledger_missing_one_check_fails` |
| A check cannot drift away from its declared name | `an_undeclared_check_name_is_rejected` |
| One check cannot stand in for the others | `one_check_cannot_stand_in_for_the_others` |
| Floating-point arithmetic in the core fails the build | `float_arithmetic = "deny"`; verified by planting an `f64` function and observing the failure |

### M1, the deterministic core

| Claim | Enforcing test |
|---|---|
| The state hash is identical on x86_64 and aarch64 | `determinism::the_state_hash_is_pinned_across_architectures`, run on both in CI |
| Nothing hashes a state except through its one canonical encoding | `determinism::the_hash_goes_through_the_canonical_encoding_and_nothing_else` |
| Board shape is part of a state's identity | `determinism::dimensions_are_part_of_identity` |
| A replay reproduces every recorded hash | `determinism::a_replay_reproduces_every_recorded_hash` |
| A record that claims a state its actions do not produce is refused | `determinism::a_replay_whose_hashes_disagree_is_refused` |
| A record from a different ruleset version is refused, not reinterpreted | `determinism::a_record_from_different_semantics_is_refused_not_reinterpreted` |
| An action after undo branches and keeps the old future | `timeline_branching::an_action_after_undo_branches_and_keeps_the_old_future` |
| Redo follows the branch most recently visited | `timeline_branching::redo_follows_the_branch_most_recently_visited` |
| Jumping to any node agrees with walking there | `timeline_branching::goto_any_node_agrees_with_walking_there` |
| A move that changes nothing is refused rather than recorded | `timeline_branching::a_move_that_changes_nothing_is_refused` |
| Undo at the root and redo with no future are errors, not silent no-ops | `timeline_branching::undoing_at_the_root_is_an_error_not_a_silent_no_op`, `redoing_with_no_future_is_an_error` |
| History that no longer matches the state is refused, not repaired | `Delta::apply` returning `DeltaError::Diverged`; reached by `Timeline::verify_cursor_hash` |
| A global constraint nested inside a conjunction is not lost | `dsl_tiers::a_global_constraint_nested_in_a_conjunction_is_not_lost` |
| An empty conjunction is local, not global | `dsl_tiers::an_empty_conjunction_is_local_and_says_so` |
| A cell's value cannot be a float | `state::assert_integer_only::<CellRepr>()`, a compile-time binding to a sealed trait with no float implementation |

### Solver agreement (added after the first real solver run)

| Claim | Enforcing test |
|---|---|
| A real solver reaches the verdict our line solver predicted, on every fixture | `solver_agreement::a_real_solver_agrees_with_our_line_solver`, **in the `solver` CI job only** |
| A uniquely-solvable puzzle's re-query is UNSAT, and an ambiguous one's is SAT | same test, via the `ambiguous 2x2` control fixture |
| A missing solver fails rather than skips in CI | the `GATEDSPARK_REQUIRE_SOLVER=1` branch in `solvers()`; verified by running the test with and without it |
| A misconfigured second solver cannot silently disable the cross-check | `solver_agreement::an_unrunnable_second_solver_is_an_error_not_a_quiet_single_check` |

The honest qualification: this is enforced **in CI**, not everywhere. On a machine with
no solver the test prints that it verified nothing and returns, which is a convenience
for contributors and would be a vacuous pass in CI — hence the environment variable.

### Generation (M3, added 2026-10-22)

| Claim | Enforcing test |
|---|---|
| A seed rebuilds its board exactly | `generation::a_seed_rebuilds_the_same_pattern`, and `generation_rng::the_stream_from_a_seed_is_pinned` underneath it |
| A block of seeds rebuilds the same candidates and the same report | `generation::the_same_block_of_seeds_gives_the_same_candidates` |
| One round of the automaton does not depend on scan order | `generation::smoothing_is_not_order_dependent`; structurally, `smooth` is double-buffered |
| The clues a candidate carries are satisfied by the answer a solver finds, with no reference to the board they came from | `generation::a_candidates_clues_describe_its_own_board` |
| Nothing ambiguous or undecided is ever returned as a candidate | `generation::nothing_accepted_is_ambiguous_or_undecided` |
| No seed is lost between the counts | `Report::is_consistent`, asserted in `generation::nothing_accepted_is_ambiguous_or_undecided` and `the_yield_is_measured_and_printed` |
| A real solver agrees with the filter on puzzles the generator **kept** and on ones it **threw out as ambiguous** | `solver_agreement::a_real_solver_agrees_with_our_line_solver`, generated fixtures, **in the `solver` CI job only** |
| The generator cannot become the verifier | structural: `generate` returns a `Candidate`, and neither it nor `Features` has any conversion to `verification::Outcome` |
| A yield is measured, never asserted from a stored number | `generation::the_yield_is_measured_and_printed` prints the counts and asserts only that they add up and that the generator produces something |

### Nonogram's difficulty levels (added 2026-10-22)

The levels are the game's to define, not the platform's, so they live in
`gs-nonogram::difficulty` and `gs-core` still has no notion of a band.

| Claim | Enforcing test |
|---|---|
| Every level can actually be produced, by generating at its own declared shape | `difficulty::every_band_is_reachable_by_generating_at_its_own_shape` |
| The level list is not empty, so the tests above are not iterating nothing | `difficulty::there_is_at_least_one_band` |
| No puzzle is claimed by two levels, so `band_of` does not depend on list order | `difficulty::no_two_bands_claim_the_same_puzzle` |
| The list is ordered easier to harder, which is the order a menu would show | `difficulty::bands_are_ordered_from_easier_to_harder` |
| A puzzle needing a guess is in no level at all | `difficulty::a_puzzle_needing_a_guess_is_in_no_band` |
| A puzzle outside every window is ungraded rather than squeezed into the nearest level | `difficulty::a_puzzle_outside_every_window_is_ungraded_rather_than_squeezed_in` |
| A level is a board size *and* a window; features alone cannot claim one | `difficulty::a_puzzle_of_the_wrong_size_is_not_claimed` |
| Asking for one level never returns a puzzle from another | `difficulty::asking_for_one_band_never_returns_another` |
| Each level's shape has a workable yield, so no level is reachable in theory and empty in practice | `difficulty::every_bands_shape_is_one_the_generator_accepts` |
| Each level generates from its own shape rather than competing with another's | `difficulty::the_shapes_are_distinct_so_each_band_has_its_own_production` |

Explicitly **not** enforced, and the honest limit of all of the above: that a player
would agree with the ordering. No play data exists. The tests prove the levels are
reachable, distinct and ordered by measured solver work; they cannot prove the
mapping to the words a player reads is right, and that stays an informed guess until
there are players. The thresholds are provisional and the module says so.

### Windows builds (added 2026-10-22, see ADR 0005)

| Claim | Enforcing test |
|---|---|
| A Windows build runs on the gnu host, which brings its own linker | the `windows` CI job asserts `rustc -vV` reports `host: x86_64-pc-windows-gnu`, rather than printing it for a human to read |
| The whole workspace compiles and its tests pass on that host | same job, `cargo test --workspace --all-targets` |
| The pinned version is written down once, not twice | `scripts/setup-windows.ps1` and the CI job both read `channel` out of `rust-toolchain.toml`; neither names a version |

Explicitly **not yet enforced**, and labelled as such in the code:

- **Digest strength.** `content::Digest` has no cryptographic implementation yet; see ADR 0004.
- **`f64: !IntegerOnly` at runtime.** Rust cannot assert a negative implementation without an extra
  dependency. The trait is sealed, which carries that half structurally, and the clippy lint covers
  arithmetic regardless.
- **The CVC5 cross-check from ADR 0002.** The test supports it today through
  `GATEDSPARK_SMT_SOLVER_2`, and refuses to run quietly if that variable names a program
  it cannot execute. What is missing is only the CI step that installs CVC5; it is left
  out rather than guessed at, because an unverified download step would simply turn CI
  red. Until that step exists, the cross-check is available but not exercised.
- **A Windows build on a machine with no Visual Studio.** The `windows` job proves the GNU
  toolchain builds and tests the workspace; it cannot prove the absence of `link.exe`, because
  GitHub's Windows runners ship with the Microsoft Build Tools and a job cannot honestly
  uninstall them. That the GNU target never calls `link.exe` follows from the target triple.
  The one real observation behind this is the failure on the project's own Windows machine on
  2026-10-22, which is evidence rather than a test.
- **Periodic state checkpoints.** The architecture plan lists them in the authoritative record.
  They are a storage and seek optimisation belonging to the save format in M4; `Timeline::goto`
  walks the tree in the meantime, and is the reference the checkpointed version will be tested
  against.

## Consequences

Adding a platform check means adding its name to `platform_suite::PLATFORM_CHECKS`, the single
definition. Forgetting to run it is then a failure rather than a silence. The list is the only
place the count of checks exists, so no document or report re-derives it.
