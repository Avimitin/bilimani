import { Controller } from '@bilimani/controller';
import { createGameClient } from '@bilimani/game-api';
import { BilibiliSource } from '@bilimani/platform-bilibili';
import { loadConfig } from './config';
import { iidxRules } from './rules';
type View = ReturnType<Controller['view']>;

export async function startPage(render: (view: View) => void): Promise<void> {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let controller: Controller | undefined;
  let source: BilibiliSource | undefined;
  let generation = 0;
  const obs = 'obsstudio' in window;
  // OBS has change events, but no synchronous getter for initial active state.
  // Use document visibility until the first active event arrives.
  let active: boolean | null = null;
  let visible = !document.hidden;
  let synchronize = () => {};
  const onActive = (event: Event) => {
    const value: unknown = (event as CustomEvent).detail?.active;
    if (typeof value === 'boolean') { active = value; synchronize(); }
  };
  const onVisible = (event: Event) => {
    const value: unknown = (event as CustomEvent).detail?.visible;
    if (typeof value === 'boolean') { visible = value; synchronize(); }
  };
  const onVisibility = () => { visible = !document.hidden; synchronize(); };
  if (obs) {
    window.addEventListener('obsSourceActiveChanged', onActive);
    window.addEventListener('obsSourceVisibleChanged', onVisible);
    document.addEventListener('visibilitychange', onVisibility);
  }
  window.addEventListener('pagehide', () => {
    stopped = true; generation++; clearTimeout(timer); controller?.stop(); source?.stop();
    window.removeEventListener('obsSourceActiveChanged', onActive);
    window.removeEventListener('obsSourceVisibleChanged', onVisible);
    document.removeEventListener('visibilitychange', onVisibility);
  }, { once: true });
  window.addEventListener('pageshow', event => { if (event.persisted) location.reload(); });
  try {
    const config = await loadConfig();
    if (stopped) return;
    controller = new Controller(createGameClient(), iidxRules(), config.requests, config.display.feedLimit);
    synchronize = () => {
      if (stopped) return;
      const shouldConnect = !obs || (visible && active !== false);
      if (!shouldConnect) {
        generation++; source?.stop(); source = undefined; controller!.suspendChat();
      } else if (!source) {
        const session = ++generation;
        source = new BilibiliSource({ ...config.platform, relayUrl: new URL(config.platform.relayUrl, document.baseURI).href.replace(/\/+$/, '') });
        source.start(event => { if (!stopped && generation === session) controller!.event(event); });
      }
    };
    synchronize();
    const tick = async () => {
      await controller!.tick();
      if (stopped) return;
      render(controller!.view());
      timer = setTimeout(tick, 500);
    };
    void tick();
  } catch (error) {
    const message = document.createElement('div');
    message.setAttribute('role', 'alert');
    Object.assign(message.style, { position: 'fixed', inset: '20px auto auto 20px', maxWidth: '600px', padding: '20px', background: '#182020', color: '#fff', font: '18px sans-serif', zIndex: '9999' });
    message.textContent = error instanceof Error ? error.message : '页面启动失败';
    document.body.append(message);
  }
}
