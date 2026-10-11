import { advanceArm, armDuration, armPose, armSettled, type ArmRequest, type ArmState } from './choice-arm-motion';

const svgNamespace = 'http://www.w3.org/2000/svg';
const screenHeight = 550;
const railY = 118;

/** DOM effects stay here; side selection and the reversible motion are pure. */
export function createChoiceArm(root: HTMLElement, onStowed: () => void) {
  const rails = root.querySelector<SVGGElement>('.arm-rails')!;
  const mount = root.querySelector<SVGGElement>('.arm-mount')!;
  const rods = mount.querySelectorAll('rect');
  const screen = root.querySelector<HTMLElement>('.arm-screen')!;
  const housing = root.querySelector<HTMLElement>('.arm-housing')!;
  const label = housing.querySelector('span')!;
  const glass = root.querySelector<HTMLElement>('.arm-glass')!;
  const bottom = root.querySelector<HTMLElement>('.arm-bottom')!;
  let state: ArmState = { position: 0, origin: 'right' };
  let request: ArmRequest = { open: false, origin: 'right', animate: true };
  let frame = 0;
  let previous = 0;
  let stopped = false;
  let occupied = false;
  let builtOrigin: ArmState['origin'] | undefined;
  let segments: SVGGElement[] = [];
  let lengths: number[] = [];
  root.style.setProperty('--arm-screen-height', `${screenHeight}px`);

  function buildRails() {
    const reach = state.origin === 'right' ? 690 : 352;
    lengths = [reach * .36, reach * .34, reach * .30];
    segments = lengths.map((length, i) => {
      const group = document.createElementNS(svgNamespace, 'g');
      group.classList.add('arm-segment');
      const h = 42 - i * 9;
      // Only fixed numeric geometry enters SVG markup. No chat or song strings.
      group.innerHTML = `<rect x="${-length - 16}" y="${-h / 2}" width="${length + 28}" height="${h}" rx="4" fill="url(#arm-metal)" stroke="#090e12" stroke-width="2"/>
        <path d="M${-length - 10} ${-h / 2 + 3}H7 M${-length - 10} ${h / 2 - 3}H7" stroke="#7c8c94" stroke-opacity=".6"/>
        <rect x="${-length - 7}" y="-5" width="${length + 6}" height="10" rx="2" fill="#0d181c"/>
        <path d="M${-length - 4} -1H-5" stroke="#66daba" stroke-opacity=".55"/>
        <rect x="-12" y="${-h / 2 - 4}" width="22" height="${h + 8}" rx="4" fill="url(#arm-cuff)" stroke="#0b1013" stroke-width="2"/>
        <path d="M-8 ${-h / 2}v${h}" stroke="#9aa9b2" stroke-opacity=".7"/>
        <circle r="4" fill="#071411" stroke="#819a95"/><circle r="1.8" fill="#91ffdb"/>`;
      return group;
    });
    rails.replaceChildren(...[...segments].reverse());
    builtOrigin = state.origin;
  }

  function draw() {
    if (builtOrigin !== state.origin) buildRails();
    const pose = armPose(state.position);
    const anchor = state.origin === 'right' ? 1958 : -38;
    const direction = state.origin === 'right' ? -1 : 1;
    rails.setAttribute('transform', `translate(${anchor} ${railY}) scale(${direction} 1)`);
    let reach = 0;
    segments.forEach((segment, i) => {
      reach += lengths[i] * pose.extensions[i];
      segment.setAttribute('transform', `translate(${reach} 0)`);
    });
    const x = anchor + direction * reach;
    const y = railY + 18 + 60 * pose.drop;
    mount.setAttribute('transform', `translate(${x} ${railY})`);
    rods.forEach(rod => rod.setAttribute('height', String(y - railY)));
    screen.style.transform = `translate(${x - 250}px, ${y}px)`;
    housing.style.width = `${64 + 436 * pose.span}px`;
    housing.style.left = `${218 * (1 - pose.span)}px`;
    label.style.opacity = String(pose.span);
    const height = screenHeight * pose.reveal;
    glass.style.height = `${height}px`;
    bottom.style.transform = `translateY(${height}px)`;
    bottom.style.opacity = String(pose.reveal);
    root.hidden = state.position === 0;
    root.dataset.origin = state.origin;
    root.dataset.stage = state.position === 0 ? 'stowed' : state.position === armDuration ? 'open' :
      pose.reveal > 0 ? 'screen' : pose.drop > 0 ? 'lowering' : pose.span > 0 ? 'housing' : 'extending';
    if (!request.open && state.position === 0 && occupied) { occupied = false; onStowed(); }
  }

  function tick(now: number) {
    frame = 0;
    state = advanceArm(state, request, Math.min(64, now - previous));
    previous = now;
    draw();
    if (!armSettled(state, request)) frame = requestAnimationFrame(tick);
  }

  window.addEventListener('pagehide', () => { stopped = true; cancelAnimationFrame(frame); }, { once: true });
  return (next: ArmRequest) => {
    if (stopped) return;
    request = next;
    occupied ||= request.open;
    root.setAttribute('aria-hidden', String(!request.open));
    if (!request.animate) {
      cancelAnimationFrame(frame); frame = 0;
      state = advanceArm(state, request, 0); draw();
    } else if (!frame && !armSettled(state, request)) {
      previous = performance.now(); frame = requestAnimationFrame(tick);
    }
  };
}
