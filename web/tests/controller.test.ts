import test from 'node:test';
import assert from 'node:assert/strict';
import { Controller, type Policy, type GameRules } from '@bilimani/controller';
import { ApiError, type GameClient, type GameState, type QueueEntry, type Song } from '@bilimani/game-api';
const policy: Policy = { prefix:'点歌',candidateLimit:20,selectionTimeoutSeconds:60,cooldownSeconds:0,maxPendingUsers:100 };
const rules: GameRules = { parse:(query,mode)=>({query,mode}), chartStyle:()=> 'neutral' };
const song = (id:number): Song => ({id,title:`Song ${id}`,charts:[{chart:{mode:'test-mode',id:'expert'},level:'15.5'}]});
function fixture() {
  let time=0;
  let state: GameState = {ready:true,phase:'select',mode:'test-mode',epoch:1,plays:0,now_playing:null,queue:{revision:0,capacity:5,current:null,items:[],events:[]}};
  const submissions: unknown[]=[];
  const api: GameClient = {
    state: async()=>structuredClone(state), search:async()=>[song(1)],
    enqueue:async input=>{submissions.push(input);const entry:QueueEntry={id:submissions.length,song_id:input.song_id,title:`Song ${input.song_id}`,mode:input.mode,chart:input.chart??null,requester:input.requester,error:null};
      state.queue.items.push({...entry,executing:false});state.queue.events.push({id:++state.queue.revision,kind:'added',entry});return entry;},
    select:async()=>{throw new Error('controller must not select directly');},operation:async()=>{throw new Error('controller must not schedule selections');},
  };
  return { api, submissions, advance:(n:number)=>{time+=n;}, state:(s:Partial<GameState>)=>{state={...state,...s};},
    selected:()=>{const entry=state.queue.items.shift()!;state.queue.current={...entry,remaining_seconds:600,duration_seconds:600};state.queue.events.push({id:++state.queue.revision,kind:'selected',entry});},
    remove:()=>{const entry=state.queue.current!;state.queue.current=null;state.queue.events.push({id:++state.queue.revision,kind:'removed',entry});},
    create:()=>new Controller(api,rules,policy,10,()=>time) };
}
const chat=(user='a',text='点歌 song')=>({user,name:user,text});
test('only requesting identity can confirm; candidate pages do not extend deadline',async()=>{
  const f=fixture();f.api.search=async()=>Array.from({length:9},(_,i)=>song(i+1));
  const c=f.create();await c.tick();await c.chat(chat());await c.chat(chat('b','2'));
  assert.equal(f.submissions.length,0);assert.equal(c.view().pending.length,1);
  f.advance(10);await c.chat(chat('a','n'));assert.equal(c.view().pending[0].page,1);assert.equal(c.view().pending[0].remaining,50);
  await c.chat(chat('a','6'));await c.tick();assert.equal(c.view().queue[0].title,'Song 6');assert.equal(c.view().pending.length,0);
  assert.deepEqual(f.submissions,[{song_id:6,mode:'test-mode',chart:undefined,requester:'a'}]);
});
test('DLL owns selection and menu deletion; page only displays snapshots and events',async()=>{
  const f=fixture(),c=f.create();await c.tick();await c.chat(chat());await c.chat(chat('b'));await c.tick();
  assert.equal(c.view().queue.length,2);assert.equal(c.view().current,null);
  f.selected();await c.tick();assert.equal(c.view().queue.length,1);assert.equal(c.view().current?.title,'Song 1');
  await c.tick();assert.equal(f.submissions.length,2);
  f.remove();await c.tick();assert.equal(c.view().current,null);assert.match(c.view().feed.at(-1)!.text,/移除/);
});
test('refresh reads existing queue/current without replaying POST or old feed',async()=>{
  const f=fixture(),c=f.create();await c.tick();await c.chat(chat());await c.chat(chat('b'));f.selected();await c.tick();c.stop();
  const fresh=f.create();await fresh.tick();assert.equal(fresh.view().queue.length,1);assert.equal(fresh.view().current?.title,'Song 1');assert.equal(fresh.view().feed.length,0);assert.equal(f.submissions.length,2);
});
test('uncertain POST is not retried; later authoritative state still shows accepted request',async()=>{
  const f=fixture(),enqueue=f.api.enqueue;f.api.enqueue=async input=>{await enqueue(input);throw new Error('response lost');};
  const c=f.create();await c.tick();await c.chat(chat());assert.match(c.view().feed.at(-1)!.text,/结果不明/);await c.tick();await c.tick();
  assert.equal(f.submissions.length,1);assert.equal(c.view().queue.length,1);
});
test('obsolete searches cannot replace newer request or enqueue after source suspension',async()=>{
  const f=fixture();let resolve!:(songs:Song[])=>void;
  f.api.search=query=>query==='old'?new Promise(r=>{resolve=r;}):Promise.resolve([song(2)]);
  const c=f.create();await c.tick();const old=c.chat(chat('a','点歌 old'));await c.chat(chat('a','点歌 new'));resolve([song(1)]);await old;await c.tick();
  assert.deepEqual(c.view().queue.map(r=>r.title),['Song 2']);
  const late=c.chat(chat('a','点歌 old'));c.suspendChat();resolve([song(3)]);await late;assert.equal(f.submissions.length,1);
  const stopped=c.chat(chat('a','点歌 old'));c.stop();resolve([song(4)]);await stopped;assert.equal(f.submissions.length,1);
});
test('expiry and server queue full are explicit; concurrent confirmation submits once',async()=>{
  const f=fixture(),c=f.create();await c.tick();f.api.search=async()=>[song(1),song(2)];await c.chat(chat());f.advance(61);await c.tick();assert.equal(c.view().pending.length,0);
  f.api.search=async()=>[song(1)];f.api.enqueue=async()=>{throw new ApiError(409,'queue_full','队列已满');};await c.chat(chat());assert.match(c.view().feed.at(-1)!.text,/队列已满/);
  let done!:(entry:QueueEntry)=>void;let count=0;f.api.enqueue=()=>{count++;return new Promise(r=>{done=r;});};
  const first=c.chat(chat());await Promise.resolve();await c.chat(chat());assert.equal(count,1);
  done({id:1,song_id:1,title:'Song 1',mode:'test-mode',chart:null,requester:'a',error:null});await first;
});
