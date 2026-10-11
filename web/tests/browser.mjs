// Production bundles + real Rust API; only the external live platform is mocked.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
const base = process.env.BILIMANI_TEST_URL ?? 'http://127.0.0.1:32134';
const example = JSON.parse(await readFile(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
const browser = await chromium.launch({ headless: true, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
const packet = (op, value) => {
  const body = Buffer.from(JSON.stringify(value)); const bytes = Buffer.alloc(16 + body.length);
  bytes.writeUInt32BE(bytes.length, 0); bytes.writeUInt16BE(16, 4); bytes.writeUInt16BE(1, 6); bytes.writeUInt32BE(op, 8); bytes.writeUInt32BE(1, 12); body.copy(bytes, 16); return bytes;
};
const wait = async (check) => { for (let i=0;i<100;i++) { if (await check()) return; await new Promise(r=>setTimeout(r,100)); } throw new Error('UI condition timed out'); };
try {
  await mkdir(new URL('../../analysis/browser/', import.meta.url), { recursive:true });
  for (const style of ['card','mecha']) {
    const prior = await (await fetch(`${base}/api/v1/queue`)).json();
    for (const item of [...prior.items, ...(prior.current ? [prior.current] : [])]) assert.ok((await fetch(`${base}/api/v1/queue/${item.id}`, {method:'DELETE'})).ok);
    const page = await browser.newPage({ viewport: style === 'card' ? {width:480,height:800} : {width:1920,height:1080} });
    page.setDefaultTimeout(5000);
    const errors=[]; page.on('pageerror',error=>errors.push(String(error)));
    let socket;
    let starts=0, ends=0;
    await page.addInitScript(() => { window.obsstudio = {pluginVersion: "test"}; });
    let invalidConfig=false;
    await page.route('**/config.json', route=>route.fulfill({json:invalidConfig ? {} : example}));
    await page.route('**/_proxy/relay/api/open_live/*', route=>{
      const action = route.request().url().split('/').at(-1);
      if (action === 'start_game') { starts++; assert.equal(route.request().postDataJSON().code, 'LOCAL-TEST-ONLY'); }
      if (action === 'end_game') ends++;
      assert.equal(route.request().headers().referer, undefined, 'authCode URL must not leak as Referer');
      return route.fulfill({json:{code:0,data:{game_info:{game_id:'fixture'},websocket_info:{wss_link:['wss://fixture.test/chat'],auth_body:'{}'},anchor_info:{room_id:1,uname:'Fixture Room'}}}});
    });
    await page.routeWebSocket('wss://fixture.test/chat', ws=>{
      socket=ws;
      ws.onMessage(data=>{const op=Buffer.from(data).readUInt32BE(8);if(op===7) ws.send(packet(8,{code:0}));else if(op===2) ws.send(packet(3,{}));});
    });
    await page.goto(`${base}/iidx/${style}/?authCode=LOCAL-TEST-ONLY`);
    await wait(async () => !!socket);
    const rendered = () => page.locator('#current-content > *').count();
    await wait(rendered);
    await wait(()=>page.locator('.connection-banner:visible').count().then(n=>n===0));
    assert.ok(socket,'platform websocket connected');
    let id=0;
    const send=(text)=>socket.send(packet(5,{cmd:'LIVE_OPEN_PLATFORM_DM',data:{open_id:'fixture-user',uname:'测试观众',msg:text,msg_id:String(++id)}}));
    send('点歌 Preview');
    await page.getByText('测试观众，请选择',{exact:true}).waitFor();
    assert.equal(await page.locator('.candidate').count(),5);
    send('n');await wait(()=>page.locator('.candidate-number').first().textContent().then(text=>text==='6'));
    send('p');await wait(()=>page.locator('.candidate-number').first().textContent().then(text=>text==='1'));
    send('2');
    await wait(async () => (await (await fetch(`${base}/api/v1/queue`)).json()).current?.song_id === 2);
    await page.getByText('已定位：Preview Song 2',{exact:true}).waitFor();
    if (style === 'card') assert.equal(await page.locator('.current-title').textContent(),'Preview Song 2');
    assert.equal(await page.locator('[role=alert]').count(),0);
    await page.screenshot({path:new URL(`../../analysis/browser/${style}.png`,import.meta.url).pathname.replace(/^\/([A-Z]:)/,'$1'),fullPage:true});
    socket=undefined;await page.reload();await wait(async()=>!!socket);
    await wait(rendered);
    await wait(()=>page.locator('.connection-banner:visible').count().then(n=>n===0));
    if (style === 'card') await wait(()=>page.locator('.current-title').textContent().then(text=>text==='Preview Song 2'));
    assert.equal((await (await fetch(`${base}/api/v1/queue`)).json()).current.song_id,2,'refresh preserves DLL current request');
    assert.equal(await page.getByRole('button',{name:'跳过当前点歌',includeHidden:true}).count(),0,'OBS is display only');
    const beforeStarts=starts, beforeEnds=ends;
    await page.evaluate(()=>window.dispatchEvent(new CustomEvent('obsSourceActiveChanged',{detail:{active:false}})));
    await wait(async()=>ends>beforeEnds);
    socket=undefined;
    await page.evaluate(()=>window.dispatchEvent(new CustomEvent('obsSourceActiveChanged',{detail:{active:true}})));
    await wait(async()=>!!socket && starts===beforeStarts+1);
    await wait(()=>page.locator('.connection-banner:visible').count().then(n=>n===0));
    const current=(await (await fetch(`${base}/api/v1/queue`)).json()).current;
    assert.ok((await fetch(`${base}/api/v1/queue/${current.id}`,{method:'DELETE'})).ok);
    await page.getByText('已移除点歌：Preview Song 2',{exact:true}).waitFor();
    send('点歌 Preview');await page.getByText('测试观众，请选择',{exact:true}).waitFor();
    socket=undefined;await page.reload();await wait(async()=>!!socket);await wait(rendered);await wait(()=>page.locator('.connection-banner:visible').count().then(n=>n===0));
    assert.equal(await page.locator('.choice-panel').count(),0,'refresh clears candidates');
    invalidConfig=true;await page.reload();await page.getByRole('alert').waitFor();
    assert.match(await page.getByRole('alert').textContent(),/platform/);
    assert.deepEqual(errors,[]);
    await page.close();console.log(`PASS ${style}: packaged resources, config, browser chat, URL identity, OBS activation, real queue/selection, refresh preserves queue`);
  }
} finally { await browser.close(); }
