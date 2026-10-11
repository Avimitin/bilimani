export type GamePhase = 'other' | 'select' | 'playing' | null;
export interface LoginState {
  stage: 'initial' | 'waiting' | 'success' | 'fading' | 'hidden';
  since: number;
  offlineSince: number | null;
}
export const initialLogin = (now: number): LoginState => ({ stage: 'initial', since: now, offlineSince: null });

/** Selection is a readiness latch. Results, folders and brief telemetry gaps are not logouts. */
export function advanceLogin(state: LoginState, phase: GamePhase, now: number, animate: boolean, mode?: string | null): LoginState {
  const unavailable = phase === null || phase === 'other' && mode === null;
  const offlineSince = unavailable ? state.offlineSince ?? now : null;
  const current = { ...state, offlineSince };
  if (unavailable && offlineSince !== null && now - offlineSince >= (phase === null ? 4000 : 1200) && state.stage !== 'waiting') {
    return { ...current, stage: 'waiting', since: now };
  }
  if (state.stage === 'initial') {
    // Refreshing a page during selection/gameplay must not cover a ready game.
    if (phase === 'select' || phase === 'playing') return { ...current, stage: 'hidden', since: now };
    if (phase === 'other' || now - state.since >= 600) return { ...current, stage: 'waiting', since: now };
  } else if (state.stage === 'waiting' && (phase === 'select' || phase === 'playing')) {
    return { ...current, stage: 'success', since: now };
  } else if (state.stage === 'success' && now - state.since >= 1400) {
    return { ...current, stage: animate ? 'fading' : 'hidden', since: now };
  } else if (state.stage === 'fading' && (!animate || now - state.since >= 1000)) {
    return { ...current, stage: 'hidden', since: now };
  }
  return current;
}

// Flat-top hexagons tile the entire 1592 × 178 bottom glass, including its clipped edges.
const rowHeight = Math.sqrt(3) * 20;
const rows = Math.ceil(178 / rowHeight);
export const loadingCells = Array.from({ length: Math.ceil(1592 / 30) * rows }, (_, i) => {
  const column = Math.floor(i / rows);
  return { x: column * 30, y: (i % rows + column % 2 / 2) * rowHeight };
});
export const loadingCycle = 3800;
export interface LoadingWindow { enabled: boolean; fillAt: number; clearAt: number }

/** Leave random holes in each sweep; jitter its edge without losing left-to-right travel. */
export const loadingPattern = (random: () => number): LoadingWindow[] => loadingCells.map(({ x }) => ({
  enabled: random() < .64,
  fillAt: x / 1592 * 2400 + random() * 360,
  clearAt: 3100 + random() * 300,
}));
export const loadingCellLit = (cell: LoadingWindow, elapsed: number): boolean =>
  cell.enabled && elapsed >= cell.fillAt && elapsed < cell.clearAt;
