// Owns a Rust preview process and drives its real stdin; no HTTP/platform mocks.
import { spawn } from 'node:child_process';
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../../', import.meta.url));
const base = 'http://127.0.0.1:32134';
if (await fetch(`${base}/api/v1/state`).then(() => true, () => false)) throw new Error('Close the running preview first; this test starts its own Rust process on 32134.');
const executable = process.env.BILIMANI_PREVIEW_EXECUTABLE ?? `${root}/target/debug/examples/api_preview${process.platform === 'win32' ? '.exe' : ''}`;
const server = spawn(executable, ['dist/preview'], { cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
let output = '', startError;
server.on('error', error => { startError = error; });
for (const stream of [server.stdout, server.stderr]) stream.on('data', bytes => { output = (output + bytes.toString()).slice(-10000); });
const wait = async (check, label) => {
  for (let i = 0; i < 100; i++) { if (startError) throw startError; if (await check()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`Timed out: ${label}\n${output}`);
};
let browser;
try {
  await wait(() => fetch(`${base}/api/v1/preview`).then(r => r.ok, () => false), 'Rust preview startup');
  browser = await chromium.launch({ headless: true, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  page.setDefaultTimeout(8000);
  const errors = [], external = [];
  page.on('pageerror', error => errors.push(String(error)));
  await page.route('https://**/*', route => { external.push(route.request().url()); return route.abort(); });
  const send = command => server.stdin.write(`${command}\n`);
  const login = stage => page.waitForFunction(stage => document.getElementById('login-overlay').dataset.stage === stage, stage);
  const arm = (stage, origin) => page.waitForFunction(({ stage, origin }) => {
    const el = document.getElementById('choice-arm');
    return el.dataset.stage === stage && (!origin || el.dataset.origin === origin);
  }, { stage, origin });
  await page.goto(`${base}/iidx/mecha/`);
  await page.getByText('本地预览', { exact: true }).waitFor();
  await login('hidden');
  send('help'); await wait(async () => output.includes('logout'), 'help');
  send('logout'); await login('waiting');
  assert.equal(await page.locator('.login-message').textContent(), '正在等待登录中');
  send('select'); await login('success'); await login('hidden');
  send('other'); await page.locator('#loading-strip').waitFor();
  assert.equal(await page.locator('#login-overlay').isVisible(), false);
  send('select'); await page.locator('#loading-strip').waitFor({ state: 'hidden' });
  send('menu'); await arm('open', 'right');
  assert.equal(await page.locator('.candidate').count(), 5);
  send('reply n'); await wait(async () => await page.locator('.candidate-number').first().textContent() === '6', 'next page');
  send('reply p'); await wait(async () => await page.locator('.candidate-number').first().textContent() === '1', 'previous page');
  send('reply 2'); await arm('stowed');
  assert.equal((await (await fetch(`${base}/api/v1/queue`)).json()).current.song_id, 2);
  send('2p');
  await wait(async () => (await (await fetch(`${base}/api/v1/iidx/user`)).json()).active_side === 2, '2P');
  send('menu'); await arm('open', 'left');
  send('dp'); await arm('open', 'right');
  await page.reload(); await page.getByText('本地预览', { exact: true }).waitFor(); await login('hidden');
  await page.waitForTimeout(1200);
  assert.equal(await page.locator('.candidate').count(), 0, 'refresh must not replay earlier menu commands');
  assert.equal((await (await fetch(`${base}/api/v1/queue`)).json()).current.song_id, 2, 'refresh preserves the real queue');
  send('logout'); await login('waiting');
  assert.equal(await page.locator('#login-overlay').isVisible(), true, 'logout can reopen the mask after a completed login');
  assert.equal(await page.locator('#login-overlay').evaluate(el => Number(getComputedStyle(el).opacity)), 1);
  assert.deepEqual(external, []); assert.deepEqual(errors, []);
  console.log('PASS preview REPL: real logout/select/other/menu/reply/2p/dp commands, real search and queue, no external platform traffic or command replay');
} finally {
  await browser?.close();
  if (server.exitCode === null && server.pid) {
    const exited = new Promise(resolve => server.once('exit', resolve));
    server.kill(); await exited;
  }
}
