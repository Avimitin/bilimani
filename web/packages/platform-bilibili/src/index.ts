import type { ChatSource, PlatformEvent, Room } from '@bilimani/controller';
import { chatFromCommand, decode, packet } from './protocol';
export interface BilibiliConfig { enabled: boolean; authCode: string; relayUrl: string; roomInfoUrl?: string }
export interface RoomTitleState { requested: boolean; value: { id: string; title: string } | null }
interface Session { game_id: string; urls: string[]; auth: string; room: Room }
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
export function parseSession(data: unknown): Session {
  if (!record(data) || !record(data.game_info) || !record(data.websocket_info)) throw new Error('直播接口返回了无效会话');
  const { game_id } = data.game_info;
  const { wss_link: urls, auth_body } = data.websocket_info;
  if (typeof game_id !== 'string' || !game_id || !Array.isArray(urls) || !urls.length || !urls.every((url: unknown): url is string => typeof url === 'string' && url.startsWith('wss://')) || typeof auth_body !== 'string') throw new Error('直播接口返回了无效会话');
  const credentials: unknown = JSON.parse(auth_body);
  if (!record(credentials)) throw new Error('直播接口返回了无效认证信息');
  const anchor = record(data.anchor_info) ? data.anchor_info : {};
  return {
    game_id, urls,
    // Negotiate zlib explicitly; no native Brotli dependency in the page.
    auth: JSON.stringify({ ...credentials, protover: 2 }),
    room: { id: typeof anchor.room_id === 'number' ? String(anchor.room_id) : '', name: typeof anchor.uname === 'string' ? anchor.uname : '', title: '' },
  };
}
class RelayError extends Error {
  constructor(readonly code: number) { super(`直播连接失败（平台返回 ${code}）`); }
}
export function parseRoomTitle(value: unknown, roomId: string): string | null {
  if (!record(value) || value.code !== 0 || !record(value.data)) return null;
  const { room_id, title } = value.data;
  if (String(room_id) !== roomId || typeof title !== 'string' || title.length > 256 || /[\x00-\x1f\x7f]/.test(title)) return null;
  return title.trim() || null;
}
const delay = (ms: number, signal: AbortSignal) => new Promise<void>(resolve => {
  if (signal.aborted) { resolve(); return; }
  const stop = () => { clearTimeout(timer); signal.removeEventListener('abort', stop); resolve(); };
  const timer = setTimeout(stop, ms);
  signal.addEventListener('abort', stop, { once: true });
});

export class BilibiliSource implements ChatSource {
  private lifetime: AbortController | null = null;
  private socket: WebSocket | null = null;
  private session: Session | null = null;
  private seen = new Set<string>();
  constructor(private config: BilibiliConfig, private roomTitle: RoomTitleState = { requested: false, value: null }) {}
  start(emit: (event: PlatformEvent) => void): void {
    if (this.lifetime) return;
    if (!this.config.enabled) { emit({ type: 'connection', connected: false, message: '弹幕连接已关闭' }); return; }
    const lifetime = this.lifetime = new AbortController();
    void this.run(emit, lifetime.signal);
  }
  stop(): void {
    this.lifetime?.abort();
    this.socket?.close();
    this.end();
  }
  private end(): void {
    const session = this.session;
    this.session = null;
    if (session) void fetch(`${this.config.relayUrl}/api/open_live/end_game`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ game_id: session.game_id, app_id: 0 }), keepalive: true,
    }).catch(() => {});
  }
  private async api(action: string, body: unknown, signal: AbortSignal): Promise<unknown> {
    const response = await fetch(`${this.config.relayUrl}/api/open_live/${action}`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
      signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]), cache: 'no-store',
    });
    if (!response.ok) throw new RelayError(response.status);
    const value: unknown = await response.json();
    if (!record(value) || typeof value.code !== 'number') throw new Error('直播接口返回了无效响应');
    if (value.code !== 0) throw new RelayError(Number(value.code));
    return value.data;
  }
  private async run(emit: (event: PlatformEvent) => void, signal: AbortSignal): Promise<void> {
    let attempt = 0;
    while (!signal.aborted) {
      emit({ type: 'connection', connected: false, message: attempt ? '弹幕连接中断，正在重连' : '正在连接弹幕' });
      try {
        const data = await this.api('start_game', { code: this.config.authCode, app_id: 0 }, signal);
        this.session = parseSession(data);
        if (signal.aborted) break;
        await this.connect(this.session, attempt % this.session.urls.length, emit, signal);
      } catch (error) {
        if (!signal.aborted) emit({ type: 'connection', connected: false, message: error instanceof RelayError ? error.message : '弹幕连接失败，请检查网络和 relay 的浏览器访问设置' });
        // Invalid identity codes cannot heal through automatic reconnects.
        if (error instanceof RelayError && error.code === 7007) break;
      } finally { this.end(); }
      await delay(Math.min(30_000, 1000 * 2 ** Math.min(attempt++, 5)), signal);
    }
  }
  private async loadRoomTitle(session: Session, emit: (event: PlatformEvent) => void, signal: AbortSignal): Promise<void> {
    if (this.roomTitle.requested || signal.aborted || !this.config.roomInfoUrl || !/^[1-9]\d*$/.test(session.room.id)) return;
    this.roomTitle.requested = true;
    try {
      const response = await fetch(`${this.config.roomInfoUrl}/room/v1/Room/get_info?room_id=${encodeURIComponent(session.room.id)}`, {
        cache: 'no-store', credentials: 'omit', referrerPolicy: 'no-referrer',
        signal: AbortSignal.any([signal, AbortSignal.timeout(8000)]),
      });
      if (response.ok) {
        const title = parseRoomTitle(await response.json(), session.room.id);
        if (!signal.aborted && this.session === session && title !== null) {
          this.roomTitle.value = { id: session.room.id, title };
          session.room = { ...session.room, title };
          emit({ type: 'room', room: session.room });
        }
      }
    } catch { /* Optional metadata must never interrupt chat or requests. */ }
  }
  private connect(session: Session, index: number, emit: (event: PlatformEvent) => void, signal: AbortSignal): Promise<void> {
    return new Promise((resolve, reject) => {
      const metadata = new AbortController();
      const socket = this.socket = new WebSocket(session.urls[index]);
      socket.binaryType = 'arraybuffer';
      let done = false, authenticated = false, lastHeartbeat = Date.now(), lastApi = Date.now(), heartbeatPending = false;
      const finish = (error?: Error) => {
        if (done) return;
        done = true;
        metadata.abort();
        clearInterval(timer);
        signal.removeEventListener('abort', stop);
        socket.onclose = socket.onerror = socket.onmessage = socket.onopen = null;
        socket.close();
        if (this.socket === socket) this.socket = null;
        error ? reject(error) : resolve();
      };
      const stop = () => finish();
      const timer = setInterval(() => {
        const now = Date.now();
        if (now - lastHeartbeat > (authenticated ? 65_000 : 15_000)) { finish(new Error('弹幕心跳超时')); return; }
        if (authenticated && socket.readyState === WebSocket.OPEN) socket.send(packet(2, '{}'));
        if (authenticated && !heartbeatPending && now - lastApi >= 20_000) {
          heartbeatPending = true; lastApi = now;
          void this.api('game_heartbeat', { game_id: session.game_id }, signal).catch(error => finish(error)).finally(() => { heartbeatPending = false; });
        }
      }, 5000);
      signal.addEventListener('abort', stop, { once: true });
      socket.onopen = () => socket.send(packet(7, session.auth));
      socket.onerror = () => finish(new Error('弹幕 WebSocket 失败'));
      socket.onclose = () => finish(new Error('弹幕 WebSocket 已关闭'));
      socket.onmessage = event => {
        try {
          if (!(event.data instanceof ArrayBuffer)) throw new Error('非二进制弹幕报文');
          for (const message of decode(new Uint8Array(event.data))) {
            switch (message.type) {
              case 'auth':
                if (message.code !== 0) throw new RelayError(message.code);
                if (authenticated) break;
                authenticated = true; lastHeartbeat = Date.now();
                emit({ type: 'connection', connected: true, message: '弹幕已连接' });
                if (this.roomTitle.value?.id === session.room.id) session.room = { ...session.room, title: this.roomTitle.value.title };
                emit({ type: 'room', room: session.room });
                void this.loadRoomTitle(session, emit, AbortSignal.any([signal, metadata.signal]));
                socket.send(packet(2, '{}'));
                break;
              case 'heartbeat': lastHeartbeat = Date.now(); break;
              case 'command': {
                if (!authenticated) break;
                const chat = chatFromCommand(message.value);
                if (!chat || (chat.id && this.seen.has(chat.id))) break;
                if (chat.id) { this.seen.add(chat.id); if (this.seen.size > 4096) this.seen.delete(this.seen.values().next().value!); }
                emit({ type: 'chat', chat: chat.chat });
                break;
              }
            }
          }
        } catch (error) { finish(error instanceof Error ? error : new Error('无效弹幕')); }
      };
      if (signal.aborted) finish();
    });
  }
}
