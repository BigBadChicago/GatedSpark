# ADR 0017 — Accessibility is held by tests, not by good intentions

- **Status:** accepted
- **Date:** 2026-10-23

## Context

Two plain definitions first. A **screen reader** is software that reads a page aloud to somebody
who cannot see it. The **tab order** is the path the Tab key walks through a page; anything left
out of it cannot be reached without a pointing device.

`plan-at-small-team.md` puts accessibility on the short list of things that must not be cut. It was
cut anyway, and the way it was cut is the interesting part. Nobody decided to drop it. Labels and
keyboard handlers were written once, correctly, and then the page kept changing around them. By
2026-10-23 the position was:

- Every square was in the tab order and answered Enter and Space. That part had held.
- Every square was named `row 2, column 3` — and **nothing else**. Whether it was blank, filled or
  crossed out was carried entirely by how it was drawn. Somebody listening to the page could move
  around the board and never learn what was on it.
- `refresh()` rewrote how every square looked and left its name alone, so even a name that had been
  right would have gone stale after an undo.
- The solved message, the refusal, the running commentary and the saved-puzzle line all changed
  their text with nothing to announce that they had. To a screen reader a silent change is no
  change.

None of this failed anything. It never would have. Every screenshot looked right, every test
passed, and the page was unusable to a player who could not see it.

## Decision

**Accessibility gets tests that fail, in two layers, and the layers have different jobs.**

**`crates/gs-web/tests/usable_without_a_mouse.rs` reads `www/index.html` as text** and checks that
the pieces are still present: squares and lines in the tab order with key handlers, a word for
every state a square or line can be in, the name rebuilt inside `refresh` rather than only where a
tap landed, a live region on everything that speaks to the player, a label on both pickers, and a
stated row or column on the clue numbers. It needs no browser, so it runs on every push with the
rest of the suite.

**`scripts/drive-page.mjs` plays with the keyboard, in a real browser.** It focuses a square with
no mouse, presses Tab and requires the next square, presses Enter and requires both the mark and
the spoken name to change, then undoes and requires the name to follow. Then the same for a line in
the loop game.

**The division is deliberate.** The first layer cannot prove anything works; it proves the page
still says the right things, in seconds, everywhere. The second proves the behaviour and needs a
browser this project has decided not to depend on in CI (ADR on the page check). Neither is
sufficient. Together they cover the two ways this breaks: somebody deletes a line, or somebody
changes what the line does.

**Every state must have a word, and the words live in one table.** `SQUARE_SAID` and `LINE_SAID`
are the only place a state is turned into English. A fourth state that gained a look without
gaining a word would be visible to everybody who can see the board and invisible to everybody else;
the test compares the tap cycle against the word table so that cannot happen quietly.

## Consequences

- Deleting an accessibility line fails `cargo test --workspace` in seconds. That is the whole point:
  the previous arrangement was a comment and a hope.
- The static layer can pass while the behaviour is broken. This is stated here rather than papered
  over. It is why the browser layer exists, and why the browser layer plays rather than inspects.
- The seven static checks were each proved to fail: the invariant was broken on purpose, one at a
  time, and in every case exactly one test failed and the other six passed. A test that cannot fail
  is the vacuous pass this project is arranged against everywhere else, and an accessibility test
  nobody has ever seen fail is exactly the shape that problem takes.
- **What is still not covered.** Colour contrast is unmeasured. There is no check that the focus
  outline is visible. Nobody who actually uses a screen reader has tried this page, and no test
  substitutes for that. The names are plausible to somebody who can see the board, which is not the
  same as useful to somebody who cannot.
- A fault the browser check found while being written: it asserted a square's state against the
  number `1`, and the core's enum puts the positive value at `0`. The assertions now compare a
  state against what it was a moment earlier, and read the **word**, which is the thing actually
  being defended. A check that knows the core's numbers is a check that breaks when the core
  renumbers them, for no gain.
