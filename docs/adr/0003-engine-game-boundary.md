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

### The batch worker (M3, added 2026-10-22)

| Claim | Enforcing test |
|---|---|
| The same request produces the same puzzles in the same order | `worker::the_same_request_produces_the_same_puzzles` |
| A quota that cannot be filled stops at the seed limit and reports the shortfall | `worker::an_impossible_quota_stops_and_says_so_rather_than_running_forever` |
| Every puzzle delivered belongs to the level it was asked for | `worker::every_puzzle_produced_is_in_the_level_it_was_asked_for` |
| Every puzzle delivered still has exactly one answer, checked end to end | `worker::every_puzzle_produced_still_has_exactly_one_answer` |
| No run contains the same puzzle twice | `worker::no_two_puzzles_in_a_run_are_the_same` |
| The duplicate check is reached rather than being a counter nothing increments | `worker::the_duplicate_check_is_not_dead_code`, which names the measured colliding seed pair |
| A recorded seed rebuilds its own puzzle, so a validation report means something | `worker::a_candidate_can_be_rebuilt_from_its_recorded_seed` |
| Nothing in the worker reads a clock | structural: no time source is imported, and `BandOutcome` records seeds rather than durations |
| Two clue sets have equal bytes exactly when they are equal | `Clues::canonical_bytes`, with lengths before contents so the encoding cannot be ambiguous |

### The validation report (M4, added 2026-10-22)

| Claim | Enforcing test |
|---|---|
| A complete report is publishable, so the tests below are not vacuous | `validation_report::a_complete_report_is_publishable` |
| A report that asked nothing refuses rather than defaulting to yes | `validation_report::a_report_with_no_queries_refuses_rather_than_defaulting_to_yes` |
| An undecided query is never a pass, at the report layer as well as the gate | `validation_report::an_undecided_query_is_not_a_pass` |
| An undecided query stays findable in a report refused for some other reason | `validation_report::an_undecided_query_is_still_findable_in_a_report_refused_for_another_reason` |
| A decisive wrong answer is not recorded as an undecided one | `validation_report::a_puzzle_with_two_answers_is_not_publishable` |
| A witness the engine refused blocks publication | `validation_report::a_witness_the_engine_refused_is_not_publishable` |
| A witness never replayed also blocks it, distinctly from one refused | `validation_report::a_witness_never_replayed_is_not_publishable_either` |
| No content id means nothing can be published | `validation_report::a_report_with_no_content_id_is_not_publishable` |
| A missing difficulty measurement is not a zero | `validation_report::a_missing_measurement_is_not_a_zero` |
| A puzzle the game grades into no level is recorded as ungraded, not given one | `validation_report::an_ungraded_puzzle_is_recorded_as_ungraded` |
| The encoding a uniqueness claim is relative to is recorded | `validation_report::the_encoding_a_uniqueness_claim_is_relative_to_is_recorded` |
| The report does not re-decide publication | structural: `publishable` calls `verification::decide`, so the report and the gate are one judgement |
| The report reads no clock | structural: there is no timestamp field, so a stored record cannot depend on the reader's clock |

### The whole pipeline, end to end (M4, added 2026-10-22)

| Claim | Enforcing test |
|---|---|
| The worker's output, asked about by a real solver, agrees with our filter on every puzzle | `solver_agreement::the_whole_pipeline_produces_reports_a_real_solver_agrees_with`, **in the `solver` CI job only** |
| The only thing stopping a good report from being publishable is the missing content id | same test, and `publishing::the_only_thing_standing_between_a_good_report_and_publication_is_the_missing_digest` |
| A solver that could not be run becomes `UNKNOWN`, never `UNSAT` | the `Answer`-to-`Outcome` mapping in `SolverAsVerifier`; a broken solver must not become a proof |
| A solver timeout is recorded and refused, not retried or reinterpreted | `publishing::a_solver_timeout_is_recorded_and_refused_not_retried` |
| A solver disagreeing with our filter is recorded as disagreeing, with the solver's answer kept | `publishing::a_solver_that_disagrees_with_the_filter_is_recorded_as_disagreeing` |
| Nobody knowing is not counted as agreement | `publishing::an_undecided_solver_answer_is_not_agreement` |
| Solvability is asked before uniqueness, since uniqueness needs the witness | `publishing::the_uniqueness_query_is_the_second_one_asked` |
| The witness is replayed through the engine's own goal test, not re-checked locally | `publishing::the_witness_is_replayed_through_the_engine`; `replay_of` calls `Ruleset::is_solved` |
| Two reports for the same candidate are identical, so a re-verification can diff them | `publishing::two_reports_for_the_same_candidate_are_identical` |
| This crate cannot answer its own solver questions | structural: `Verifier` is a trait (now in `gs_core::verification`, re-exported here) with no implementation in either the core or a puzzle family, so an `Outcome` can only arrive from outside; the real one is in `gs-solver` (ADR 0007) |

### Windows builds (added 2026-10-22, see ADR 0005)

| Claim | Enforcing test |
|---|---|
| A Windows build runs on the gnu host, which brings its own linker | the `windows` CI job asserts `rustc -vV` reports `host: x86_64-pc-windows-gnu`, rather than printing it for a human to read |
| The whole workspace compiles and its tests pass on that host | same job, `cargo test --workspace --all-targets` |
| The pinned version is written down once, not twice | `scripts/setup-windows.ps1` and the CI job both read `channel` out of `rust-toolchain.toml`; neither names a version |

### The package: the bytes that leave the building (M4)

| Claim | Enforcing test |
|---|---|
| Nothing in a package is decoded before its bytes have been vouched for | `package::a_body_that_fails_its_digest_is_never_decoded` and `package::a_rejected_signature_is_never_decoded`, both of which give `open` a body that is malformed *and* unvouched-for and require the refusal to name the check, not the body |
| A changed byte anywhere in the body is refused | `package::a_flipped_bit_anywhere_in_the_body_is_refused`, which flips every byte in turn |
| A changed byte in the stated fingerprint is refused | `package::a_flipped_bit_in_the_stated_content_id_is_refused` |
| A body cannot be swapped under someone else's signature | `package::a_body_cannot_be_swapped_under_a_signature` |
| A signature from a key the verifier does not trust is refused | `package::a_key_the_verifier_does_not_trust_is_refused` |
| Extra bytes after the end are refused, not ignored | `package::trailing_bytes_are_refused` |
| Every truncation is refused and none panics | `package::every_truncation_is_refused_and_none_panics`, over every prefix |
| A container version this reader does not know is refused, not guessed at | `package::an_unknown_format_version_is_refused_rather_than_guessed_at` |
| A byte nobody can read is a broken file, never `UNKNOWN` | `package::a_tag_nobody_can_read_is_a_broken_file_and_not_an_unknown_outcome` |
| A zero byte is not a valid outcome or replay result | `package::a_zero_byte_is_not_a_valid_outcome_or_replay`; no tag constant is zero, so a zero-filled region cannot decode as `Sat` |
| `UNKNOWN` survives into the stored file as `UNKNOWN` | `package::unknown_survives_as_unknown` |
| The encoding has no slack: decode then re-encode is byte-identical | `package::the_encoding_has_no_slack` |
| Lengths come before contents, so a field split cannot be moved | `package::lengths_come_before_contents_so_a_split_cannot_be_moved` |
| An absent level is not an empty one | `package::an_absent_band_is_not_an_empty_band` |
| An over-long field is an error, never a truncation | `package::an_over_long_field_is_an_error_rather_than_a_truncation` |
| The fingerprint handed back is recomputed, not taken from the file | `package::the_content_id_is_recomputed_from_the_body_rather_than_taken_from_the_file` |
| "These bytes were checked" cannot be claimed without checking them | `Opened` has a private field, so `open` is the only code that can build one; the `compile_fail` doctest on `Opened` holds this, and the `checks` CI job runs `cargo test --workspace --doc` because `--all-targets` skips doctests |
| The core cannot sign or verify its own packages | structural: `Signer` and `SignatureVerifier` are traits with no implementation in `gs-core`; the real ones are in `gs-crypto`, which `gs-core` does not depend on (ADR 0006) |
| A `ContentId` still only ever comes from digesting bytes | structural: `OpenError::ContentIdMismatch` carries the stated value as raw bytes, because a claim found in a file is not something this code digested |
| A package becomes a playable puzzle again | `packaging::a_sealed_package_becomes_a_playable_puzzle`, which rebuilds the puzzle from the payload, re-solves it, and checks the engine accepts the answer |
| Clue bytes round-trip, including empty lines and movable splits | `packaging::clue_bytes_round_trip` and `packaging::clue_bytes_round_trip_for_every_difficulty_level` |
| A payload that is not clues is refused rather than guessed at | `packaging::a_payload_that_is_not_clues_is_refused_rather_than_guessed_at` |
| A clue set whose line count disagrees with its board is refused | `packaging::a_line_count_that_disagrees_with_the_board_is_refused`; accepting one would let a decoded puzzle reach a state no generated puzzle can |
| A package does not carry the answer | `packaging::the_package_does_not_carry_the_answer`, which first checks the search finds the clues, so a search that found nothing cannot pass by accident |
| The evidence the report recorded survives into the file | `packaging::the_package_carries_the_evidence_the_report_recorded` |
| The digest really is SHA-256 and the signature really is Ed25519 | `gs-crypto/tests/known_answers.rs`, against the published FIPS 180-4 and RFC 8032 vectors rather than against another run of our own code |
| A changed byte anywhere in a published file is detected | `packaging::a_single_flipped_bit_anywhere_in_a_published_file_is_detected`, every bit of every byte, one at a time |
| A genuine signature from a key the reader was not told to trust is refused | `packaging::a_file_signed_by_a_key_this_reader_does_not_trust_is_refused`, which then widens the trusted list and opens the same bytes, so the refusal is shown to be about the key and not the file |
| Nothing is signed before the gate has agreed to it | structural: `publish` seals only when `publishable` returns `NoContentId` and nothing else; `packaging::a_candidate_the_solver_will_not_vouch_for_is_never_sealed` |
| A report names the fingerprint of the package actually written from it | structural: `seal` returns the id it computed and `bind_to` takes the `Sealed`, so a caller cannot supply an id it derived itself; `packaging::publish_hands_back_a_report_bound_to_the_package_it_sealed` |
| The whole pipeline is reproducible, signature included | `packaging::publishing_the_same_candidate_twice_gives_byte_identical_files` |
| The content id covers which container version the body is read under | `package::the_content_id_covers_the_container_version_and_not_only_the_body` |
| A solver that could not answer is never recorded as a refutation | `external_solver::every_failure_in_this_crate_is_unknown_and_none_is_unsat`, over every failure mode the runner has |
| A verdict is a whole line, so `unsat` is never read as `sat` | `external_solver::the_word_unsat_inside_a_sentence_is_not_a_refutation`, verified by mutation; and against a real solver's own error output by `real_solver::a_real_solvers_error_output_is_never_mistaken_for_a_verdict` |
| A solver's time limit is real, and the runner cannot deadlock waiting for one | `external_solver::a_solver_that_never_finishes_is_killed_and_reported_as_a_timeout` and `..._a_very_talkative_solver_does_not_deadlock` |
| CI's one outside-the-building job checks the runner that ships, not a copy of it | structural: `solver_agreement.rs` drives `gs_solver::ExternalSolver`; see ADR 0007 |
| Nothing reaches the store that the store cannot verify itself | `store::a_package_the_store_cannot_verify_never_reaches_the_disk`, `..._a_package_signed_by_an_untrusted_key_is_refused` |
| A stored puzzle is named by its own fingerprint, and the name is checked against it | structural: `Store::put` is not given an id and `ContentId` has no constructor from bytes; `store::the_directory_is_named_by_the_content_id_and_holds_the_two_files`, `..._a_renamed_directory_is_a_refusal_rather_than_a_different_puzzle` |
| A stored answer that disagrees with the signed report is caught, against damage rather than against an adversary | `store::an_answer_that_does_not_match_the_signed_report_is_refused`, `..._an_answer_corrupted_on_disk_after_the_fact_is_caught_on_the_way_out` |
| A puzzle whose answer was never replayed is not kept | `store::a_package_with_no_replayed_answer_is_refused` |
| A half-written puzzle is never served, listed or counted | `store::a_half_written_puzzle_is_not_a_puzzle` |
| A re-check of an empty store reports that it verified nothing | `command::recheck_on_an_empty_store_says_it_verified_nothing` |
| A missing solver is reported as a missing solver, and nothing is stored | `command::a_missing_solver_is_reported_as_a_missing_solver` |
| A mistyped option is an error rather than a silently ignored default | `command::a_misspelled_option_is_an_error_rather_than_being_ignored`, `..._an_option_with_no_value_is_an_error_rather_than_a_guess` |
| A development signing key cannot be written into a repository or overwrite another | `command::keygen_refuses_to_write_inside_a_git_repository_and_writes_nothing`, `..._keygen_never_overwrites_an_existing_key`, `..._two_keys_are_different_keys` |
| An empty folder becomes signed, solver-verified, replayable puzzles | `command::an_empty_folder_becomes_signed_verified_puzzles_and_then_rechecks`, against a real solver in CI |
| The solver runner and the store are never compiled into the browser | `boundary::the_solver_runner_and_the_store_are_not_compiled_into_the_browser`, with `..._the_forbidden_crates_still_exist_under_those_names` as its anti-vacuity guard |
| The browser recomputes a puzzle's fingerprint rather than believing it | `loading::the_fingerprint_is_recomputed_rather_than_believed` |
| The browser refuses a puzzle signed by a key its page was not given | `loading::a_puzzle_signed_by_somebody_else_is_refused`, `..._a_page_that_trusts_nothing_accepts_nothing` |
| Every single changed byte of a package is refused by the browser | `loading::every_single_changed_byte_is_refused` |
| The browser refuses another family's puzzle, or another rules version, rather than attempting it | `loading::another_games_puzzle_is_refused_rather_than_attempted`, `..._a_puzzle_from_different_rules_is_refused_in_either_direction` |
| A `Playable` can only be obtained by loading a verified package | structural: `load` is the only function returning one; recorded in ADR 0009's not-enforced list, since a negative cannot be proved |
| The browser's translation layer works in a real browser | `in_a_real_browser::*`, six tests in headless Chrome, run in CI |
| Every reason a package can be refused is written as a sentence, not a type name | `loading::every_single_reason_a_package_can_be_refused_is_written_in_words` |
| No refusal shown to a player contains a type name | `loading::reads_as_words`, applied in both refusal tests |
| The page draws a grid with a clue for every row and column | `scripts/drive-page.mjs`, run by `scripts/check-page.sh` — **by hand, not in CI** |
| Tapping fills, crosses out and clears; undo and redo go back and forward | `drive-page.mjs` — by hand |
| The page shows "solved" when the rules say so, and not when one square is changed | `drive-page.mjs` — by hand; the second half is the guard that makes the first mean anything |
| Switching puzzles rebuilds the grid and forgets the old game | `drive-page.mjs` — by hand |
| A package with one byte changed is refused by the page, in readable words | `drive-page.mjs` — by hand |

Explicitly **not yet enforced**, and labelled as such in the code:

- **The page is only checked by hand.** `scripts/check-page.sh` drives the real page in a real
  browser and is the only thing that tests the page at all, but it is not in CI: it needs a
  browser-driving dependency this project has not taken on. The wasm tests in CI prove the *core*
  works in a browser; nothing in CI proves the *page* does. Nothing anywhere checks how it looks.
- **Whether every blank must be crossed out.** `Nonogram::is_solved` requires every square to be
  decided, so finishing a 15 by 15 puzzle with 93 filled squares took 372 taps. That is the rule as
  written and as tested; whether it is the rule the project wants is an open question for its owner,
  recorded in ADR 0010.
- ~~**Digest strength.**~~ **Resolved 2026-10-22 by ADR 0006.** The digest is SHA-256 and the
  signature is Ed25519, both in `gs-crypto`, both checked against published test vectors. What is
  still true, and is the reason the traits stay unimplemented in `gs-core`, is that this crate's
  own package tests use stand-ins and so prove the *order and shape* of the checks rather than their
  strength. The strength claims live where the real implementations are reachable:
  `gs-nonogram/tests/packaging.rs` flips every bit of a published file one at a time and requires
  each one to be refused, and requires a genuine signature from an untrusted key to be refused.
  Still not decided, and tracked in ADR 0006 rather than here: where a production signing key comes
  from.
- **Re-judging a package that was read back from a file.** `verification::decide` takes queries
  whose names are `&'static str`, and a name read out of a file is not static, so a
  `package::Manifest` cannot be handed to the gate. That blocks the first of the four jobs
  `content-delivery.md` section 6 names for the report: re-verifying the corpus when the rules or
  the solver change. It is recorded here rather than worked around, because the obvious
  workaround — a second copy of the gate's logic that reads owned strings — is the one thing
  worse than the gap. Fixing it properly means changing the signature of the most load-bearing
  function in the crate, which is a decision and not a chore.
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
- **A stored answer against somebody who can write to the store.** `gs-store` checks `witness.bin`
  against the `StateHash` in the signed report, and `gs_core::hash` is explicit that the strength
  needed of a `StateHash` is "none; an accidental collision is what is being guarded against". It
  is 64-bit FNV-1a, so it catches corruption and a careless edit, and does not stop a writer
  willing to craft a colliding answer. The package itself is signed, so only the cached answer is
  reachable, and an answer is recomputable from the clues. Making it adversarial means recording
  the witness under a cryptographic digest too, which changes `Manifest` and therefore changes what
  every existing signature covers — a decision, not a tidy-up. See ADR 0008.
- **Periodic state checkpoints.** The architecture plan lists them in the authoritative record.
  They are a storage and seek optimisation belonging to the save format in M4; `Timeline::goto`
  walks the tree in the meantime, and is the reference the checkpointed version will be tested
  against.

## Consequences

Adding a platform check means adding its name to `platform_suite::PLATFORM_CHECKS`, the single
definition. Forgetting to run it is then a failure rather than a silence. The list is the only
place the count of checks exists, so no document or report re-derives it.
