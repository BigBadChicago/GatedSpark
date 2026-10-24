# ADR 0020 — A third game on the same page, and what ten states broke

- **Status:** accepted
- **Date:** 2026-10-24

## Context

ADR 0015 decided that one page would read two games rather than one page per game, and set out why:
with a crate per game there would be two answers to "is this package trustworthy", they would drift
apart, and the one in front of a player would be whichever page they happened to open.

That was a claim about an arrangement, made when the arrangement had been used once. Kakuro is the
test of it, and it was chosen to be the test. From `crates/gs-kakuro/src/lib.rs`, written before any
of this part was built:

> **A square holds a number the player chooses.** Nonogram's squares are filled or not.
> Slitherlink's edges are drawn or not. Every layer built so far — the board, the page, the hints,
> the accessibility rules — quietly assumes a square has two or three states. This family has ten.
> If all of that survives, the platform's claim to be general is worth something; if it does not,
> the place it breaks is the thing worth knowing.

This records what survived, what did not, and the two faults found by looking rather than by
testing.

## Decision

**The third game goes through the same door as the first two, and the places it could not were
changed rather than worked around.**

### What did not change at all

Nothing below the session. Named here because the list is the finding:

- `gs_core::state`, `Delta`, `Timeline`, `StateHash` — none of them ever enumerated how many values
  a cell can take, so none of them noticed. Undo, redo and the board's fingerprint work on a
  ten-state square exactly as they work on a three-state one.
- `gs_core::package`, `report`, `verification`, `schedule`, `withdrawal` — a payload is bytes and a
  fingerprint is a fingerprint.
- `gs-offline` — the buffer, the shelf and the "do not play these" list hold packages by
  fingerprint. `crates/gs-web/tests/every_game_in_one_buffer.rs` is the old two-game file with a
  third package added and two numbers changed, which is the whole of the proof.
- `gs-store`, `gs-crypto`, `gs-solver`.

`crates/gs-web/src/lib.rs` — the loader, the trust boundary, the thing ADR 0015 was actually about —
changed by **four lines**: a family in `KNOWN`, a variant in `Board`, a variant in `BoardProblem`,
and a branch handing the payload to this game's own reader.

### What had to change, and why each one is real rather than incidental

**A move carries a value.** `Session::write` takes a digit; `Session::clear` is its own method. A
shared `mark(place)` would have had to grow a value argument that two of the three games ignore.
This is the one that justified three session types rather than one.

**Zero is not a digit, and there is no number that means "empty".** The family refuses nought
outright — a nought beside a nine satisfies a total of nine and the no-repeats rule does not catch
it, which ADR 0018 records — so emptying a square is a move of its own rather than `write(0)`.
Backspace, Delete and a typed nought all reach `clear`, and that translation lives in
`Game::write_and_say` beside the rule it comes from. A page reads a keystroke, and a keystroke is
not a digit.

**`Place::Digit` is a separate variant from `Place::Square`**, although both carry a column and a
row. They do not mean the same thing: a Nonogram's pair indexes the only squares there are, a
Kakuro's indexes the whole grid including the black squares. A page that treated them alike would
draw the right highlight on the wrong square.

**The page gets a second table, not the first one with different classes on it.** A black square is
part of the puzzle rather than a state a player reached, so there is nothing for a tap to do to it;
a white square holds text rather than a fill. One table doing both would have meant a square whose
meaning depended on which game was loaded.

**The board's shape has to be asked for, and asked for once.** `kakuro_shape()` is called when the
board is drawn and never again, because the black squares cannot move. `digits()` is called after
every move. Two calls rather than one, split along what can change.

### Two faults found by looking, which no test would have caught

ADR 0017 and `gatedspark-svg-is-not-hidden` both say the same thing: a page can be wrong in ways
every passing test agrees with. Both faults here were found in a screenshot.

1. **A diagonal on every black square.** In a printed Kakuro a diagonal means "runs start here" and
   separates the two totals. The first version drew one on every black square, including the plain
   blockers that carry no numbers — so the board said a run started at a dozen squares whose total
   was missing. Correct to the core, wrong to anybody who knows the game. Blockers are now drawn
   solid, with no diagonal.

2. **The driver was reading the wrong game.** `scripts/drive-page.mjs` opened whatever the page
   offered by default and then checked a picture board. That worked while the default was a
   Nonogram. The game picker is in alphabetical order, so adding `kakuro` made it first, and the
   whole picture-game section began reading a Kakuro's board and reporting "no grid was drawn". The
   driver now asks for the game each section is about, which it should always have done.

### Accessibility is three layers now, and the third is different

A Nonogram square and a Slitherlink line are **switched**: Enter or Space, and a table of one word
per state. A Kakuro square takes a **value**, so:

- It is announced as a `textbox`, not a `button`. Telling somebody using a screen reader to press
  Enter, when what they need to do is type a digit, is worse than saying nothing.
- There is no word table, because ten states do not want one. The name is built from the digit
  itself, and what the test defends is that an empty square says "empty", a black square says what
  totals it carries or that it is blocked, and the digit appears in the name.
- **The arrow keys are part of the claim, not a convenience.** This is the first board with holes
  in it. Tab alone cannot cross a row that a black square sits in the middle of, so the arrows move
  between white squares and skip the black ones rather than stopping on them.

Each of the three new static checks was proved to fail, one at a time, exactly as ADR 0017 requires:
removing the arrow keys, removing the name rebuild in `refreshKakuro`, and removing the word
"empty". In every case exactly one test failed and the other seven passed.

## Consequences

- ADR 0015's claim held, and this is the first evidence for it rather than the second assertion of
  it. Four lines in the loader, nothing below the session, and an offline story that needed a third
  package and two numbers.
- **One page now carries three games' rules into every browser.** `crates/gs-web/tests/boundary.rs`
  names what is compiled in, and `gs-kakuro` is now on that list. Worth noting what it did not drag
  in: Kakuro's crate names `gs-solver` in its `[dev-dependencies]`, and the scanner's distinction
  between a real dependency and a test-only one is what keeps the boundary holdable.
- **The wire format is stricter than either other game's, and still cannot catch everything.** A
  Kakuro's blocks and totals are not independent lists: reading the blocks produces the runs, and
  the totals only mean anything in that order. `from_canonical_bytes` refuses a file whose totals
  cannot belong to its blocks. It cannot refuse one where they can — blocking a corner sometimes
  leaves the run count unchanged, and the old totals then fit the new runs, giving a legal puzzle
  nobody made. What catches that is the signature over the bytes, which is why the payload is a
  byte string with one spelling rather than a structure a reader reconstructs.
  `the_wire_format.rs::changed_one_block_can_give_a_different_legal_puzzle_and_the_format_cannot_tell`
  states this as a test rather than leaving it as an assumption.
- **A Kakuro has no half-finished finish.** Nonogram and Slitherlink both needed a second, stricter
  check that a *stored* answer decides every cell, because their `is_solved` deliberately permits an
  unfinished board. A Kakuro with an empty square has a run that does not meet its total, so the two
  questions coincide and `publish::replay_of` says so rather than adding a check that could only
  ever pass.
- **No difficulty levels, for the third family running**, and this one has a particular reason to
  wait: the number most likely to move is how much of the board it gives away (ADR 0019), and a
  level set from today's boards would describe boards we intend to stop making.
- **What is still not covered.** Nobody who uses a screen reader has tried this page. Colour
  contrast is unmeasured, and the black squares are now a second shade that nothing checks. The page
  check is still a by-hand script rather than a CI job, for the reason ADR 0017 gives — and both
  faults above were found by it, which is an argument for running it rather than for automating it
  badly.
