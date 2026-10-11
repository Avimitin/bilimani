import { danLabel } from '../user';

// Rotation belongs to this layout. Updating telemetry never restarts its clock.
export function createIdentityRenderer(node, vfdNumber) {
  let userKey = '';
  let since = performance.now();
  let page = 'song';
  const update = (element, value, draw) => {
    const signature = JSON.stringify(value);
    if (element.dataset.signature === signature) return;
    element.dataset.signature = signature;
    draw();
  };
  return (state, intervalSeconds) => {
    const host = document.querySelector('.game-identity');
    if (!host) return;
    let viewport = host.querySelector('.identity-viewport');
    if (!viewport) {
      viewport = node('div', 'identity-viewport');
      const track = node('div', 'identity-track');
      track.append(node('div', 'identity-slide identity-song'), node('div', 'identity-slide identity-user'));
      const scan = node('div', 'identity-refresh');
      scan.setAttribute('aria-hidden', 'true');
      viewport.append(track, scan);
      host.append(viewport);
    }
    const song = state.now_playing?.song;
    const user = state.user.players.find(p => p.side === state.user.active_side);
    const nextKey = user ? `${user.side}:${user.dj_name}` : '';
    const now = performance.now();
    const reset = nextKey !== userKey;
    if (reset) { userKey = nextKey; page = 'song'; since = now; }
    if (!user) { page = 'song'; since = now; }
    else if (!song) { page = 'user'; since = now; }
    else if (now - since >= intervalSeconds * 1000) { page = page === 'song' ? 'user' : 'song'; since = now; }
    const previousPage = viewport.dataset.page;
    viewport.dataset.reset = String(reset);
    viewport.dataset.page = page;
    if (!reset && previousPage && previousPage !== page && document.body.dataset.vfdEffects === 'true') {
      const scan = viewport.querySelector('.identity-refresh');
      scan.getAnimations().forEach(animation => animation.cancel());
      scan.animate([
        { transform: 'translateY(-100%)', opacity: 0 },
        { opacity: .8, offset: .25 },
        { transform: 'translateY(350%)', opacity: 0 },
      ], { duration: 900, easing: 'ease-in-out' });
    }
    const songSlide = viewport.querySelector('.identity-song');
    const userSlide = viewport.querySelector('.identity-user');
    songSlide.setAttribute('aria-hidden', String(page !== 'song'));
    userSlide.setAttribute('aria-hidden', String(page !== 'user'));
    update(songSlide, song ? [song.title, song.genre, song.artist] : null, () => {
      const title = node('h1', 'current-title vfd-text', song?.title ?? '等待选择歌曲');
      title.title = title.textContent;
      const credits = node('div', 'song-credits vfd-text', song ? [song.genre, song.artist].filter(Boolean).join(' / ') : '');
      credits.title = credits.textContent;
      songSlide.replaceChildren(title, credits);
    });
    update(userSlide, user ?? null, () => {
      userSlide.replaceChildren();
      if (!user) return;
      const heading = node('div', 'profile-heading');
      const name = node('span', 'profile-name vfd-text', user.dj_name);
      name.title = user.dj_name;
      heading.append(name, node('span', 'profile-label', 'DJ NAME'));
      const ranks = node('div', 'profile-ranks');
      for (const [mode, style] of [['SP', user.sp], ['DP', user.dp]]) {
        const rank = node('div', 'profile-rank');
        rank.append(node('span', 'profile-value vfd-text', danLabel(style.dan)), node('span', 'profile-label', `${mode} 段位`));
        ranks.append(rank);
      }
      const stats = node('div', 'profile-stats');
      // Do not report a partial style as a complete total, or missing data as 0.
      const week = user.weekly;
      const total = field => week?.sp && week?.dp ? week.sp[field] + week.dp[field] : null;
      const values = [
        ['DJ POINT', user.sp.dj_points + user.dp.dj_points, `SP ${user.sp.dj_points} / DP ${user.dp.dj_points}`],
        ['本周游玩', total('songs'), 'SP + DP 游玩曲目次数'],
        ['本周按键', total('keys'), 'SP + DP 按键数，不含转盘'],
      ];
      for (const [label, value, hint] of values) {
        const row = node('div', 'profile-stat');
        const number = node('span', 'profile-number');
        vfdNumber(number, value === null ? '—' : String(value));
        row.title = value === null ? '暂无完整的本周统计' : hint;
        row.append(number, node('span', 'profile-label', label));
        stats.append(row);
      }
      userSlide.append(heading, ranks, stats);
    });
  };
}

// Broadcast effects follow the page's explicit setting, independently of the
// desktop motion preference OBS may inherit. The glass/capture never moves.
export function startVfdInstability() {
  const timers = new Set();
  const animations = new Set();
  const schedule = effect => {
    const timer = setTimeout(() => {
      timers.delete(timer);
      if (document.body.dataset.vfdEffects === 'true' && !document.hidden) {
        const targets = [...document.querySelectorAll(effect.selector)].filter(target => !target.getAnimations().length);
        const target = targets[Math.floor(Math.random() * targets.length)];
        if (target) {
          // Stepping the entire animation skips every intermediate keyframe.
          // Step between frames, while the overall timeline advances linearly.
          const animation = target.animate(effect.frames.map(frame => ({ ...frame, easing: 'steps(1)' })), {
            duration: effect.duration + Math.random() * 100, easing: 'linear',
          });
          animations.add(animation);
          animation.finished.then(() => animations.delete(animation), () => animations.delete(animation));
        }
      }
      schedule(effect);
    }, effect.interval + Math.random() * effect.variance);
    timers.add(timer);
  };
  schedule({
    selector: '.identity-slide[aria-hidden="false"]',
    interval: 4000, variance: 5000, duration: 200,
    frames: [
      { transform: 'translate(0, 0)', opacity: 1, filter: 'brightness(1)' },
      { transform: 'translate(.3px, -.1px)', opacity: .97, filter: 'brightness(1.04)', offset: .16 },
      { transform: 'translate(-.2px, .1px)', opacity: 1, filter: 'brightness(1.02)', offset: .34 },
      { transform: 'translate(.12px, 0)', opacity: .95, filter: 'brightness(.99)', offset: .48 },
      { transform: 'translate(-.1px, -.06px)', opacity: .98, filter: 'brightness(1.02)', offset: .67 },
      { transform: 'translate(0, 0)', opacity: 1, filter: 'brightness(1)', offset: .82 },
      { transform: 'translate(0, 0)', opacity: 1, filter: 'brightness(1)' },
    ],
  });
  schedule({
    selector: '.identity-slide[aria-hidden="false"] :is(.current-title, .profile-name, .profile-value, .profile-number)',
    interval: 7000, variance: 8000, duration: 220,
    frames: [
      { opacity: 1 },
      { opacity: .2, offset: .10 },
      { opacity: 1, offset: .20 },
      { opacity: .4, offset: .30 },
      { opacity: 1, offset: .42 },
      { opacity: .25, offset: .53 },
      { opacity: 1, offset: .65 },
      { opacity: .65, offset: .76 },
      { opacity: 1, offset: .86 },
      { opacity: 1 },
    ],
  });
  window.addEventListener('pagehide', () => {
    timers.forEach(clearTimeout);
    animations.forEach(animation => animation.cancel());
  }, { once: true });
}
