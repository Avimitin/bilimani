// Only build:preview resolves this factory. Production bundles keep BilibiliSource.
import type { ChatSource } from '@bilimani/controller';
import type { Config } from '../apps/iidx/config';

interface PreviewEvent { id: number; action: { kind: 'request'; query: string } | { kind: 'chat'; text: string } }
interface PreviewSnapshot { last_id: number; events: PreviewEvent[] }

export function createChatSource(_platform: Config['platform'], requests: Config['requests']): ChatSource {
  let lifetime: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    start(emit) {
      if (lifetime) return;
      const controller = lifetime = new AbortController();
      let cursor: number | undefined;
      let connected = false;
      const poll = async () => {
        try {
          const response = await fetch('/api/v1/preview', { cache: 'no-store', signal: AbortSignal.any([controller.signal, AbortSignal.timeout(2000)]) });
          if (!response.ok) throw new Error(`Preview API: HTTP ${response.status}`);
          const snapshot: PreviewSnapshot = await response.json();
          if (controller.signal.aborted) return;
          if (!connected) {
            emit({ type: 'connection', connected: true, message: '本地预览' });
            emit({ type: 'room', room: { id: 'preview', name: '本地预览', title: '' } });
            connected = true;
          }
          // Refresh/reconnect starts at the current cursor, never replays commands.
          if (cursor === undefined || snapshot.last_id < cursor) cursor = snapshot.last_id;
          for (const { id, action } of snapshot.events) {
            if (id <= cursor) continue;
            const text = action.kind === 'request' ? `${requests.prefix} ${action.query}` : action.text;
            emit({ type: 'chat', chat: { user: 'preview-viewer', name: '预览观众', text } });
            cursor = id;
          }
        } catch {
          if (controller.signal.aborted) return;
          if (connected) emit({ type: 'connection', connected: false, message: '等待本地预览服务' });
          connected = false; cursor = undefined;
        }
        if (!controller.signal.aborted) timer = setTimeout(poll, 150);
      };
      void poll();
    },
    stop() { lifetime?.abort(); clearTimeout(timer); lifetime = undefined; },
  };
}
