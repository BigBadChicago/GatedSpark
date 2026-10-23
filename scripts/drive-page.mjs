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
const afterTaps = await page.evaluate(() => ({
  moves: document.getElementById('moves').textContent,
  filled: window.gatedspark.boardAt(0, 0),
  crossed: window.gatedspark.boardAt(1, 1),
  untouched: window.gatedspark.boardAt(9, 9),
  solved: window.gatedspark.isSolved(),
}));
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
  // One tap fills. Two cross out. The rules call a board finished only when every
  // square has been decided one way or the other, so the blanks get crossed too.
  for (const [row, column] of answer.filled) w.tap(row, column);
  for (const [row, column] of answer.empty) { w.tap(row, column); w.tap(row, column); }
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

console.log('console problems ' + JSON.stringify(problems));
await browser.close();
if (problems.length) { console.log('FAIL: the browser reported errors'); process.exit(1); }
console.log('OK');
