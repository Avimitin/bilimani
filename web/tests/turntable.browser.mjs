// Production page and the real Rust simulator. Only the platform config is isolated.
import { spawn } from 'node:child_process';
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const base = 'http://127.0.0.1:32134';
if (await fetch(`${base}/api/v1/state`).then(() => true, () => false)) throw new Error('Close the preview first; this test owns port 32134.');
const executable = process.env.BILIMANI_PREVIEW_EXECUTABLE ?? `${root}/target/debug/examples/api_preview${process.platform === 'win32' ? '.exe' : ''}`;
const server = spawn(executable, ['dist/web'], { cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
let output = '', startError;
server.on('error', error => { startError = error; });
for (const stream of [server.stdout, server.stderr]) stream.on('data', bytes => { output = (output + bytes.toString()).slice(-8000); });
const wait = async (check, label) => {
  for (let i = 0; i < 100; i++) {
    if (startError) throw startError;
    if (await check()) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${label}\n${output}`);
};
let browser;
try {
  await wait(() => fetch(`${base}/api/v1/state`).then(r => r.ok, () => false), 'Rust startup');
  browser = await chromium.launch({ headless: true, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, reducedMotion: 'reduce' });
  const errors = [], external = [];
  page.on('pageerror', error => errors.push(String(error)));
  await page.route('https://**/*', route => { external.push(route.request().url()); return route.abort(); });
  const config = JSON.parse(await readFile(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
  let effects = true;
  await page.route('**/config.json', route => route.fulfill({ json: { ...config, platform: { ...config.platform, enabled: false }, display: { ...config.display, vfdEffects: effects } } }));
  const send = command => server.stdin.write(`${command}\n`);
  const disc = page.locator('#turntable-disc');
  const spinning = value => page.waitForFunction(value => document.getElementById('turntable-disc').dataset.spinning === String(value), value);
  const rotor = page.locator('.turntable-rotor');
  const angle = () => rotor.evaluate(el => getComputedStyle(el).transform);
  const light = () => page.locator('.turntable-light').evaluate(el => getComputedStyle(el).opacity);
  await page.goto(`${base}/iidx/mecha/`);
  await page.locator('#login-overlay').waitFor({ state: 'hidden' });
  assert.equal(await disc.getAttribute('data-spinning'), 'false');
  send('decide'); await spinning(true);
  const first = await angle();
  await page.waitForTimeout(300);
  assert.notEqual(await angle(), first, 'platter rotates under the explicit effects setting');
  assert.equal(await light(), '1');
  const time = () => rotor.evaluate(el => el.getAnimations()[0].currentTime);
  const beforePoll = await time(); await page.waitForTimeout(1200);
  assert.ok(await time() > beforePoll + 1000, 'polling must not restart the spin');
  await mkdir(new URL('../../analysis/browser/', import.meta.url), { recursive: true });
  await page.screenshot({ path: fileURLToPath(new URL('../../analysis/browser/turntable-deciding.png', import.meta.url)), omitBackground: true });
  await page.reload(); await spinning(true);
  await page.locator('#login-overlay').waitFor({ state: 'hidden' });
  send('other'); await spinning(false);
  const stopped = await angle(); await page.waitForTimeout(300);
  assert.equal(await angle(), stopped, 'scene exit freezes the platter before gameplay');
  assert.equal(await light(), '0');
  send('play'); await page.waitForTimeout(700); await spinning(false);
  send('select'); await page.waitForTimeout(700);
  send('decide'); await spinning(true);
  send('select'); await spinning(false);
  send('decide'); await spinning(true);
  send('play'); await spinning(false);
  send('decide'); await spinning(true);
  send('logout'); await spinning(false);
  send('select'); await page.waitForTimeout(700);
  send('decide'); await spinning(true);
  effects = false;
  await page.reload(); await page.locator('#login-overlay').waitFor({ state: 'hidden' });
  assert.equal(await disc.getAttribute('data-spinning'), 'false');
  assert.equal(await light(), '0');
  const disabled = await angle(); await page.waitForTimeout(300);
  assert.equal(await angle(), disabled);
  assert.deepEqual(external, []); assert.deepEqual(errors, []);
  console.log('PASS turntable: real Rust decision state, rotation and purple trails, no polling restart, refresh, pre-game exit, cancellation, gameplay, logout, disabled effects');
} finally {
  await browser?.close();
  if (server.exitCode === null && server.pid) {
    const exited = new Promise(resolve => server.once('exit', resolve));
    server.kill(); await exited;
  }
}
