# 0010. The page a player taps on

Date: 2026-10-23

Status: accepted

## Context

ADR 0009 put the core in a browser and proved, in a real browser, that a package can be
checked and a puzzle played. What it did not do is draw anything. Nobody had yet seen a
board.

A page is where the discipline of the rest of this project is easiest to lose. It is the
one layer where the pressure to "just work it out here, for the display" is constant,
and where a second opinion about the state of the board would be invisible until the day
it disagreed with the first one.

## Decision

### The page decides nothing

`crates/gs-web/www/index.html` draws a grid, sends taps to the core, and copies the
core's answers onto the screen. In particular it never works out whether a board is
finished. It calls `game.is_solved()` on every change, which asks `Session`, which asks
`Nonogram::is_solved`, which checks the clues.

The specific mistake this forbids is a page that counts filled squares itself "because
the core call is slow". A board state is a few hundred bytes; the call is not slow.

The page holds no answer and is given none. The answer exists in `witness.bin`, which
the store keeps and never publishes.

### The whole board in one call

`Game::board()` returns every square in one `Vec<u8>`. A 20 by 20 board redrawn by
asking for each square separately is 400 calls across the boundary between JavaScript
and the core, per redraw. The page calls `board()` once and reads the array.

`Square` is the only place the numbers the page reads are defined: `Filled = 0`,
`Empty = 1`, `Unknown = 2`.

### Refusals are sentences

A refusal shown to a player is written for a player. Before this change the message for a
rejected signature was:

    this is not a puzzle this page will accept: SignatureRejected

`gs-web::plainly` now turns each reason a package can be refused into a sentence. The
match is exhaustive, so a new reason added to `OpenError` will not compile until somebody
has written words for it.

The test that was supposed to catch the original fault checked only that the string did
not contain `NotPlayable`, and so it passed on `SignatureRejected`. It has been replaced
by `reads_as_words`, which refuses any string containing a lowercase letter immediately
followed by a capital — which is what a type name looks like and what English does not.
The project's own name is the one exception and is named in the code.

### Every square must be decided

`Nonogram::is_solved` requires every square to be marked filled or crossed out, not just
the filled ones. A board whose filled squares are all correct but whose blanks are
untouched is not solved.

This is the existing rule, not a new one, and it is recorded here because it is the first
time anybody has met it: it took 372 taps to finish a 15 by 15 puzzle with 93 filled
squares, because the other 132 had to be crossed out one at a time. **It is a question
for the owner of this project, not a decision taken here.** Most nonogram implementations
treat a board as finished when the filled squares match, and leave crossing out as an
optional aid to the player. See `/mnt/project-files/decisions/decisions-waiting-on-you.md`.

### The page checks the signature with its own key

The trusted key comes from `trusted-key.js`, which belongs to the page and is written by
`scripts/demo-web.sh`. It is not fetched alongside the puzzle. A page that trusted
whatever key arrived with the content would be checking that the content agrees with
itself.

### Where the puzzles come from

A puzzle is fetched as a plain file at `store/<fingerprint>/package.gspk`. A puzzle's
name is its own fingerprint, which is why the page needs no index to find one, and why
`?puzzle=<name>` is a working link to one puzzle. This is the plain-files option from the
delivery decision, which is still open; nothing here forecloses a service later, because
the page's only assumption is that bytes arrive.

## Enforcement

| Claim | What holds it |
| --- | --- |
| A grid is drawn, with a clue for every row and column | `scripts/drive-page.mjs`, `details` |
| Tapping fills, taps again to cross out, again to clear | `drive-page.mjs`, `afterTaps` |
| An untapped square is left undecided | `drive-page.mjs`, `afterTaps.untouched` |
| Undo puts the square back; redo restores the mark | `drive-page.mjs`, the undo and redo checks |
| The real answer solves the puzzle, in the page | `drive-page.mjs`, `end` |
| One square changed is no longer solved | `drive-page.mjs`, `oneShort` — the guard that makes the line above mean something |
| Switching puzzles rebuilds the grid and forgets the old game | `drive-page.mjs`, `switched` |
| A changed package is refused, in readable words | `drive-page.mjs`, `refusal` |
| The browser reports no errors while all of that happens | `drive-page.mjs`, `problems` |
| Every reason a package can be refused is a sentence | `gs-web/tests/loading.rs::every_single_reason_a_package_can_be_refused_is_written_in_words` |
| A refusal never shows a type name | `loading.rs::reads_as_words`, applied in both refusal tests |
| The page is never compiled against the solver or the store | ADR 0009's boundary test, unchanged |

## Not enforced

* **`scripts/check-page.sh` is not in CI.** It needs Playwright's Chromium, which is not
  a declared dependency of this project. Adding a browser-driving dependency is worth
  deciding on purpose rather than in passing, and the wasm test in CI already proves the
  core works in a browser. What is not proven on every change is that the *page* works.
* **Nothing checks how the page looks.** The pictures `check-page.sh` takes are for a
  person to look at. No test compares them to anything.
* **The page is not accessible to a screen reader beyond the basics.** Each square is a
  focusable control with a row and column label, and that is all. Nothing tests it.
* **The demo content is generated, not committed.** `scripts/demo-web.sh` makes it from a
  development key. There is no fixed sample puzzle in the repository, so a change that
  broke loading would be caught by the tests but not by a stale file.
