# 0018. A square that holds a number

Date: 2026-10-24

Status: accepted

## Context

Kakuro is family three, and the reason it was chosen over cheaper options is that it
is the first family where a square holds a value the player picks out of ten
possibilities rather than two or three. Nonogram's squares are filled, empty or
undecided. Slitherlink's edges are drawn, ruled out or undecided. Every layer built so
far — the board types, the page, the hints, the accessibility rules — was written
against that shape, and nothing has ever tested whether the assumption is load-bearing.

Part one is the board and the rules. Three choices in it are not obvious, interact
with each other, and would be re-derived wrongly from the code later, so they are
recorded here.

## Decision 1: the logical state is white-shaped, not square-shaped

A Kakuro grid has block squares and white squares. Only white squares can hold a
digit. The state could be one entry per grid square, with blocks pinned to some
sentinel value, or one entry per white square with no representation of a block at
all.

**One entry per white square.** The same reasoning as Slitherlink's edge-shaped state:
a square-shaped state would have somewhere to put a digit inside a block, and making
that unrepresentable is cheaper and more reliable than checking for it at every call
site. It also keeps a board's canonical bytes to the squares that carry information,
which matters because those bytes are what gets hashed, stored and replayed.

The cost is a second index space and the lookups between it and the grid, which is why
`layout` exists as its own module with no rules in it. That cost is paid once.

## Decision 2: zero is not a digit, and the check is explicit

A white square holds one to nine, or nothing. Zero is refused by
`rules::is_digit` and has its own test.

This is worth a decision rather than a line of code because zero is a perfectly
ordinary `i16`, and both existing families use it as a meaningful value —
Nonogram's empty mark and Slitherlink's ruled-out edge are both zero. So nothing in
the platform's types stops a zero reaching a Kakuro square, and a zero there is not
merely odd: it would let a run of two satisfy a total of nine with `9 + 0`, passing
the adding-up rule while breaking the rule a player understands the game by. The
no-repeats rule would not catch it either, since one zero does not repeat.

`Cell::UNKNOWN` stays legal, because rubbing a digit out is a real move.

## Decision 3: adding up and no-repeats are two constraints, not one

Slitherlink's loop rule is deliberately welded into a single
`Global::SingleClosedLoop` rather than split into "nought or two lines per corner"
plus "all in one piece", because a compiler that encoded only the first half would
accept two separate rings and be *wrong* while looking right.

Kakuro's two rules per run are emitted separately, and the difference is worth
stating so the two cases do not look inconsistent.

Splitting the loop rule produces a partial encoding that is wrong about the board.
Splitting these does not: a compiler that encodes `Sum` and not `Distinct` is not
mistaken about `Distinct`, it simply does not support this family, and the DSL's tier
check already refuses a constraint nothing can encode. Against that, welding them
would hide which of the two a solver disagreed about — and with two solvers
cross-checking every result, knowing which rule they split on is most of the value of
running the second one.

Both are Tier A, so Kakuro is the first family that is local and still needs
arithmetic. `Constraint::Sum` and `Constraint::Distinct` have been in the rule
language since the slice's first day with nothing producing one; this crate is the
first thing to emit either.

## Decision 4: a run longer than nine squares is refused by the layout

A run holds distinct digits from one to nine, so a tenth square cannot be filled by
any assignment at all. `Layout::new` refuses such a layout, and `Totals::new` likewise
refuses a total outside what its run's length could reach.

Both are arithmetic facts, not search results. Refusing them here means the solver is
never asked a question whose answer was settled by counting, and a refusal that names
the floor or the ceiling is more use to whoever reads it than an `unsat` with no
reason attached.

The limit is on run length and not on board size, which is a distinction the layout
tests hold: a twelve-column board is legal when blocks keep every run to nine or
fewer.

## Consequences

* Every white square must be filled for a board to be finished. This looks stricter
  than the 2026-10-23 decision that a Nonogram or Slitherlink player need not mark the
  squares they are ruling out, and it is not a change to that decision. There, an
  unmarked square is a note the player chose not to keep. Here, an empty square is a
  square with no answer in it, and no run containing one can be said to add up.
* A clash is visible before the board is full, because the no-repeats check runs over
  the digits written so far. That is this game's moment-to-moment feedback, and it is
  the first feedback in the project that is not about a finished line.
* The page, the hints and the accessibility rules all have to deal with ten states per
  square. Whether they survive that unchanged is the question this family was picked
  to answer, and it gets answered in part four, not here.
