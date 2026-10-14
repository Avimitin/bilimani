import { startPage } from '../bootstrap';

const element = (id: string) => document.getElementById(id)!;
const text = (id: string, value: string) => { element(id).textContent = value; };
function row(title: string, detail: string, tag = 'li') {
  const item = document.createElement(tag);
  const heading = document.createElement('span'); heading.className = 'entry-title'; heading.textContent = title; item.append(heading);
  const small = document.createElement('small'); small.textContent = detail; item.append(small);
  return item;
}
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === 'object'; }
void startPage((view, display) => {
  text('streamer-name', display.streamerName || view.room?.name || '等待主播连接');
  text('stream-title', display.streamTitle || view.room?.title || '直播标题未提供');
  text('status', view.status); element('status').dataset.ready = String(view.ready);
  text('count', String(view.queue.length).padStart(2, '0'));
  element('current').hidden = !view.current;
  document.querySelector<HTMLElement>('.current-label')!.hidden = !view.current;
  element('current').replaceChildren(...(view.current
    ? [row(view.current.title, `${view.current.chart ?? view.current.mode} · ${view.current.requester} · ${view.current.remaining}s`, 'div')]
    : []));
  element('queue').replaceChildren(...view.queue.slice(0, 3).map(entry => row(entry.title, `${entry.chart ?? entry.mode} · ${entry.requester}${entry.error ? ` · ${entry.error}` : ''}`)));
  element('empty').hidden = view.queue.length > 0 || !!view.current;
  element('feed').replaceChildren(...view.feed.slice(-5).map(entry => {
    const p = document.createElement('p'); const name = document.createElement('b'); name.textContent = entry.name;
    p.append(name, document.createTextNode(entry.text)); return p;
  }));
  const playing = record(view.now_playing) ? view.now_playing : {};
  const song = record(playing.song) ? playing.song : {};
  text('title', typeof song.title === 'string' ? song.title : 'LET’S MOVE.');
  text('artist', typeof song.artist === 'string' ? song.artist : '等待游戏选曲');
  text('phase', `${view.mode ?? 'DDR WORLD'} / ${view.phase === 'playing' ? 'ON THE FLOOR' : view.phase === 'select' ? 'SELECT YOUR TRACK' : 'WAITING FOR DANCERS'}`);
  const selected = record(playing.selected_chart) ? playing.selected_chart : undefined;
  const chart = selected && record(selected.chart) ? selected.chart : undefined;
  const names: Record<string, string> = { bSP: 'BEGINNER', BSP: 'BASIC', DSP: 'DIFFICULT', ESP: 'EXPERT', CSP: 'CHALLENGE', BDP: 'BASIC', DDP: 'DIFFICULT', EDP: 'EXPERT', CDP: 'CHALLENGE' };
  const chartName = typeof chart?.id === 'string' ? names[chart.id] : undefined;
  element('charts').replaceChildren();
  if (chartName && selected && typeof selected.level === 'string') {
    const badge = document.createElement('div'); badge.className = 'selected-difficulty';
    const label = document.createElement('span'); label.className = 'difficulty-name'; label.textContent = chartName;
    const level = document.createElement('strong'); level.className = 'difficulty-level'; level.textContent = selected.level;
    const mode = document.createElement('small'); mode.textContent = `${chart?.mode === 'DP' ? 'DOUBLE' : 'SINGLE'} / LEVEL`;
    badge.append(label, level, mode); element('charts').append(badge);
  }
  text('players', view.user.players.map(p => `${p.side}P${p.name ? ` / ${p.name}` : ""}`).join(' · ') || 'PLAYER —');
  const pending = view.pending[0]; element('choices').hidden = !pending;
  if (pending) {
    const start = pending.page * pending.page_size;
    element('candidates').replaceChildren(row(pending.requester, `回复编号 · N/P 翻页 · ${pending.remaining}s`, 'p'),
      ...pending.candidates.slice(start, start + pending.page_size).map((candidate, index) => {
        const p = row(`${start + index + 1}. ${candidate.title}`, candidate.available ? '' : '当前谱面不可用', 'p');
        if (!candidate.available) p.className = 'unavailable'; return p;
      }));
  }
}, { user: true });
