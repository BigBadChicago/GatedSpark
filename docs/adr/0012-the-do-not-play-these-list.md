# 0012. The "do not play these" list

Date: 2026-10-23

Status: accepted

## Context

Every other check in this system answers the question *are these bytes what their
publisher signed?* A withdrawal is the one question that cannot be answered that way,
because the bytes are exactly what the publisher signed. The puzzle is genuine and the
signature is perfect, and it still must not be played: it turned out to have no answer,
or more than one, or it should never have been offered.

Once a package has been downloaded, nothing built so far can take it back.
`Buffer::discard_spoiled` (ADR 0011) throws away anything that fails its own checks,
which covers damage and tampering and nothing else. ADR 0011 named this as the last
remaining hole in the offline story. This closes it.

`content-delivery.md` asked for it in one line: *"reconcile against a revocation list
when it next connects, hiding anything withdrawn."* The design below differs from that
line in one way, deliberately, and the difference is the point: it **deletes** rather
than hides.

## The question that was the project owner's to answer

What happens to somebody who is part-way through a puzzle when it is withdrawn? Asked on
a decision card, and answered: **stop it right away.** The board closes with a short note
saying the puzzle had a problem, and whatever was filled in is lost.

Implemented as asked. The alternative — letting them finish — means either a puzzle that
cannot be finished at all, or marking somebody wrong when they were right.

## Decision

### A signed file with its own magic, in `gs_core::withdrawal`

Magic `GATEDSWD`, its own format version, a serial number, and a list of content ids each
with a reason. Signed by the same kind of key that signs puzzles, and read by the same
order of operations `package::open` follows: framing, then the digest, then the signature,
and only then the contents.

**Its magic is part of what the signature covers**, which is the one way it differs from a
package. That is what stops a publisher's signature over a *puzzle* being replayed as a
signature over a *withdrawal list*, which would otherwise let anybody holding a signed
puzzle withdraw puzzles with no key at all. The package's own digest input is left exactly
as it was, because changing it would change what every signature already issued covers,
and the two provably cannot collide: a package's begins with its version bytes
`[0x01, 0x00]` and a list's begins with `GA`.

### A serial number that only goes up, and not a date

A device remembers the highest serial it has accepted and **refuses any list numbered at
or below it**.

Without that rule the attack needs no key and forges nothing: keep serving the genuine,
correctly signed list from before the bad puzzle was named. Every check passes and the
withdrawal never arrives.

**A number and not an expiry date**, for the reason `ValidationReport` carries no
timestamp: a record whose validity depends on the reader's clock cannot be re-checked
later, and a device's clock is the easiest thing on it to change. A serial number needs no
clock and works with no signal.

### A deny-list and not an allow-list

An allow-list would be stronger against a device that never connects, and it would destroy
the offline guarantee just built: a device with no signal could not play anything missing
from the latest list it happened to hold. The thing being protected is play without a
network, so the mechanism cannot require one.

### Delete, do not hide

Accepting a list throws the named packages off the shelf there and then, inside
`Notices::accept`, rather than filtering them when they are read.

Filtering would pass every obvious test and still be wrong: a player who cleared the
stored list would get the bad puzzle back. Once the bytes are deleted, tampering with the
device's own notes cannot bring them back. It is also why wiping local storage is only a
small loss — the worst it does is make the device accept an old list again, and an old list
names fewer puzzles than it has already acted on.

The buffer keeps the on-the-way-out check as well, for the case where the bytes come back:
storage restored from a backup, or edited by hand.

### Withdrawing does not delete the puzzle from the store

The store keeps the puzzle and the list names it. Deleting it from the store would leave
nothing to point at and no way to undo a mistake. `gatedspark withdraw --keep` is the undo,
and it works because a newer list replaces an older one outright rather than adding to it.

### Every call takes the list, and none of them has a default

`Buffer::keep`, `next`, `stock` and `discard_spoiled` all take a `Standing`. A default
would be a way for a caller to skip the check without noticing, which is exactly the
failure this ADR exists to prevent.

### `Page::open_for_play`, beside `Page::open`

`open` is the package check and knows nothing about withdrawals, because it has to work
where there is no storage to read a list from. `open_for_play` is the question a player's
screen depends on. The page calls the second one everywhere, and `why_withdrawn` after a
list arrives, so the page still works nothing out for itself.

## What is enforced, and by what

| Claim | Test |
| --- | --- |
| A list round-trips, and the module's bytes are the documented layout | `gs-core/tests/withdrawal.rs::a_sealed_list_opens_and_round_trips`, `the_hand_written_container_and_the_module_agree` |
| A signature over a puzzle can never stand for a withdrawal | `withdrawal.rs::a_signature_over_a_puzzle_can_never_stand_for_a_withdrawal`, `a_list_is_not_a_package_and_a_package_is_not_a_list` |
| Every byte of the body is covered | `withdrawal.rs::a_changed_byte_anywhere_in_the_body_is_refused` (walks the whole body) |
| An unsigned or untrusted list is refused | `withdrawal.rs::a_list_nobody_signed_is_refused`, `a_key_the_reader_does_not_trust_is_refused` |
| The signature is checked before the body is decoded | `withdrawal.rs::the_signature_is_checked_before_...`, `the_digest_is_checked_before_...`, with the anti-vacuity half in `a_malformed_body_that_is_honestly_signed_fails_on_the_body` |
| An unknown version or reason is refused rather than guessed at | `withdrawal.rs::a_version_this_reader_does_not_know_is_refused_rather_than_guessed_at`, `a_reason_nobody_can_read_is_a_malformed_file_and_never_a_default_reason` |
| No reason tag is zero | `withdrawal.rs::no_reason_is_written_as_a_zero_byte` |
| An `OpenedList` cannot be forged | `withdrawal.rs` doctest, `compile_fail`, verified in both directions |
| A withdrawn puzzle is deleted and not hidden | `gs-offline/tests/withdrawing.rs::a_withdrawn_puzzle_is_deleted_from_the_device_and_not_merely_hidden`, `clearing_the_devices_notes_cannot_bring_a_withdrawn_puzzle_back` |
| An older list replayed cannot un-withdraw anything | `withdrawing.rs::an_older_list_served_again_cannot_un_withdraw_a_puzzle`, `the_same_list_twice_is_refused_the_second_time`, `a_list_numbered_zero_is_never_accepted` |
| A newer list replaces the older one, so a mistake can be undone | `withdrawing.rs::a_newer_list_replaces_the_older_one_outright` |
| A puzzle nobody named is untouched | `withdrawing.rs::a_puzzle_that_is_not_named_is_left_completely_alone` |
| A withdrawal is remembered for a puzzle the device does not have yet | `withdrawing.rs::a_withdrawal_naming_a_puzzle_this_device_never_had_is_still_remembered` |
| The stored list is checked every time it is read | `withdrawing.rs::a_stored_list_that_has_been_tampered_with_is_reported_and_not_obeyed` |
| Broken storage is reported, never read as "nothing withdrawn" | `withdrawing.rs::storage_that_will_not_answer_is_reported_rather_than_treated_as_nothing_withdrawn` |
| Every refusal a person sees is a sentence | `withdrawing.rs::every_refusal_a_person_can_see_is_written_in_words` |
| The two ways of naming a shelf entry cannot disagree | they are one function: `buffer::name_for` calls `buffer::name_for_bytes` |
| It all works in a real browser's own storage | `gs-web/tests/in_a_real_browser.rs`, five tests, run in CI |
| A stranger's list is ignored in a browser too | `in_a_real_browser.rs::a_list_from_a_stranger_is_ignored_in_a_browser_too` |
| The whole story works on the real page, including stopping a board mid-play | `scripts/drive-page.mjs` — **by hand, not in CI** |

## What it cost

The browser download went from **62 KB compressed to 71 KB**. Measured by
`scripts/build-web.sh`, not guessed.

## Not enforced

* **A device that never connects never hears about a withdrawal.** No design fixes this
  and none can: the decision is made elsewhere and reaching the device takes a connection.
  What the rules above buy is that the withdrawal arrives the *first* time the device
  connects and cannot be talked out of arriving.
* **Nothing asks for a list.** `Page::accept_withdrawals` takes bytes and
  `Page::withdrawals_seen` says which number to ask past; deciding *when* to fetch belongs
  with the thing that has a network, as with `Stock::wanted` in ADR 0011. On the demo page
  it is a button.
* **The list is not checked by `gatedspark recheck`.** `recheck` walks the puzzle
  directories. `Store::withdrawals` re-opens the list through its signature, but nothing
  calls it as part of a routine sweep yet.
* **A list has no size limit.** The count is a `u16`, so 65,535 entries, and a list that
  long would be about two megabytes. Nothing yet prunes entries for puzzles that have been
  deleted everywhere, which is the thing that would keep it from growing forever.
* **`Page::open` is still reachable and still unguarded.** It has to be: it works where
  there is no storage. A caller that uses it instead of `open_for_play` gets no withdrawal
  check. The page uses the right one, and nothing stops a future caller using the wrong
  one.
* **Nothing proves the serial came from the publisher's own counter.** The rule enforced is
  only that it goes up. A publisher who re-used a number would publish a list every device
  refuses, which is safe and silent.
