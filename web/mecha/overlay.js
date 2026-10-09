"use strict";
(() => {
  let state = null;
  let lastSuccess = 0;
  let stopped = false;
  let timer;
  let pageSince = performance.now();
  let pageIndex = 0;
  let activeChoice = null;
  const choicePages = new Map();
  const candidatePageSize = 5;
  const candidatePage = (p) => Math.min(Math.max(0, Math.floor(Number(p.page) || 0)), Math.max(0, Math.ceil(p.candidates.length / candidatePageSize) - 1));
  const signatures = new Map();
  const $ = (id) => document.getElementById(id);
  const frame = document.body.dataset.layout === "frame";
  if (frame) {
    const resize = () => document.querySelector(".stream-frame").style.setProperty("--frame-scale", Math.min(innerWidth / 1920, innerHeight / 1080));
    window.addEventListener("resize", resize);
    resize();
  }
  const node = (tag, className, text) => {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (text !== undefined) el.textContent = text;
    return el;
  };
  const text = (el, value) => { if (el.textContent !== String(value)) el.textContent = value; };
  const pad = (n) => String(n).padStart(2, "0");
  const clock = (n) => `${pad(Math.floor(n / 60))}:${pad(n % 60)}`;
  const digitSegments = ["abcdef", "bc", "abdeg", "abcdg", "bcfg", "acdfg", "acdefg", "abc", "abcdefg", "abcdfg"];
  const segmentShapes = ["4,2 16,2 18,4 16,6 4,6 2,4", "16,6 18,4 20,6 20,14 18,16 16,14", "16,20 18,18 20,20 20,28 18,30 16,28", "4,28 16,28 18,30 16,32 4,32 2,30", "0,20 2,18 4,20 4,28 2,30 0,28", "0,6 2,4 4,6 4,14 2,16 0,14", "4,15 16,15 18,17 16,19 4,19 2,17"];
  function vfdNumber(el, value) {
    if (el.dataset.value === value) return;
    el.dataset.value = value;
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    const width = value.length * 25 - 5;
    svg.setAttribute("viewBox", `0 0 ${width} 34`);
    svg.setAttribute("width", width);
    svg.setAttribute("height", 34);
    svg.setAttribute("class", "vfd-digits");
    svg.setAttribute("aria-hidden", "true");
    for (const [i, character] of [...value].entries()) {
      const lit = digitSegments[character] || "g";
      segmentShapes.forEach((points, segment) => {
        const polygon = document.createElementNS(svg.namespaceURI, "polygon");
        polygon.setAttribute("points", points);
        polygon.setAttribute("transform", `translate(${i * 25} 0)`);
        polygon.setAttribute("class", lit.includes("abcdefg"[segment]) ? "segment-on" : "segment-off");
        svg.append(polygon);
      });
    }
    el.replaceChildren(node("span", "vfd-readable", value), svg);
  }
  const chart = (r) => {
    const label = r.chart || `${r.mode} · 当前难度`;
    const el = node("span", "chart", label);
    el.dataset.tone = r.chart_style || "neutral";
    el.dataset.automatic = String(!r.chart);
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
  function fitFeed() {
    document.querySelectorAll(".activity-row").forEach((row) => {
      const style = getComputedStyle(row);
      const lineHeight = parseFloat(getComputedStyle(row.querySelector(".activity-text")).lineHeight);
      const available = row.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
      row.style.setProperty("--feed-lines", Math.max(1, Math.min(2, Math.floor(available / lineHeight))));
    });
  }
  function renderNowPlaying(game, offline) {
    signatures.delete("current");
    const song = offline ? null : game.song;
    changed("game-song", [game, offline], () => {
      const box = $("current-content");
      box.classList.add("game-content");
      box.replaceChildren();
      if (!song) {
        const empty = node("div", "current-idle");
        empty.append(node("h2", "", offline ? "等待游戏连接" : "等待选择歌曲"));
        box.append(empty);
        return;
      }
      const identity = node("div", "game-identity");
      const identityFrame = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      identityFrame.setAttribute("class", "song-identity-frame");
      identityFrame.setAttribute("viewBox", "0 0 500 90");
      identityFrame.setAttribute("preserveAspectRatio", "none");
      identityFrame.setAttribute("aria-hidden", "true");
      const phosphor = document.createElementNS(identityFrame.namespaceURI, "linearGradient");
      phosphor.id = "song-identity-phosphor";
      phosphor.setAttribute("x2", "0");
      phosphor.setAttribute("y2", "1");
      [[0, "#a6efd1", .85], [.28, "#76c8ae", .68], [.62, "#66b49a", .5], [1, "#7dcbb0", .42]].forEach(([offset, color, opacity]) => {
        const stop = document.createElementNS(identityFrame.namespaceURI, "stop");
        stop.setAttribute("offset", offset);
        stop.setAttribute("stop-color", color);
        stop.setAttribute("stop-opacity", opacity);
        phosphor.append(stop);
      });
      const frameDefs = document.createElementNS(identityFrame.namespaceURI, "defs");
      frameDefs.append(phosphor);
      identityFrame.append(frameDefs);
      const outline = document.createElementNS(identityFrame.namespaceURI, "path");
      outline.setAttribute("d", "M18 3H302l12 7h166l17 17v42l-18 18H160l-12-7H3V18Z");
      identityFrame.append(outline);
      const title = node("h1", "current-title", song.title);
      title.title = song.title;
      const credits = node("div", "song-credits", [song.genre, song.artist].filter(Boolean).join(" / "));
      credits.title = credits.textContent;
      identity.append(identityFrame, title, credits);
      const charts = node("div", "game-charts");
      const selected = (game.players || []).map((player) => ({player, info: song.charts.find((c) => c.chart.id === player.chart.id)})).filter(({info}) => info);
      selected.forEach(({player, info}) => {
        const group = node("div", "game-chart");
        const badge = chart({chart: `${player.side}P ${info.chart.id} ${info.level}`, chart_style: info.style});
        badge.replaceChildren(node("span", "chart-side", `${player.side}P`), document.createTextNode(" "),
          node("span", "chart-difficulty", `${info.chart.id} ${info.level}`));
        const bpm = info.bpm ? (info.bpm.min === info.bpm.max ? String(info.bpm.max) : `${info.bpm.min}–${info.bpm.max}`) : "—";
        const top = node("div", "chart-summary");
        const bpmDisplay = node("span", "song-bpm");
        const digits = node("span", "");
        vfdNumber(digits, bpm);
        bpmDisplay.append(document.createTextNode("BPM "), digits);
        top.append(bpmDisplay, badge);
        group.append(top, node("div", "song-notes", `${info.difficulty} · ${info.note_count ?? "—"} NOTES`));
        charts.append(group);
      });
      box.append(identity, charts);
      if (selected.length) {
        const plots = node("div", "song-densities");
        selected.forEach(({player, info}) => plots.append(density(info.density, player.side, info.chart.id)));
        box.append(plots);
      }
      if (selected.length) box.append(radar(selected[0].info.radar, selected[0].player.side));
    });
    progress($("current-progress"), 0, 1);
  }
  function density(values, side, chartId) {
    const wrap = node("div", "song-density");
    const heading = node("div", "density-heading");
    heading.append(node("span", "", "音符密度"));
    wrap.append(heading);
    const valid = values && values.bin_ms === 1000 && Number.isInteger(values.duration_ms)
      && values.duration_ms > 0 && values.duration_ms <= 3600000
      && Array.isArray(values.notes) && Array.isArray(values.scratch)
      && values.notes.length > 0 && values.notes.length <= Math.floor(values.duration_ms / 1000) + 1
      && values.notes.length === values.scratch.length
      && values.notes.every((n, i) => Number.isInteger(n) && n >= 0 && n <= 10000
        && Number.isInteger(values.scratch[i]) && values.scratch[i] >= 0 && values.scratch[i] <= n)
      && values.notes.some((n) => n > 0);
    if (!valid) {
      wrap.append(node("div", "density-empty", "暂无密度数据"));
      return wrap;
    }
    const peak = Math.max(...values.notes);
    heading.append(node("b", "", `峰值 ${peak} 个/秒`));
    const binMs = values.duration_ms > 180000 ? 10000 : 5000;
    const columns = [];
    for (let start = 0; start < values.duration_ms; start += binMs) {
      const end = Math.min(start + binMs, values.duration_ms);
      const first = start / values.bin_ms;
      const last = Math.ceil(end / values.bin_ms);
      const sum = (kind) => values[kind].slice(first, last).reduce((a, b) => a + b, 0);
      columns.push({start, end, notes: sum("notes") * 1000 / (end - start), scratch: sum("scratch") * 1000 / (end - start)});
    }
    // The API remains at 1s resolution. The VFD shows time-window averages
    // quantized to 12 segments, retaining empty windows and a partial tail.
    const scale = Math.max(10, Math.ceil(Math.max(...columns.map((c) => c.notes)) / 5) * 5);
    wrap.title = `每柱 ${binMs / 1000} 秒平均密度，12 段灯条，量程 ${scale} /秒；青绿至暗橙表示密度高度，底部亮青绿为转盘（包含在总量中）。峰值保留原始 1 秒统计。长押在起点计 2 个 note。`;
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 380 72");
    svg.setAttribute("preserveAspectRatio", "none");
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", `${side}P ${chartId} 谱面密度，每 ${binMs / 1000} 秒平均，每秒峰值 ${peak}`);
    svg.dataset.binMs = binMs;
    const appendPath = (parent, className, d) => {
      const path = document.createElementNS(svg.namespaceURI, "path");
      path.setAttribute("class", className);
      path.setAttribute("d", d.join(" "));
      parent.append(path);
    };
    const rect = (x, y, w, h) => `M${x.toFixed(3)},${y.toFixed(3)}h${w.toFixed(3)}v${h}h-${w.toFixed(3)}z`;
    const dots = [];
    for (let row = 0; row < 12; row++) {
      dots.push(rect(1, 68 - row * 6, 2, 2), rect(377, 68 - row * 6, 2, 2));
    }
    appendPath(svg, "density-ticks", dots);
    columns.forEach((column) => {
      const group = document.createElementNS(svg.namespaceURI, "g");
      group.setAttribute("class", "density-column");
      group.dataset.average = column.notes.toFixed(3);
      const title = document.createElementNS(svg.namespaceURI, "title");
      title.textContent = `${clock(column.start / 1000)}–${clock(Math.ceil(column.end / 1000))} · 平均 ${column.notes.toFixed(1)} /秒 · 转盘 ${column.scratch.toFixed(1)} /秒`;
      group.append(title);
      const width = (column.end - column.start) / values.duration_ms * 364;
      const gap = Math.min(3, width * .25);
      const x = 8 + column.start / values.duration_ms * 364 + gap / 2;
      const w = width - gap;
      const levels = (n) => n > 0 ? Math.min(12, Math.max(1, Math.round(n / scale * 12))) : 0;
      const lit = levels(column.notes), scratch = Math.min(lit, levels(column.scratch));
      const paths = {off: [], notes: [], hot: [], scratch: [], cap: []};
      for (let row = 0; row < 12; row++) {
        const segment = rect(x, 67 - row * 6, w, 4);
        paths.off.push(segment);
        if (row < lit) paths[row >= 8 ? "hot" : "notes"].push(segment);
        if (row < scratch) paths.scratch.push(segment);
        if (row === lit - 1) paths.cap.push(rect(x, 67 - row * 6, w, .8));
      }
      Object.entries(paths).forEach(([kind, d]) => appendPath(group, `density-${kind}`, d));
      svg.append(group);
    });
    const axis = node("div", "density-axis");
    axis.append(node("span", "", "0:00"), node("span", "", clock(Math.ceil(values.duration_ms / 1000))));
    wrap.append(svg, axis);
    return wrap;
  }
  function radar(values, side) {
    const wrap = node("div", "song-radar");
    wrap.setAttribute("aria-label", `${side}P 谱面雷达`);
    const axes = ["notes", "peak", "scratch", "soflan", "charge", "chord"];
    if (values) {
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", "0 0 72 72");
      svg.setAttribute("aria-hidden", "true");
      const scale = Math.max(200, ...axes.map((key) => values[key]));
      const points = (amount) => axes.map((key, i) => {
        const r = amount(key) / scale * 31;
        const angle = i * Math.PI / 3 - Math.PI / 2;
        return `${36 + Math.cos(angle) * r},${36 + Math.sin(angle) * r}`;
      }).join(" ");
      [100, scale, null].forEach((ring) => {
        const polygon = document.createElementNS(svg.namespaceURI, "polygon");
        polygon.setAttribute("points", points((key) => ring ?? values[key]));
        polygon.setAttribute("class", ring === null ? "radar-value" : "radar-ring");
        svg.append(polygon);
      });
      wrap.append(svg);
    }
    const data = node("div", "radar-data");
    data.append(node("span", "radar-caption", `${side}P RADAR`));
    axes.forEach((key) => {
      const item = node("span", "radar-axis");
      item.append(node("span", "", key.toUpperCase()), node("b", "", values ? Number(values[key]).toFixed(2) : "—"));
      data.append(item);
    });
    wrap.append(data);
    return wrap;
  }
  function render(s, offline = false) {
    const warning = offline ? "等待游戏连接 · 游戏启动后会自动恢复" : !s.connected ? s.status : !s.ready ? "等待游戏曲库 · 请进入普通选曲界面" : "";
    document.querySelectorAll(".connection-banner").forEach((el) => { el.hidden = !warning; text(el, warning); });
    if (frame) {
      const connected = !offline && s.connected;
      const name = connected && typeof s.room?.name === "string" ? s.room.name.trim() : "";
      const roomName = $("room-name");
      text(roomName, connected ? name : "未连接");
      roomName.hidden = connected && !name;
      roomName.title = name;
      roomName.dataset.connected = String(connected);
    }
    const c = s.current;
    if (frame && s.now_playing) {
      renderNowPlaying(s.now_playing, offline);
    } else {
      signatures.delete("game-song");
      $("current-content").classList.remove("game-content");
      changed("current", c ? [c.token, c.title, c.requester, c.chart, c.mode, c.chart_style] : [null, offline, s.ready, s.queue.length > 0], () => {
        const box = $("current-content");
        box.replaceChildren();
        if (c) {
          const title = node("h1", "current-title", c.title);
          title.title = c.title;
          const meta = node("div", "current-meta");
          const time = node("span", "remaining");
          time.append(node("b", "", ""), document.createTextNode(" 后跳过"));
          if (frame) {
            const song = node("div", "current-song");
            song.append(title, chart(c));
            box.append(song);
          } else {
            box.append(title);
            meta.append(chart(c));
          }
          meta.append(node("span", "requester", `由 ${c.requester} 点歌`), time);
          box.append(meta);
        } else {
          const empty = node("div", "current-idle");
          empty.append(node("h2", "", "暂无点歌"));
          box.append(empty);
        }
      });
      if (c) text($("current-content").querySelector(".remaining b"), clock(c.remaining));
      progress($("current-progress"), c ? c.remaining : 0, c ? c.duration : 1);
    }
    changed("count", [s.queue.length, s.capacity], () => {
      $("queue-count").replaceChildren(document.createTextNode(pad(s.queue.length) + " "), node("span", "", `/ ${s.capacity || "—"}`));
    });
    // Each viewer controls their candidate page through chat. Viewers rotate independently.
    const choices = s.pending.filter((p) => p.candidates.length > 0);
    const choiceKey = (p) => JSON.stringify([p.requester, p.mode, p.chart, p.candidates]);
    const pages = choices.length;
    const previousIndex = choices.findIndex((p) => choiceKey(p) === activeChoice);
    const flippedIndex = choices.findIndex((p) => choicePages.has(choiceKey(p)) && choicePages.get(choiceKey(p)) !== candidatePage(p));
    choicePages.clear();
    choices.forEach((p) => choicePages.set(choiceKey(p), candidatePage(p)));
    pageIndex = flippedIndex >= 0 ? flippedIndex : previousIndex < 0 ? 0 : previousIndex;
    if (previousIndex < 0 || flippedIndex >= 0) pageSince = performance.now();
    if (performance.now() - pageSince > 6000) { pageIndex = (pageIndex + 1) % Math.max(1, pages); pageSince = performance.now(); }
    activeChoice = pages ? choiceKey(choices[pageIndex]) : null;
    const pending = choices.slice(pageIndex, pageIndex + 1);
    const queueState = pages || s.queue.length ? "expanded" : c ? "current" : "collapsed";
    document.querySelector(".overlay").dataset.queueState = queueState;
    const queueVisible = !frame || pages > 0 || s.queue.length > 0;
    const queuePanel = document.querySelector(".queue-panel");
    queuePanel.hidden = !queueVisible;
    if (frame) document.querySelector(".sidebar").dataset.queueVisible = String(queueVisible);
    $("pending-area").hidden = !pages;
    $("queue-area").hidden = pages > 0;
    $("queue-area").setAttribute("aria-hidden", String(!queueVisible || pages > 0 || (!frame && queueState === "collapsed")));
    $("waiting-area").setAttribute("aria-hidden", String(!queueVisible || pages > 0 || (!frame && !s.queue.length)));
    queuePanel.classList.toggle("choosing", pages > 0);
    changed("pending", [pageIndex, pending.map(({remaining, ...p}) => p)], () => {
      $("pending-list").replaceChildren(...pending.map((p) => {
        const card = node("section", "choice-panel");
        const count = p.candidates.length;
        const currentPage = candidatePage(p);
        const pageCount = Math.ceil(count / candidatePageSize);
        const first = currentPage * candidatePageSize;
        const last = Math.min(first + candidatePageSize, count);
        card.style.setProperty("--choice-columns", 1);
        card.style.setProperty("--choice-rows", Math.min(count, candidatePageSize));
        const prompt = node("p", "choice-prompt", "请在弹幕发送编号选歌");
        const heading = node("div", "choice-heading");
        const person = node("div", "");
        const name = node("h2", "", `${p.requester}，请选择`);
        name.title = p.requester;
        person.append(name);
        const countdown = node("div", "countdown");
        countdown.append(node("b", "", ""), node("span", "", "秒"));
        heading.append(person, chart(p), countdown);
        const list = node("ol", "candidates");
        list.append(...p.candidates.slice(first, last).map((song, i) => {
          const row = node("li", `candidate${song.available ? "" : " unavailable"}`);
          const name = node("div", "candidate-title", song.title);
          name.title = song.title;
          if (!song.available) name.append(node("span", "unavailable-note", "所请求谱面不存在"));
          row.append(node("span", "candidate-number", first + i + 1), name);
          return row;
        }));
        const track = node("div", "time-track");
        track.append(node("span", ""));
        const help = node("p", "choice-help");
        help.append(document.createTextNode("发送 "), node("strong", "", first + 1 === last ? String(last) : `${first + 1}–${last}`), document.createTextNode(" 选歌 · 由点歌本人回复"));
        card.append(prompt, heading, list, help);
        if (pageCount > 1) {
          const pager = node("div", "choice-pagination");
          const previous = node("span", "", "p 上一页");
          const next = node("span", "", "n 下一页");
          previous.classList.toggle("at-boundary", currentPage === 0);
          next.classList.toggle("at-boundary", currentPage === pageCount - 1);
          pager.append(previous, node("span", "candidate-page", `${currentPage + 1} / ${pageCount}`), next);
          card.append(pager);
        }
        card.append(track);
        return card;
      }));
    });
    $("pending-list").querySelectorAll(".choice-panel").forEach((card, i) => {
      vfdNumber(card.querySelector(".countdown b"), pad(pending[i].remaining));
      card.querySelector(".countdown").classList.toggle("urgent", pending[i].remaining <= 10);
      progress(card.querySelector(".time-track span"), pending[i].remaining, pending[i].duration);
    });
    $("pending-page").hidden = pages <= 1;
    text($("pending-page"), `待选观众 ${pageIndex + 1} / ${pages} · 每 6 秒轮换`);
    renderQueue(s);

    const limit = Math.max(1, Math.min(100, Number(s.feed_limit) || 10));
    const history = s.feed.slice(-limit);
    const chats = history.filter((message) => message.kind === "chat");
    const events = history.filter((message) => message.kind !== "chat");
    const activity = $("activity-panel");
    // Measure the reclaimed queue space before choosing how many chats to show.
    activity.dataset.feedState = (frame ? chats : history).length ? "active" : "collapsed";
    if (frame) $("chat-empty").hidden = chats.length > 0;
    const feedBody = activity.querySelector(".feed-body");
    const feed = frame ? chats : history;
    activity.style.setProperty("--feed-rows", Math.max(1, feed.length));
    const chatRows = feed.filter((message) => message.kind === "chat").length;
    activity.style.setProperty("--feed-chat-rows", chatRows);
    activity.style.setProperty("--feed-event-rows", feed.length - chatRows);
    activity.dataset.feedState = feed.length ? "active" : "collapsed";
    const banner = activity.querySelector(".connection-banner");
    activity.style.setProperty("--feed-warning-height", `${banner.getBoundingClientRect().height}px`);
    feedBody.setAttribute("aria-hidden", String(!feed.length));
    function renderFeed(key, list, messages, availableHeight = null) {
      changed(key, [messages, availableHeight], () => {
        list.replaceChildren(...messages.map((message) => {
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
        if (availableHeight !== null) {
          // Pack actual one/two-line row heights, keeping a contiguous recent history.
          const rows = [...list.children];
          const gap = parseFloat(getComputedStyle(list).rowGap) || 0;
          let used = 0;
          let first = rows.length;
          for (let i = rows.length - 1; i >= 0; i--) {
            const height = rows[i].offsetHeight;
            const next = height + (first < rows.length ? gap : 0);
            if (used + next > availableHeight && first < rows.length) break;
            used += next;
            first = i;
          }
          rows.slice(0, first).forEach((row) => row.remove());
        }
      });
      return list.children.length;
    }
    const visibleFeed = renderFeed("feed", $("activity-list"), feed, frame ? feedBody.clientHeight : null);
    text($("feed-count"), `${visibleFeed} / ${frame ? chats.length : limit}`);
    if (frame) {
      renderFeed("events", $("event-list"), events.slice(-3));
      text($("event-count"), `${Math.min(3, events.length)} / ${events.length}`);
      $("chat-empty").hidden = feed.length > 0;
      $("event-empty").hidden = events.length > 0;
    }
    fitFeed();
  }
  const empty = () => ({connected: false, ready: false, status: "等待游戏连接", current: null, now_playing: {phase: "idle", song: null, players: []}, capacity: 0, queue: [], pending: [], feed: state?.feed || [], feed_limit: state?.feed_limit || 10});
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
  new ResizeObserver(fitFeed).observe($("activity-list"));
  tick();
})();
