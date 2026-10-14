export interface UserSnapshot { players: { side: number; name: string }[] }
export const emptyUser = (): UserSnapshot => ({ players: [] });
export async function readUser(signal: AbortSignal): Promise<UserSnapshot> {
  const response = await fetch('/api/v1/ddr/user', { cache: 'no-store', signal: AbortSignal.any([signal, AbortSignal.timeout(4000)]) });
  if (!response.ok) throw new Error('Player information unavailable');
  const value = await response.json();
  if (!value || !Array.isArray(value.players)) return emptyUser();
  return { players: value.players.filter((p: unknown): p is UserSnapshot['players'][number] => {
    if (!p || typeof p !== 'object') return false;
    const player = p as Record<string, unknown>;
    return (player.side === 1 || player.side === 2) && typeof player.name === 'string' && player.name.length <= 128;
  }) };
}
