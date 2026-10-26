# ADR 0022 — One menu, pictures for the actions, and a way to check your work

- **Status:** accepted
- **Date:** 2026-10-26
- **Follows:** ADR 0017, which put the page's accessibility behind tests, and ADR 0020,
  which put a third game on the same page.

## Context

The page grew by addition. Every piece of work since the first slice added a row to it:
a game picker and a day picker, five buttons for saving and offline and withdrawals, a
fold holding the rules, a panel saying what the page had checked, a status line, a hint
line. All of it was on the main screen, above or below the board, in the order it was
written.

On a phone the result was a board in the middle of a column of housekeeping. The project
owner looked at four design options and asked for five changes:

1. Put the rules, the saving and the checks behind one menu.
2. Use pictures for Hint, Undo and Redo instead of words.
3. Move the three games to a bar at the bottom, in reach of a thumb.
4. Take away the boxes and cards behind the boards.
5. Use the accent colour far more sparingly.

They also asked for something new: **a way for a player to check how their answers are
doing.**

## Decision

### The look

All five changes, as asked, with one deliberate departure. The picture buttons keep a
small word under each picture. A lightbulb on its own is a guess for anybody meeting the
game for the first time, and plain language is this project's standing rule. The words
are small and grey; the accessible name is on the button either way, so a screen reader
is unaffected by the choice.

The accent colour now has exactly two jobs: **the game you are on**, and **the square you
are on**. Not a filled Hint button, not a box round a line of text. Where a check finds
something wrong the warn colour is used instead, because a square cannot mean "you are
here" and "this is wrong" at once.

The games bar carries each game's name in words beside its picture, and says which game
is showing with `aria-current`, so that which one is selected is not carried by colour
alone. The menu is a real dialog with a name, closes on Escape, and moves the keyboard
into itself when it opens. Four new tests in
`crates/gs-web/tests/usable_without_a_mouse.rs` hold all of this.

### Checking your work

A new, free, unlimited action: **Check**. It reports two things — everything already
wrong, and how far along the board is — and it changes nothing.

Each game's crate gained a `check` module next to its `hint` module:

| Game | What it faults |
| --- | --- |
| Nonogram | A row or column whose filled squares cannot match its numbers |
| Slitherlink | A square with more sides drawn than its number allows; a corner where three or four lines meet; more than one loop |
| Kakuro | A run with a digit twice, a run already past its total, a full run that does not add up |

**Nothing in a check is worked out.** No solver runs and no answer is read, because none
is published. Every fault above is counting: the player's own marks against the clues
printed on the board. That is the whole reason it is free while hints are three to a
puzzle. A hint tells a player something they did not know; a check tells them only what
is already in front of them.

### What a check deliberately does not look at

**Crossing out is never checked, in any game.** A crossed-out Nonogram square and a
ruled-out Slitherlink line are read as "not filled" and "not drawn". The owner settled
that on 2026-10-23: a puzzle is finished when the *filled* squares match, and crossing
out is a note the player keeps for themselves. A check that held a player to their own
notes would turn the thing that "nothing checks" into the thing that fails them.

This has a cost worth stating. A Slitherlink number can never be reported as *impossible
to meet*, because ruling its other sides out is not a commitment. Only "too many sides
drawn" is a fault, and that one cannot be taken back by changing a note.

The line is also drawn at counting, not reasoning. A Kakuro run that cannot *reach* its
total with the digits it has left is a real mistake and is not reported, because finding
it means working out which digits are still available — which is the hint's job, and the
hint's budget.

### One place for the numbering

`Hinted::places` and `Checked::places` are flattened by one shared function in
`gs_web::browser`. Two copies of that numbering would agree until the day one of them
changed, and the page would then mark the wrong square for one of the two.

## Consequences

- The main screen is the board, four picture buttons, two short lines of text, and the
  games bar. Everything else is one tap away.
- A check is free, so a player can lean on it as hard as they like. Nothing is given
  away by that, which is exactly why it can be free.
- The page's own driver, `scripts/drive-page.mjs`, now opens the menu before using the
  day picker, drives the Check button on a right board and a wrong one, and proves that
  crossing a square out is not reported as a mistake.
- Two faults were found by looking at screenshots and by nothing else: on a phone the
  10 by 10 board ran off the edge and cut the row clues off, and the keyboard's place on
  a Slitherlink line was drawn as a box two squares wide sitting on top of the numbers,
  because an outline round an SVG group includes the wide invisible strip a finger aims
  at. That is the fifth and sixth time looking at a picture has caught something here
  that every automated check passed. The case for putting a browser in CI is now made six
  times over, and is still not made here.
