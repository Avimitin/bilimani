import { ApiError, type GameClient, type GameState, type Song, type QueueEntry } from '@bilimani/game-api';
export interface Chat { user: string; name: string; text: string }
export interface Room { id: string; name: string; title: string }
export type PlatformEvent = { type: 'chat'; chat: Chat } | { type: 'connection'; connected: boolean; message: string } | { type: 'room'; room: Room };
export interface ChatSource { start(emit: (event: PlatformEvent) => void): void; stop(): void }
export interface ParsedRequest { query: string; mode: string; chart?: string }
export interface GameRules {
  parse(text: string, mode: string): ParsedRequest;
  chartStyle(chart?: string): string;
}
export interface Policy {
  prefix: string; candidateLimit: number; selectionTimeoutSeconds: number; cooldownSeconds: number; maxPendingUsers: number;
}
interface Pending extends ParsedRequest { name: string; songs: Song[]; until: number; page: number }
interface Feed { id: number; at: number; kind: 'chat' | 'event'; name: string; text: string }
const emptyGame = (): GameState => ({ ready: false, phase: 'other', mode: null, epoch: 0, plays: 0, now_playing: null,
  queue: { revision: 0, capacity: 0, current: null, items: [], events: [] } });
const supports = (song: Song, request: ParsedRequest) => song.charts.some(c => c.chart.mode === request.mode && (!request.chart || c.chart.id === request.chart));

/** One instance is one page session. No persistence, DOM, platform or game imports. */
export class Controller {
  private game = emptyGame();
  private online = false;
  private connected = false;
  private status = '等待弹幕连接';
  private room: Room | null = null;
  private eventCursor: number | null = null;
  private posting = new Set<string>();
  private pending = new Map<string, Pending>();
  private cooldowns = new Map<string, number>();
  private searches = new Map<string, number>();
  private feed: Feed[] = [];
  private sequence = 0;
  private generation = 0;
  private stopped = false;
  private ticking = false;
  constructor(private api: GameClient, private rules: GameRules, private policy: Policy,
    private feedLimit = 10, private now: () => number = () => performance.now() / 1000) {}

  event(event: PlatformEvent): void {
    if (this.stopped) return;
    switch (event.type) {
      case 'connection': this.connected = event.connected; this.status = event.message; if (!event.connected) this.room = null; break;
      case 'room': this.room = event.room; break;
      case 'chat': void this.chat(event.chat); break;
    }
  }
  private add(kind: Feed['kind'], text: string, name = '') {
    this.feed = [...this.feed, { id: ++this.sequence, at: this.now(), kind, name, text }].slice(-this.feedLimit);
  }
  private expire() {
    const now = this.now();
    for (const [user, pending] of this.pending) if (pending.until <= now) { this.pending.delete(user); this.add('event', `${pending.name}：选择超时，请重新点歌`); }
    for (const [user, until] of this.cooldowns) if (until <= now) this.cooldowns.delete(user);
  }
  async chat(chat: Chat): Promise<void> {
    if (this.stopped || !chat.user || chat.text.length > 1024) return;
    if (this.posting.has(chat.user)) return;
    this.expire();
    this.add('chat', chat.text, chat.name);
    const text = chat.text.trim();
    const prefix = this.policy.prefix;
    if (text === prefix || (text.startsWith(prefix) && /^\s/u.test(text.slice(prefix.length)))) {
      this.pending.delete(chat.user);
      const generation = ++this.generation;
      this.searches.delete(chat.user);
      const rest = text.slice(prefix.length).trim();
      if (!rest) { this.add('event', `${chat.name}：请输入曲名`); return; }
      if (!this.online || !this.game.ready || !this.game.mode) { this.add('event', '游戏模式或曲库尚未就绪'); return; }
      if ((this.cooldowns.get(chat.user) ?? 0) > this.now()) { this.add('event', `${chat.name}：点歌冷却中`); return; }
      if (this.pending.size + this.searches.size >= this.policy.maxPendingUsers) { this.add('event', '待选人数已满'); return; }
      const request = this.rules.parse(rest, this.game.mode);
      this.searches.set(chat.user, generation);
      try {
        const songs = await this.api.search(request.query, this.policy.candidateLimit);
        if (this.stopped || this.searches.get(chat.user) !== generation) return;
        if (request.mode !== this.game.mode) { this.add('event', `${chat.name}：游戏模式不符`); return; }
        if (songs.length === 0) this.add('event', `${chat.name}：没有找到匹配歌曲`);
        else if (songs.length === 1) await this.enqueue(chat, request, songs[0]);
        else if (this.pending.size < this.policy.maxPendingUsers) {
          this.pending.set(chat.user, { ...request, name: chat.name, songs, until: this.now() + this.policy.selectionTimeoutSeconds, page: 0 });
          this.add('event', `${chat.name}：请回复编号选择歌曲`);
        }
      } catch { if (!this.stopped && this.searches.get(chat.user) === generation) this.add('event', `${chat.name}：搜索失败，请重新点歌`); }
      finally { if (this.searches.get(chat.user) === generation) this.searches.delete(chat.user); }
      return;
    }
    const pending = this.pending.get(chat.user);
    if (!pending) return;
    if (/^[np]$/i.test(text)) {
      pending.page = Math.max(0, Math.min(Math.ceil(pending.songs.length / 5) - 1, pending.page + (text.toLowerCase() === 'n' ? 1 : -1)));
    } else if (/^\d+$/.test(text)) {
      const song = pending.songs[Number(text) - 1];
      if (!song) { this.add('event', `${chat.name}：候选编号无效`); return; }
      this.pending.delete(chat.user);
      await this.enqueue(chat, pending, song);
    }
  }
  private async enqueue(chat: Chat, request: ParsedRequest, song: Song) {
    if (request.mode !== this.game.mode) { this.add('event', `${chat.name}：游戏模式已改变`); return; }
    if (!supports(song, request)) { this.add('event', `${chat.name}：所请求谱面不存在`); return; }
    if ((this.cooldowns.get(chat.user) ?? 0) > this.now()) { this.add('event', `${chat.name}：点歌冷却中`); return; }
    this.posting.add(chat.user);
    try {
      await this.api.enqueue({ song_id: song.id, mode: request.mode, chart: request.chart, requester: chat.name });
      if (!this.stopped && this.policy.cooldownSeconds > 0) this.cooldowns.set(chat.user, this.now() + this.policy.cooldownSeconds);
    } catch (error) {
      if (!this.stopped) this.add('event', error instanceof ApiError ? `${chat.name}：${error.message}` : `入队结果不明：${song.title}，请确认队列后重新点歌`);
    } finally { this.posting.delete(chat.user); }
  }
  async tick(): Promise<void> {
    if (this.stopped || this.ticking) return;
    this.ticking = true;
    this.expire();
    try {
      const state = await this.api.state();
      if (this.stopped) return;
      this.game = state;
      this.online = true;
      const queue = state.queue;
      // First read (or a restarted DLL) is a snapshot, not a feed replay.
      if (this.eventCursor !== null && queue.revision >= this.eventCursor) {
        for (const event of queue.events.filter(event => event.id > this.eventCursor!)) {
          const { title, requester, error } = event.entry;
          const text = { added: `已加入队列：${title} — ${requester}`, removed: `已移除点歌：${title}`,
            prioritized: `优先选曲：${title}`, selected: `已定位：${title}`, failed: `无法定位 ${title}：${error ?? ''}`,
            played: `点歌等待结束：${title}`, expired: `点歌已超时跳过：${title}` }[event.kind];
          this.add('event', text);
        }
      }
      this.eventCursor = queue.revision;
    } catch { if (!this.stopped) this.online = false; }
    finally { this.ticking = false; }
  }
  suspendChat(): void { this.pending.clear(); this.searches.clear(); this.connected = false; this.room = null; this.status = '浏览器源未激活，弹幕已暂停'; }
  stop(): void { this.stopped = true; this.pending.clear(); this.searches.clear(); }
  view() {
    const remaining = (until: number) => Math.max(0, Math.ceil(until - this.now()));
    const request = (r: QueueEntry) => ({ token: r.id, title: r.title, requester: r.requester, mode: r.mode, chart: r.chart, chart_style: this.rules.chartStyle(r.chart ?? undefined), error: r.error });
    const queue = this.game.queue;
    return {
      connected: this.connected, ready: this.online && this.game.ready,
      status: this.online ? this.status : '等待游戏连接', room: this.connected ? this.room : null,
      current: queue.current ? { ...request(queue.current), remaining: queue.current.remaining_seconds, duration: queue.current.duration_seconds } : null,
      queue: queue.items.map(request), capacity: queue.capacity,
      pending: [...this.pending.values()].map(p => ({ requester: p.name, mode: p.mode, chart: p.chart ?? null, chart_style: this.rules.chartStyle(p.chart),
        remaining: remaining(p.until), duration: this.policy.selectionTimeoutSeconds, page: p.page, page_size: 5, page_count: Math.ceil(p.songs.length / 5),
        candidates: p.songs.map(song => ({ title: song.title, available: supports(song, p) })) })),
      feed: this.feed.slice(), feed_limit: this.feedLimit, now_playing: this.online ? this.game.now_playing : emptyGame().now_playing,
    };
  }
}
