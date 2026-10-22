# ADR 0009 — The browser gets the rules and the signature, and cannot reach a solver

- **Status:** accepted
- **Date:** 2026-10-22

## Context

The user decided (2026-10-22) to build the core so it can later run in a web browser or on a
desktop, which made the plan web first by way of WebAssembly. This is M5's third piece: the point
where a verified puzzle reaches a person.

A browser is a different kind of place from the one the first two pieces were built for. It cannot
start a program, has no folders, and nothing in it should be trusted to be honest — least of all
the web server the page came from.

## The measurement that shaped this

Every crate in the workspace was compiled for `wasm32-unknown-unknown` before anything was
designed. All five succeeded, **including the two that must never be in a browser.**

| Crate | Compiles for the browser | Belongs there |
| --- | --- | --- |
| `gs-core` | yes | yes |
| `gs-nonogram` | yes | yes |
| `gs-crypto` | yes | yes |
| `gs-store` | yes | **no** — it reads directories |
| `gs-solver` | yes | **no** — it starts programs |

The bottom two rows are the finding. The compiler will not stop anybody from shipping the solver
runner into a browser; on that target, the standard library's process and filesystem calls are
present and simply do not work. So a browser built carelessly would not crash. `gs-solver` turns
every failure into `Outcome::Unknown` by design (ADR 0007), the pipeline correctly refuses to treat
`Unknown` as a proof, and the result is a browser in which **no puzzle is ever provably unique**.
Every puzzle rejected, nothing visibly broken, and the cause three crates down a dependency list.

That failure mode — a check that silently stops checking — is the one this repository exists to
prevent. It does not get a note in a document.

## Decision

### The dependency list is the boundary, and a test holds it

`gs-web` depends on `gs-core`, `gs-crypto` and `gs-nonogram`, and on nothing else in the
workspace, not even for its own tests. `tests/boundary.rs` walks the manifests and fails if
`gs-solver` or `gs-store` is compiled into it, printing the path by which it arrived.

Three things make that test more than decoration:

- **It names the forbidden crates from one place.** `gs_web::MUST_NOT_REACH` is the single list;
  the test does not keep its own.
- **It checks the forbidden crates still exist under those names.** A renamed crate would make the
  guard pass while guarding nothing, since a name matching nothing can never be found.
- **It checks what *is* compiled in, exactly.** Otherwise deleting `gs-web`'s dependencies
  entirely, or a manifest scanner that silently stopped finding anything, would both pass.

**What the first run of that test found.** It failed, correctly as written and wrongly as
intended: `gs-nonogram` names `gs-solver` in its `[dev-dependencies]`, which is compiled into
`gs-nonogram`'s own test binaries and into nothing that depends on it. The scanner now distinguishes
the sections, and the rule is stated rather than assumed: **a crate's test-only dependencies count
for that crate and not for its dependents.** `gs-web`'s own test-only dependencies do count, because
one appearing there means the design has drifted and the next step is somebody moving the line up a
section.

A consequence worth keeping on purpose: **nothing in the browser can produce a random number.** The
only crate that wanted randomness was the key generator, and the browser neither generates puzzles
nor signs anything. Nothing in it can make a key, and nothing in it can sign.

### The browser checks the signature itself, against a key the page supplies

`gs_web::load` recomputes the fingerprint over the bytes and checks the signature over the
fingerprint. Nothing it believes comes from the server it fetched from.

The key list comes from the page's own code, through `Page::trust`. It is the same reasoning that
makes `gatedspark show` take `--key` (ADR 0008): a reader that trusts a key list it finds beside
the content trusts whatever anybody put there, so the list would be decoration. A page starts
trusting nothing, so a page that forgot to install its key accepts nothing — visibly wrong straight
away, rather than a page that accepts everything.

### The family and the rules version are checked, and a mismatch is a refusal

A package names which game it is and which version of that game's rules it was verified under. A
reader with a different version does not have nearly the same puzzle: a replay against different
semantics is not a replay, and every claim in the report was made about rules this build may not
implement. Both directions are refused, because newer and older are equally unjudgeable.

`gs_nonogram::rules::RULESET_VERSION` is new, and exists for this: the version was previously a
literal inside `Nonogram::new`, and a reader cannot compare against a literal.

### `Playable` can only be obtained by loading

There is no constructor for it besides `load`, so holding one *is* the evidence that a signature
was checked. This is the same discipline as `ContentId` (only from digesting bytes, ADR 0006) and
`Opened` (only from `open`).

### One new dependency: `wasm-bindgen`, target-gated

Rust cannot talk to a web page by itself. `wasm-bindgen` 0.2.129 (MIT OR Apache-2.0) is the only
maintained way to do it — every route into a browser either uses it or reimplements it, and the
alternative is writing and maintaining the same translation ourselves. That is the recorded reason
ADR 0004 requires.

It is declared under `[target.'cfg(target_arch = "wasm32")'.dependencies]`, so a build or a test on
an ordinary machine never compiles it, and nothing outside `src/browser.rs` can reach it.

### The translation layer decides nothing

`src/browser.rs` is the only file that knows a web page exists. It holds no rules, decides no
question, and checks nothing `load` has not already checked — every method on its `Puzzle` type
reads a field that `load` filled in.

The specific mistake this guards against is a page working out for itself whether a board is
finished, "just for the display". There would then be two answers to that question, they would
disagree one day, and the wrong one would be the one the player saw. The division is also what
keeps most of this crate testable without a browser, and a test that needs a browser is a test
that will eventually be skipped.

## Consequences

- **The browser download is 57 KB compressed** (155 KB uncompressed, release build, before
  `wasm-bindgen`'s own post-processing). Download size is not a constraint on this design, which
  was worth knowing before deciding anything about splitting the module up.
- CI builds `gs-web` for the browser's target, so a change that breaks the browser build fails on
  the pull request rather than being discovered by hand.
- The browser cannot re-verify a puzzle, and is not meant to. It checks the signature and the
  rules version; the proof was done by a machine with a solver, and the report records it.
- It also does not refuse a package whose answer was never replayed. The store refuses that on the
  way in (ADR 0008), which is the right place: by the time bytes reach a browser the judgement has
  been made, and the report carries it for a page that wants to show it.
- Nothing is playable yet. This piece carries the trust chain into the browser and stops there,
  deliberately, because that is the part that could have turned out to be impossible.

## Enforcement

| Claim | Test |
| --- | --- |
| The solver runner and the store are not compiled into the browser | `boundary::the_solver_runner_and_the_store_are_not_compiled_into_the_browser` |
| The guard cannot pass by naming crates that do not exist | `boundary::the_forbidden_crates_still_exist_under_those_names` |
| What the browser does contain is exactly the three allowed crates | `boundary::the_browser_contains_exactly_what_it_is_supposed_to` |
| The dependency walk is transitive rather than one level deep | `boundary::the_walk_really_does_follow_dependencies` |
| The manifest scanner finds real dependencies and not comments or prose | `gs_web::tests::*`, eight cases |
| A real signed puzzle loads, and twice running gives the same thing | `loading::a_real_puzzle_loads_and_knows_what_it_is` |
| The fingerprint is recomputed from the bytes, not read out of them | `loading::the_fingerprint_is_recomputed_rather_than_believed` |
| A puzzle signed by a stranger is refused | `loading::a_puzzle_signed_by_somebody_else_is_refused` |
| A page that trusts nothing accepts nothing | `loading::a_page_that_trusts_nothing_accepts_nothing` |
| Every single changed byte is refused | `loading::every_single_changed_byte_is_refused`, over the whole package |
| A truncated download is refused rather than half drawn | `loading::a_truncated_download_is_refused_rather_than_half_drawn` |
| Another game's puzzle is refused rather than attempted | `loading::another_games_puzzle_is_refused_rather_than_attempted` |
| A different rules version is refused in either direction | `loading::a_puzzle_from_different_rules_is_refused_in_either_direction` |
| The version this build implements is one it accepts | `loading::the_version_this_build_implements_is_the_one_it_accepts` |
| A signed package whose board does not read is refused, not guessed at | `loading::a_signed_package_whose_board_does_not_read_is_a_refusal_not_a_guess` |
| Nothing that is not a package loads | `loading::nothing_that_is_not_a_package_loads` |
| Every refusal explains itself without showing a type name | `loading::every_refusal_explains_itself_in_words_a_person_can_read` |
| The browser does not re-decide publication | `loading::a_replay_that_was_never_reproduced_still_loads_but_is_visible` |

| The translation layer works in a real browser | `in_a_real_browser::*`, six tests inside headless Chrome |

### On the browser test

This was written up as *not enforced* when the crate was first built, with a headless-browser test
named as the next thing needed. It was then written, because a gap recorded in an ADR is a gap that
ships.

It is one file and deliberately not the way this crate is tested. A test needing a browser needs a
matching browser, driver and `wasm-bindgen-cli`, and a test that cannot run is a test that will
eventually be skipped. So the native suite stays the measurement and this is the narrow claim no
native test can make: that `wasm_bindgen` really carries a byte array in from a page and a string
back out, that real SHA-256 and real Ed25519 give the same answers inside a browser's WebAssembly
engine as they do natively, and that a stranger's puzzle is refused there too.

It runs in `run_in_browser` mode rather than under Node on purpose. Node has a filesystem and a
process model, so a test passing there would say little about the place this code is meant to run.

One thing it shows that is worth stating: generating a puzzle, deriving its clues, replaying the
answer and signing the result are all pure computation, so the test builds its own signed package
inside the browser. What a browser cannot do is ask a *solver* — which is the entire reason
`gs-solver` is not allowed in this crate.

### Not enforced

- **Every byte of a package, in a browser.** The browser test changes four bytes; the exhaustive
  version runs natively. A browser test is slow, and the claim being made here is that the digest
  behaves the same, not that the byte loop works twice.
- **That `Playable` has no other constructor.** True today and checked by reading;
  `loading::the_browser_has_no_way_to_build_a_playable_puzzle_without_loading_one` records the
  claim but cannot prove a negative. Unlike `Opened`, `Playable` is not sealed by a private field,
  because its fields are what a page needs.
