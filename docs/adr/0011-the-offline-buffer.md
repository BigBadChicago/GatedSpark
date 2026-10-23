# 0011. The offline buffer

Date: 2026-10-23

Status: accepted

## Context

A player should be able to open the app with no signal and still have puzzles. Three
rules were agreed before any of this was built, in `content-delivery.md`:

* **Five ahead.** The device holds five unplayed puzzles, downloaded in advance.
* **Delivered is not played.** Bytes arriving is not a player playing. Counting a
  download as a play would quietly burn puzzles nobody ever opened.
* **One bookmark in a shuffled order.** One number per level instead of one row per
  puzzle played, which keeps the one thing that *does* grow with the number of players
  from growing without bound.

## Decision

### A new crate, `gs-offline`, which knows about no game and no device

It decides **policy** — how many to hold, which is next, what counts as played, what a
player is told when there are none left. It touches no files and no network, which is
what lets it run in a browser.

Its dependency list is `gs-core` and nothing else. In particular **there is no game
crate**: it holds packages as bytes and checks them against a key. Which family a puzzle
belongs to is the page's question, and a buffer that knew about Nonogram would have to
learn about every family after it. `tests/common/mod.rs` makes this a test rather than
an intention: every package in the buffer's tests is sealed with a family that does not
exist, carrying a payload that is not a puzzle in any family, and the buffer keeps,
returns and counts them without noticing.

### Where the bytes live is somebody else's problem

`Shelf` is four operations: put, get, remove, list. A browser has no folders, and
`gs-store`, the crate that does use folders, is the one a browser is forbidden from
carrying (ADR 0009). So the host supplies the place.

Four operations because four is what the policy needs. A fifth — a shelf that could sort
or search — would be a second place where an opinion about the buffer's order could grow.

`Shelf::names` promises a **stable** order. Two devices holding the same five puzzles
must agree about which is next, and `localStorage` makes no promise about its own order,
so `LocalStorage::names` sorts.

Two implementations ship: `Memory`, which every test here runs on, and `AlwaysFails`,
which exists so the failure paths are exercised rather than merely declared.

### Everything is checked on the way out, every time

A shelf is not trusted. On a device it is `localStorage`, a few clicks from being edited
by hand in any browser's developer tools. So every puzzle is checked against the trusted
key *as it leaves*, exactly as one arriving over the network would be.

Checking on the way in as well is a convenience: a bad download fails where somebody can
see it, rather than in three days with no signal. The on-the-way-out check is the one
that matters. Dropping it would make the buffer the one place in this system where bytes
are believed because of where they were found.

### Reading never deletes

`Buffer::next` and `Buffer::stock` skip anything that fails its checks and **leave it
where it is**. `discard_spoiled` is a separate call a caller makes on purpose.

The reason: a puzzle that fails today because the device's storage is misbehaving would
be gone forever if reading deleted it, and a player with no signal would have nothing
left. Spoiled entries are counted in `Stock::spoiled` rather than hidden, because a
device quietly throwing things away is a device nobody can debug.

### The shuffled order only ever grows at the end

`Order::grow_to` shuffles the new entries among themselves and appends them. Nothing
already in the sequence moves. That is the promise the whole design rests on: if the
order could be reshuffled, a bookmark part way along would point at a different puzzle
tomorrow than it did today, and the player would be handed something they had already
played.

Each appended block is seeded from the player's seed **and** from where the block
starts, so a player whose pool grows twice by the same amount does not get the same
arrangement twice.

A `grow_to` with a smaller size does nothing. A pool does not shrink, and if one ever
appeared to, dropping entries a bookmark might already be past is the worst available
response.

`Bookmark::played` takes the order so it cannot walk off the end, and `Bookmark::at`
does no validation, because a bookmark and its order are stored separately and can be
read back in either sequence.

### `SplitMix64` moved from `gs-nonogram` to `gs-core`

Two things now need a reproducible sequence and they must be the same sequence: Nonogram
generation rebuilds a pattern from a recorded seed, and the buffer rebuilds a player's
order from one. Two copies of SplitMix64 in one workspace would be two spellings of one
thing. `gs_nonogram::rng` is now a re-export, so every recorded seed in every validation
report still means what it did.

### The browser uses `localStorage`, with its limits stated

It is the only browser storage that can be read and written without waiting, which
matters because every other call on the page is immediate. It is a few megabytes and
five packages are tens of kilobytes, so the limit is nowhere near.

It is **not private and not reliable**, and nothing depends on it being either. A
private window may refuse it outright, which `Page::saved` reports as its own sentence
rather than as an empty device.

`web-sys` (MIT OR Apache-2.0) is the dependency, feature-gated to `Window` and `Storage`
so nothing else about a browser becomes reachable by accident. It is `wasm-bindgen`'s own
binding to the browser's interfaces, and every alternative is a hand-written copy of it —
the recorded reason ADR 0004 requires.

Bytes go into text storage as hex. Not base64: hex needs no dependency and no padding
rules, and at tens of kilobytes the extra size is not worth a second encoding to be
wrong about. Text that will not decode is treated as "not one of ours" rather than as an
error, so one corrupted entry cannot make a device unreadable.

## Enforcement

| Claim | What holds it |
| --- | --- |
| A puzzle kept can be played back with no network at all | `gs-offline/tests/buffering.rs::a_puzzle_kept_can_be_played_back_with_no_network_of_any_kind` |
| A puzzle is stored under its own fingerprint, not a supplied name | `buffering.rs::the_name_a_puzzle_is_kept_under_is_its_own_fingerprint` |
| A stranger's, an edited, and a non-package are never kept | `buffering.rs::a_puzzle_from_a_stranger_is_never_kept`, `..._with_one_byte_changed_...`, `something_that_is_not_a_package_...` |
| A puzzle edited on the shelf is never handed over | `buffering.rs::a_puzzle_that_was_edited_on_the_shelf_is_never_handed_over` |
| A stranger's package planted on the shelf is never handed over | `buffering.rs::a_stranger_s_puzzle_planted_on_the_shelf_is_never_handed_over` |
| …and one bad entry does not hide the good ones | `buffering.rs::a_bad_puzzle_does_not_hide_the_good_ones_behind_it` — the anti-vacuity half |
| Being handed a puzzle is not having played it | `buffering.rs::being_handed_a_puzzle_is_not_the_same_as_having_played_it` |
| Every puzzle is handed out exactly once | `buffering.rs::every_puzzle_is_handed_out_exactly_once` |
| The target is five, and a device knows how many more to ask for | `buffering.rs::the_target_is_five_and_a_device_knows_how_many_more_to_ask_for` |
| A spoiled entry is counted and a replacement asked for | `buffering.rs::a_spoiled_puzzle_counts_against_the_target_...` |
| Reading never deletes | `buffering.rs::reading_the_buffer_never_throws_anything_away_by_itself` |
| Anything on the shelf that is not ours is left alone | `buffering.rs::things_on_the_shelf_that_are_not_puzzles_are_left_alone` |
| Running out, and storage failing, are different facts, both in plain words | `buffering.rs::a_device_with_nothing_on_it_says_so_in_words_a_person_can_read`, `..._will_not_read_says_so_rather_than_claiming_to_be_empty`, `both_storage_failures_are_explained_in_words_...` |
| The buffer knows about no game | `buffering.rs::the_buffer_holds_a_package_from_a_family_that_does_not_exist` |
| An order contains every entry exactly once, and is really shuffled | `ordering.rs::an_order_contains_every_entry_exactly_once`, `an_order_is_actually_shuffled` |
| Growing the pool never moves anything already in the order | `ordering.rs::growing_the_pool_never_moves_anything_already_in_the_order`, `growing_twice_keeps_both_earlier_blocks_in_place` |
| Nothing already played is handed out again after growth | `ordering.rs::nothing_already_played_is_ever_handed_out_again_after_the_pool_grows` |
| A bookmark never walks off the end | `ordering.rs::a_bookmark_at_the_end_asks_for_nothing_and_stays_there`, `..._read_back_from_beyond_the_end_...` |
| The bookmark is one number | `ordering.rs::the_bookmark_is_one_number_and_that_is_the_whole_record` |
| It all works in a real browser's own storage | `gs-web/tests/in_a_real_browser.rs`, six tests, run in CI |
| A saved puzzle edited in `localStorage` is refused there too | `in_a_real_browser.rs::a_saved_puzzle_edited_in_the_browsers_storage_is_never_handed_over` |
| The browser is still not compiled against the solver or the store | `gs-web/tests/boundary.rs`, which now expects `gs-offline` in the list |
| The whole story works on the real page | `scripts/drive-page.mjs` — **by hand, not in CI** |

## What it cost

The browser download went from **57 KB compressed to 62 KB** — the buffer, the shuffled order, and
`web-sys`'s two interfaces. Measured rather than guessed, by `scripts/build-web.sh`.

## Not enforced

* ~~**Withdrawing a puzzle already downloaded.**~~ **Closed 2026-10-23 by ADR 0012.**
  `discard_spoiled` still only throws away what fails its *own* checks; withdrawing a
  perfectly signed puzzle is now `gs_core::withdrawal` and `gs_offline::notices`, and the
  four `Buffer` calls above all take the device's withdrawal list.
* **Refilling from a server.** There is no server. The buffer is filled by being handed
  bytes, which is what a download does; deciding *when* to top up belongs with the thing
  that has a network. `Stock::wanted` is the number that side will need.
* **The five-days-ahead daily window.** This piece is the on-demand buffer. Dailies are
  scheduled by date rather than drawn from a pool, so their window is a different
  mechanism that does not exist yet.
* **Storage filling up.** `localStorage` is a few megabytes and five packages are tens of
  kilobytes, so the limit is nowhere near. A device that refuses a write is reported, but
  nothing watches how close it is getting.
* **A device shared by two players.** The buffer belongs to a device and a bookmark
  belongs to a player, which is why `Buffer` does not hold one. Nothing yet keeps two
  players' bookmarks apart, because nothing yet has two players.
