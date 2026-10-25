interface TurntableView {
  phase: string | null;
  mode?: string | null;
  now_playing?: { phase?: string } | null;
}

export const turntableSpinning = (view: TurntableView, effects: boolean): boolean =>
  effects && view.phase === 'other' && view.mode != null && view.now_playing?.phase === 'deciding';

export function createTurntable(disc: SVGElement) {
  const rotor = disc.querySelector<SVGElement>('.turntable-rotor')!;
  const light = disc.querySelector<SVGElement>('.turntable-light')!;
  const spin = rotor.animate([{ transform: 'rotate(0deg)' }, { transform: 'rotate(360deg)' }], {
    duration: 1600, iterations: Infinity,
  });
  const pulses = [...light.querySelectorAll('g')].map((ring, index) => ring.animate([
    { transform: 'rotate(0deg) scale(.94)', opacity: 0 },
    { transform: 'rotate(70deg) scale(1)', opacity: .55, offset: .25 },
    { transform: 'rotate(240deg) scale(1.22)', opacity: 0 },
  ], { duration: 1500, delay: index * 750, iterations: Infinity }));
  const animations = [spin, ...pulses];
  animations.forEach(animation => animation.pause());
  let spinning = false;
  let stopped = false;
  disc.dataset.spinning = 'false';
  window.addEventListener('pagehide', () => {
    stopped = true;
    animations.forEach(animation => animation.cancel());
  }, { once: true });
  return (view: TurntableView, effects: boolean) => {
    if (stopped) return;
    const next = turntableSpinning(view, effects);
    if (next === spinning) return;
    spinning = next;
    disc.dataset.spinning = String(next);
    // Ordinary snapshots never restart the motor. Pause retains its final angle;
    // native scene exit/gameplay/disconnect removes the light immediately.
    if (next) {
      pulses.forEach(animation => { animation.currentTime = 0; });
      animations.forEach(animation => animation.play());
    } else animations.forEach(animation => animation.pause());
  };
}
