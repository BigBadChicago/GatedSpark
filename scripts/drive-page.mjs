// Drives the real page the way a person does: open it, look at it, lay the answer in,
// and check that the page says "solved" only because the core said so.
import { readFileSync } from 'node:fs';
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

const browser = await chromium.launch();
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
  choices: document.getElementById('pick').options.length,
  name: document.getElementById('pick').value,
}));
console.log('details ' + JSON.stringify(details));
if (details.squares === 0) await fail('no grid was drawn', 'empty');
if (details.colClues !== details.rowClues) await fail('clue counts disagree with a square board', 'clues');
if (details.solvedShowing) await fail('it said solved before a single tap', 'wrongly-solved');
if (!details.fingerprint.startsWith(details.name.slice(0, 8))) await fail('the fingerprint shown is not this puzzle', 'wrong-id');

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
const second = await page.evaluate(() => document.getElementById('pick').options[1]?.value);
if (second) {
  await page.selectOption('#pick', second);
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
const names = await page.evaluate(() => Array.from(document.getElementById('pick').options, (o) => o.value));
for (const name of names.slice(0, 3)) {
  await page.selectOption('#pick', name);
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
  squares: document.querySelectorAll('td.square').length,
}));
console.log('playing a saved one with no signal ' + JSON.stringify(playingSaved));
if (playingSaved.refusal) await fail('a saved puzzle should open with no signal', 'saved-play');
if (playingSaved.squares === 0) await fail('a saved puzzle should draw a board', 'saved-play');
if (!/3 puzzles saved/.test(playingSaved.saved)) {
  await fail('being handed a saved puzzle is not playing it, so the count must not drop', 'delivered-not-played');
}
await page.screenshot({ path: OUT + '/playing-saved.png', fullPage: true });

// Finish it. Only now does the count drop.
const savedAnswer = filledSquares(`${WWW}/store/${playingSaved.fingerprint}/witness.bin`);
await page.evaluate((filled) => {
  for (const [row, column] of filled) window.gatedspark.tap(row, column);
}, savedAnswer.filled);
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
  const fingerprint = await page.evaluate(() => window.gatedspark.fingerprint());
  const answer = filledSquares(`${WWW}/store/${fingerprint}/witness.bin`);
  await page.evaluate((filled) => {
    for (const [row, column] of filled) window.gatedspark.tap(row, column);
  }, answer.filled);
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
const others = await page.evaluate(() => [...document.getElementById('pick').options].map((o) => o.value));
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

console.log('console problems after the withdrawal ' + JSON.stringify(problems));
await browser.close();
if (problems.length) { console.log('FAIL: the browser reported errors'); process.exit(1); }
console.log('OK');
