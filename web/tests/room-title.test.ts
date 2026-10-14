import test from 'node:test';
import assert from 'node:assert/strict';
import { BilibiliSource, parseRoomTitle } from '../packages/platform-bilibili/src/index';
import { packet } from '../packages/platform-bilibili/src/protocol';
import type { PlatformEvent } from '../packages/controller/src/index';

test('room titles require successful responses and the expected room', () => {
  const response = { code: 0, data: { room_id: 123, title: ' DDR night ' } };
  assert.equal(parseRoomTitle(response, '123'), 'DDR night');
  assert.equal(parseRoomTitle(response, '456'), null);
  assert.equal(parseRoomTitle({ ...response, code: -352 }, '123'), null);
  for (const title of [null, 1, '', 'a\nb', 'x'.repeat(257)]) {
    assert.equal(parseRoomTitle({ code: 0, data: { room_id: 123, title } }, '123'), null);
  }
});

for (const outcome of ['success', 'failure', 'stopped'] as const) {
  test(`room metadata ${outcome} does not disrupt the chat session`, async t => {
    const events: PlatformEvent[] = [];
    let socket: FakeSocket;
    class FakeSocket {
      static OPEN = 1;
      readyState = 1;
      onopen: (() => void) | null = null;
      onmessage: ((event: { data: ArrayBuffer }) => void) | null = null;
      onclose: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor() { socket = this; }
      send() {}
      close() { this.readyState = 3; }
    }
    const originalSocket = globalThis.WebSocket;
    globalThis.WebSocket = FakeSocket as unknown as typeof WebSocket;
    t.after(() => { globalThis.WebSocket = originalSocket; });
    let finishTitle!: (value: Response) => void;
    const titleResponse = new Promise<Response>(resolve => { finishTitle = resolve; });
    let metadataSignal: AbortSignal | undefined;
    let titleRequests = 0;
    t.mock.method(globalThis, 'fetch', async (input: string | URL | Request, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/start_game')) return Response.json({ code: 0, data: {
        game_info: { game_id: 'test' }, websocket_info: { wss_link: ['wss://example.test'], auth_body: '{}' },
        anchor_info: { room_id: 123, uname: 'Dancer' },
      } });
      if (url.includes('/get_info?')) {
        titleRequests++;
        assert.equal(url, './_proxy/bilibili/room/v1/Room/get_info?room_id=123');
        assert.equal(init?.referrerPolicy, 'no-referrer');
        assert.equal(init?.credentials, 'omit');
        metadataSignal = init?.signal as AbortSignal;
        return titleResponse;
      }
      return Response.json({ code: 0 });
    });
    const config = { enabled: true, authCode: 'private-test-code', relayUrl: './_proxy/relay', roomInfoUrl: './_proxy/bilibili' };
    const titleState = { requested: false, value: null };
    const source = new BilibiliSource(config, titleState);
    t.after(() => source.stop());
    source.start(event => events.push(event));
    await new Promise(resolve => setImmediate(resolve));
    socket!.onmessage!({ data: packet(8, '{"code":0}').buffer as ArrayBuffer });
    assert.ok(metadataSignal);
    if (outcome === 'stopped') source.stop();
    finishTitle(outcome === 'failure' ? new Response('', { status: 412 }) : Response.json({ code: 0, data: { room_id: 123, title: 'DDR night' } }));
    await new Promise(resolve => setImmediate(resolve));
    const rooms = events.filter(e => e.type === 'room');
    assert.equal(rooms.at(-1)?.room.title, outcome === 'success' ? 'DDR night' : '');
    if (outcome === 'stopped') assert.equal(metadataSignal.aborted, true);
    else assert.equal(socket!.readyState, 1);
    source.stop();
    const resumed = new BilibiliSource(config, titleState);
    t.after(() => resumed.stop());
    resumed.start(event => events.push(event));
    await new Promise(resolve => setImmediate(resolve));
    socket!.onmessage!({ data: packet(8, '{"code":0}').buffer as ArrayBuffer });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(titleRequests, 1, 'reconnecting or showing OBS must not query the title again');
    assert.equal(events.filter(e => e.type === 'room').at(-1)?.room.title, outcome === 'success' ? 'DDR night' : '');
  });
}
