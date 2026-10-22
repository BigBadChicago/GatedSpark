# ADR 0008 — The store is named by content, and the command is the only way in

- **Status:** accepted
- **Date:** 2026-10-22

## Context

ADR 0006 made a package carry its own fingerprint and a signature over it. ADR 0007 made a real
solver answerable. Neither of those puts a puzzle anywhere. M5's exit test is a sentence: one
command takes an empty folder to a signed, verified, playable puzzle, and a second command can
check it later.

That needs two new things: somewhere to keep a puzzle, and something a person can run. Both were
decided together, because the questions turn out to be the same question. A store that can be
written to by anything is a store whose contents mean nothing, and a command that can store
something the solver did not vouch for is a command that undoes M4 and M5 at once.

## Decision

### A puzzle is kept in a directory named by its own fingerprint

```text
<store>/
  9890887818e3d290e6a58b4b06b159b6452dd90473a34b5cc4a0e2fddb974813/
    package.gspk      the signed package: the puzzle, its report, its signature
    witness.bin       the answer, in the engine's canonical form
```

The name is the 64-character hexadecimal content fingerprint from ADR 0006. Nothing else names a
puzzle: no counter, no generated identifier, no file of metadata beside it.

That gets three properties without any code:

- **The same puzzle cannot be stored twice under two names.** Two seeds that happen to produce the
  same clues produce the same bytes, the same fingerprint, and the same directory. `put` reports
  `AlreadyStored` and does nothing, which is why `fill` treats it as ordinary rather than as a
  failure.
- **A name can be checked against the thing it names.** `get` reads the directory, opens the
  package, and compares the fingerprint it computed to the directory's name. A disagreement is
  `DirectoryNameDisagrees`, which names both. A store where a directory was renamed, or its
  contents swapped for another puzzle's, is caught by reading it.
- **Nothing outside the signature decides anything.** Every fact about the puzzle — its family, its
  rules version, its seed, its level, the queries the solver answered, the measurements — is read
  out of the signed bytes.

### The store keeps no second copy of the report

The obvious convenience is a `report.json` beside the package, so a person can read a puzzle's
report without a tool. It was rejected. Two copies of one set of facts is how the two come to
disagree, and the honest answer to "which one is right" would always have been the signed one. The
tool for reading the report is `gatedspark show`, which reads it out of the package.

### `put` is not told the fingerprint, it learns it

`Store::put` takes the package bytes, the answer, a digest and a trusted-key list. It does **not**
take a content fingerprint. It opens the package, which verifies the fingerprint against the bytes
and the signature against the key list, and the fingerprint it learns that way is where the
directory name comes from.

A `put(id, bytes, ...)` would have let a caller file one puzzle's bytes under another puzzle's name,
and the store would have had no way to know. The same reasoning is already in `gs-core`:
`ContentId` can only be obtained by digesting bytes, and nothing in the workspace can build one
from a name, a seed or a counter.

### The answer is bound to the signature through the report

`witness.bin` is not signed. It does not need to be. A published report records
`Replay::Reproduced(StateHash)` — the fingerprint of the answer the real game accepted — and the
report is inside the signed bytes. So the store hashes the stored answer and compares:

```text
stored answer hashes to ff7f8ddbbcad7f50, signed report says fcaabdcb4d4ee585
```

One changed byte in the answer file is caught, and the failure names both numbers. This works only
because `StateHash::of(state)` and `StateHash::of_bytes(state.canonical_bytes())` agree, which has
its own test. A package whose replay is anything other than `Reproduced` is refused on the way in
with `NoReproducedWitness`: an answer nobody replayed is not an answer worth keeping.

### Writes are staged and renamed

`put` writes into `.writing-<fingerprint>` and renames it into place. A process killed halfway
through leaves a directory whose name starts with a dot, which `list` does not return and `recheck`
does not count. A half-written puzzle is never a puzzle.

### The command refuses to pretend

`gatedspark` has four subcommands and three refusals that matter more than the subcommands:

- **`fill` checks the solver before it starts.** Without that check, a machine with no solver runs
  for a long time and reports every puzzle refused for an undecided query. That is *correct*, and
  it says nothing about why. So a missing solver is reported as a missing solver, before any work:
  "Nothing can be published without a solver, and nothing here will pretend otherwise."
- **`fill` says when it came up short.** A run asked for 20 puzzles at a level and able to make 6
  prints the shortfall. A worker that quietly delivered fewer than asked would be discovered as an
  empty level in front of a player.
- **`recheck` prints how many it checked, first and always.** A store with nothing in it has no
  failures. A run that printed only "every signature checks out" would read as a clean bill of
  health for an empty directory, which is exactly the shape of a check that passes while checking
  nothing. An empty store is told it is empty, in those words.

### `show` and `recheck` take a key, and the store does not supply one

Both need the *public* half of a key to check signatures, and both get it from the private key file
with `--key`, because that is what exists in a development setup.

The tempting alternative is for the store to keep a list of the keys it trusts, so the commands
need no flag. That is worse than it looks: a reader that trusts whatever key list it finds next to
the content is a reader that trusts whatever anybody put there, so the list would be decoration. A
trusted-key list has to come from outside the content it vouches for. In a real deployment that
means the device ships with it. Here it means a flag.

### There is a development key generator, and it refuses more than it accepts

The user asked for one, with guards. It has four:

- Its first line is `# GATEDSPARK DEVELOPMENT SIGNING KEY - NOT FOR PUBLISHING ANYTHING REAL`, so
  a key found later says what it is without anybody having to remember.
- **It refuses to write inside a Git working tree.** It walks up from the output path looking for
  `.git`, and stops if it finds one. A convenient key generator is the usual way a private key ends
  up committed, and this is the only moment anybody is paying attention. `.gitignore` carries
  `*.key` as a second line of defence for a key written by hand.
- **It never overwrites.** The file is created with `create_new`, so a race cannot win either.
  Replacing a key invalidates every signature made with the old one, and those packages sit in a
  store looking fine until something tries to read them.
- On Unix the file is created mode `0600`, and a failure to get randomness from the operating
  system writes nothing at all: a key from a broken source of randomness is worse than no key.

Where a *production* signing key comes from is deliberately still undecided. It is a question about
custody, not about code, and this command is not the answer to it.

### No argument-parsing library

ADR 0004's rule is no dependency without a recorded reason, and four subcommands with eight flags
between them are not a reason. The parsing is deliberately dull, with one property worth the hand
work: **an unknown flag is an error, and it lists the flags that do exist.** A silently ignored
`--cuont 50` would generate the default number of puzzles and be discovered days later as a store
with the wrong contents. A flag with a missing value is an error too, including the disguised case
where a flag swallows the next flag (`--store --key …`), because `--store` with a forgotten path
must not become a store in the current directory.

## Consequences

- A store is a plain directory of plain files. It copies, syncs, serves over static hosting, and
  diffs. Nothing in it needs a database or an index to be read correctly.
- `list` is a directory listing, which will not scale to a large pool. That is fine for now and is
  a known replacement: the pool scheduler in `content-delivery.md` section 3 needs an index anyway,
  and it will be built next to the store rather than inside it.
- There is no `remove`. Nothing in M5 needs one, and the question of revoking already-delivered
  content is still open in the design notes.
- `recheck` reads and verifies every package. On a large store that is slow, and it is meant to be:
  it is the operation that answers "is this store still sound", and a fast answer to that question
  would be a worse answer.

## Enforcement

| Claim | Test |
| --- | --- |
| `put` is not given a fingerprint; it learns it from the bytes | `gs-store` has no API taking one; `the_directory_is_named_by_the_content_id_and_holds_the_two_files` |
| The same puzzle twice is `AlreadyStored`, not two directories | `storing_the_same_puzzle_twice_is_refused_rather_than_overwritten` |
| A renamed or swapped directory is caught on read | `a_renamed_directory_is_a_refusal_rather_than_a_different_puzzle` |
| A package the key list does not vouch for is refused on the way in | `a_package_signed_by_an_untrusted_key_is_refused` |
| An answer that does not match the signed report is refused on the way in | `an_answer_that_does_not_match_the_signed_report_is_refused` |
| A package with no replayed answer is refused | `a_package_with_no_replayed_answer_is_refused` |
| One flipped byte in a stored answer fails `recheck`, naming both hashes | `an_empty_folder_becomes_signed_verified_puzzles_and_then_rechecks` |
| An answer swapped on disk after the fact is caught on the way out | `an_answer_swapped_on_disk_after_the_fact_is_caught_on_the_way_out` |
| A truncated package is caught rather than served | `a_truncated_package_on_disk_is_caught_rather_than_served` |
| A name that is not a fingerprint is refused without touching the disk | `a_name_that_is_not_a_content_id_is_refused_without_touching_the_disk` |
| A half-written puzzle is not listed or counted | `a_half_written_puzzle_is_not_a_puzzle` |
| `recheck` on an empty store says it verified nothing, not that all is well | `recheck_on_an_empty_store_says_it_verified_nothing` |
| A store signed by another key is rejected by `recheck` | `a_store_signed_by_another_key_is_not_accepted` |
| `fill` reports a missing solver as a missing solver, and stores nothing | `a_missing_solver_is_reported_as_a_missing_solver` |
| An unknown flag is an error that lists the known flags | `a_misspelled_option_is_an_error_rather_than_being_ignored` |
| A flag with no value is an error, including when it swallows a flag | `an_option_with_no_value_is_an_error_rather_than_a_guess` |
| A zero time limit is refused rather than timing out every query | `a_time_limit_of_zero_is_refused` |
| `keygen` refuses inside a Git tree, and writes nothing | `keygen_refuses_to_write_inside_a_git_repository_and_writes_nothing` |
| `keygen` never overwrites an existing key | `keygen_never_overwrites_an_existing_key` |
| Two generated keys are different keys | `two_keys_are_different_keys` |
| A key file is not readable by the group or the world | `a_key_file_is_not_readable_by_anybody_else` |
| The repository ignores `*.key` | `the_repository_ignores_key_files` |
| An empty folder becomes a signed, Z3-verified, replayable puzzle | `an_empty_folder_becomes_signed_verified_puzzles_and_then_rechecks` |

The last row is the one that matters, and it is the only test in the repository that runs the whole
thing. It needs a real solver, and when there is none it prints what went unverified rather than
passing quietly.
