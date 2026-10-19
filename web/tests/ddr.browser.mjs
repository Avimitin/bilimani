import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
const browser = await chromium.launch({ headless: true, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  const config = JSON.parse(await readFile('web/apps/ddr/config.json', 'utf8'));
  config.platform.enabled = false;
  await page.route('**/config.json', route => route.fulfill({ json: config }));
  await page.goto('http://127.0.0.1:32134/ddr/street/');
  await page.locator('#title').filter({ hasText: 'Streetlight Shuffle' }).waitFor();
  await page.evaluate(() => document.fonts.ready);
  await page.locator('.difficulty-name').filter({ hasText: 'EXPERT' }).waitFor();
  assert.equal(await page.locator('.selected-difficulty').count(), 1);
  assert.equal(await page.locator('.difficulty-level').innerText(), '8');
  assert.equal(await page.locator('#players').innerText(), '1P / PREVIEW');
  const bounds = await page.locator('.game-window').boundingBox();
  assert.deepEqual(bounds, { x: 48, y: 174, width: 1280, height: 720 });
  assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--green').trim()), '#00d88d');
  assert.ok(await page.locator('.graffiti').evaluate(img => img.complete && img.naturalWidth > 0));
  const artBounds = await page.locator('.graffiti').boundingBox();
  assert.ok(artBounds.x < bounds.x + bounds.width && artBounds.y + artBounds.height > bounds.y, 'Graffiti overlaps the capture edge');
  assert.ok(artBounds.x < 48 && artBounds.y > 720, 'Mural occupies the lower-left corner');
  assert.equal(await page.locator('.floor-tag').count(), 0);
  const cameraBounds = await page.locator('.camera-window').boundingBox();
  assert.deepEqual(cameraBounds, { x: 1344, y: 734, width: 544, height: 306 });
  assert.equal(cameraBounds.width / cameraBounds.height, 1280 / 720);
  assert.ok(await page.locator('.camera-spray').evaluate(img => img.complete && img.naturalWidth > 0));
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth), 1920);
  const chartBounds = await page.locator('#charts').boundingBox();
  assert.ok(chartBounds.y >= 922 && chartBounds.y + chartBounds.height <= 1080);
  assert.deepEqual(errors, []);
  await mkdir('analysis/ddr', { recursive: true });
  await page.screenshot({ path: 'analysis/ddr/street-preview.png', omitBackground: true });
  const queued = [];
  try {
    for (const song_id of [2, 3, 4, 5, 1]) {
      queued.push(await fetch('http://127.0.0.1:32134/api/v1/queue', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({song_id, mode: 'DP', chart: 'EDP', requester: 'Preview dancer'}) }).then(r => r.json()));
    }
    await page.locator('#queue li').filter({ hasText: 'Neon Footwork' }).waitFor();
    await page.reload();
    await page.locator('#queue li').filter({ hasText: 'Neon Footwork' }).waitFor();
    await page.waitForFunction(() => document.querySelectorAll('#queue li').length === 3);
    const queueBounds = await page.locator('.queue').boundingBox();
    const lastRow = await page.locator('#queue li').last().boundingBox();
    assert.ok(lastRow.y + lastRow.height <= queueBounds.y + queueBounds.height, 'Three queue rows fit below danmu');
    assert.equal(await page.locator('#count').innerText(), '05');
    const chatBounds = await page.locator('.chat').boundingBox();
    assert.ok(chatBounds.height >= 250 && chatBounds.y + chatBounds.height <= queueBounds.y);
    assert.ok(chatBounds.y < 100 && queueBounds.y + queueBounds.height < cameraBounds.y, 'Chat and queue fit above the camera');
    await page.screenshot({ path: 'analysis/ddr/street-queue-preview.png', omitBackground: true });
  } finally { for (const entry of queued) await fetch(`http://127.0.0.1:32134/api/v1/queue/${entry.id}`, {method: 'DELETE'}); }
  console.log('DDR frame: telemetry, charts, player, fonts and 1920x1080 layout passed');
} finally { await browser.close(); }
