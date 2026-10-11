// Production bundles + real Rust API; mock platform traffic and targeted user-API failures.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
const base = process.env.BILIMANI_TEST_URL ?? 'http://127.0.0.1:32134';
const example = JSON.parse(await readFile(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
example.display.infoRotationSeconds = 1;
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
    let vfdEffects=true;
    await page.route('**/config.json', route=>route.fulfill({json:invalidConfig ? {} : {...example,display:{...example.display,vfdEffects}}}));
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
    if (style === 'mecha') {
      const nativeUser = await (await fetch(`${base}/api/v1/iidx/user`)).json();
      assert.equal(nativeUser.players[0].dj_name, 'PREVIEW');
      const chart = await page.locator('.song-density svg').elementHandle();
      await page.locator('.identity-viewport[data-page="song"]').waitFor();
      await page.locator('.identity-viewport[data-page="user"]').waitFor();
      assert.equal(await page.locator('.profile-name').textContent(), 'PREVIEW');
      assert.deepEqual(await page.locator('.profile-rank .profile-value').allTextContents(), ['皆伝','八段']);
      assert.equal(await page.locator('.profile-stat').first().locator('.vfd-readable').textContent(), '26037');
      await page.waitForTimeout(920); // Capture the completed scroll, not its midpoint.
      await page.screenshot({path:new URL('../../analysis/browser/mecha-user.png',import.meta.url).pathname.replace(/^\/([A-Z]:)/,'$1'),fullPage:true});
      await page.locator('.identity-viewport[data-page="song"]').waitFor();
      assert.ok(await chart.evaluate(el=>el.isConnected), 'rotation preserves the chart DOM');
      await page.emulateMedia({reducedMotion:'reduce'});
      await wait(()=>page.locator('.identity-viewport').getAttribute('data-reset').then(value=>value==='false'));
      assert.equal(await page.locator('.identity-track').evaluate(el=>getComputedStyle(el).transitionDuration), '0.9s', 'broadcast effects do not inherit desktop reduced motion');
      // Check rendered intermediate values, not just the presence of animations:
      // steps(1) on the whole timeline used to skip every jitter keyframe.
      await page.evaluate(()=>new Promise((resolve,reject)=>{
        const started=performance.now();
        let moved=false, flashed=false;
        const sample=()=>{
          moved ||= [...document.querySelectorAll('.identity-slide')]
            .some(el=>Math.abs(new DOMMatrix(getComputedStyle(el).transform).m41)>=.2);
          flashed ||= [...document.querySelectorAll('.current-title,.profile-name,.profile-value,.profile-number,.song-bpm .vfd-digits')]
            .some(el=>Number(getComputedStyle(el).opacity)<.5);
          if([...document.querySelectorAll('.song-bpm .vfd-digits')].some(el=>el.getAnimations().length)) return reject(new Error('BPM must remain steady'));
          if(moved&&flashed) return resolve();
          if(performance.now()-started>17000) return reject(new Error(`VFD intermediate frames missing: moved=${moved}, flashed=${flashed}`));
          requestAnimationFrame(sample);
        };sample();
      }));
      await page.route('**/api/v1/iidx/user', route=>route.fulfill({json:{...nativeUser, players:nativeUser.players.map(p=>({...p,weekly:null}))}}));
      await wait(()=>page.locator('.profile-stat .vfd-readable').allTextContents().then(values=>values.join(',')==='26037,—,—'));
      await page.unroute('**/api/v1/iidx/user');
      await page.route('**/api/v1/iidx/user', route=>route.fulfill({status:503,json:{error:{code:'unavailable'}}}));
      await wait(()=>page.locator('.profile-name').count().then(n=>n===0));
      assert.equal(await page.locator('.identity-viewport').getAttribute('data-page'), 'song');
      assert.equal(await page.locator('.connection-banner:visible').count(),0,'player API failure does not stop chat/queue');
      assert.equal((await (await fetch(`${base}/api/v1/queue`)).json()).current.song_id,2);
      await page.unroute('**/api/v1/iidx/user');
      await page.locator('.profile-name').waitFor({state:'attached'});
      vfdEffects=false;
    }
    socket=undefined;await page.reload();await wait(async()=>!!socket);
    await wait(rendered);
    if (style === 'mecha') assert.equal(await page.locator('.identity-track').evaluate(el=>getComputedStyle(el).transitionDuration),'0s','config can disable broadcast effects');
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
