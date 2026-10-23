// Drives the real page the way a person does: open it, look at it, lay the answer in,
// and check that the page says "solved" only because the core said so.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';

// Playwright is not a dependency of this project. It is whatever copy is installed on
// the machine running this, which is why this is a by-hand check and not a CI job.
const where = process.env.PLAYWRIGHT_PATH ?? 'playwright';
const loaded = await import(where).catch(() =>
  import(createRequire(`${process.cwd()}/`).resolve(where))
);
// Playwright ships as both shapes depending on how it was installed, so take whichever
// one this copy turned out to be rather than assuming.
const chromium = loaded.chromium ?? loaded.default?.chromium;
if (!chromium) throw new Error(`no Chromium in the Playwright at ${where}`);

const OUT = process.argv[2];
const WWW = process.argv[3];
const URL = process.argv[4] ?? 'http://127.0.0.1:8099/';
// The puzzle check-page.sh has published a withdrawal list for. The list is on the
// server from the start; this device knows nothing about it until the page asks.
const DOOMED = process.argv[5];
// The loop puzzle, driven at the end. A different game on the same page, from the same
// store, checked by the same key.
const LOOP = process.argv[6];

/** The witness file is a canonical board: width, height, count, then one i16 per cell. */
function filledSquares(path) {
  const bytes = readFileSync(path);
  const width = bytes.readUInt16LE(0);
  const height = bytes.readUInt16LE(2);
  const count = bytes.readUInt32LE(4);
  if (count !== width * height) throw new Error(`witness header: ${width}x${height} but ${count} cells`);
  const filled = [];
  const empty = [];
  for (let i = 0; i < count; i += 1) {
    const where = [Math.floor(i / width), i % width];
    (bytes.readInt16LE(8 + i * 2) === 1 ? filled : empty).push(where);
  }
  return { width, height, filled, empty };
}

/** For the loop game the witness is one cell per line, in the order the core reports
 *  the lines, so the answer is just which of them are drawn. */
function drawnLines(path) {
  const bytes = readFileSync(path);
  const count = bytes.readUInt32LE(4);
  const drawn = [];
  for (let i = 0; i < count; i += 1) {
    if (bytes.readInt16LE(8 + i * 2) === 1) drawn.push(i);
  }
  return { count, drawn };
}

/** Every puzzle in the store, by name.
 *
 *  Read off the disk rather than out of the page. The page used to carry a list of
 *  names; since the schedule arrived it carries none on purpose — it knows about today
 *  and the days before it, and nothing else. A puzzle's name is its own fingerprint, so
 *  `window.gatedspark.open(name)` still works for any of them, which is how a link to
 *  one puzzle works and how these checks reach a puzzle that is not on the calendar. */
function everyPuzzle(www) {
  return readdirSync(`${www}/store`)
    .filter((name) => name.length === 64 && statSync(`${www}/store/${name}`).isDirectory())
    .sort();
}

// Playwright normally launches the browser it downloaded for itself. On a machine where
// the browser came from somewhere else — a container that ships one, a distribution
// package — point this at it. Same reasoning as PLAYWRIGHT_PATH above: this is a by-hand
// check, so it uses whatever that machine happens to have.
const executablePath = process.env.PLAYWRIGHT_CHROMIUM || undefined;
const browser = await chromium.launch(executablePath ? { executablePath } : {});
const page = await browser.newPage({ viewport: { width: 430, height: 940 } });

const problems = [];
page.on('console', (m) => { if (m.type() === 'error') problems.push(m.text()); });
page.on('pageerror', (e) => problems.push(String(e)));

const fail = async (why, shot) => {
  console.log('FAIL: ' + why);
  await page.screenshot({ path: `${OUT}/${shot}.png`, fullPage: true }).catch(() => {});
  await browser.close();
  process.exit(1);
};

/** Lay the stored answer into whatever game is on the screen.
 *
 *  The answer comes out of the witness file beside the package — the one the solver
 *  vouched for and the store checked — rather than being worked out here. */
async function layTheAnswerIn(fingerprint) {
  const witness = `${WWW}/store/${fingerprint}/witness.bin`;
  const family = await page.evaluate(() => window.gatedspark.family());
  if (family === 'slitherlink') {
    const { count, drawn } = drawnLines(witness);
    const laid = await page.evaluate((want) => {
      const places = window.gatedspark.linePlaces();
      if (places.length / 3 !== want.count) {
        return `the page has ${places.length / 3} lines and the answer has ${want.count}`;
      }
      for (const i of want.drawn) {
        window.gatedspark.tapLine(places[i * 3], places[i * 3 + 1], places[i * 3 + 2]);
      }
      return null;
    }, { count, drawn });
    if (laid) await fail(laid, 'loop-witness');
  } else {
    const answer = filledSquares(witness);
    await page.evaluate((filled) => {
      for (const [row, column] of filled) window.gatedspark.tap(row, column);
    }, answer.filled);
  }
}

await page.goto(URL, { waitUntil: 'networkidle' });
if (await page.locator('#refused').isVisible()) {
  await fail('the page refused the puzzle: ' + (await page.locator('#refused').textContent()), 'refused');
}
await page.waitForSelector('#game:not([hidden])', { timeout: 20000 });

const details = await page.evaluate(() => ({
  signedBy: document.getElementById('signed-by').textContent,
  level: document.getElementById('level').textContent,
  fingerprint: document.getElementById('fingerprint').textContent,
  encoding: document.getElementById('encoding').textContent,
  squares: document.querySelectorAll('td.square').length,
  colClues: document.querySelectorAll('th.col-clue').length,
  rowClues: document.querySelectorAll('th.row-clue').length,
  solvedShowing: !document.getElementById('solved').hidden,
  moves: document.getElementById('moves').textContent,
  games: document.getElementById('game-pick').options.length,
  days: [...document.getElementById('day-pick').options].map((o) => o.textContent),
  name: document.getElementById('day-pick').value,
  todayLine: document.getElementById('today-line').textContent,
}));
console.log('details ' + JSON.stringify(details));
if (details.squares === 0) await fail('no grid was drawn', 'empty');
// The page opened today's puzzle with nothing in the address bar, which is the whole
// point of the schedule: a visitor gets a puzzle without being handed a name.
if (details.games < 1) await fail('the page offered no games, so it has no schedule', 'no-schedule');
if (details.days.length < 1) await fail('the day picker is empty, so there is no puzzle for today', 'no-today');
if (!details.days[0].endsWith('(today)')) await fail('the first day offered is not today: ' + details.days[0], 'not-today');
if (!/^Today is \d{4}-\d{2}-\d{2}\./.test(details.todayLine)) await fail('the page does not say what today is: ' + details.todayLine, 'no-today-line');
// And never a day that has not arrived. Days are published ahead, so the schedule holds
// future days; listing one would hand a player tomorrow's puzzle today.
{
  const today = new Date();
  const asText = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const latest = details.days[0].slice(0, 10);
  if (latest > asText(today)) await fail('the day picker offered a day that has not arrived: ' + latest, 'future-day');
}
if (details.colClues !== details.rowClues) await fail('clue counts disagree with a square board', 'clues');
if (details.solvedShowing) await fail('it said solved before a single tap', 'wrongly-solved');
if (!details.fingerprint.startsWith(details.name.slice(0, 8))) await fail('the fingerprint shown is not this puzzle', 'wrong-id');

// ---------------------------------------------------------------------------
// Hints, on a blank picture board
// ---------------------------------------------------------------------------
//
// Driven before anything is tapped, because that is when a player needs one. Three
// things are checked that only the real page can show: that the hint arrives at all in
// a browser, that what it names agrees with the signed answer sitting beside the
// package, and that it is announced and reachable without a mouse.
//
// The agreement check is the one that matters. The page has no answer in it; the hint is
// worked out from the clue numbers alone. If it ever disagrees with the answer the
// solver vouched for, the hint is lying to the player and this stops the build.
{
  const stored = filledSquares(`${WWW}/store/${details.name}/witness.bin`);
  const before = await page.evaluate(() => ({
    offered: window.gatedspark.hintOffered(),
    left: window.gatedspark.hintsLeftSaid(),
    said: window.gatedspark.hintSaid(),
    announced: window.gatedspark.hintIsAnnounced(),
  }));
  console.log('before any hint ' + JSON.stringify(before));
  if (!before.offered) await fail('the hint button is not offered on a fresh puzzle', 'hints');
  if (!/3 hints left/.test(before.left)) await fail('a fresh puzzle should offer three hints: ' + before.left, 'hints');
  if (before.said !== null) await fail('a hint was on the screen before one was asked for', 'hints');
  if (!before.announced) await fail('a hint is not announced, so a screen reader never reads it out', 'hints');

  const first = await page.evaluate(() => {
    window.gatedspark.hint();
    return {
      said: window.gatedspark.hintSaid(),
      at: window.gatedspark.hintedSquare(),
      left: window.gatedspark.hintsLeftSaid(),
      focused: window.gatedspark.focusIsOnTheHint(),
      moves: window.gatedspark.movesMade(),
      solved: window.gatedspark.isSolved(),
    };
  });
  console.log('first hint ' + JSON.stringify(first));
  if (!first.said) await fail('the hint button said nothing', 'hints');
  if (!/because/.test(first.said)) await fail('a hint should say why: ' + first.said, 'hints');
  if (/[a-z][A-Z]|_/.test(first.said)) await fail('a hint is showing a code name: ' + first.said, 'hints');
  if (!first.at) await fail('the hint named no square to look at', 'hints');
  if (!/2 hints left/.test(first.left)) await fail('the hint was not counted: ' + first.left, 'hints');
  if (!first.focused) await fail('the keyboard was not moved to the hinted square', 'hints');
  if (first.moves !== 0) await fail('asking for a hint counted as a move', 'hints');
  if (first.solved) await fail('a hint solved the puzzle, which is not what a hint is for', 'hints');

  // The square it named is filled in the signed answer, or crossed out in it, and the
  // sentence says which. Either is a true hint; naming the wrong one is not.
  const [row, column] = first.at;
  const inTheAnswer = stored.filled.some(([r, c]) => r === row && c === column);
  const saysFilled = /has to be filled/.test(first.said);
  const saysBlank = /has to be blank/.test(first.said);
  if (!saysFilled && !saysBlank) {
    await fail('the hint named a square without saying what it is: ' + first.said, 'hints');
  }
  if (saysFilled !== inTheAnswer) {
    await fail(
      `the hint says row ${row + 1}, column ${column + 1} ${saysFilled ? 'is filled' : 'is blank'} ` +
        `and the signed answer says otherwise`,
      'hints'
    );
  }
  await page.screenshot({ path: OUT + '/hint.png', fullPage: true });

  // Tapping Hint again without having marked anything is free: the board has not
  // changed, so there is nothing new to say and nothing to charge for.
  const twice = await page.evaluate(() => {
    window.gatedspark.hint();
    return { said: window.gatedspark.hintSaid(), left: window.gatedspark.hintsLeftSaid() };
  });
  console.log('the same hint again ' + JSON.stringify(twice));
  if (twice.said !== first.said) await fail('asking again said something else: ' + twice.said, 'hints');
  if (!/2 hints left/.test(twice.left)) await fail('a repeated hint was charged for: ' + twice.left, 'hints');

  // Marking a square takes the ring off, so the player is not left looking at an
  // answered hint. The sentence stays, because it is worth reading twice.
  const afterMarking = await page.evaluate(() => {
    const at = window.gatedspark.hintedSquare();
    if (!at) return { skipped: true };
    window.gatedspark.tap(at[0], at[1]);
    const still = { at: window.gatedspark.hintedSquare(), said: window.gatedspark.hintSaid() };
    window.gatedspark.undo();
    return still;
  });
  console.log('after marking the hinted square ' + JSON.stringify(afterMarking));
  if (afterMarking.skipped) await fail('no square was ringed to mark', 'hints');
  if (afterMarking.at) await fail('the hint ring stayed on a square the player has answered', 'hints');
  if (!afterMarking.said) await fail('the hint sentence vanished as soon as the player moved', 'hints');

  // Three and no more. Each one is acted on, so each is a new hint rather than the same
  // sentence again, and the button goes away when the allowance is gone.
  const spent = await page.evaluate(() => {
    const act = () => {
      const said = window.gatedspark.hintSaid();
      const at = window.gatedspark.hintedSquare();
      if (!at) return;
      // Tap round to what the hint said: once for filled, twice for crossed out.
      window.gatedspark.tap(at[0], at[1]);
      if (/has to be blank/.test(said)) window.gatedspark.tap(at[0], at[1]);
    };
    window.gatedspark.hint(); // the ring back after the undo above, free
    act();
    window.gatedspark.hint();
    act();
    window.gatedspark.hint();
    act();
    return {
      left: window.gatedspark.hintsLeftSaid(),
      offered: window.gatedspark.hintOffered(),
      moves: window.gatedspark.movesMade(),
    };
  });
  console.log('after three hints ' + JSON.stringify(spent));
  if (!/No hints left/.test(spent.left)) await fail('the count did not run down to nothing: ' + spent.left, 'hints');
  if (spent.offered) await fail('the hint button is still offered with no hints left', 'hints');
  if (spent.moves < 3) await fail('doing what three hints said should be at least three moves, not ' + spent.moves, 'hints');

  // Put the board back as it was found. The checks below this one start from an
  // untouched board on purpose — the keyboard check needs a square it can call blank —
  // and hints are help, so taking the help back has to leave no trace.
  const cleared = await page.evaluate(() => {
    while (window.gatedspark.movesMade() > 0) window.gatedspark.undo();
    return { moves: window.gatedspark.movesMade(), ringed: window.gatedspark.hintedSquare() };
  });
  if (cleared.moves !== 0) await fail('the hint checks left marks on the board', 'hints');
  if (cleared.ringed) await fail('a hint ring was left on the board', 'hints');
}

// A few taps, so the picture shows a part-played board rather than an empty one, and so
// the three states each appear.
await page.evaluate(() => {
  for (const [r, c] of [[0, 0], [1, 1], [2, 3], [3, 0], [4, 4]]) window.gatedspark.tap(r, c);
  window.gatedspark.tap(1, 1); // a second tap: filled becomes crossed out
});
// The far corner, which none of the taps above reach. Worked out from the board rather
// than written down: the demo's first puzzle is whatever level comes first, and a square
// that is off the edge reads as nothing at all rather than as undecided.
const corner = [details.rowClues - 1, details.colClues - 1];
const afterTaps = await page.evaluate((far) => ({
  moves: document.getElementById('moves').textContent,
  filled: window.gatedspark.boardAt(0, 0),
  crossed: window.gatedspark.boardAt(1, 1),
  untouched: window.gatedspark.boardAt(far[0], far[1]),
  solved: window.gatedspark.isSolved(),
}), corner);
console.log('after taps ' + JSON.stringify(afterTaps));
if (afterTaps.filled !== 0) await fail('a tap did not fill the square', 'tap');
if (afterTaps.crossed !== 1) await fail('a second tap did not cross it out', 'tap');
if (afterTaps.untouched !== 2) await fail('an untapped square was not left undecided', 'tap');
if (afterTaps.solved) await fail('six taps cannot solve a puzzle', 'tap');
await page.screenshot({ path: OUT + '/part-played.png', fullPage: true });

// Playing with the keyboard alone, and listening rather than looking.
//
// This is the half of accessibility a screenshot cannot show and the test in
// crates/gs-web/tests/usable_without_a_mouse.rs cannot reach: that one proves the page
// still *says* the right things, this one proves they still *work*. The plan put
// accessibility on the list of things that must not be cut, and it was cut once
// already, so it is checked here with the real keyboard on the real page.
//
// No mouse is used below. Focus is moved with Tab and the square is pressed with Enter.
// A square the taps above did not reach, so "blank" below means blank rather than
// "whatever the last test left here". Worked out from the board, like `corner` is.
const beforeKeys = await page.evaluate((far) => {
  const first = document.querySelector(`td.square[data-row="${far[0]}"][data-column="0"]`);
  first.focus();
  return { name: first.getAttribute('aria-label'), focused: document.activeElement === first };
}, corner);
if (!beforeKeys.focused) await fail('a square cannot take focus at all', 'keyboard');
if (!/blank/.test(beforeKeys.name)) {
  await fail('an untouched square does not say it is blank: ' + beforeKeys.name, 'keyboard');
}

// Tab must reach the next square, not stop at the first. A board where only one cell
// is in the tab order is a board that cannot be played through.
await page.keyboard.press('Tab');
const moved = await page.evaluate(() => ({
  onASquare: document.activeElement?.classList.contains('square') ?? false,
  name: document.activeElement?.getAttribute('aria-label') ?? null,
  mark: Number(document.activeElement?.dataset.mark),
}));
if (!moved.onASquare) {
  await fail('Tab does not move from one square to the next: ' + JSON.stringify(moved), 'keyboard');
}
if (moved.name === beforeKeys.name) await fail('Tab landed back on the same square', 'keyboard');

// Enter must mark it, and the square must then say so out loud. A square that changes
// colour and keeps its old name has told somebody listening nothing at all.
await page.keyboard.press('Enter');
const afterKeys = await page.evaluate(() => {
  const here = document.activeElement;
  return {
    name: here.getAttribute('aria-label'),
    mark: Number(here.dataset.mark),
  };
});
console.log('after a key press ' + JSON.stringify(afterKeys));
if (afterKeys.mark === moved.mark) {
  await fail('Enter did not change the square the keyboard was on', 'keyboard');
}
if (!/filled/.test(afterKeys.name)) {
  await fail('a filled square still calls itself something else: ' + afterKeys.name, 'keyboard');
}

// And the name must follow an undo, which changes a square nobody touched.
await page.evaluate(() => window.gatedspark.undo());
const afterUndoName = await page.evaluate(() =>
  document.activeElement.getAttribute('aria-label')
);
if (!/blank/.test(afterUndoName)) {
  await fail('after undo the square still calls itself filled: ' + afterUndoName, 'keyboard');
}

// Undo and redo, checked by looking at the square rather than at the button.
await page.evaluate(() => window.gatedspark.undo());
if (await page.evaluate(() => window.gatedspark.boardAt(1, 1)) !== 0) await fail('undo did not put the square back', 'undo');
await page.evaluate(() => window.gatedspark.redo());
if (await page.evaluate(() => window.gatedspark.boardAt(1, 1)) !== 1) await fail('redo did not restore the mark', 'redo');

// Now lay the whole answer in. The answer comes from the witness file beside the
// puzzle, read here rather than by the page: the page has no answer and must not.
const answer = filledSquares(`${WWW}/store/${details.name}/witness.bin`);
console.log(`answer ${answer.width}x${answer.height}, ${answer.filled.length} filled, ${answer.empty.length} blank`);
if (answer.width * answer.height !== details.squares) await fail('the drawn grid is not the size the answer is', 'size');

await page.evaluate((answer) => {
  const w = window.gatedspark;
  // Clear every square first, by tapping it round to undecided.
  for (const td of document.querySelectorAll('td.square')) {
    const row = Number(td.dataset.row);
    const column = Number(td.dataset.column);
    while (Number(td.dataset.mark) !== 2) w.tap(row, column);
  }
  // One tap each, on the filled squares only. Crossing out the blanks is optional for
  // a player, so this is the shortest honest way to finish the puzzle.
  for (const [row, column] of answer.filled) w.tap(row, column);
}, answer);

const end = await page.evaluate(() => ({
  pageSaysSolved: !document.getElementById('solved').hidden,
  coreSaysSolved: window.gatedspark.isSolved(),
  banner: document.getElementById('solved').textContent.trim(),
  moves: document.getElementById('moves').textContent,
}));
console.log('end ' + JSON.stringify(end));
await page.screenshot({ path: OUT + '/solved.png', fullPage: true });
if (!end.coreSaysSolved) await fail('the real answer did not solve it', 'not-solved');
if (!end.pageSaysSolved) await fail('the core says solved and the page does not show it', 'not-shown');

// Crossing out the blanks is allowed, and must not unfinish the board.
await page.evaluate((empty) => {
  for (const [row, column] of empty) {
    window.gatedspark.tap(row, column); // undecided to filled
    window.gatedspark.tap(row, column); // filled to crossed out
  }
}, answer.empty);
const withCrosses = await page.evaluate(() => ({
  page: !document.getElementById('solved').hidden,
  core: window.gatedspark.isSolved(),
  moves: document.getElementById('moves').textContent,
}));
console.log('with every blank crossed out ' + JSON.stringify(withCrosses));
if (!withCrosses.core) await fail('crossing out the blanks unfinished the board', 'crosses');
for (const [row, column] of answer.empty) {
  await page.evaluate(([r, c]) => window.gatedspark.tap(r, c), [row, column]);
}
if (!await page.evaluate(() => window.gatedspark.isSolved())) {
  await fail('taking the crosses off again unfinished the board', 'crosses-off');
}

// One square away is not solved. This is the guard that makes the line above mean
// something: a page that always showed the banner would have passed up to here.
const [row, column] = answer.filled[0];
await page.evaluate(([r, c]) => window.gatedspark.tap(r, c), [row, column]);
const oneShort = await page.evaluate(() => ({
  page: !document.getElementById('solved').hidden,
  core: window.gatedspark.isSolved(),
}));
console.log('one square changed ' + JSON.stringify(oneShort));
if (oneShort.page || oneShort.core) await fail('still said solved with a square changed', 'still-solved');
await page.evaluate(() => window.gatedspark.undo());
if (!await page.evaluate(() => window.gatedspark.isSolved())) await fail('undo did not get back to solved', 'undo-solved');

// A second puzzle, to prove switching rebuilds the grid and forgets the old game.
const second = await page.evaluate(() => document.getElementById('day-pick').options[1]?.value);
if (second) {
  await page.selectOption('#day-pick', second);
  await page.waitForFunction((n) => document.getElementById('fingerprint').textContent.startsWith(n.slice(0, 8)), second, { timeout: 10000 });
  const switched = await page.evaluate(() => ({
    squares: document.querySelectorAll('td.square').length,
    solved: !document.getElementById('solved').hidden,
    moves: document.getElementById('moves').textContent,
    level: document.getElementById('level').textContent,
  }));
  console.log('switched ' + JSON.stringify(switched));
  if (switched.solved) await fail('a fresh puzzle came up already solved', 'fresh');
  if (switched.moves !== 'No moves yet') await fail('a fresh puzzle kept the old move count', 'fresh');
  await page.screenshot({ path: OUT + '/second.png', fullPage: true });
}

// And a puzzle the page must refuse: the same bytes with one changed.
await page.evaluate(async (n) => {
  const bytes = new Uint8Array(await (await fetch(`store/${n}/package.gspk`)).arrayBuffer());
  bytes[bytes.length - 1] ^= 1;
  await window.gatedspark.openBytes(bytes);
}, details.name);
const refusal = await page.evaluate(() => ({
  shown: !document.getElementById('refused').hidden,
  words: document.getElementById('refused').textContent.trim(),
  gameHidden: document.getElementById('game').hidden,
}));
console.log('changed byte ' + JSON.stringify(refusal));
await page.screenshot({ path: OUT + '/refused.png', fullPage: true });
if (!refusal.shown) await fail('a changed puzzle was not refused', 'not-refused');
if (/NotPlayable|Err\(/.test(refusal.words)) await fail('the refusal is not in words a person reads', 'jargon');

// ---------------------------------------------------------------------------
// The offline buffer, through the page
// ---------------------------------------------------------------------------

await page.evaluate(() => window.gatedspark.forgetEverythingSaved());
await page.reload({ waitUntil: 'networkidle' });
await page.waitForSelector('#game:not([hidden])', { timeout: 20000 });

const blank = await page.evaluate(() => window.gatedspark.savedLine());
console.log('saved at the start ' + JSON.stringify(blank));
if (!/No puzzles saved/.test(blank)) await fail('a fresh device should say it has nothing saved', 'saved-blank');
if (!/Room for 5 more/.test(blank)) await fail('it should know it has room for five', 'saved-blank');

// Save the one on screen, and two more.
const names = everyPuzzle(WWW);
for (const name of names.slice(0, 3)) {
  await page.evaluate((n) => window.gatedspark.open(n), name);
  await page.waitForFunction((n) => document.getElementById('fingerprint').textContent === n, name, { timeout: 10000 });
  await page.evaluate(() => window.gatedspark.save());
}
const afterSaving = await page.evaluate(() => window.gatedspark.savedLine());
console.log('after saving three ' + JSON.stringify(afterSaving));
if (!/3 puzzles saved/.test(afterSaving)) await fail('three saved puzzles should be reported', 'saved-three');
if (!/Room for 2 more/.test(afterSaving)) await fail('and room for two more', 'saved-three');

// Saving the same one again changes nothing: the name is the fingerprint.
await page.evaluate(() => window.gatedspark.save());
if (!/3 puzzles saved/.test(await page.evaluate(() => window.gatedspark.savedLine()))) {
  await fail('saving the same puzzle twice must not count twice', 'saved-twice');
}

// Now cut the signal. Fetching must fail and saved play must work.
await page.evaluate(() => window.gatedspark.toggleOffline());
await page.evaluate((n) => window.gatedspark.open(n), names[0]);
const offlineRefusal = await page.evaluate(() => window.gatedspark.refusal());
console.log('fetching with no signal ' + JSON.stringify(offlineRefusal));
if (!offlineRefusal || !/no signal/.test(offlineRefusal)) {
  await fail('fetching with no signal should be refused in words', 'offline-fetch');
}
await page.screenshot({ path: OUT + '/no-signal.png', fullPage: true });

await page.evaluate(() => window.gatedspark.playSaved());
const playingSaved = await page.evaluate(() => ({
  refusal: window.gatedspark.refusal(),
  fingerprint: window.gatedspark.fingerprint(),
  saved: window.gatedspark.savedLine(),
  family: window.gatedspark.family(),
  // Either game's board, because the buffer hands back whichever was saved.
  parts:
    document.querySelectorAll('td.square').length +
    document.querySelectorAll('#loop g.edge').length,
}));
console.log('playing a saved one with no signal ' + JSON.stringify(playingSaved));
if (playingSaved.refusal) await fail('a saved puzzle should open with no signal', 'saved-play');
if (playingSaved.parts === 0) await fail('a saved puzzle should draw a board', 'saved-play');
if (!/3 puzzles saved/.test(playingSaved.saved)) {
  await fail('being handed a saved puzzle is not playing it, so the count must not drop', 'delivered-not-played');
}
await page.screenshot({ path: OUT + '/playing-saved.png', fullPage: true });

// Finish it. Only now does the count drop.
//
// Which game it is decides how. The buffer hands back whatever was saved, and both
// games are in this store, so a step that assumed squares would work until the day the
// buffer handed back a loop puzzle — which is exactly what it now does.
await layTheAnswerIn(playingSaved.fingerprint);
const afterFinishing = await page.evaluate(() => ({
  solved: window.gatedspark.isSolved(),
  saved: window.gatedspark.savedLine(),
}));
console.log('after finishing it ' + JSON.stringify(afterFinishing));
if (!afterFinishing.solved) await fail('the saved puzzle did not solve', 'saved-solve');
if (!/2 puzzles saved/.test(afterFinishing.saved)) {
  await fail('finishing a saved puzzle should use it up, got: ' + afterFinishing.saved, 'played');
}
await page.screenshot({ path: OUT + '/saved-solved.png', fullPage: true });

// Play the last two out, and check the ending is a sentence rather than a blank screen.
for (let i = 0; i < 2; i += 1) {
  await page.evaluate(() => window.gatedspark.playSaved());
  await layTheAnswerIn(await page.evaluate(() => window.gatedspark.fingerprint()));
}
await page.evaluate(() => window.gatedspark.playSaved());
const ranOut = await page.evaluate(() => ({
  refusal: window.gatedspark.refusal(),
  saved: window.gatedspark.savedLine(),
}));
console.log('after playing them all ' + JSON.stringify(ranOut));
if (!ranOut.refusal) await fail('running out should say something', 'ran-out');
if (!/no more puzzles/i.test(ranOut.refusal) || !/internet/i.test(ranOut.refusal)) {
  await fail('running out should say what happened and what to do: ' + ranOut.refusal, 'ran-out');
}
if (/NothingToPlay|Empty/.test(ranOut.refusal)) {
  await fail('running out should not show a type name: ' + ranOut.refusal, 'ran-out');
}
await page.screenshot({ path: OUT + '/ran-out.png', fullPage: true });

console.log('console problems so far ' + JSON.stringify(problems));
if (problems.length) { console.log('FAIL: the browser reported errors'); await browser.close(); process.exit(1); }

// ---------------------------------------------------------------------------
// The "do not play these" list, in a real browser, against real storage
// ---------------------------------------------------------------------------

if (!DOOMED) { console.log('OK (no withdrawal to drive)'); await browser.close(); process.exit(0); }

// Signal back on, and a clean device, so the counts below mean what they say.
await page.evaluate(() => {
  if (document.getElementById('offline').getAttribute('aria-pressed') === 'true') {
    window.gatedspark.toggleOffline();
  }
  window.gatedspark.forgetEverythingSaved();
});

// Save the doomed puzzle and one other, and open the doomed one so that somebody is
// part-way through it when the withdrawal lands.
const others = everyPuzzle(WWW);
const bystander = others.find((name) => name !== DOOMED);
if (!bystander) await fail('need two puzzles to tell a withdrawal from a wipe', 'withdraw-setup');

for (const name of [bystander, DOOMED]) {
  await page.evaluate((n) => window.gatedspark.open(n), name);
  await page.waitForSelector('#game:not([hidden])');
  await page.evaluate(() => window.gatedspark.save());
}
await page.evaluate(() => { for (const [r, c] of [[0, 0], [1, 1]]) window.gatedspark.tap(r, c); });

const beforeWithdrawal = await page.evaluate(() => ({
  saved: window.gatedspark.savedLine(),
  // A u64 crosses into JavaScript as a BigInt, which does not serialize.
  seen: String(window.gatedspark.withdrawalsSeen()),
  showing: window.gatedspark.fingerprint(),
  board: window.gatedspark.boardIsShowing(),
  keys: Object.keys(localStorage).filter((k) => k.startsWith('gatedspark/')).length,
}));
console.log('before the withdrawal ' + JSON.stringify(beforeWithdrawal));
if (!/2 puzzles saved/.test(beforeWithdrawal.saved)) await fail('two should be saved: ' + beforeWithdrawal.saved, 'withdraw-setup');
if (beforeWithdrawal.seen !== '0') await fail('this device should know of no list yet, got ' + beforeWithdrawal.seen, 'withdraw-setup');
if (beforeWithdrawal.showing !== DOOMED) await fail('the doomed puzzle should be on screen', 'withdraw-setup');
if (!beforeWithdrawal.board) await fail('a board should be showing', 'withdraw-setup');
if (beforeWithdrawal.keys !== 2) await fail('two packages should be in storage, got ' + beforeWithdrawal.keys, 'withdraw-setup');
await page.screenshot({ path: OUT + '/before-withdrawal.png', fullPage: true });

// The list arrives.
await page.evaluate(() => window.gatedspark.checkWithdrawn());
const afterWithdrawal = await page.evaluate(() => ({
  note: document.getElementById('note').textContent,
  refusal: window.gatedspark.refusal(),
  board: window.gatedspark.boardIsShowing(),
  saved: window.gatedspark.savedLine(),
  seen: String(window.gatedspark.withdrawalsSeen()),
  keys: Object.keys(localStorage).filter((k) => k.startsWith('gatedspark/ahead/')).length,
}));
console.log('after the withdrawal ' + JSON.stringify(afterWithdrawal));

// The owner's decision, in as many words: stop it right away.
if (afterWithdrawal.board) await fail('the board should have closed', 'withdrawn');
if (!afterWithdrawal.refusal) await fail('closing the board should say why', 'withdrawn');
if (!/withdrawn/i.test(afterWithdrawal.refusal) || !/more than one answer/i.test(afterWithdrawal.refusal)) {
  await fail('the reason should be a sentence saying what happened: ' + afterWithdrawal.refusal, 'withdrawn');
}
if (/[a-z][A-Z]|::|\{/.test(afterWithdrawal.refusal.replace(/GatedSpark/g, 'x'))) {
  await fail('the reason reads like a type name: ' + afterWithdrawal.refusal, 'withdrawn');
}
// Deleted, not hidden. This is the assertion that a filter-on-the-way-out version of
// this code would fail.
if (afterWithdrawal.keys !== 1) await fail('the withdrawn package should be gone from storage, packages left: ' + afterWithdrawal.keys, 'withdrawn');
if (!/1 puzzle saved/.test(afterWithdrawal.saved)) await fail('one should be left: ' + afterWithdrawal.saved, 'withdrawn');
if (afterWithdrawal.seen !== '1') await fail('this device should now be going by list 1, got ' + afterWithdrawal.seen, 'withdrawn');
if (!/deleted/i.test(afterWithdrawal.note)) await fail('the note should say a puzzle was deleted: ' + afterWithdrawal.note, 'withdrawn');
await page.screenshot({ path: OUT + '/withdrawn.png', fullPage: true });

// Downloading it again is refused, by the same words.
await page.evaluate((n) => window.gatedspark.open(n), DOOMED);
const reDownload = await page.evaluate(() => ({
  refusal: window.gatedspark.refusal(),
  board: window.gatedspark.boardIsShowing(),
}));
console.log('fetching it again ' + JSON.stringify(reDownload));
if (reDownload.board) await fail('a withdrawn puzzle should not open', 're-download');
if (!/withdrawn/i.test(reDownload.refusal || '')) await fail('re-fetching should be refused in words: ' + reDownload.refusal, 're-download');

// The anti-vacuity half: everything else still works exactly as before.
await page.evaluate((n) => window.gatedspark.open(n), bystander);
await page.waitForSelector('#game:not([hidden])');
const bystanderState = await page.evaluate(() => ({
  refusal: window.gatedspark.refusal(),
  squares: document.querySelectorAll('td.square').length,
  saved: window.gatedspark.savedLine(),
}));
console.log('the other puzzle ' + JSON.stringify(bystanderState));
if (bystanderState.refusal) await fail('an unwithdrawn puzzle must still open: ' + bystanderState.refusal, 'bystander');
if (bystanderState.squares === 0) await fail('an unwithdrawn puzzle must still draw', 'bystander');

// And the same list again is refused, because it is not newer. This is the replay the
// serial number exists for, and the device must say so rather than fail silently.
await page.evaluate(() => window.gatedspark.checkWithdrawn());
const replayed = await page.evaluate(() => ({
  note: document.getElementById('note').textContent,
  seen: String(window.gatedspark.withdrawalsSeen()),
  saved: window.gatedspark.savedLine(),
}));
console.log('the same list again ' + JSON.stringify(replayed));
if (!/newer/i.test(replayed.note)) await fail('replaying a list should be refused in words: ' + replayed.note, 'replayed');
if (replayed.seen !== '1') await fail('the number this device goes by must not move', 'replayed');
if (!/1 puzzle saved/.test(replayed.saved)) await fail('nothing more should have been deleted: ' + replayed.saved, 'replayed');
await page.screenshot({ path: OUT + '/withdrawal-replayed.png', fullPage: true });

// ---------------------------------------------------------------------------
// The second game, on the same page
// ---------------------------------------------------------------------------
//
// Everything above was a picture puzzle. This is a loop puzzle, from the same store,
// opened by the same page, checked against the same key. What is being proved is that
// the page draws the right board for the game it was handed, that tapping between two
// dots draws a line, and that "solved" still comes from the rules — which here means
// one loop AND every number met, not either one on its own.

if (!LOOP) {
  console.log('no loop puzzle was named, so the second game was not driven');
} else {
  // A fresh page, because the one above has a withdrawn puzzle and a used-up buffer.
  await page.goto(URL, { waitUntil: 'networkidle' });
  await page.evaluate(() => window.gatedspark.forgetEverythingSaved());
  await page.evaluate((name) => window.gatedspark.open(name), LOOP);
  await page.waitForSelector('#game:not([hidden])', { timeout: 20000 });

  const loop = await page.evaluate(() => ({
    family: window.gatedspark.family(),
    level: document.getElementById('level').textContent,
    encoding: document.getElementById('encoding').textContent,
    squares: document.querySelectorAll('td.square').length,
    gridShowing: !document.getElementById('board').hidden,
    gridEmptied: document.querySelectorAll('td.square').length === 0,
    // `hasAttribute`, not `.hidden`: see the note in the page. Asking the wrong one
    // here agreed with the page's own mistake and reported an invisible board as shown.
    loopShowing: !document.getElementById('loop').hasAttribute('hidden'),
    loopSize: document.getElementById('loop').getBoundingClientRect().width,
    lines: window.gatedspark.linesOnTheBoard(),
    numbers: window.gatedspark.numbersOnTheBoard(),
    dots: document.querySelectorAll('#loop circle.dot').length,
    shape: window.gatedspark.shapeSaid(),
    solved: window.gatedspark.isSolved(),
  }));
  console.log('loop puzzle ' + JSON.stringify(loop));
  if (loop.family !== 'slitherlink') await fail('the wrong game was opened: ' + loop.family, 'loop-wrong-game');
  if (loop.gridShowing) await fail('a grid of squares was drawn for a loop puzzle', 'loop-grid');
  if (!loop.gridEmptied) await fail('the other game\'s board was left on the page', 'loop-grid');
  if (!loop.loopShowing) await fail('no loop board was drawn', 'loop-empty');
  if (loop.loopSize < 100) await fail('the loop board is on the page but has no size: ' + loop.loopSize, 'loop-empty');
  if (loop.lines === 0) await fail('no lines were drawn to tap', 'loop-empty');
  if (loop.numbers === 0) await fail('no numbers were drawn', 'loop-empty');
  if (loop.dots === 0) await fail('no dots were drawn', 'loop-empty');
  if (loop.level !== 'none given') await fail('this game has no levels, and the page must not invent one', 'loop-level');
  if (!/loop/.test(loop.encoding)) await fail('the uniqueness claim should name the encoding: ' + loop.encoding, 'loop-encoding');
  if (!/no lines are drawn yet/.test(loop.shape)) await fail('a blank board should say so: ' + loop.shape, 'loop-shape');
  if (loop.solved) await fail('it said solved before a single tap', 'loop-wrongly-solved');

  // Hints on a blank loop board. The same three questions as for the picture game, and
  // the same one that matters: what the hint names has to agree with the signed answer,
  // which the page does not have and the hint never reads.
  {
    const stored = drawnLines(`${WWW}/store/${LOOP}/witness.bin`);
    const hinted = await page.evaluate(() => {
      const before = window.gatedspark.hintsLeftSaid();
      window.gatedspark.hint();
      return {
        before,
        said: window.gatedspark.hintSaid(),
        at: window.gatedspark.hintedLine(),
        left: window.gatedspark.hintsLeftSaid(),
        focused: window.gatedspark.focusIsOnTheHint(),
        moves: window.gatedspark.movesMade(),
        places: window.gatedspark.linePlaces(),
      };
    });
    console.log('loop hint ' + JSON.stringify({ ...hinted, places: hinted.places.length }));
    if (!/3 hints left/.test(hinted.before)) await fail('a fresh loop puzzle should offer three hints: ' + hinted.before, 'loop-hints');
    if (!hinted.said) await fail('the hint button said nothing on a loop puzzle', 'loop-hints');
    if (!/because/.test(hinted.said)) await fail('a hint should say why: ' + hinted.said, 'loop-hints');
    if (/[a-z][A-Z]|_/.test(hinted.said)) await fail('a hint is showing a code name: ' + hinted.said, 'loop-hints');
    if (!hinted.at) await fail('the hint named no line to look at', 'loop-hints');
    if (!/2 hints left/.test(hinted.left)) await fail('the hint was not counted: ' + hinted.left, 'loop-hints');
    if (!hinted.focused) await fail('the keyboard was not moved to the hinted line', 'loop-hints');
    if (hinted.moves !== 0) await fail('asking for a hint counted as a move', 'loop-hints');

    // Which line it named, in the core's own order, so it can be looked up in the
    // answer without this check working out any numbering of its own.
    const [side, column, row] = hinted.at;
    let which = -1;
    for (let i = 0; i < hinted.places.length; i += 3) {
      if (hinted.places[i] === side && hinted.places[i + 1] === column && hinted.places[i + 2] === row) {
        which = i / 3;
        break;
      }
    }
    if (which < 0) await fail('the hint pointed at a line the page does not have', 'loop-hints');
    const inTheLoop = stored.drawn.includes(which);
    const saysDrawn = /is part of the loop/.test(hinted.said);
    const saysNot = /is not part of the loop/.test(hinted.said);
    if (!saysDrawn && !saysNot) {
      await fail('the hint named a line without saying whether to draw it: ' + hinted.said, 'loop-hints');
    }
    if (saysDrawn !== inTheLoop) {
      await fail(
        `the hint says that line ${saysDrawn ? 'is' : 'is not'} part of the loop and the signed answer says otherwise`,
        'loop-hints'
      );
    }
    await page.screenshot({ path: OUT + '/loop-hint.png', fullPage: true });
  }

  // Tap one line on, round to ruled out, and back to blank. Three states, one line.
  const first = await page.evaluate(() => {
    const places = window.gatedspark.linePlaces();
    return [places[0], places[1], places[2]];
  });
  const cycle = [];
  for (let i = 0; i < 3; i += 1) {
    await page.evaluate((at) => window.gatedspark.tapLine(at[0], at[1], at[2]), first);
    cycle.push(await page.evaluate((at) => window.gatedspark.lineAt(at[0], at[1], at[2]), first));
  }
  console.log('one line tapped three times ' + JSON.stringify(cycle));
  if (cycle[0] !== 0) await fail('a tap did not draw the line', 'loop-tap');
  if (cycle[1] !== 1) await fail('a second tap did not rule it out', 'loop-tap');
  if (cycle[2] !== 2) await fail('a third tap did not clear it', 'loop-tap');

  const afterOne = await page.evaluate((at) => {
    window.gatedspark.tapLine(at[0], at[1], at[2]);
    return { shape: window.gatedspark.shapeSaid(), solved: window.gatedspark.isSolved() };
  }, first);
  console.log('one line drawn ' + JSON.stringify(afterOne));
  if (!/mid-air/.test(afterOne.shape)) await fail('one line is a loose end, and the page should say so: ' + afterOne.shape, 'loop-shape');
  if (afterOne.solved) await fail('one line cannot solve a puzzle', 'loop-tap');
  await page.screenshot({ path: OUT + '/loop-part-played.png', fullPage: true });

  // Now lay the real answer in. It comes out of the witness file beside the package —
  // the answer the solver vouched for and the store checked — rather than being worked
  // out here. For this game the witness is one cell per line, in the order the core
  // reports the lines, so there is nothing to translate.
  const witness = readFileSync(`${WWW}/store/${LOOP}/witness.bin`);
  const count = witness.readUInt32LE(4);
  const drawn = [];
  for (let i = 0; i < count; i += 1) {
    if (witness.readInt16LE(8 + i * 2) === 1) drawn.push(i);
  }
  if (drawn.length === 0) await fail('the stored answer draws no lines at all', 'loop-witness');

  const laid = await page.evaluate((want) => {
    const places = window.gatedspark.linePlaces();
    if (places.length / 3 !== want.count) {
      return { error: `the page has ${places.length / 3} lines and the answer has ${want.count}` };
    }
    // Start from a blank board, so the loose end from the taps above is not left in.
    while (window.gatedspark.movesMade() > 0) window.gatedspark.undo();
    for (const i of want.drawn) {
      window.gatedspark.tapLine(places[i * 3], places[i * 3 + 1], places[i * 3 + 2]);
    }
    return {
      drawn: window.gatedspark.linesDrawn(),
      shape: window.gatedspark.shapeSaid(),
      numbersMet: window.gatedspark.numbersMet(),
      numbers: window.gatedspark.numbersOnTheBoard(),
      solved: window.gatedspark.isSolved(),
      solvedShowing: !document.getElementById('solved').hidden,
      said: document.getElementById('solved').textContent,
    };
  }, { drawn, count });
  console.log('the answer laid in ' + JSON.stringify(laid));
  if (laid.error) await fail(laid.error, 'loop-witness');
  if (laid.drawn !== drawn.length) await fail('not every line of the answer went on the board', 'loop-answer');
  if (!/one closed loop/.test(laid.shape)) await fail('the answer should be one loop: ' + laid.shape, 'loop-answer');
  if (laid.numbersMet !== laid.numbers) await fail('the answer should meet every number', 'loop-answer');
  if (!laid.solved) await fail('the real answer was not accepted as solved', 'loop-answer');
  if (!laid.solvedShowing) await fail('the page did not show the solved message', 'loop-answer');
  if (!/one loop/i.test(laid.said)) await fail('the solved message should be this game\'s: ' + laid.said, 'loop-answer');
  await page.screenshot({ path: OUT + '/loop-solved.png', fullPage: true });

  // And taking one line back unsolves it, so nothing on the page is remembering.
  const afterUndo = await page.evaluate(() => {
    window.gatedspark.undo();
    return { solved: window.gatedspark.isSolved(), showing: !document.getElementById('solved').hidden };
  });
  console.log('one line taken back ' + JSON.stringify(afterUndo));
  if (afterUndo.solved || afterUndo.showing) await fail('an undone answer is still being called solved', 'loop-undo');

  // One loop puzzle, saved and brought back. The buffer knows nothing about any game,
  // and this is what that is worth.
  const savedLoop = await page.evaluate(() => {
    window.gatedspark.save();
    const line = window.gatedspark.savedLine();
    window.gatedspark.playSaved();
    return { line, family: window.gatedspark.family(), showing: window.gatedspark.boardIsShowing() };
  });
  console.log('a loop puzzle saved and brought back ' + JSON.stringify(savedLoop));
  if (!/1 puzzle saved/.test(savedLoop.line)) await fail('a loop puzzle was not saved: ' + savedLoop.line, 'loop-saved');
  if (savedLoop.family !== 'slitherlink') await fail('the saved puzzle came back as another game', 'loop-saved');
  if (!savedLoop.showing) await fail('the saved puzzle did not come back onto the screen', 'loop-saved');
  await page.screenshot({ path: OUT + '/loop-saved.png', fullPage: true });
}

// ---------------------------------------------------------------------------
// The schedule, driven: switching game, and downloading days ahead
// ---------------------------------------------------------------------------

// Start from a clean device, so the counts below mean something.
await page.goto(URL, { waitUntil: 'networkidle' });
await page.evaluate(() => window.gatedspark.forgetEverythingSaved());
await page.reload({ waitUntil: 'networkidle' });
await page.waitForSelector('#game:not([hidden])', { timeout: 20000 });

// Choosing the other game from the picker opens that game's puzzle for today, with
// nothing typed and no name known to the page.
const bothGames = await page.evaluate(() => window.gatedspark.games());
console.log('games offered ' + JSON.stringify(bothGames));
if (bothGames.length < 2) await fail('both games should be on the calendar', 'one-game');
const other = bothGames[1];
await page.evaluate((family) => window.gatedspark.chooseGame(family), other);
await page.waitForFunction(() => window.gatedspark.boardIsShowing(), null, { timeout: 15000 });
const afterSwitch = await page.evaluate(() => ({
  family: window.gatedspark.family(),
  days: window.gatedspark.days(),
  line: window.gatedspark.todayLine(),
  solved: window.gatedspark.isSolved(),
}));
console.log('after switching game ' + JSON.stringify(afterSwitch));

// The rules on screen must be this game's rules. A page that explained one game while
// showing the other would be worse than one that explained nothing, because a player
// would believe it. Checked by reading what is on the page, not by trusting the table
// it came from.
const rules = await page.evaluate(() => ({
  title: window.gatedspark.rulesTitle(),
  lines: window.gatedspark.rulesShown(),
  open: window.gatedspark.rulesOpen(),
}));
console.log('rules on screen ' + JSON.stringify(rules));

// The loop game, with the keyboard and with the ears. Same two questions as the
// picture game: can a line be reached and pressed without a mouse, and does it say
// what it has become.
if (afterSwitch.family === 'slitherlink') {
  const beforeLine = await page.evaluate(() => {
    const first = document.querySelector('g.edge');
    first.focus();
    return {
      name: first.getAttribute('aria-label'),
      drawn: Number(first.dataset.drawn),
      focused: document.activeElement === first,
    };
  });
  if (!beforeLine.focused) await fail('a line cannot take focus at all', 'keyboard-loop');
  if (!/not decided/.test(beforeLine.name)) {
    await fail('an untouched line does not say it is undecided: ' + beforeLine.name, 'keyboard-loop');
  }
  await page.keyboard.press('Enter');
  const afterLine = await page.evaluate(() => ({
    name: document.activeElement.getAttribute('aria-label'),
    drawn: Number(document.activeElement.dataset.drawn),
  }));
  console.log('after a key press on a line ' + JSON.stringify(afterLine));
  if (afterLine.drawn === beforeLine.drawn) {
    await fail('Enter did not change the line the keyboard was on', 'keyboard-loop');
  }
  if (!/drawn/.test(afterLine.name)) {
    await fail('a drawn line still calls itself undecided: ' + afterLine.name, 'keyboard-loop');
  }
  await page.evaluate(() => window.gatedspark.undo());
}
if (!rules.lines.length) await fail('the game is showing no rules at all', 'rules');
if (!rules.open) await fail('the rules start folded away, so a new player never sees them', 'rules');
const saysLoop = rules.lines.join(' ').includes('loop');
if (afterSwitch.family === 'slitherlink' && !saysLoop) {
  await fail('the loop game is not being told to draw a loop: ' + JSON.stringify(rules.lines), 'rules');
}
if (afterSwitch.family === 'nonogram' && saysLoop) {
  await fail('the picture game is being given the loop game rules', 'rules');
}
if (!rules.title.toLowerCase().includes(afterSwitch.family)) {
  await fail(`the rules are titled "${rules.title}" while a ${afterSwitch.family} is on screen`, 'rules');
}
if (afterSwitch.family !== other) await fail(`choosing ${other} opened a ${afterSwitch.family}`, 'switch-game');
if (afterSwitch.days.length < 1 || !afterSwitch.days[0].endsWith('(today)')) {
  await fail('the other game has no puzzle for today: ' + JSON.stringify(afterSwitch.days), 'switch-game');
}
if (afterSwitch.solved) await fail('a freshly opened puzzle came up solved', 'switch-game');
await page.screenshot({ path: OUT + '/other-game.png', fullPage: true });

// Downloading days ahead. The schedule in the demo ends today, so there is nothing
// after it: the page must say so plainly rather than appearing to save something.
const nothingAhead = await page.evaluate(async () => {
  await window.gatedspark.saveAhead();
  return { note: document.getElementById('note').textContent, saved: window.gatedspark.savedLine() };
});
console.log('nothing to download ahead ' + JSON.stringify(nothingAhead));
if (!/no further days/.test(nothingAhead.note)) {
  await fail('with no days left it should say so: ' + nothingAhead.note, 'save-ahead-none');
}
if (!/No puzzles saved/.test(nothingAhead.saved)) {
  await fail('nothing should have been saved: ' + nothingAhead.saved, 'save-ahead-none');
}

// And with days ahead there are, which the driver arranges by asking for the days after
// the first day of the calendar rather than after today. Same call the button makes.
const ahead = await page.evaluate(() => {
  const earliest = window.gatedspark.days().slice(-1)[0].slice(0, 10).split('-').map(Number);
  const coming = window.gatedspark.toDownload(earliest[0], earliest[1], earliest[2], window.gatedspark.family(), 5);
  return coming.map((d) => d.date());
});
console.log('days after the first day of the calendar ' + JSON.stringify(ahead));
if (ahead.length === 0) await fail('the calendar should have days after its own first day', 'to-download');

console.log('console problems after both games ' + JSON.stringify(problems));
await browser.close();
if (problems.length) { console.log('FAIL: the browser reported errors'); process.exit(1); }
console.log('OK');
