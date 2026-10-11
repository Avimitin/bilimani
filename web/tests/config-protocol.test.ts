import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { zlibSync } from 'fflate';
import { parseConfig } from '../apps/iidx/config';
import { iidxRules } from '../apps/iidx/rules';
import { packet, decode, chatFromCommand, ProtocolError } from '../packages/platform-bilibili/src/protocol';
import { parseSession } from '../packages/platform-bilibili/src/index';
test('runtime configuration rejects invalid policy and never falls back to storage',()=>{
  const example=JSON.parse(readFileSync(new URL('../apps/iidx/config.json',import.meta.url),'utf8'));
  assert.throws(()=>parseConfig(example),/authCode/);
  example.platform.enabled=false;assert.equal(parseConfig(example).display.feedLimit,10);
  example.requests.candidateLimit=-1;assert.throws(()=>parseConfig(example),/candidateLimit/);
});
test('IIDX grammar and colors stay outside shared controller',()=>{
  const rules=iidxRules();
  assert.deepEqual(rules.parse('别名 dpa','SP'),{query:'别名',mode:'DP',chart:'DPA'});
  assert.equal(rules.chartStyle('SPL'),'purple');
});

test('independent pages read identity from URL without a room mapping or stored credential',()=>{
  const example=JSON.parse(readFileSync(new URL('../apps/iidx/config.json',import.meta.url),'utf8'));
  assert.equal(parseConfig(example,'?authCode=one').platform.authCode,'one');
  assert.equal(parseConfig(example,'?authCode=two%2Bthree').platform.authCode,'two+three');
  assert.equal(example.platform.authCode,undefined);
  assert.throws(()=>parseConfig(example,'?authCode=a&authCode=b'),/无效/);
  example.platform.authCode='ignored-old-value';assert.throws(()=>parseConfig(example),/URL/);
});
test('Bilibili frames support concatenation, compression and stable identities',()=>{
  const session = {game_info:{game_id:'test'},websocket_info:{wss_link:['wss://example.test/chat'],auth_body:'{"protover":3}'},anchor_info:{room_id:1,uname:'Example'}};
  assert.equal(JSON.parse(parseSession(session).auth).protover,2);
  assert.deepEqual(parseSession(session).room,{id:'1',name:'Example',title:''});
  assert.throws(()=>parseSession({...session,websocket_info:{...session.websocket_info,auth_body:'null'}}));
  const value={cmd:'LIVE_OPEN_PLATFORM_DM',data:{open_id:'user-1',uname:'Alice',msg:'点歌 AA',msg_id:'event-1'}};
  const command=packet(5,JSON.stringify(value));
  const auth=packet(8,'{"code":0}');const combined=new Uint8Array(auth.length+command.length);combined.set(auth);combined.set(command,auth.length);
  assert.equal(decode(combined).length,2);
  const compressed=zlibSync(command);const wrapper=new Uint8Array(16+compressed.length);wrapper.set(packet(5,''));wrapper.set(compressed,16);const view=new DataView(wrapper.buffer);view.setUint32(0,wrapper.length);view.setUint16(6,2);
  const decoded=decode(wrapper)[0];assert.equal(decoded.type,'command');
  const chat=chatFromCommand(value);assert.equal(chat?.chat.user,'open:user-1');assert.equal(chat?.id,'event-1');
  assert.equal(chatFromCommand({...value,data:{...value.data,is_mirror:true}}),null);
  assert.throws(()=>decode(new Uint8Array(3)),ProtocolError);
  assert.throws(()=>decode(command.slice(0,-1)),ProtocolError);
});
