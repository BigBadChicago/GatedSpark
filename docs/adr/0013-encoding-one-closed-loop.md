# 0013. Encoding "the lines make one closed loop"

Date: 2026-10-23

Status: accepted

## Context

Slitherlink's central rule is the first thing in this project with no local encoding.
A Nonogram clue is about one line and nothing else, so its encoding is a disjunction
over that line's placements and the whole puzzle is a pile of independent per-line
assertions. "These edges form exactly one closed loop" is not like that. It is a fact
about the whole board at once, and the usual local approximation of it is wrong in a
way that is invisible from inside: a board of two separate rings satisfies every rule
a person would think to write down, and publishing it would mean publishing a puzzle
whose recorded answer is not an answer.

The family options review named this as the reason to make Slitherlink family two
rather than family five, and `gs_core::dsl::Global::SingleClosedLoop` was reserved on
the slice's first day so the rule language would have somewhere to put it. This is
the decision that makes the reservation real.

The constraint is also the one place in the project where the obvious encodings are
genuinely different from each other, so the choice is worth writing down rather than
re-deriving later from the code.

## The options

**Lazy solve-and-block.** Ask for a board satisfying the local rules, check whether
it is one loop, and if not, assert that this particular set of edges is forbidden and
ask again. Standard practice, often fast, and rejected here: the number of rounds is
unbounded and the loop lives in *our* code, so the published `unsat` would be a claim
about a conversation rather than about one query. It would also mean the thing
recorded in the validation report is not the thing that was asked, which breaks
re-verification later.

**Flow.** Push a unit of flow round the loop and require conservation. Exact, and it
needs a chosen source, which is the same difficulty as the option below with more
machinery and more integers.

**Cardinality.** Assert the number of drawn edges equals the number of dots the loop
passes through. True of any union of rings, so it does not separate one ring from
several. Rejected as not a constraint at all.

**Distance labels, chosen.** One integer `root` naming a dot, one integer `d` per dot,
and two assertions per dot.

## The decision

The loop rule is encoded as, for every dot:

* if `root` names this dot, it is on the loop and `d = 0`;
* if it is on the loop and `root` does not name it, then some drawn edge at it leads
  to a dot with a strictly smaller `d`.

Together with the local "nought or two drawn edges at every dot", that is exactly one
loop, in both directions.

*No second ring.* Nought-or-two makes every piece of the drawn edges a ring. Suppose a
ring not containing `root`. Take its dot with the smallest `d`. It is on the loop and
is not `root`, so it needs a neighbour **in that same ring** with a strictly smaller
`d`, and it was the smallest. There is no such ring.

*Every real loop is accepted.* Label each dot with its number of steps round the loop
from `root`. Going either way round, every dot but `root` has a neighbour one step
closer. So a labelling exists for any single loop, and the encoding never refuses a
board it should accept.

`root` is an integer naming a dot rather than one Boolean per dot. Booleans would need
an "exactly one root" constraint, which is `n(n-1)/2` pairwise clauses — 7,260 of them
on a 10x10 board — where an integer bounded to the dot range gives "exactly one" for
free.

The upper bound on each `d` is a hint, not part of the rule. It is sound because the
step-counts above never exceed the number of dots.

## The consequence that is easy to get wrong

`root` and the `d` labels are working-out, not part of the answer. One board can be
labelled many ways, so one board appears in many models.

**So the uniqueness query blocks edges, not models.** The blocking constraint says at
least one *edge* differs from the witness. A model differing only in its labels does
not satisfy it, so the only thing that can answer `sat` is a genuinely different
board.

Had it blocked whole models, the solver could have answered `sat` by relabelling the
very board it was handed, every puzzle would have looked ambiguous, and nothing would
ever have been published — the harmless direction of the fault. The harmful direction
is the same mistake made while also treating `sat` as a pass.
`tests/solver_agreement.rs::relabelling_the_same_loop_is_not_a_second_answer` holds
this, on a long thin loop because that is the shape with the most labellings to choose
from.

## What is enforced, and by which test

| Claim | Test |
| --- | --- |
| Two separate rings are refused by a real solver | `gs-slitherlink/tests/solver_agreement.rs::two_rings_are_refused_by_the_solver` |
| ...and would be **accepted** without the one-piece assertions | `...::without_the_one_piece_rule_the_solver_would_accept_two_rings` |
| A genuine loop is accepted whatever its shape or length | `...::a_real_loop_is_accepted_whatever_its_length` |
| Relabelling a board is not a second answer | `...::relabelling_the_same_loop_is_not_a_second_answer` |
| A board of noughts has no answer | `...::a_board_where_every_square_says_nought_has_no_answer` |
| A step only ever goes to a real neighbour | `gs-slitherlink/tests/encoding.rs::a_step_only_ever_goes_to_a_real_neighbour` |
| The DSL's other two globals are refused by name | `...::the_dsls_other_global_rules_are_refused_by_name` |
| Rules missing the loop rule are refused, not patched | `...::rules_without_the_loop_rule_are_refused_rather_than_patched` |
| A Tier A encoder refuses this family by name | `gs-slitherlink/tests/refused_by_a_tier_a_encoder.rs` |

The second row is the one that makes the first mean something. Without it, a passing
`unsat` could be an encoding that refuses everything.

## The cross-check, now running

ADR 0002 asked for a second, independent solver checking every verdict, and the tests
have accepted one through `GATEDSPARK_SMT_SOLVER_2` since M2 — while nothing in CI
ever set it. The cross-check existed and had never run.

It runs now. CVC5 1.1.2 is in Ubuntu's own packages, so it costs one apt line and no
vendoring, and CI sets `GATEDSPARK_SMT_SOLVER_2=cvc5` for both families. Z3 4.8.12 and
CVC5 1.1.2 agree on all of Slitherlink's fixtures and all of Nonogram's. A
disagreement about a uniqueness result fails the build.

## What this does not decide

Nothing here is about speed. The encoding is one integer per dot on top of one Boolean
per edge, which is small at the board sizes a person plays, and no board large enough
to be slow has been measured yet. If it ever becomes the bottleneck, the lazy
solve-and-block loop is the known faster option, and it would have to be tested
against this encoding rather than trusted — faithfulness is not something to take on
trust from a cleverer encoding.
