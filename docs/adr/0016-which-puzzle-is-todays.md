# 0016 — Which puzzle is today's

Status: accepted, 2026-10-23.

## The problem

A daily puzzle needs a notion of "today". This project has a firm rule, set when the
validation report was designed and repeated when the withdrawal list was: **nothing
deciding whether content can be trusted may depend on the reader's clock.** A device's
clock is the easiest thing on it to change, and a record whose validity depends on the
reader's clock cannot be re-checked later.

Those two things look like they contradict each other. They do not, but only if the
clock's job is cut down to one thing and that one thing is written down.

## The decision

A schedule maps a **day number** to a puzzle. Day numbers count from a start day written
inside the signed file. The device's clock turns today's date into a day number and is
used for nothing else.

So a wrong clock shows somebody the wrong day's puzzle. That is the entire blast radius.
It cannot make a bad puzzle look good, cannot revive a withdrawn one, and cannot invent
a day nobody signed, because all it does is pick a number to look up in a file that was
signed before it was ever read. Everything that decides trust is still the package's own
signature and the withdrawal list.

Nothing in this workspace reads a clock, and that stays true: the page passes the date
in. `gs_core::schedule` does arithmetic on dates it is handed.

## What the file looks like, and why

It is `gs_core::withdrawal`'s shape, because it has the same problems.

**Its own magic bytes, inside what the signature covers.** A publisher's signature on a
puzzle, or on a withdrawal list, is a real signature from a trusted key. If the three
files digested the same way, one of those signatures could be lifted onto a schedule,
and whoever lifted it would choose what every player plays today with no key at all.
Held by `a_signature_over_a_withdrawal_list_can_never_stand_for_a_schedule` and its
twin for packages.

**A serial number that only goes up.** A device refuses any schedule at or below the
highest it has accepted. Without it the attack needs no key: keep serving last month's
genuine, correctly signed schedule, and the player never sees a new day — including a
day whose puzzle was replaced because the first one turned out to be wrong. At or below,
not just below, because an equal serial carrying different days is exactly the
substitution the number exists to stop.

**No timestamps.** Day numbers are arithmetic.

**One schedule for every game.** Each entry names its family, the same way there is one
withdrawal list and one buffer for both (ADR 0015). A schedule per game would be another
file, another serial number and another fetch for every family added.

**Exactly one byte string per calendar.** Entries are sorted and no day may name the same
game twice. Refused when writing *and* when reading: the first binds this code, the
second binds a file written by anything else. Without the second, a device could hold a
calendar with two answers for one day and silently take whichever came first.

## What was considered and not done

**A date in each entry instead of a day number.** Simpler to read, and it would have put
a calendar inside the file that the reader must agree with. Day numbers need one agreed
start and plain addition.

**Signing each day separately.** It would let a day be published without re-signing the
calendar, but there would be no way to tell a missing day from a withheld one, and the
replay defence would need a serial number per day.

**Letting the device fetch "today" from the server.** That is a program on the server,
and then the server decides what is true, which is the thing this platform is built not
to need.

## The consequence worth stating

The archive is bounded by the day the caller asks about, not by the end of the file.
Days are published ahead, so a schedule almost always holds days that have not arrived.
A reader that listed them would hand a player tomorrow's puzzle today, which for a daily
puzzle spoils the only thing it has. The one call that deliberately looks forward is
`Schedule::after`, which is what the offline buffer downloads from, and what it
downloads is saved rather than shown.

## Revisit when

A game wants more than one puzzle a day, or days that are not days — a weekly, say.
Both would change the key from `(day, family)` to something else, and the
one-per-game-per-day rule with it.
