"use strict";
(() => {
  const path = location.pathname;
  const preview = path !== "/queue" && path !== "/interaction";
  document.body.dataset.view = preview ? "preview" : path.slice(1);
  let demo = new URLSearchParams(location.search).get("demo") === "1" || (preview && !location.search.includes("live=1"));
  let state = null;
  let lastSuccess = 0;
  let stopped = false;
  let timer;
  let pageSince = performance.now();
  let pageIndex = 0;
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
    el.dataset.difficulty = r.chart ? r.chart.slice(-1) : "";
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
  function render(s, offline = false) {
    const warning = offline ? "等待游戏连接 · 游戏启动后会自动恢复" : !s.connected ? s.status : !s.ready ? "等待游戏曲库 · 请进入普通选曲界面" : "";
    document.querySelectorAll(".connection-banner").forEach((el) => { el.hidden = !warning; text(el, warning); });
    const c = s.current;
    changed("current", c ? [c.token, c.title, c.requester, c.chart, c.mode] : [null, offline, s.ready, s.queue.length > 0], () => {
      const box = $("current-content");
      box.replaceChildren();
      if (c) {
        box.append(node("h1", "current-title", c.title));
        const meta = node("div", "current-meta");
        const time = node("span", "remaining");
        time.append(node("b", "", ""), document.createTextNode(" 后跳过"));
        meta.append(chart(c), node("span", "requester", `由 ${c.requester} 点歌`), time);
        box.append(meta);
      } else {
        const empty = node("div", "current-idle");
        empty.append(node("h2", "", offline ? "等待游戏连接" : "等待下一首"),
          node("p", "", s.queue.length ? "准备好选曲后，自动为你定位。" : "弹幕里的好音乐，马上就来。"));
        box.append(empty);
      }
    });
    if (c) text($("current-content").querySelector(".remaining b"), clock(c.remaining));
    progress($("current-progress"), c ? c.remaining : 0, c ? c.duration : 1);
    changed("count", [s.queue.length, s.capacity], () => {
      $("queue-count").replaceChildren(document.createTextNode(pad(s.queue.length) + " "), node("span", "", `/ ${s.capacity || "—"}`));
    });
    changed("queue", s.queue, () => {
      $("queue-list").replaceChildren(...s.queue.slice(0, 6).map((r, i) => {
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
    $("queue-more").hidden = s.queue.length <= 6;
    text($("queue-more"), `另有 ${Math.max(0, s.queue.length - 6)} 首等待中`);

    const perPage = s.pending.some((p) => p.candidates.length > 6) ? 1 : 2;
    const pages = Math.ceil(s.pending.length / perPage);
    if (performance.now() - pageSince > 6000) { pageIndex += 1; pageSince = performance.now(); }
    pageIndex = pages ? pageIndex % pages : 0;
    const pending = s.pending.slice(pageIndex * perPage, (pageIndex + 1) * perPage);
    changed("pending", [pageIndex, pending.map(({remaining, ...p}) => p)], () => {
      $("pending-list").replaceChildren(...pending.map((p) => {
        const card = node("section", `panel choice-panel${p.candidates.length > 6 ? " compact" : ""}`);
        const top = node("header", "panel-heading");
        top.append(node("span", "eyebrow", "找到这些歌曲"), chart(p));
        const heading = node("div", "choice-heading");
        const person = node("div", "");
        person.append(node("h2", "", `${p.requester}，选哪一首？`), node("p", "", "用点歌的账号，直接回复编号"));
        const countdown = node("div", "countdown");
        countdown.append(node("b", "", ""), node("span", "", "秒"));
        heading.append(person, countdown);
        const list = node("ol", "candidates");
        list.append(...p.candidates.map((song, i) => {
          const row = node("li", `candidate${song.available ? "" : " unavailable"}`);
          const name = node("div", "candidate-title", song.title);
          if (!song.available) name.append(node("span", "unavailable-note", "所请求谱面不存在"));
          row.append(node("span", "candidate-number", i + 1), name);
          return row;
        }));
        const footer = node("div", "choice-footer");
        footer.append(document.createTextNode("例如发送 "), node("b", "", "1"), document.createTextNode(" · 超时后可重新点歌"));
        const track = node("div", "time-track");
        track.append(node("span", ""));
        card.append(top, heading, list, footer, track);
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
    $("interaction-empty").hidden = s.pending.length > 0;
    changed("notices", s.notices.slice(-2), () => {
      $("notices").replaceChildren(...s.notices.slice(-2).map((message) => {
        const warning = /无法|失败|不存在|超时|已满|冷却|不能|未就绪/.test(message.text);
        const item = node("div", "notice");
        item.dataset.tone = warning ? "warning" : "success";
        item.append(node("span", "notice-icon", warning ? "!" : "✓"), node("p", "", message.text));
        return item;
      }));
    });
  }
  const empty = () => ({connected: false, ready: false, status: "等待游戏连接", current: null, capacity: 0, queue: [], pending: [], notices: []});
  const sample = () => ({
    connected: true, ready: true, status: "弹幕已连接", capacity: 20,
    current: {token: 1, title: "AA -rebuild-", requester: "今晚练皿", mode: "SP", chart: "SPA", remaining: 428, duration: 600},
    queue: [
      {token: 2, title: "冥", requester: "凌晨两点", mode: "SP", chart: "SPA"},
      {token: 3, title: "雪月花", requester: "柚子", mode: "SP", chart: "SPH"},
      {token: 4, title: "V", requester: "今天也要全连", mode: "SP", chart: "SPA"},
      {token: 5, title: "ピアノ協奏曲第1番“蠍火”", requester: "白昼流星", mode: "SP", chart: "SPN"}
    ],
    pending: [{requester: "柚子", mode: "SP", chart: "SPA", remaining: 47, duration: 60,
      candidates: [{title: "AA", available: true}, {title: "AA -rebuild-", available: true}]}],
    notices: [{at: 1, text: "凌晨两点 的「冥」已加入队列"}]
  });
  async function tick() {
    if (stopped) return;
    if (demo) {
      render(sample());
    } else {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 2500);
      try {
        const response = await fetch("/api/state", {cache: "no-store", signal: controller.signal});
        if (!response.ok) throw new Error("Unavailable");
        const next = await response.json();
        if (!Array.isArray(next.queue) || !Array.isArray(next.pending) || !Array.isArray(next.notices)) throw new Error("Invalid state");
        if (!demo) {
          state = next;
          lastSuccess = performance.now();
          render(state);
        }
      } catch {
        if (!state || performance.now() - lastSuccess > 3000) render(empty(), true);
      } finally { clearTimeout(timeout); }
    }
    if (!stopped) timer = setTimeout(tick, 500);
  }
  for (const name of ["queue", "interaction"]) $(name + "-url").value = `${location.origin}/${name}`;
  document.querySelectorAll("[data-copy]").forEach((button) => button.addEventListener("click", async () => {
    const input = $(button.dataset.copy);
    try {
      await navigator.clipboard.writeText(input.value);
      text(button, "已复制");
      setTimeout(() => text(button, "复制地址"), 1500);
    } catch { input.focus(); input.select(); text(button, "按 Ctrl+C 复制"); }
  }));
  $("preview-toggle").addEventListener("click", () => {
    demo = !demo;
    state = null;
    text($("preview-label"), demo ? "样式预览 · 示例数据" : "实时内容 · 当前游戏");
    text($("preview-toggle"), demo ? "查看实时内容 ↗" : "返回样式预览 ↗");
    // Never leave sample requests visible while waiting for real data.
    render(demo ? sample() : empty(), !demo);
  });
  if (!demo) { text($("preview-label"), "实时内容 · 当前游戏"); text($("preview-toggle"), "返回样式预览 ↗"); }
  window.addEventListener("pagehide", () => { stopped = true; clearTimeout(timer); });
  window.addEventListener("pageshow", (event) => { if (event.persisted) { stopped = false; tick(); } });
  tick();
})();
