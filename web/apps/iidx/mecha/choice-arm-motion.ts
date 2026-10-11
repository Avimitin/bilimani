export type Origin = 'left' | 'right';
export interface ArmState { position: number; origin: Origin }
export interface ArmRequest { open: boolean; origin: Origin; animate: boolean }

export const armDuration = 2100;
const phase = (position: number, start: number, end: number) => {
  const t = Math.max(0, Math.min(1, (position - start) / (end - start)));
  return t * t * (3 - 2 * t);
};

// A single reversible timeline: retract the glass and mount before the three rails.
export function armPose(position: number) {
  return {
    extensions: [phase(position, 0, 360), phase(position, 360, 720), phase(position, 720, 1040)],
    span: phase(position, 1040, 1260),
    drop: phase(position, 1260, 1530),
    reveal: phase(position, 1530, armDuration),
  };
}

export function advanceArm(state: ArmState, request: ArmRequest, elapsed: number): ArmState {
  if (!request.animate) return { origin: request.origin, position: request.open ? armDuration : 0 };
  // Change sides only outside the frame, including when the request changes mid-motion.
  const origin = state.position === 0 ? request.origin : state.origin;
  const target = request.open && origin === request.origin ? armDuration : 0;
  const distance = Math.max(0, elapsed);
  const position = target > state.position ? Math.min(target, state.position + distance) : Math.max(target, state.position - distance);
  return { origin, position };
}

export const armSettled = (state: ArmState, request: ArmRequest) =>
  request.open ? state.position === armDuration && state.origin === request.origin : state.position === 0;

const record = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === 'object' ? value as Record<string, unknown> : {};

export function choiceOrigin(view: { mode?: string | null; now_playing?: unknown; user?: { active_side: number | null } }): Origin {
  const playing = record(view.now_playing);
  const players = (Array.isArray(playing.players) ? playing.players.map(record) : []).filter(p => p.side === 1 || p.side === 2);
  if (view.mode === 'DP' || !view.mode && players.some(p => record(p.chart).mode === 'DP')) return 'right';
  const side = players.length === 1 ? players[0].side : view.user?.active_side ?? players[0]?.side;
  return side === 2 ? 'left' : 'right';
}
