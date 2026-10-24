# ADR 0021 — Building a Kakuro's board instead of drawing one at random

- **Status:** accepted
- **Date:** 2026-10-24
- **Follows:** ADR 0019, which left this open and named it as the next piece of work.

## Context

ADR 0019 built Kakuro's generator and recorded, in plain sight, the thing wrong with it:
about **38 of every 100 white squares were given away.** Those squares sat alone in a run
of one square, where the run's own total names the digit outright, so the player was
handed over a third of the board before starting.

It also recorded why that was not a tuning problem. The method was: draw a random block
pattern, fill it in, read the totals off the filling, and then repair the ambiguity by
blacking out a square where a second answer differed. A random pattern has runs of every
length including one, so the repair starts from a board that is already partly given away,
and every square it blacks out can make another run of one. Choosing the square greedily
took the number from 59 in 100 down to 38, and 38 was where it stopped.

ADR 0019 said what to do about it — build the board the way a published Kakuro is built —
and that a first attempt had been written and thrown away the same day, because fixing
rows kept breaking columns and the other way round.

## Decision

**Build the board, then settle its uniqueness with the digits rather than with more
blocks.** Three pieces, and none of them works without the other two.

### 1. The board is built so that it cannot be unfair

The rule, stated over a board: **every white square's across run and down run are both at
least two squares long.** No run of one, so nothing is given away, by construction rather
than by a cap on a measurement.

The discarded attempt tried to reach that by fixing a board that broke it, and did not
converge: lengthening a row's run of one means clearing a block, which lengthens a
column's run past its limit, and blacking a square to fix the column makes a new run of one
in some row. The two directions pull against each other.

`crate::construct` never builds a board that breaks the rule in the first place. Squares
are decided in reading order, black or white, and both directions are checked **at the
moment of choosing**:

- A block closes the run of whites to its left and the run above it. Neither, if it is not
  empty, may be too short.
- A white square lengthens both of those runs, so neither may go past the longest allowed;
  and if the square is last in its row or its column, the run it closes must be long
  enough.

Both rules are about runs the choice **closes**, which is why reading order works at all:
a square's across run reaches left and its down run reaches up, so both lie entirely in the
part of the board already decided. When a square has no legal choice left, the search takes
back the previous choice and tries the other one. That is the difference from the discarded
attempt: going back resolves a conflict, instead of another local fix creating the next
one.

It finds a board at every size tried, on every seed, well inside its choice limit.

### 2. Why that is not enough, which is the finding worth keeping

A board with no run of one square is **more** prone to having many answers, not less.

Take four white squares at the corners of a rectangle, two sharing an across run and two
sharing a down run. Add `k` to two opposite corners and take `k` from the other two. Every
run through them keeps its total, because each one gained exactly what it lost. If the four
digits stay between one and nine and stay distinct within their runs, the board has a
second answer and no total can tell the two apart. Call it a **swap rectangle**.

Two measurements followed, and both are in the code as tests rather than as notes:

- **Refilling a fair board does not help.** Ten built boards each at 5 by 5, 7 by 7 and 9
  by 9, twenty different fillings of each: **one of six hundred** gave a puzzle with a
  single answer.
- **More adding-up rules do not help either.** A board of nothing but runs of two has
  exactly one rule per unknown, and is still ambiguous. The rules are not independent: a
  board's across totals and its down totals both add up to the same grand total, so a board
  always has at least one fewer real rule than it appears to have.

And a third, from trying the obvious fix: **forbidding swap rectangles outright leaves no
board at all.** Not at 4 by 4, not at any size tried. The search is exhaustive, so that is
a fact about Kakuro rather than a limit of the search; the short argument is that every
white square needs a neighbour across and a neighbour down, and those two drag the fourth
corner in behind them.

Which is why real Kakuro grids are full of swap rectangles. **What stops the swap in a
published puzzle is never the shape. It is a digit that cannot move.**

So the rectangles are a preference in the builder and not a rule: among the boards the run
rules allow, it tries the ones with fewer rectangles first. That is also what makes a
printed Kakuro look like offset brickwork — a board whose runs line up row under row is
thick with rectangles, and one whose runs are offset has far fewer. The pattern is not
decoration.

### 3. The digits do the rest, in two levers

**Pin the rectangles.** For each swap rectangle still live, set one **diagonal** pair of
corners to a one and a nine. With `a` top-left and `d` bottom-right, `a = 9` kills the
direction that would raise `a`, and `d = 1` kills the one that would lower `d`, so both
directions die at once. It has to be a diagonal pair: the two corners in a row share an
across run and the two in a column share a down run, and a run holds no digit twice, so no
run-sharing pair could both be a nine.

This is ordinary Kakuro craft rather than something the measurements turned up, and it is
what took the method from unusable to usable: at 7 by 7 it went from keeping **none** of 40
seeds to keeping 20, and from 10.9 seconds to 2.2 for the batch.

**Then nudge, if the solver still finds a second answer.** Change one digit in a square the
two answers disagree about. The totals are read off our filling, so moving a digit moves
the totals of its two runs, and the second answer — whose own sums matched the *old* totals
— stops being an answer at once.

The nudge may only move a digit **outwards**, farther from five. The first version allowed
any change and cycled: a square moved out one round and back the next, and the board still
had many answers when the rounds ran out (0 of 20 kept at 5 by 5). Outwards gives the
repair something that cannot go backwards — add up how far every digit sits from five and
each change makes that total strictly larger, bounded by four per white square. So the
repair either finishes or runs out of moves, and never goes round in a circle. That bound is
where `rounds_for` comes from, which is why the round allowance is derived rather than
guessed.

**Blacking out a square is the last lever, and it is nearly always unavailable.** On a fair
board, blacking an end square trims its run to `len - 1`, legal only when the run was three
or longer; blacking an inside square splits it, legal only when the run was five or longer;
and *both* of the square's runs must come out fair. On a board of runs of two and three
there is often no such square anywhere. Measured before the digit levers existed: 9 of 40
kept at 4 by 4, none at all at 7 by 7.

**And when all of that runs out, the board gets shorter runs.** Each run is one adding-up
rule, so a board cut into shorter pieces has strictly more rules over its squares. The
escalation rebuilds at a longest run one square shorter and tries again, down to two. This
is the same thing a human constructor does when a grid will not pin down: make it blacker.

## Consequences

- **Nothing is given away any more.** Zero in 1000, against 360 to 395 in 1000 for the
  older method, at every size. The cap in `Shape::max_given_away_per_mille` is now zero for
  built boards: not a tightened guess, but the cheapest possible check that the
  construction and the repair both still do what they claim.
- **It is cheaper, not dearer**, which was not expected. At 7 by 7 the older method takes
  about 10.9 seconds for 40 seeds and the built one about 2.2, because pinning the
  rectangles settles with arithmetic what the solver was otherwise asked dozens of times.
- **Fewer seeds are kept**, and that is the trade: 34, 23, 27 and 20 of 40 at 4 by 4
  through 7 by 7, against 31, 30, 35 and 39. Generation cost is independent of how many
  people play (`content-delivery.md`, section 3), so a lower keep rate at a lower cost per
  seed is a good exchange.
- **It is the better method up to 7 by 7 and the worse one above it.** Measured at 8 by 8:
  3 of 8 seeds kept, 3.9 seconds each. At 9 by 9: **2 of 8, and 31.5 seconds each**,
  against the random method's 34 of 40 at 6.5 seconds each. A fair board is blacker, so a
  big fair board has a great many swap rectangles to pin at once, and the escalation ends
  up rebuilding the board several times over.

  So the older method stays reachable, by name, as `--boards random`. It is **asked for
  and never fallen back to**: a generator that quietly switched to handing a third of the
  board away, because the board it was asked for was large, would be the worst of both.
  7 by 7 is the command's default size and the size the demos make, so the default method
  is the built one.
- **The boards are smaller.** 21 white squares at 7 by 7 against 27, because fair boards
  are blacker. A published Kakuro is blacker than people expect, for exactly this reason.
- **The new number to bring down is `digits_at_the_ends_per_mille`**: about **480 in 1000**
  of a built puzzle's digits are ones or nines, against about 200 for the older method. That
  is the price of pinning the rectangles shut, and a board of nothing but ones and nines
  would be unique and dull. It sits in the feature vector and in the published measurements
  beside `given_away_squares`, for the reason that number was there: a measurement nobody
  can see does not get improved. **This is the honest successor to the 38-in-100 problem,
  not its disappearance.**
- **`swap_rectangles` is published too**, because it is what this family's uniqueness
  actually turns on, and a reader grading difficulty should be able to see it.
- **`GENERATOR_VERSION` is 2.** A seed means a different puzzle than it did, and the
  measurements are over a different kind of board, so reports from the two versions are not
  comparable. That is what the version is for.
- **Still no difficulty levels**, for the third family running, and the reason has changed
  rather than gone away: the number most likely to move is now the ones-and-nines share, and
  a level set from today's boards would describe boards we still intend to change.
- **Both methods are kept**, and the random one is not dead code: `built_boards.rs` and
  `generation.rs` measure them against each other, so the claim that building the board is
  worth its code cannot quietly stop being true.
- **The command changed.** `--family kakuro` builds boards by default, `--longest-run`
  replaces guessing at density as the lever that matters, `--black` now means the built
  board's density, and `--given` defaults to zero. Every stored puzzle prints its
  ones-and-nines share as well as its giveaway share, because a run that printed only the
  cheerful numbers would be a run nobody fixed anything from.
- **What is still not known.** Whether a built board *plays* better. It is fairer by the one
  measurement that was wrong before, and it is blacker, smaller and fuller of ones and nines
  than the boards it replaces. Nobody has played one. Difficulty levels wait on that, and so
  should any further tuning of the run limits.
