// Production assets from Rust preview; browser routes isolate all game/platform traffic.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const base = process.env.BILIMANI_TEST_URL ?? 'http://127.0.0.1:32134';
const config = JSON.parse(await readFile(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
const native = await (await fetch(`${base}/api/v1/state`)).json();
const user = await (await fetch(`${base}/api/v1/iidx/user`)).json();
// UI checks must also work while a developer uses logout in the preview terminal.
const selectedSong = native.now_playing.song ? native.now_playing : {
  phase: 'selecting', song: { id: 1, title: 'Preview Song', artist: 'Synthetic Artist', genre: 'DEVELOPMENT FIXTURE', game_version: 33, charts: [] },
  players: [], lane_order: [], playback: null,
};
const artifacts = new URL('../../analysis/browser/', import.meta.url);
await mkdir(artifacts, { recursive: true });
const headed = process.argv.includes('--headed');
const browser = await chromium.launch({ headless: !headed, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
try {
  const page = await browser.newPage({ viewport: headed ? { width: 1280, height: 720 } : { width: 1920, height: 1080 }, reducedMotion: 'reduce' });
  page.setDefaultTimeout(7000);
  const errors = []; page.on('pageerror', error => errors.push(String(error)));
  let phase = 'other', hasSong = false, effects = true, mode = 'SP';
  await page.route('**/config.json', route => route.fulfill({ json: { ...config, platform: { ...config.platform, enabled: false }, display: { ...config.display, vfdEffects: effects } } }));
  await page.route('**/api/v1/state', route => route.fulfill({ json: { ...native, ready: true, phase, mode,
    queue: { revision: 0, capacity: 20, current: null, items: [], events: [] },
    now_playing: hasSong ? selectedSong : { phase: 'idle', song: null, players: [], lane_order: [], playback: null },
  } }));
  await page.route('**/api/v1/iidx/user', route => route.fulfill({ json: user }));
  await page.route('**/api/v1/preview', route => route.fulfill({ json: { last_id: 0, events: [] } }));
  const mask = page.locator('#login-overlay'), strip = page.locator('#loading-strip');
  const stage = value => page.waitForFunction(value => document.getElementById('login-overlay').dataset.stage === value, value);
  const pattern = value => page.waitForFunction(value => document.getElementById('login-overlay').dataset.pattern === value, value);
  const screenshot = name => page.screenshot({ path: fileURLToPath(new URL(`startup-${name}.png`, artifacts)), omitBackground: true });
  const lit = () => page.locator('.loading-cell[data-lit="true"]').count();

  await page.goto(`${base}/iidx/mecha/`);
  await stage('waiting'); await strip.waitFor();
  assert.equal(await page.locator('.login-message').textContent(), '正在等待登录中');
  assert.equal(await page.locator('.login-markers use').count(), 20);
  assert.match(await mask.evaluate(el => getComputedStyle(el).backgroundImage), /linear-gradient\(.*rgba\(/);
  assert.equal(await mask.getAttribute('data-pattern'), 'plus');
  const upperLabel = await page.locator('.login-label').boundingBox();
  assert.ok(upperLabel.x < 100 && upperLabel.y < 100, 'initial label sits at the upper left');
  assert.ok(await page.locator('.login-markers use').evaluateAll(els => els.every(el => getComputedStyle(el).transform === 'matrix(1, 0, 0, 1, 0, 0)')), 'initial grid contains only plus signs');
  assert.ok(await strip.evaluate(el => Number(getComputedStyle(el).zIndex)) < await mask.evaluate(el => Number(getComputedStyle(el).zIndex)), 'loading strip stays beneath the tint');
  assert.equal(await page.locator('#current-content').evaluate(el => getComputedStyle(el).visibility), 'hidden');
  const counts = [];
  for (let i = 0; i < 24; i++) { counts.push(await lit()); await page.waitForTimeout(150); }
  assert.ok(counts.some((n, i) => i > 0 && n > counts[i - 1]), 'loader advances');
  assert.ok(counts.some((n, i) => i > 0 && n < counts[i - 1]), 'filled cells clear between sweeps');
  assert.ok(Math.max(...counts) < await page.locator('.loading-cell').count(), 'random holes remain in the honeycomb');
  await screenshot('waiting');

  await pattern('cross');
  const lowerLabel = await page.locator('.login-label').boundingBox();
  assert.ok(lowerLabel.x > upperLabel.x + 400 && lowerLabel.y > upperLabel.y + 400, 'cross layout moves the label to the lower right');
  const maskBox = await mask.boundingBox();
  const bottomInset = maskBox.y + maskBox.height - lowerLabel.y - lowerLabel.height;
  assert.ok(Math.abs(bottomInset - (upperLabel.y - maskBox.y)) < 1, 'lower label mirrors the upper caption inset at the screen bottom');
  const markerBox = await page.locator('.login-markers').boundingBox();
  assert.ok(markerBox.y + markerBox.height < lowerLabel.y, 'all four marker rows stay above the lower caption');
  assert.ok(await page.locator('.login-markers use').evaluateAll(els => els.every(el => {
    const matrix = new DOMMatrixReadOnly(getComputedStyle(el).transform);
    return Math.abs(matrix.a - Math.SQRT1_2) < .001 && Math.abs(matrix.b - Math.SQRT1_2) < .001;
  })), 'the whole grid rotates together');
  await screenshot('waiting-cross');
  await pattern('plus');
  await pattern('cross');
  // Confirm just before another five-second boundary: the caption must not jump mid-fade.
  await page.waitForTimeout(3500);

  phase = 'select'; hasSong = true;
  await stage('success');
  assert.equal(await page.locator('.login-message').textContent(), '登录成功');
  assert.equal(await page.locator('.login-label').evaluate(el => getComputedStyle(el).color), 'rgb(162, 123, 233)');
  const confirmedCells = await lit();
  const blink = [];
  for (let i = 0; i < 8; i++) { blink.push(await page.locator('.login-label').evaluate(el => Number(getComputedStyle(el).opacity))); await page.waitForTimeout(70); }
  assert.ok(Math.min(...blink) < .6 && Math.max(...blink) > .85, 'success blinks even when desktop reduced motion is enabled');
  assert.equal(await lit(), confirmedCells, 'confirmation holds the current honeycomb pattern');
  await screenshot('success');
  await stage('fading');
  assert.equal(await mask.getAttribute('data-pattern'), 'cross', 'confirmation freezes the current camera layout');
  const before = await mask.evaluate(el => Number(getComputedStyle(el).opacity));
  await page.waitForTimeout(300);
  assert.ok(await mask.evaluate(el => Number(getComputedStyle(el).opacity)) < before - .15, 'actual rendered opacity fades');
  await screenshot('fading');
  await stage('hidden');
  assert.equal(await mask.isVisible(), false);
  assert.equal(await strip.isVisible(), false);
  assert.equal(await page.locator('#current-content').evaluate(el => getComputedStyle(el).visibility), 'visible');

  phase = 'other'; hasSong = false;
  await strip.waitFor();
  assert.equal(await mask.isVisible(), false, 'missing song after login only affects the bottom strip');
  assert.equal(await page.locator('#current-content').evaluate(el => getComputedStyle(el).visibility), 'hidden');
  await page.waitForTimeout(1300);
  await screenshot('between-songs');
  phase = 'select'; await page.waitForTimeout(1000);
  assert.equal(await mask.isVisible(), false);
  hasSong = true; await strip.waitFor({ state: 'hidden' });
  // A real logout must restore the mask after its previous success/fade completed.
  phase = 'other'; hasSong = false; mode = null;
  await stage('waiting');
  assert.equal(await mask.isVisible(), true, 'logout restores the full-screen mask');
  assert.equal(await mask.evaluate(el => Number(getComputedStyle(el).opacity)), 1);
  assert.equal(await mask.getAttribute('data-pattern'), 'plus', 'logout restarts the waiting layout');
  await screenshot('logout');
  phase = 'select'; hasSong = true; mode = 'SP';
  await stage('success'); await stage('hidden');
  phase = 'playing'; await page.reload(); await stage('hidden');
  assert.equal(await mask.isVisible(), false, 'refresh during gameplay skips startup confirmation');

  phase = 'other'; hasSong = false; effects = false;
  await page.reload(); await stage('waiting'); await strip.waitFor();
  const staticCount = await lit(); await page.waitForTimeout(700);
  assert.equal(await lit(), staticCount, 'effects setting also controls loading motion');
  phase = 'select'; hasSong = true; await stage('success');
  assert.equal(await page.locator('.login-label').evaluate(el => el.getAnimations().length), 0);
  await stage('hidden');
  assert.deepEqual(errors, []);
  console.log('PASS startup: dark-purple tint, four-row plus/cross layout cycle, bottom caption inset, loading cells beneath mask, confirmation holds position, purple blink and fade, song gaps, ready-page refresh and disabled effects');
} finally { await browser.close(); }
