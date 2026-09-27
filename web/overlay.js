"use strict";
(() => {
  let state = null;
  let lastSuccess = 0;
  let stopped = false;
  let timer;
  let pageSince = performance.now();
  let pageIndex = 0;
  let hadPending = false;
  const signatures = new Map();
  const $ = (id) => document.getElementById(id);
  const node = (tag, className, text) => {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (text !== undefined) el.textContent = text;
    return el;
  };
  const text = (el, value) => { if (el.textContent !== String(value)) el.textContent = value; };
  const pad = (n) => String(n).padStart(2, "0");
  const clock = (n) => `${pad(Math.floor(n / 60))}:${pad(n % 60)}`;
  const chart = (r) => {
    const label = r.chart || `${r.mode} · 当前难度`;
    const el = node("span", "chart", label);
    el.dataset.tone = r.chart_style || "neutral";
    return el;
  };
  function changed(key, value, draw) {
    const signature = JSON.stringify(value);
    if (signatures.get(key) !== signature) {
      signatures.set(key, signature);
      draw();
    }
  }
  function progress(el, remaining, duration) {
    el.style.width = `${Math.min(100, Math.max(0, remaining / Math.max(1, duration) * 100))}%`;
  }
  function renderQueue(s) {
    const rowHeight = parseFloat(getComputedStyle($("queue-list")).getPropertyValue("--queue-row-height"));
    const slots = Math.max(0, Math.min(6, Math.floor($("queue-body").clientHeight / rowHeight)));
    changed("queue", [s.queue, slots], () => {
      $("queue-list").replaceChildren(...s.queue.slice(0, slots).map((r, i) => {
        const row = node("li", "queue-row");
        const info = node("div", "queue-song");
        const title = node("div", "queue-title", r.title);
        title.title = r.title;
        info.append(title, node("div", "requester", r.requester));
        row.append(node("span", "row-number", pad(i + 1)), info, chart(r));
        return row;
      }));
    });
    $("queue-empty").hidden = s.queue.length > 0;
    text($("queue-more"), s.queue.length > slots ? `另有 ${s.queue.length - slots} 首等待中` : "");
  }
  function render(s, offline = false) {
    const warning = offline ? "等待游戏连接 · 游戏启动后会自动恢复" : !s.connected ? s.status : !s.ready ? "等待游戏曲库 · 请进入普通选曲界面" : "";
    document.querySelectorAll(".connection-banner").forEach((el) => { el.hidden = !warning; text(el, warning); });
    const c = s.current;
    changed("current", c ? [c.token, c.title, c.requester, c.chart, c.mode, c.chart_style] : [null, offline, s.ready, s.queue.length > 0], () => {
      const box = $("current-content");
      box.replaceChildren();
      if (c) {
        const title = node("h1", "current-title", c.title);
        title.title = c.title;
        box.append(title);
        const meta = node("div", "current-meta");
        const time = node("span", "remaining");
        time.append(node("b", "", ""), document.createTextNode(" 后跳过"));
        meta.append(chart(c), node("span", "requester", `由 ${c.requester} 点歌`), time);
        box.append(meta);
      } else {
        const empty = node("div", "current-idle");
        empty.append(node("h2", "", "暂无点歌"));
        box.append(empty);
      }
    });
    if (c) text($("current-content").querySelector(".remaining b"), clock(c.remaining));
    progress($("current-progress"), c ? c.remaining : 0, c ? c.duration : 1);
    changed("count", [s.queue.length, s.capacity], () => {
      $("queue-count").replaceChildren(document.createTextNode(pad(s.queue.length) + " "), node("span", "", `/ ${s.capacity || "—"}`));
    });
    // Candidate pages share the fixed song panel; chat rows keep their space.
    const perPage = 3;
    const choices = s.pending.flatMap((p) => Array.from({length: Math.ceil(p.candidates.length / perPage)}, (_, index) => ({
      ...p, offset: index * perPage, candidates: p.candidates.slice(index * perPage, (index + 1) * perPage)
    })));
    const pages = choices.length;
    if (pages && !hadPending) { pageIndex = 0; pageSince = performance.now(); }
    hadPending = pages > 0;
    if (performance.now() - pageSince > 6000) { pageIndex += 1; pageSince = performance.now(); }
    pageIndex = pages ? pageIndex % pages : 0;
    const pending = choices.slice(pageIndex, pageIndex + 1);
    $("pending-area").hidden = !pages;
    changed("pending", [pageIndex, pending.map(({remaining, ...p}) => p)], () => {
      $("pending-list").replaceChildren(...pending.map((p) => {
        const card = node("section", "choice-panel");
        const heading = node("div", "choice-heading");
        const person = node("div", "");
        person.append(node("h2", "", `${p.requester}，请回复编号`));
        const countdown = node("div", "countdown");
        countdown.append(node("b", "", ""), node("span", "", "秒"));
        heading.append(person, chart(p), countdown);
        const list = node("ol", "candidates");
        list.append(...p.candidates.map((song, i) => {
          const row = node("li", `candidate${song.available ? "" : " unavailable"}`);
          const name = node("div", "candidate-title", song.title);
          name.title = song.title;
          if (!song.available) name.append(node("span", "unavailable-note", "所请求谱面不存在"));
          row.append(node("span", "candidate-number", p.offset + i + 1), name);
          return row;
        }));
        const track = node("div", "time-track");
        track.append(node("span", ""));
        card.append(heading, list, track);
        return card;
      }));
    });
    $("pending-list").querySelectorAll(".choice-panel").forEach((card, i) => {
      text(card.querySelector(".countdown b"), pad(pending[i].remaining));
      card.querySelector(".countdown").classList.toggle("urgent", pending[i].remaining <= 10);
      progress(card.querySelector(".time-track span"), pending[i].remaining, pending[i].duration);
    });
    $("pending-page").hidden = pages <= 1;
    text($("pending-page"), `${pageIndex + 1} / ${pages} 页 · 每 6 秒轮换`);
    renderQueue(s);

    const limit = Math.max(1, Math.min(100, Number(s.feed_limit) || 10));
    const feed = s.feed.slice(-limit);
    $("activity-list").style.setProperty("--feed-limit", limit);
    text($("feed-count"), `${feed.length} / ${limit}`);
    $("feed-empty").hidden = feed.length > 0;
    changed("feed", feed, () => {
      $("activity-list").replaceChildren(...feed.map((message) => {
        const item = node("li", "activity-row");
        item.dataset.id = message.id;
        item.dataset.kind = message.kind;
        const content = node("p", "activity-text");
        if (message.kind === "chat") {
          content.append(node("strong", "chat-name", `${message.name}：`), document.createTextNode(message.text));
        } else {
          const warning = /无法|失败|不存在|超时|已满|冷却|不能|未就绪/.test(message.text);
          item.dataset.tone = warning ? "warning" : "success";
          item.append(node("span", "notice-icon", warning ? "!" : "✓"));
          content.textContent = message.text;
        }
        item.title = content.textContent;
        item.append(content);
        return item;
      }));
    });
  }
  const empty = () => ({connected: false, ready: false, status: "等待游戏连接", current: null, capacity: 0, queue: [], pending: [], feed: state?.feed || [], feed_limit: state?.feed_limit || 10});
  async function tick() {
    if (stopped) return;
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 2500);
    try {
      const response = await fetch("/api/state", {cache: "no-store", signal: controller.signal});
      if (!response.ok) throw new Error("Unavailable");
      const next = await response.json();
      if (!Array.isArray(next.queue) || !Array.isArray(next.pending) || !Array.isArray(next.feed)) throw new Error("Invalid state");
      state = next;
      lastSuccess = performance.now();
      render(state);
    } catch {
      if (!state || performance.now() - lastSuccess > 3000) render(empty(), true);
    } finally { clearTimeout(timeout); }
    if (!stopped) timer = setTimeout(tick, 500);
  }
  window.addEventListener("pagehide", () => { stopped = true; clearTimeout(timer); });
  window.addEventListener("pageshow", (event) => { if (event.persisted) { stopped = false; tick(); } });
  new ResizeObserver(() => renderQueue(state && performance.now() - lastSuccess <= 3000 ? state : empty())).observe($("queue-body"));
  tick();
})();
