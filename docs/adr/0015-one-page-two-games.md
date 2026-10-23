# 0015. One page that plays two games

Date: 2026-10-23

Status: accepted

## Context

`gs-web` was written for one game. Its doc comment said so plainly: "this build has
one family's rules in it and cannot judge another's", and its refusal message named
Nonogram as the only game it knew. Slitherlink is the second game, so that sentence
had to change — and the question was which way.

There were two ways to put a second game in front of a player.

**A second page.** Copy `gs-web` to `gs-web-slitherlink`, change the family check, and
serve a different page for a different game. Cheap to start, and each page stays
simple.

**One page that knows both.** Keep one loader and one trust chain, and let the games
differ only where they genuinely differ.

The deciding argument is not the duplication. It is where the duplication would land.
A second page would open packages, recompute content fingerprints, verify Ed25519
signatures against a built-in key list, check rules versions, and refuse withdrawn
content — all of it again. There would then be two answers to "is this package
trustworthy", they would drift apart as one page was changed and the other was not, and
the one in front of a player would be whichever page they happened to open. That is the
same argument that put the validation report inside the signed package rather than
beside it (0006), and the same one that made the offline buffer refuse to know anything
about any game (0011).

## Decision

One page. One `load`, one key list, one buffer, one withdrawal list, two games.

**What is shared, and written once:** recomputing the fingerprint over the bytes,
checking the signature against the page's own key list, reading the level and the
encoding out of the manifest, saving a puzzle for later, and refusing a withdrawn one.
None of this ever learns which game it is handling.

**What is per game, and only this:**

1. Matching the family name against `KNOWN`.
2. Matching that game's `RULESET_VERSION`.
3. Handing the payload to that game's own reader.

`KNOWN` is a list of `{family, rules_version}` pairs and is the only list. The refusal
message for an unknown game is built from it, so a third game cannot be added without
the message saying so.

**Playing is two types, not one.** A Nonogram move marks a square. A Slitherlink move
marks a line between two dots. A single `mark(index)` for both would have compiled, and
would have marked whichever line happened to share a square's number. So
`play::nonogram::Session` and `play::slitherlink::Session` each have their own moves,
and `play::Session` holds either and answers only the questions that are the same for
both: undo, redo, moves made, the board's fingerprint, and whether it is solved.

A page asks for the game it wants and gets nothing if that is not what it is holding.
Where that nothing could hide a bug — a page making a move the game does not have —
the `_and_say` methods return a sentence naming the game instead, because the quietest
possible answer takes the longest to find.

**The page never works out which line is which.** `Game::line_places` reports where
every line is, in the same order `Game::lines` reports their marks. A page builds its
lines from that list and reads the marks at the same positions. Had the page computed
the numbering itself, it would have held a second opinion about which line is which,
and the day the two disagreed it would have drawn one line and marked another.

## What this costs

The browser now carries two games' rules. `tests/boundary.rs` records that as a
decision rather than letting it pass as a detail, and it is a real cost: every player
downloads the rules of a game they may not play. It is small — both families are
rules and encodings, not assets — and the alternative was two trust chains.

It still carries no solver runner and no store. That half of the boundary test is
unchanged and is the half that matters: a browser containing the solver runner would
answer "don't know" to every question and quietly become a browser in which no puzzle
is ever provably unique (0009).

## What is enforced, and where

| Claim | What holds it |
|---|---|
| One loader, one trust chain | `load` is the only way to get a `Playable`; there is no second constructor |
| A game the page does not know is refused | `KNOWN` lookup, before anything else; `another_games_puzzle_is_refused_rather_than_attempted` |
| The refusal names the games the page does know | message built from `KNOWN`; `every_refusal_explains_itself_in_words_a_person_can_read` |
| A wrong rules version is refused, in either direction | per-game `rules_version` compare |
| A game's bytes are read by that game's own reader | the three-line per-game branch; `BoardProblem` keeps the two readers' errors apart |
| A Nonogram move cannot be made on a Slitherlink | two session types; `a_page_cannot_make_a_nonogram_move_on_a_slitherlink`, and in a browser `a_nonogram_move_on_a_loop_puzzle_is_refused_in_words` |
| Whether a board is solved comes from the rules | each session's `is_solved` calls `Ruleset::is_solved`; `being_solved_is_asked_again_after_an_undo` |
| The page does not number the lines itself | `Game::line_places`; `a_line_is_the_same_line_whichever_way_the_page_asks_for_it` |
| The buffer and the withdrawal list still know no game | `both_games_can_be_saved_on_one_device_at_once`, `a_withdrawn_loop_puzzle_is_refused_and_deleted_like_any_other` |
| The browser carries two games and no solver | `tests/boundary.rs`, both halves |

## What this does not settle

Whether a third game belongs in the same page, or whether at some point the page should
fetch only the rules it needs. That is a question about download size, and there is no
measurement of it yet. The decision above is about the trust chain, and that answer does
not change with the number of games: it is written once however many games there are.

Still open from before, and untouched by this: whether browser puzzles arrive as plain
files on a static host or through a service, and whether to take on a browser-driving
dependency so the page itself is checked in the build rather than only by hand.
