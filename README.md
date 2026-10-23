# GatedSpark

A platform for building original digital puzzles: AI proposes candidates, formal verification
decides what can be trusted, and deterministic software delivers the result reliably, online or
offline.

The verification layer is the trust boundary. LLM output is untrusted input and is never evidence
of correctness, uniqueness, security or difficulty. `SAT`, `UNSAT` and `UNKNOWN`/timeout stay
distinct outcomes, and `UNKNOWN` never counts as a pass.

## Where things are

| | |
|---|---|
| `crates/gs-core` | The shared deterministic core: the puzzle DSL, logical state, transition semantics, the branching timeline, replay, and the platform's verification vocabulary. |
| `crates/gs-nonogram` | The launch puzzle family: clues, rules, the line solver, the difficulty feature vector, and the SMT-LIB2 encoding a real solver reads. |
| `crates/gs-slitherlink` | The second puzzle family: the board's three index spaces, the one-closed-loop encoding, and generation by taking numbers away while the answer stays the only one. |
| `crates/gs-crypto` | The workspace's only cryptography: SHA-256 for the content fingerprint, Ed25519 for the signature. |
| `crates/gs-solver` | Runs a real SMT solver as a separate program and turns every failure into "nobody knows". |
| `crates/gs-store` | The puzzle store: a plain folder of plain files, one folder per puzzle, named by the puzzle's fingerprint. |
| `crates/gs-cli` | The `gatedspark` command: make a key, fill a store, read a report, re-check the lot, schedule the days, and publish a folder a web host can serve. |
| `crates/gs-offline` | Puzzles kept on a device, so that losing signal does not end the game. No files, no network, and no idea what game it is holding. |
| `crates/gs-web` | The browser's half: open a signed puzzle and refuse everything else. It cannot reach a solver or a folder, and a test holds that. |
| `docs/adr/` | Architecture decision records, including the language, solver and toolchain choices. |
| `scripts/` | Setup for Windows, building the core for a browser, filling the page with real puzzles, and driving the page in a real browser. |

The project's foundation documents, the build plan for this slice, and the content-delivery design
live in the project's shared folder rather than in the repository.

## Current state: both games are finished, and stage 5 has begun

M0 was the repository and its test harness. M1 is the deterministic core: the DSL, logical state,
`S(t+1) = F(S(t), A(t), R)`, state hashing, undo and redo with branching, and replay against
recorded hashes. M2 is Nonogram: the clue model, the line solver, the difficulty feature vector,
the SMT-LIB2 encoding, and a CI job that runs the encoding past a real solver and fails if the
solver and the line solver disagree.

The launch puzzle family is **Nonogram**, with **Slitherlink** as family two. The DSL already
reserves the Tier B global constraints Slitherlink needs, because freezing the DSL around local
constraints is the one decision here that would be expensive to reverse.

M3 (generation) and most of M4 now exist: a pattern generator feeds mechanical clue derivation,
the solver decides what is accepted, every accepted puzzle carries a report of the questions a
solver was asked and what it answered, and that report packs into a file a device can open and play
from. The generator is never a correctness oracle.

M4 is now complete. The two pieces that were missing — the content fingerprint and the signature —
are real: SHA-256 for the fingerprint and Ed25519 for the signature, both from reviewed libraries,
both in `gs-crypto`, which is the only crate in the workspace that contains any cryptography. The
core still has no dependencies and still cannot sign anything, which is what keeps its own tests
from checking a signature against a signer they could have produced. The reasoning, and what was
weighed against what, is in
[`docs/adr/0006-content-id-and-signature.md`](docs/adr/0006-content-id-and-signature.md).

Because the fingerprint is real, the tests around the package are claims rather than wiring checks.
Every single bit of a published file is flipped, one at a time, and the file must be refused each
time. A genuinely signed file from a key the reader was not told to trust is refused. And
`gs_nonogram::publish::publish` closes the loop the slice exists to demonstrate: generate, ask a
solver, write the evidence, seal a file only if the evidence says yes, and bind the report to the
file that was actually written.

What exists and is enforced by a test is listed in
[`docs/adr/0003-engine-game-boundary.md`](docs/adr/0003-engine-game-boundary.md). Nothing in this
repository claims to be enforced without naming the test that enforces it; where something is not
yet enforced, it says so.

M5 is built. Five pieces: the solver runner, the store and the command, the browser crate, the page,
and the offline buffer.

Piece one made a solver answerable from
shipped code rather than from a test file: `gs-solver` runs a solver as a separate program and
turns *every* failure — a solver that will not start, dies, runs out of time, prints nothing,
prints something unparseable, or prints two contradictory answers — into "nobody knows", never into
"the puzzle is unique". The reasoning is in
[`docs/adr/0007-running-a-real-solver.md`](docs/adr/0007-running-a-real-solver.md).

Piece two is the store and the command, described in the next section. Piece three carries the trust
chain into a browser. Piece four is the page a player actually taps on. Piece five keeps puzzles on
the device so that losing signal does not end the game. All are described below.

## One command, end to end

M5's point is a single sentence that can be run: an empty folder becomes signed, solver-verified,
playable puzzles, and a second command can check them later.

```bash
# A development signing key. It refuses to write inside a Git working tree.
cargo run -p gs-cli -- keygen --out ~/gatedspark-dev.key

# Generate, ask Z3 about each candidate, and keep only the ones it vouches for.
cargo run -p gs-cli -- fill --store ./store --key ~/gatedspark-dev.key --count 2

# Read one puzzle's report, out of the signed file itself.
cargo run -p gs-cli -- show --store ./store --key ~/gatedspark-dev.key <puzzle name>

# Re-open every puzzle: recompute its fingerprint, check its signature, and check its
# stored answer against the fingerprint inside the signed report.
cargo run -p gs-cli -- recheck --store ./store --key ~/gatedspark-dev.key
```

A store looks like this, and nothing but the puzzle's own fingerprint names a puzzle:

```text
store/
  9890887818e3d290e6a58b4b06b159b6452dd90473a34b5cc4a0e2fddb974813/
    package.gspk      the signed package: the puzzle, its report, its signature
    witness.bin       the answer, in the form the game agrees on
```

Three refusals are worth knowing about, because they are the reason the commands can be believed:

- **`fill` checks the solver before it starts.** On a machine with no solver it says so, in those
  words, instead of running for a long time and reporting every puzzle refused for an undecided
  query. Nothing can be published without a solver, and nothing here pretends otherwise.
- **`fill` says when it came up short.** A level it could not fill prints the shortfall rather than
  quietly delivering fewer puzzles than asked.
- **`recheck` prints how many it checked, first and always.** An empty store is told it is empty.
  A run that printed only "every signature checks out" would read as a clean bill of health for an
  empty directory, which is exactly the shape of a check that passes while checking nothing.

The store and the command, and what each refusal is for, are in
[`docs/adr/0008-the-store-and-the-command.md`](docs/adr/0008-the-store-and-the-command.md).

## The browser

Piece three of M5 is `gs-web`: the core compiled to WebAssembly, so the same verified code can run
on the web now and on a desktop later without being rewritten. It loads a puzzle file that
`gatedspark fill` wrote, **checks the signature itself** against a key the page ships with, and
refuses everything else. The compressed download is 62 KB.

A puzzle is also playable: marking squares, undo and redo, and knowing when it is solved. And there
is now a page to play it on, below. What is still missing is the offline buffer.

The rule that keeps playing honest is that **nothing in the browser decides whether a board is
finished.** It asks `Nonogram::is_solved`, which checks the clues rather than a remembered answer,
and it asks every single time with no cached flag. A page that worked it out for itself would be a
second answer to that question, and the one the player saw would eventually be the wrong one.

The thing worth knowing about it came from a measurement taken before any of it was designed:
**every crate in this workspace compiles for a browser, including the two that must never be in
one.** `gs-store` reads directories and `gs-solver` starts programs, and a browser can do neither —
but on that target those calls exist and simply do not work, so a browser built carelessly would
not crash. The solver runner turns every failure into "nobody knows", the pipeline correctly refuses
to treat that as a proof, and the result is a browser in which **no puzzle is ever provably
unique**. Every puzzle rejected, nothing visibly broken, and the cause three crates down a
dependency list.

So the dependency list is the boundary and a test holds it, rather than a note in a document:

```text
cargo test -p gs-web --test boundary
```

It walks the manifests, fails if either crate is compiled into `gs-web`, prints the path by which
it arrived, and checks that the crates it forbids still exist under those names — because a guard
naming a renamed crate passes while guarding nothing. Its first run failed for a reason worth
keeping: `gs-nonogram` names `gs-solver` among its *test* dependencies, which puts nothing into
anything that depends on `gs-nonogram`. The rule is now stated rather than assumed.

One consequence kept on purpose: **nothing in the browser can produce a random number.** The only
crate that wanted randomness was the key generator, and the browser neither generates puzzles nor
signs anything.

### Running the browser test

One test in `gs-web` runs inside a real browser, because "compiles for a browser" and "works in a
browser" are different claims. It needs three things that match each other:

```bash
cargo install wasm-bindgen-cli --version 0.2.129   # must match the wasm-bindgen dependency
rustup target add wasm32-unknown-unknown
CHROMEDRIVER=$(which chromedriver) \
    cargo test -p gs-web --target wasm32-unknown-unknown --test in_a_real_browser
```

ChromeDriver's version has to match the browser's, and `wasm-bindgen-cli`'s has to match the
`wasm-bindgen` dependency. Both mismatches read as a broken test rather than a setup problem, which
is why CI prints all three versions before running anything.

It is one file and deliberately not how this crate is tested: a test that needs a browser is a test
that will eventually be skipped, so the native suite stays the measurement. This covers the narrow
claim no native test can make — that the translation layer really carries data in and out, and that
real SHA-256 and real Ed25519 give the same answers in a browser's WebAssembly engine.

The reasoning for all of it is in
[`docs/adr/0009-the-browser-version.md`](docs/adr/0009-the-browser-version.md).


## The page

`crates/gs-web/www/index.html` is a board you can tap: the clue numbers along the top and the left,
a grid, one tap to fill a square, another to cross it out, another to clear it, undo and redo, a
count of moves, and a line that appears when the puzzle is solved. Underneath it, the page shows
what it checked — who signed the puzzle, its level, its fingerprint, and what "one answer only" was
proved relative to.

**The page decides nothing.** It asks the core on every change whether the board is finished, and
draws whatever it is told. A page that counted the squares itself would be a second answer to that
question, and one of the two answers would eventually be wrong.

To see it:

```text
scripts/build-web.sh    # compiles the core for a browser
scripts/demo-web.sh     # makes a development key and fills the page with real puzzles
python3 -m http.server 8080 --directory crates/gs-web/www
```

Then open `http://127.0.0.1:8080/`, or `?puzzle=<name>` for one in particular. A puzzle's name is
its own fingerprint, which is why a link to one needs no index to resolve it.

### Driving the real page

```text
scripts/check-page.sh
```

Opens the page in a real browser, plays a whole puzzle to the end, and takes pictures along the way.
It checks that a grid is drawn, that taps and undo do what they say, that the solved message appears
when the rules say the board is finished **and not when one square is changed**, that switching
puzzles forgets the old game, and that a package with one byte altered is refused in a sentence a
person can read. It is not in CI: it needs a browser-driving dependency this project has not taken
on, and that is a decision worth making on purpose. Run it by hand before changing the page.

One thing it found, which was a question rather than a bug: finishing a 15 by 15 puzzle used to take
**372 taps** for 93 filled squares, because a board counted as finished only when every blank had
been crossed out too. On 2026-10-23 that rule was changed, by decision: a puzzle is finished when
its **filled** squares match the clues, and crossing out is a note the player keeps for themselves.
The same puzzle now takes 93 taps. Missing a filled square still leaves it unfinished, and a
*stored* answer must still decide every square — `publish::replay_of` requires that separately. See
ADR 0010.


## Which puzzle is today's

A daily puzzle needs a notion of "today", and this project forbids letting the reader's
clock decide whether content can be trusted. The schedule settles that: it maps a day
**number** to a puzzle, counting from a start day written inside the signed file. A
device turns today's date into a day number and uses the clock for nothing else.

So a wrong clock shows the wrong day's puzzle. It cannot make a bad puzzle look good,
revive a withdrawn one, or invent a day nobody signed.

The file is built exactly like the withdrawal list, because it has the same problems:
its own magic bytes **inside** what the signature covers, so a signature over a puzzle
can never be lifted onto a schedule; a serial number that only goes up, because
replaying last month's genuine schedule needs no key at all; and no timestamps anywhere.
Entries are sorted with one puzzle per game per day, refused on the way in *and* on the
way out, so a given calendar has exactly one possible encoding.

```bash
# Give every game another week. Run it again and it extends rather than restarting.
cargo run -p gs-cli -- schedule --store ./store --key ~/gatedspark-dev.key     --start 2026-10-24 --days 7

# Write a folder any web host can serve.
cargo run -p gs-cli -- publish --store ./store --key ~/gatedspark-dev.key     --out ./site --page crates/gs-web/www
```

Five refusals, each of them a day that would otherwise break for every player at once:
the start day cannot be moved once signed (it would renumber every published day), two
puzzles cannot share a day for one game, a withdrawn puzzle is never scheduled, a
schedule number must go up, and asking for more days than there are puzzles prints the
shortfall instead of quietly delivering fewer.

**`publish` leaves the answers behind.** A store keeps the filled-in grid beside each
puzzle so nothing has to solve it twice; uploading that folder as it stands would
publish every answer next to every puzzle. Only the signed package is copied, and
`publishing_leaves_every_answer_behind` holds it.

There is no program on the server, on purpose. Every check that matters already happens
on the device: the page checks each package's signature itself, and the schedule and the
withdrawal list the same way. A server that only handed over files would be a server to
run, pay for, patch and have go down, in exchange for nothing.

## Playing with no signal

`gs-offline` is what a device keeps so that losing signal does not end the game. Three rules, agreed
before it was written:

- **Five ahead.** The device holds five unplayed puzzles, downloaded in advance.
- **Delivered is not played.** Bytes arriving is not a player playing. Downloading five leaves five;
  the count drops when somebody actually finishes one. Counting a download as a play would quietly
  burn puzzles nobody opened.
- **One bookmark in a shuffled order.** Instead of a record of every puzzle a player has finished,
  one number saying how far along their own shuffled list they are. One number per level rather than
  one row per puzzle played, which keeps the one thing that *does* grow with the number of players
  from growing without bound.

**The crate knows about no game and no device.** It decides how many to hold, which is next, and what
counts as played. Where the bytes live is four operations — put, get, remove, list — supplied by
whatever is hosting it, because a browser has no folders and the crate in this workspace that does
use folders is the one a browser is forbidden from carrying. It does not know about Nonogram either:
every package in its own tests is sealed with a family that does not exist, carrying a payload that
is not a puzzle, and the buffer handles all of them without noticing.

**Everything is checked on the way out, every time.** A device's storage is not trusted. In a browser
it is `localStorage`, a few clicks from being edited by hand in the developer tools, so a saved
puzzle is checked against the trusted key as it leaves, exactly as one off the network is. There is
a test that plants an edited puzzle in a real browser's storage and requires it to be refused.

**Reading never deletes.** Anything that fails its checks is skipped, counted and left alone.
Throwing it away is a separate call somebody makes on purpose: a puzzle that fails today because the
device's storage is misbehaving would be gone forever if reading deleted it, and a player with no
signal would have nothing left.

**The order only ever grows at the end.** New puzzles join the end of a player's shuffled list and
nothing already behind the bookmark moves. If the order could be reshuffled, a bookmark part way
along would point at a different puzzle tomorrow than it did today.

On the page there is a "Save this one for later" button, a "Play a saved one" button, a line saying
how many are saved and how many more there is room for, and a switch that pretends the signal is
gone so the whole thing can be tried without unplugging anything. Running out says so in a sentence:
*"There are no more puzzles saved on this device. Connect to the internet to get some more."*

What is **not** here, and is the last remaining hole in the offline story: withdrawing a puzzle that
has already been downloaded. Throwing away damaged files is covered; telling a device to stop playing
a perfectly signed puzzle is not, and needs an agreed way of publishing "do not play these". ADR 0011
records it.

## Building

```sh
cargo test --workspace     # everything
cargo clippy --all-targets # floating-point arithmetic in the core is a build error
```

The toolchain is pinned in `rust-toolchain.toml`. Building needs the package registry
(`index.crates.io`) for the three crates `gs-crypto` depends on. In an environment whose network
policy blocks `static.rust-lang.org`, prefix commands with `RUSTUP_TOOLCHAIN=stable`; see
[`docs/adr/0004-dependencies.md`](docs/adr/0004-dependencies.md).

M5 has started. `gs-solver` is the first piece: the code that actually runs a solver, as a separate
program speaking SMT-LIB2. Its one rule is that **every failure is an `UNKNOWN`** — a solver that
will not start, dies, times out, or prints something unreadable is "nobody knows", never a
refutation — because `UNSAT` on the uniqueness query is the one claim this whole platform makes. See
[`docs/adr/0007-running-a-real-solver.md`](docs/adr/0007-running-a-real-solver.md). The test that
compares our encoding against a real solver now drives that code rather than a copy of it, so CI's
one outside-the-building job checks what ships.

### The solver check is skipped unless a solver is installed

`cargo test --workspace` compares this code against this code. Exactly one test compares it against
an outside program: `solver_agreement`, which asks a real SMT solver whether it agrees with our line
solver. With no solver on the PATH — the list of folders your system searches when you type a
command — that test prints a notice saying it verified nothing and returns. Rust hides whatever a
*passing* test prints, so what you see is a bare `ok` that checked nothing.

To see the notice, or a real verification:

```sh
cargo test -p gs-nonogram --test solver_agreement -- --nocapture
```

A real run prints `verifying against N solver(s)`. If that line is absent, the check did not happen.
CI does not depend on remembering any of this: the `solver` job installs Z3 and sets
`GATEDSPARK_REQUIRE_SOLVER=1`, under which a missing solver fails the test instead of skipping it.

### On Windows, run the setup script once

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-windows.ps1
```

After that, the commands above work as written, with no extra flags, and so do plain `cargo` runs
from an editor. It is safe to run the script more than once.

The one thing to know before running it: it changes this machine's **default** Rust host, so other
Rust projects on the same machine also start building with the `gnu` toolchain. That is reversible
with `rustup set default-host x86_64-pc-windows-msvc`.

Why it is needed: a *linker* is the last step of a build, the part that joins the compiled pieces
into one program you can run. Rust's default Windows setup borrows that linker from Visual Studio,
so on a machine without Visual Studio the build stops with ``linker `link.exe` not found`` before any
test runs. The script switches the machine to Rust's `gnu` toolchain, which brings its own linker,
and installs the version named in `rust-toolchain.toml`. The host is deliberately **not** pinned in
that file, because pinning it would break Linux and the Apple and Android targets; see
[`docs/adr/0005-windows-gnu-toolchain.md`](docs/adr/0005-windows-gnu-toolchain.md).
