import { advanceLogin, initialLogin, loadingCells, loadingCycle, loadingPattern, loadingCellLit, type GamePhase } from './startup-motion';

interface StartupView { phase: GamePhase; mode?: string | null; now_playing?: { song?: unknown } | null }
const svgNamespace = 'http://www.w3.org/2000/svg';

export function createStartupDisplay(overlay: HTMLElement, strip: HTMLElement, frame: HTMLElement) {
  const message = overlay.querySelector<HTMLElement>('.login-message')!;
  const grid = overlay.querySelector<SVGGElement>('.login-markers')!;
  const cells = strip.querySelector<SVGGElement>('.loading-cells')!;
  // Static vector art; no platform/user strings are interpolated into markup.
  grid.innerHTML = Array.from({ length: 15 }, (_, i) => {
    const x = 170 + i % 5 * 390, y = 190 + Math.floor(i / 5) * 280;
    return `<g transform="translate(${x} ${y})"><use href="#login-cross"/></g>`;
  }).join('');
  const hexagons = loadingCells.map(({ x, y }) => {
    const hexagon = document.createElementNS(svgNamespace, 'path');
    hexagon.setAttribute('transform', `translate(${x} ${y})`);
    hexagon.setAttribute('d', 'M-19 0-9.5-16.454H9.5L19 0 9.5 16.454H-9.5Z');
    hexagon.classList.add('loading-cell');
    hexagon.dataset.lit = 'false';
    return hexagon;
  });
  cells.replaceChildren(...hexagons);
  let login = initialLogin(performance.now());
  let waitingSince = login.since;
  let sweepSince = login.since;
  let sweep = loadingPattern(Math.random);
  const lit = loadingCells.map(() => false);
  let view: StartupView = { phase: null };
  let animate = true;
  let scheduled = 0;
  let stopped = false;
  let previousLoading = false;

  function draw(now: number) {
    const previousStage = login.stage;
    login = advanceLogin(login, view.phase, now, animate, view.mode);
    if (login.stage === 'waiting' && previousStage !== 'initial' && previousStage !== 'waiting') waitingSince = now;
    const visible = login.stage !== 'hidden';
    const loading = visible || view.phase === null || !view.now_playing?.song;
    const confirmed = login.stage === 'success' || login.stage === 'fading';
    const stage = login.stage === 'initial' ? 'waiting' : login.stage;
    if (overlay.dataset.stage !== stage) overlay.dataset.stage = stage;
    // Switch the whole camera layout together; hold its position during confirmation.
    if (stage === 'waiting') {
      const pattern = Math.floor((now - waitingSince) / 5000) % 2 ? 'cross' : 'plus';
      if (overlay.dataset.pattern !== pattern) overlay.dataset.pattern = pattern;
    }
    const label = confirmed ? '登录成功' : '正在等待登录中';
    if (message.textContent !== label) message.textContent = label;
    overlay.hidden = !visible;
    overlay.setAttribute('aria-hidden', String(!visible));
    const opacity = login.stage === 'fading' ? Math.max(0, 1 - (now - login.since) / 1000) : 1;
    overlay.style.opacity = String(opacity);
    strip.style.opacity = String(login.stage === 'fading' && view.now_playing?.song ? opacity : 1);
    strip.hidden = !loading;
    frame.dataset.loading = String(loading);
    document.getElementById('current-content')!.setAttribute('aria-hidden', String(loading));
    if (loading && !confirmed) {
      if (!previousLoading || animate && now - sweepSince >= loadingCycle) {
        sweepSince = now;
        sweep = loadingPattern(Math.random);
      }
      const elapsed = animate ? now - sweepSince : 1450;
      hexagons.forEach((hexagon, i) => {
        const nextLit = loadingCellLit(sweep[i], elapsed);
        if (nextLit !== lit[i]) { hexagon.dataset.lit = String(nextLit); lit[i] = nextLit; }
      });
    }
    previousLoading = loading;
    return login.stage === 'success' || login.stage === 'fading' || loading && animate;
  }

  function tick(now: number) {
    scheduled = 0;
    if (draw(now)) scheduled = requestAnimationFrame(tick);
  }
  window.addEventListener('pagehide', () => { stopped = true; cancelAnimationFrame(scheduled); }, { once: true });
  return (next: StartupView, effects: boolean) => {
    if (stopped) return;
    view = next; animate = effects;
    const running = draw(performance.now());
    if (running && !scheduled) scheduled = requestAnimationFrame(tick);
    else if (!running && scheduled) { cancelAnimationFrame(scheduled); scheduled = 0; }
  };
}
