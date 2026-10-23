# 0014. Making Slitherlink puzzles by taking numbers away

Date: 2026-10-23

Status: accepted

## Context

Nonogram's generator is "draw a picture, count the runs, keep it if the solver says
the clues have one answer". The counting is total and deterministic, there is nothing
to leave out, and one uniqueness question is asked per puzzle.

Slitherlink is not like that, and the difference is not a detail. A cell may carry no
number at all, and that is an ordinary correct puzzle. Numbering every cell gives a
board that states its answer several times over — correct, and not worth playing. So
the generator's real work is deciding which numbers to **remove** while the puzzle
still has exactly one answer, which means one uniqueness question per cell rather than
one per puzzle.

That changes the economics of generation for this family, and it is the first time the
platform has met a family whose generation cost is not dominated by the final
verification.

## The decision

Three parts.

**1. Grow the inside, not the loop.** `grow_loop` builds a set of cells for the loop
to enclose, one randomly chosen neighbour at a time, and keeps an addition only if the
boundary of the resulting shape is still one closed loop. The check is `Loop::of` —
the same checker the engine uses — so the generator cannot produce a shape the engine
would not accept. An addition that would put a hole in the middle fails that check and
is dropped, because the boundary of a shape with a hole is two loops.

Growing the inside rather than walking a path is what makes "one loop" true by
construction instead of by luck.

**2. Remove numbers in a seeded random order.** A fixed order biases every puzzle
towards bare top-left corners. The order comes from the seed, so the whole puzzle is
reproducible from the seed the validation report records.

**3. Cap the work per question, and keep the number when the cap is hit.** The reason
is measured rather than assumed.

| board | cap on our solver | numbers left of … | time per puzzle |
| --- | --- | --- | --- |
| 7x7 | 2,000 nodes | 14–18 of 49 | 16–122 ms |
| 7x7 | 200,000 nodes | 13–18 of 49 | 17–234 ms |
| 10x10 | 2,000 nodes | 30–36 of 100 | 378 ms – 1.2 s |
| 10x10 | 200,000 nodes | 27–34 of 100 | 380 ms – 13.7 s |

Six seeds each, release build, 2026-10-23. The cap leaves one to three more numbers on
a board of a hundred and makes generation ten to thirty times faster. `REMOVAL_BUDGET`
is that cap.

Nothing unsafe follows from capping. A number comes off only on a clear "exactly one
answer"; an exhausted budget **puts it back**, because a budget running out is not
permission. And the real solver still has the last word on the finished puzzle, at the
same standard as every other puzzle this platform publishes.

## Why our own solver is weak, and why that is written down

Nonogram's fast solver is complete for a single line, so "needs no guessing" is a fact
about the puzzle. This family's is not: it knows the per-cell count rule, the
nought-or-two rule at each dot, and the loop-closing rule, and none of the well-known
corner and diagonal patterns a Slitherlink player uses. A fully numbered 5x5 board
costs it one guess.

Two consequences, both recorded on the code:

* `Features::decision_points` grades puzzles **against this solver**, and may only be
  compared with other numbers from the same solver. Adding a deduction rule changes it
  for every puzzle, so such a change belongs with a bump of `GENERATOR_VERSION`.
* Changing a deduction rule changes *which puzzles the generator keeps*, because the
  removal loop asks this solver after every removal. That is the same reason, and it is
  the more surprising half.

## No difficulty levels yet

`ValidationReport.band` is `None` for every Slitherlink puzzle, on purpose.

Levels are the game's to set, which the project owner settled on 2026-10-22, and this
game has no measured play data to set them from. The Nonogram slice's lesson was
sharper than that: the number everyone would have graded on (the forced-move ratio)
turned out to be exactly 1000 for every puzzle that needed no guessing, and carried no
information at all. A threshold invented now would sort puzzles into levels that mean
nothing and would fail silently rather than loudly.

`examples/sweep.rs` is the tool that produces the numbers. The levels come after them.

## What is enforced, and by which test

| Claim | Test |
| --- | --- |
| A grown shape is always one loop | `tests/generation.rs::a_grown_shape_is_always_one_loop` |
| Asking for more enclosed cells grows longer loops | `...::a_bigger_enclosed_share_grows_a_bigger_loop` |
| Numbers really do come off | `...::numbers_really_do_come_off` |
| The same seed rebuilds the same puzzle | `...::the_same_seed_gives_the_same_puzzle` |
| …and different seeds do not give one puzzle | `...::different_seeds_give_different_puzzles` |
| A tighter cap removes less, and never unsafely | `...::a_tighter_cap_removes_less_and_never_removes_something_unsafe` |
| Removing numbers never changes what the answer is | `...::every_removal_keeps_the_puzzle_answerable_by_the_same_loop` |
| A candidate carries no loop, only its clue grid | `...::a_candidate_does_not_carry_the_loop_it_came_from` |
| A stored answer decides every edge | `tests/publishing.rs::a_witness_that_leaves_an_edge_undecided_is_refused` |
| An unanswered question is `UNKNOWN` and refuses | `...::an_unanswered_question_is_recorded_as_undecided_and_refuses` |
| No difficulty level is claimed | `...::no_difficulty_level_is_claimed_yet` |
| Generated puzzles pass real solvers, which agree with ours | `tests/solver_agreement.rs::the_whole_pipeline_produces_reports_a_real_solver_agrees_with` |

The last row is the exit test for this ADR, and it runs against both Z3 and CVC5 in
CI. The generator decides what to keep using our own solver, so a disagreement between
that solver and a real one would make every puzzle this family produces suspect, and
nothing else in the repository would notice.

## One thing the witness needed

A board where a ring has closed can still have undecided edges, and the engine's own
goal test does not mind: a player has finished when the drawn lines are right. A
*stored answer* must decide every edge. So `solve` completes the board before recording
an answer, ruling out every edge still undecided.

That is filling in a deduction, not making an assumption: once a ring is closed nothing
can ever be added to it, so every undecided edge is forced to be undrawn. Without it
the pipeline refused its own witnesses, for a reason that was nobody's fault.
