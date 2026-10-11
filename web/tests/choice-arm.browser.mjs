// Production page served by Rust preview; all game/platform traffic is intercepted.
// This visual/animation regression never mutates the developer's preview queue.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const base = process.env.BILIMANI_TEST_URL ?? 'http://127.0.0.1:32134';
const config = JSON.parse(await readFile(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
const snapshot = await (await fetch(`${base}/api/v1/state`)).json();
const profile = await (await fetch(`${base}/api/v1/iidx/user`)).json();
const catalog = await (await fetch(`${base}/api/v1/songs?q=Preview&limit=20`)).json();
const artifacts = new URL('../../analysis/browser/', import.meta.url);
await mkdir(artifacts, { recursive: true });
const headed = process.argv.includes('--headed');
const browser = await chromium.launch({ headless: !headed, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
const packet = (op, data) => {
  const body = Buffer.from(JSON.stringify(data)), bytes = Buffer.alloc(16 + body.length);
  bytes.writeUInt32BE(bytes.length, 0); bytes.writeUInt16BE(16, 4); bytes.writeUInt16BE(1, 6); bytes.writeUInt32BE(op, 8); bytes.writeUInt32BE(1, 12); body.copy(bytes, 16); return bytes;
};
const wait = async (check, label) => {
  for (let i = 0; i < 120; i++) { if (await check()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`Timed out: ${label}`);
};

try {
  const page = await browser.newPage({ viewport: headed ? { width: 1280, height: 720 } : { width: 1920, height: 1080 }, reducedMotion: 'reduce' });
  page.setDefaultTimeout(7000);
  const errors = [];
  page.on('pageerror', error => errors.push(String(error)));
  let socket, id = 0, queued = 0, side = 1, mode = 'SP', effects = true, selectionTimeout = 60, waitingQueue = false;
  const titles = ['灼熱 Beach Side Bunny', '灼熱 Pt.2 Long Train Running', '灼熱 Lost Summer Day - a very long song title for wrapping', '灼熱 ～終わらない夏の思い出～', '灼熱之夏 / 光与影的交响曲'];
  await page.route('**/config.json', route => route.fulfill({ json: { ...config, requests: { ...config.requests, selectionTimeoutSeconds: selectionTimeout }, display: { ...config.display, vfdEffects: effects } } }));
  const waiting = { id: 99, song_id: 3, title: 'Already queued', mode: 'SP', chart: null, requester: '先来的观众', error: null, executing: false };
  await page.route('**/api/v1/state', route => route.fulfill({ json: { ...snapshot, ready: true, mode, phase: 'select', queue: { revision: 0, capacity: 20, current: null, items: waitingQueue ? [waiting] : [], events: [] },
    now_playing: { ...snapshot.now_playing, players: [{ side, chart: { mode, id: mode === 'DP' ? 'DPA' : 'SPA' } }] } } }));
  await page.route('**/api/v1/iidx/user', route => route.fulfill({ json: { ...profile, active_side: side, players: profile.players.slice(0, 1).map(p => ({ ...p, side })) } }));
  await page.route('**/api/v1/songs?*', route => route.fulfill({ json: { songs: catalog.songs.map((song, i) => ({ ...song, title: titles[i] ?? song.title })) } }));
  await page.route('**/api/v1/queue', route => {
    assert.equal(route.request().method(), 'POST'); queued++;
    const selection = route.request().postDataJSON();
    return route.fulfill({ status: 201, json: { id: queued, ...selection, title: titles[selection.song_id - 1] ?? `Preview Song ${selection.song_id}`, chart: selection.chart ?? null, error: null } });
  });
  await page.route('**/_proxy/relay/api/open_live/*', route => route.fulfill({ json: { code: 0, data: { game_info: { game_id: 'fixture' }, websocket_info: { wss_link: ['wss://fixture.test/chat'], auth_body: '{}' }, anchor_info: { room_id: 1, uname: 'MECHA 测试直播间' } } } }));
  await page.routeWebSocket('wss://fixture.test/chat', ws => {
    socket = ws;
    ws.onMessage(data => { const op = Buffer.from(data).readUInt32BE(8); if (op === 7) ws.send(packet(8, { code: 0 })); else if (op === 2) ws.send(packet(3, {})); });
  });
  const send = (text, user = 'viewer') => socket.send(packet(5, { cmd: 'LIVE_OPEN_PLATFORM_DM', data: { open_id: user, uname: user === 'viewer' ? '测试观众' : '另一位观众', msg: text, msg_id: String(++id) } }));
  const arm = page.locator('#choice-arm');
  const stage = value => wait(async () => await arm.getAttribute('data-stage') === value, `arm ${value}`);
  const opened = origin => wait(async () => await arm.getAttribute('data-stage') === 'open' && await arm.getAttribute('data-origin') === origin, `open from ${origin}`);
  const trace = () => page.evaluate(() => {
    window.armTrace = [];
    window.armObserver?.disconnect();
    const arm = document.getElementById('choice-arm');
    window.armObserver = new MutationObserver(() => window.armTrace.push({ stage: arm.dataset.stage, origin: arm.dataset.origin }));
    window.armObserver.observe(arm, { attributes: true, attributeFilter: ['data-stage'] });
  });
  const screenshot = name => page.screenshot({ path: fileURLToPath(new URL(`arm-${name}.png`, artifacts)), omitBackground: true });
  const reset = async () => {
    socket = undefined;
    await page.goto(`${base}/iidx/mecha/?authCode=LOCAL-TEST-ONLY`);
    await wait(async () => !!socket && await page.locator('.connection-banner:visible').count() === 0, 'page connected');
  };
  await reset();
  send('今天也来听好听的歌！');
  await page.getByText('测试观众：今天也来听好听的歌！', { exact: true }).waitFor();
  const chatBounds = await page.locator('#activity-panel').boundingBox();
  send('点歌 Preview');
  await stage('extending');
  await screenshot('extending');
  await stage('open');
  assert.equal(await arm.getAttribute('data-origin'), 'right');
  assert.deepEqual(await page.locator('#activity-panel').boundingBox(), chatBounds, 'choices must not resize or move chat');
  assert.equal(await page.locator('.candidate').count(), 5);
  assert.equal(await page.locator('.arm-glass').evaluate(el => el.clientHeight), 550);
  await screenshot('1p');
  send('n');
  await wait(async () => await page.locator('.candidate-number').first().textContent() === '6', 'page 2');
  assert.equal(await arm.getAttribute('data-stage'), 'open', 'paging must not restart animation');
  send('p');
  await wait(async () => await page.locator('.candidate-number').first().textContent() === '1', 'page 1');
  // Multiple viewers change the content, never the deployed mechanism.
  send('点歌 Preview', 'other');
  await page.locator('#pending-page').waitFor();
  assert.ok(await page.locator('.candidate-title').nth(2).evaluate(el => el.scrollHeight <= el.clientHeight + 1), 'two-line titles fit with the viewer pagination footer');
  await screenshot('multiple-viewers');
  send('n', 'other');
  await page.getByText('另一位观众，请选择', { exact: true }).waitFor();
  assert.equal(await arm.getAttribute('data-stage'), 'open');
  send('6', 'other');
  await wait(async () => queued === 1, 'second viewer selection');
  await page.getByText('测试观众，请选择', { exact: true }).waitFor();
  assert.equal(await arm.getAttribute('data-stage'), 'open');
  await trace();
  send('2');
  await wait(async () => queued === 2, 'selection submitted');
  await stage('screen');
  assert.equal(await page.locator('.candidate').count(), 5, 'closing retains glass contents');
  await screenshot('closing');
  await stage('stowed');
  const closingStages = await page.evaluate(() => [...new Set(window.armTrace.map(s => s.stage).filter(s => s !== 'open'))]);
  assert.deepEqual(closingStages, ['screen', 'lowering', 'housing', 'extending', 'stowed']);
  assert.equal(await arm.isVisible(), false);
  assert.equal(await page.locator('.candidate').count(), 0);
  assert.deepEqual(await page.locator('#activity-panel').boundingBox(), chatBounds);

  side = 2; waitingQueue = true;
  await reset();
  const withQueueBounds = await page.locator('#activity-panel').boundingBox();
  send('点歌 Preview'); await stage('open');
  assert.deepEqual(await page.locator('#activity-panel').boundingBox(), withQueueBounds, 'existing queue also keeps its space during choices');
  await page.getByText('Already queued', { exact: true }).waitFor();
  assert.equal(await arm.getAttribute('data-origin'), 'left');
  await screenshot('2p');
  await trace(); mode = 'DP';
  await opened('right');
  const sideTrace = await page.evaluate(() => window.armTrace);
  assert.ok(sideTrace.some(s => s.stage === 'stowed' && s.origin === 'left'), 'side switch retracts completely');
  assert.equal(await arm.getAttribute('data-origin'), 'right', 'DP uses right even for 2P profile');
  await screenshot('dp');

  waitingQueue = false; mode = 'SP'; side = 1;
  await reset(); send('点歌 Preview'); await stage('open');
  send('1'); await stage('screen');
  send('点歌 Preview'); await stage('open');
  assert.equal(await page.locator('.candidate').count(), 5, 'new choices reopen during closing');
  // Use a short-lived candidate to exercise cancellation before full extension.
  selectionTimeout = 1;
  await reset(); await trace(); send('点歌 Preview'); await stage('extending'); await stage('stowed');
  assert.ok(!(await page.evaluate(() => window.armTrace)).some(s => s.stage === 'open'));

  effects = false; selectionTimeout = 60;
  await reset(); send('点歌 Preview'); await stage('open');
  send('1'); await stage('stowed');
  assert.equal(await page.locator('.candidate').count(), 0);

  effects = true; selectionTimeout = 5; mode = 'SP'; side = 1;
  await reset(); send('点歌 Preview'); await stage('open'); await stage('stowed');
  await page.getByText('测试观众：选择超时，请重新点歌', { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log('PASS mechanical screen: production bundle, unchanged chat bounds, SP/DP origins, paging, selection, timeout, disabled effects and side-change retraction');
} finally { await browser.close(); }
