export type DanRank = `kyu${1 | 2 | 3 | 4 | 5 | 6 | 7}` | `dan${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10}` | 'chuden' | 'kaiden';
export interface StyleProfile { dan: DanRank | null; dj_points: number }
export interface ActivityCounts { songs: number; keys: number }
export interface UserProfile {
  side: 1 | 2;
  dj_name: string;
  sp: StyleProfile;
  dp: StyleProfile;
  weekly: { period: 'game_week'; week_id: number; date: number; sp: ActivityCounts | null; dp: ActivityCounts | null } | null;
}
export interface UserSnapshot { active_side: 1 | 2 | null; players: UserProfile[] }
export const emptyUser = (): UserSnapshot => ({ active_side: null, players: [] });
const record = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const count = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
const rank = (v: unknown): v is DanRank | null => v === null || typeof v === 'string' && /^(kyu[1-7]|dan([1-9]|10)|chuden|kaiden)$/.test(v);
const style = (v: unknown): v is StyleProfile => record(v) && rank(v.dan) && count(v.dj_points);
const activity = (v: unknown): v is ActivityCounts | null => v === null || record(v) && count(v.songs) && count(v.keys);
function profile(v: unknown): v is UserProfile {
  if (!record(v) || (v.side !== 1 && v.side !== 2) || typeof v.dj_name !== 'string' || !v.dj_name.trim()
    || v.dj_name.length > 32 || /[\x00-\x1f\x7f]/.test(v.dj_name) || !style(v.sp) || !style(v.dp)) return false;
  const w = v.weekly;
  return w === null || record(w) && w.period === 'game_week' && count(w.week_id) && count(w.date) && activity(w.sp) && activity(w.dp);
}
export function parseUser(v: unknown): UserSnapshot {
  if (!record(v) || !Array.isArray(v.players) || v.players.length > 2 || !v.players.every(profile)
    || new Set(v.players.map(p => p.side)).size !== v.players.length
    || (v.active_side !== null && !v.players.some(p => p.side === v.active_side))) throw new Error('Invalid IIDX user snapshot');
  return { active_side: v.active_side as UserSnapshot['active_side'], players: v.players };
}
export async function readUser(signal: AbortSignal): Promise<UserSnapshot> {
  const response = await fetch('/api/v1/iidx/user', { cache: 'no-store', signal: AbortSignal.any([signal, AbortSignal.timeout(2000)]) });
  if (!response.ok) throw new Error(`User API: HTTP ${response.status}`);
  return parseUser(await response.json());
}
export function danLabel(dan: DanRank | null): string {
  if (dan === null) return '未取得';
  if (dan === 'chuden') return '中伝';
  if (dan === 'kaiden') return '皆伝';
  const digits = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十'];
  const n = Number(dan.slice(3));
  return dan.startsWith('kyu') ? `${digits[n - 1]}級` : n === 1 ? '初段' : `${digits[n - 1]}段`;
}
