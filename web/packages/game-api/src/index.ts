export interface Chart { mode: string; id: string }
export interface Song { id: number; title: string; charts: { chart: Chart; level: string }[] }
export interface QueueEntry { id: number; song_id: number; title: string; mode: string; chart: string | null; requester: string; error: string | null }
export interface QueueView {
  revision: number; capacity: number;
  current: (QueueEntry & { remaining_seconds: number; duration_seconds: number }) | null;
  items: (QueueEntry & { executing: boolean })[];
  events: { id: number; kind: 'added' | 'removed' | 'prioritized' | 'selected' | 'failed' | 'played' | 'expired'; entry: QueueEntry }[];
}
export interface GameState {
  queue: QueueView;
  ready: boolean;
  phase: 'other' | 'select' | 'playing';
  mode: string | null;
  epoch: number;
  plays: number;
  now_playing: unknown;
}
export interface Selection { song_id: number; mode: string; chart?: string }
export type Operation = { id: number; song_id: number } & (
  { status: 'waiting' | 'executing' | 'selected'; error?: null } |
  { status: 'failed'; error: { code: string; message: string } }
);
export interface GameClient {
  enqueue(selection: Selection & { requester: string }): Promise<QueueEntry>;
  state(): Promise<GameState>;
  search(query: string, limit: number): Promise<Song[]>;
  select(selection: Selection): Promise<Operation>;
  operation(id: number): Promise<Operation>;
}
export class ApiError extends Error {
  constructor(readonly status: number, readonly code: string, message: string) { super(message); }
}
export function createGameClient(base = '/api/v1'): GameClient {
  async function request<T>(path: string, body?: Selection & { requester?: string }): Promise<T> {
    const response = await fetch(`${base}${path}`, {
      method: body ? 'POST' : 'GET', cache: 'no-store', signal: AbortSignal.timeout(4000),
      ...(body ? { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) } : {}),
    });
    const value = await response.json();
    if (!response.ok) throw new ApiError(response.status, value.error?.code ?? 'http_error', value.error?.message ?? `HTTP ${response.status}`);
    return value as T;
  }
  return {
    enqueue: selection => request<QueueEntry>('/queue', selection),
    state: () => request<GameState>('/state'),
    search: async (query, limit) => (await request<{ songs: Song[] }>(`/songs?${new URLSearchParams({ q: query, limit: String(limit) })}`)).songs,
    // Deliberately no automatic retry: refresh/connection loss does not replay selections.
    select: selection => request<Operation>('/selections', selection),
    operation: id => request<Operation>(`/selections/${id}`),
  };
}
