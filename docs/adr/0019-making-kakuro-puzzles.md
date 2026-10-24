# 0019. Making Kakuro puzzles

Date: 2026-10-24

Status: accepted

## Context

Both earlier families generate the same way in outline: build something, read the
clues off it, and keep it if it has exactly one answer. Nonogram draws a picture.
Slitherlink draws a loop, numbers every cell, and then does its real work by removing
numbers while uniqueness holds.

Neither shape fits Kakuro, and the reason is not a detail.

**No total can be removed.** A run with no total means "any digits at all", which is
not a harder puzzle but a puzzle with nothing to work out. So there is no removal step
and no removal budget.

**No total can be chosen.** The totals are the sums of the answer, so they follow from
the filling rather than being picked.

That leaves exactly one lever: where the blocks go.

## What was measured before deciding

The obvious method was built first and measured on 2026-10-24: propose a random block
pattern, fill it, read the totals off it, keep it if it has one answer.

| Board | Blocks | Kept per 100 patterns |
|---|---|---|
| 4 by 4 | 25% | 20 |
| 5 by 5 | 25% | 9 |
| 7 by 7 | 25% | 1 |
| 5 by 5 | 45% | 46 |
| 7 by 7 | 45% | 20 |

Density buys uniqueness, and it buys it by giving the board away: at 45% blocks, about
half the white squares sat in a run of a single square, and a run of one names its
digit outright.

Three fixes were tried and none worked.

- **Capping run length at five.** Random patterns almost never produce a layout whose
  every run is between two and five squares, and the few found were still ambiguous.
- **Biasing the fill towards extreme digits**, so totals would have fewer digit
  combinations. No measurable difference.
- **Refilling.** Forty different fillings of one layout with no run of one square
  produced **no** unique puzzle, at every size tried.

That last result is the one that explains the others, and it is why this decision
exists rather than a tuning commit.

## The diagnosis

Each run contributes one adding-up equation. So a layout has about as many equations as
runs and as many unknowns as white squares, and the ratio that matters is **runs per
white square**. Random sparse layouts have long runs, so few runs: measured at 0.5 to
0.7 runs per white square. A system with half as many equations as unknowns has room
for many answers whatever digits are put in it, so no amount of refilling can help.

The second answers found were not near misses. On a 5 by 5 with 23 white squares, 18 of
them held a different digit.

## Decision: repair the layout, guided by the second answer

1. Propose a **sparse** block pattern and fill it in.
2. Read the totals off the filling, and ask our own solver for an answer that is **not**
   the one we started from.
3. If there is one, the squares where the two answers differ are where the board has
   slack. **Turn one of them black.** That removes an unknown and adds a run boundary,
   so it adds equations and takes away a degree of freedom — the thing the diagnosis
   says is missing.
4. Repeat until there is exactly one answer, or the round limit is reached.

Measured over 40 seeds per size, with the solver's default budget:

| Board | Kept | White squares | Repairs | Given away |
|---|---|---|---|---|
| 4 by 4 | 31 of 40 | 10 | 2 | 393 in 1000 |
| 5 by 5 | 30 of 40 | 14 | 5 | 359 in 1000 |
| 6 by 6 | 35 of 40 | 20 | 7 | 362 in 1000 |
| 7 by 7 | 39 of 40 | 27 | 12 | 374 in 1000 |
| 8 by 8 | 36 of 40 | 35 | 15 | 372 in 1000 |
| 9 by 9 | 34 of 40 | 43 | 20 | 384 in 1000 |

This also explains, after the fact, why real Kakuro grids are so black. Density is not a
style choice in this puzzle. It is what buys uniqueness.

## Decision: which square to black out

Any differing square works for correctness. Taking the first one found was measured
first: same keep rate, and about **59 in 100** white squares end up given away.

So the choice is greedy on quality: among the differing squares, take the one whose
blocking creates the fewest runs of a single square, breaking ties towards keeping more
white squares. Same keep rate, and the giveaway share falls to about **38 in 100**.

Both tie-breaks are fixed rather than random, because one seed must always give one
puzzle: a validation report records a seed, and a seed that produced a different puzzle
tomorrow would make the report a record of nothing.

## What this does not fix, stated plainly

**About 38 in 100 white squares are still given away.** That is a lot. A player is
handed more than a third of the board and works out the rest. It is better than the 59
in 100 the simpler choice gives and much better than a density-only approach, and it is
not good.

It is recorded in the feature vector as `given_away_squares` and in the generator's
quality gate rather than being left as an absence, because a number nobody can see does
not get improved. The likely next step is to stop proposing random starting patterns
and start from a **constructed** layout — a black top row and left column holding the
totals, with each line cut into short runs, the way a published Kakuro is built. A first
attempt at that was written and discarded on 2026-10-24: the row-wise construction kept
breaking the column rule, and repairing the columns broke the rows again. It is a real
piece of work rather than a tweak, and it is not needed for the slice to exist.

## Consequences

- **Generation cost is higher than either earlier family and still not a constraint.**
  Measured per kept puzzle: about a thousandth of a second at 4 by 4, a quarter of a
  second at 7 by 7, and six and a half seconds at 9 by 9. Against a daily puzzle per
  game, that is seconds of computer time per day. The conclusion in
  `back-on-plan.md` section 3 holds: making puzzles is not a cost at any plausible
  size.
- The round limit is 80, which also bounds how much of the board repair may eat, since
  each round removes exactly one white square. A test asserts that the white count
  falls by exactly one per repair, so a future repair that moved a block instead would
  fail the build rather than quietly changing what the numbers mean.
- The quality gate lives in the generator, not in `rules.rs`. A board that is mostly
  giveaways is a perfectly legal Kakuro, so the rules must accept it, and only the
  thing choosing what to publish may decline it.
- The keep rates above are measurements **against our own fast solver**, whose total
  deduction is a deliberate relaxation. `tests/solver_agreement.rs` runs generated
  puzzles past Z3 and CVC5 and requires them to agree, which is what makes the keep
  rate a fact about puzzles rather than about our own code.
