use crate::{
    catalog::{Catalog, Chart, Mode, Song},
    config::Config,
};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Clone, Debug)]
pub struct Chat {
    pub user: String,
    pub name: String,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub token: u64,
    pub user: String,
    pub name: String,
    pub song: Song,
    pub mode: Mode,
    pub chart: Option<Chart>,
}
impl Request {
    pub fn label(&self) -> String {
        format!(
            "{} [{}] — {}",
            self.song.title,
            self.chart
                .map(|c| c.label())
                .unwrap_or_else(|| format!("{:?}", self.mode)),
            self.name
        )
    }
}
pub struct Pending {
    pub name: String,
    pub songs: Vec<Song>,
    pub mode: Mode,
    pub chart: Option<Chart>,
    pub until: u64,
}
pub struct Current {
    pub request: Request,
    pub until: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Other,
    Select,
    Playing,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub mode: Option<Mode>,
    pub epoch: u64,
}
#[derive(Clone, Debug)]
pub struct Jump {
    pub request: Request,
    pub epoch: u64,
}

pub struct Engine {
    pub config: Config,
    pub catalog: Catalog,
    pub queue: VecDeque<Request>,
    pub current: Option<Current>,
    pub pending: BTreeMap<String, Pending>,
    pub messages: VecDeque<(u64, String)>,
    pub status: String,
    pub snapshot: Snapshot,
    cooldowns: HashMap<String, u64>,
    in_flight: Option<Jump>,
    next_token: u64,
    diagnostics: VecDeque<String>,
}
impl Engine {
    pub fn new(config: Config, catalog: Catalog) -> Self {
        Self {
            config,
            catalog,
            queue: VecDeque::new(),
            current: None,
            pending: BTreeMap::new(),
            messages: VecDeque::new(),
            status: "等待弹幕连接".into(),
            snapshot: Snapshot::default(),
            cooldowns: HashMap::new(),
            in_flight: None,
            next_token: 1,
            diagnostics: VecDeque::new(),
        }
    }
    pub fn notice(&mut self, now: u64, message: impl Into<String>) {
        let message = message.into();
        self.diagnostics.push_back(message.clone());
        if self.diagnostics.len() > 2048 {
            self.diagnostics.pop_front();
        }
        self.messages.push_back((now, message));
        while self.messages.len() > self.config.output.recent_messages {
            self.messages.pop_front();
        }
    }
    pub fn take_diagnostics(&mut self) -> impl Iterator<Item = String> + '_ {
        self.diagnostics.drain(..)
    }
    pub fn activity(&self) -> &'static str {
        if self.snapshot.phase != Phase::Select {
            "waiting_for_song_select"
        } else if self.current.is_some() {
            "waiting_for_play_or_timeout"
        } else if self.in_flight.is_some() {
            "waiting_for_game_ack"
        } else if self.queue.is_empty() {
            "idle_queue_empty"
        } else {
            "ready_to_jump"
        }
    }
    pub fn chat(&mut self, mut chat: Chat, now: u64) -> &'static str {
        self.expire(now);
        if chat.user.is_empty() || chat.user == "0" || chat.text.len() > 1024 {
            return "ignored_invalid_sender_or_oversized_message";
        }
        chat.name = clean(&chat.name, 40);
        let text = chat.text.trim();
        if let Some(rest) = text
            .strip_prefix("点歌")
            .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        {
            self.pending.remove(&chat.user);
            let rest = rest.trim();
            if rest.is_empty() {
                self.notice(now, format!("{}：用法 点歌 <曲名> [SPA 等难度]", chat.name));
                return "rejected_missing_song";
            }
            let Some(mode) = self.snapshot.mode else {
                self.notice(now, "游戏模式尚未就绪，请进入选曲后重试");
                return "rejected_game_mode_not_ready";
            };
            let (query, chart) = match rest.rsplit_once(char::is_whitespace) {
                Some((name, tail)) if Chart::parse(tail).is_some() => {
                    (name.trim(), Chart::parse(tail))
                }
                _ => (rest, None),
            };
            if chart.is_some_and(|c| c.mode != mode) {
                self.notice(
                    now,
                    format!("{}：当前为 {:?}，无法接受另一模式的谱面", chat.name, mode),
                );
                return "rejected_opposite_mode";
            }
            if self.cooling(&chat.user, now) {
                self.notice(now, format!("{}：点歌冷却中，请稍后重试", chat.name));
                return "rejected_cooldown";
            }
            if self.queue.len() >= self.config.requests.queue_capacity {
                self.notice(now, format!("{}：队列已满，请稍后重试", chat.name));
                return "rejected_queue_full";
            }
            let songs = self.catalog.search(query, self.config.requests.candidates);
            if songs.is_empty() {
                self.notice(
                    now,
                    format!("{}：未找到歌曲 {}", chat.name, clean(query, 100)),
                );
                "rejected_no_matches"
            } else if songs.len() == 1 {
                self.enqueue(&chat.user, &chat.name, songs[0].clone(), mode, chart, now)
            } else if self.pending.len() >= self.config.requests.max_pending_users {
                self.notice(now, "待选择请求过多，请稍后重试");
                "rejected_pending_limit"
            } else {
                self.pending.insert(
                    chat.user,
                    Pending {
                        name: chat.name,
                        songs,
                        mode,
                        chart,
                        until: now + self.config.requests.selection_timeout_seconds,
                    },
                );
                "awaiting_selection"
            }
        } else if text.bytes().all(|b| b.is_ascii_digit()) && !text.is_empty() {
            let Some(pending) = self.pending.get(&chat.user) else {
                return "ignored_no_pending_selection";
            };
            let number = text.parse::<usize>().unwrap_or(0);
            if number == 0 || number > pending.songs.len() {
                self.notice(now, format!("{}：请输入候选列表中的编号", chat.name));
                return "rejected_invalid_selection";
            }
            let pending = self.pending.remove(&chat.user).unwrap();
            self.enqueue(
                &chat.user,
                &chat.name,
                pending.songs[number - 1].clone(),
                pending.mode,
                pending.chart,
                now,
            )
        } else {
            "ignored_not_a_request"
        }
    }
    fn cooling(&self, user: &str, now: u64) -> bool {
        self.cooldowns.get(user).is_some_and(|&until| now < until)
    }
    fn enqueue(
        &mut self,
        user: &str,
        name: &str,
        song: Song,
        mode: Mode,
        chart: Option<Chart>,
        now: u64,
    ) -> &'static str {
        if self.snapshot.mode != Some(mode) {
            self.notice(now, format!("{name}：游戏模式已改变，请重新点歌"));
            return "rejected_mode_changed";
        }
        if !song.supports(mode, chart) {
            self.notice(now, format!("{name}：{} 不存在所请求的谱面", song.title));
            return "rejected_chart_missing";
        }
        if self.cooling(user, now) {
            self.notice(now, format!("{name}：点歌冷却中"));
            return "rejected_cooldown";
        }
        if self.queue.len() >= self.config.requests.queue_capacity {
            self.notice(now, format!("{name}：队列已满，请稍后重试"));
            return "rejected_queue_full";
        }
        let request = Request {
            token: self.next_token,
            user: user.into(),
            name: name.into(),
            song,
            mode,
            chart,
        };
        self.next_token += 1;
        self.notice(now, format!("已加入队列：{}", request.label()));
        self.queue.push_back(request);
        if self.config.requests.cooldown_seconds > 0 {
            self.cooldowns
                .insert(user.into(), now + self.config.requests.cooldown_seconds);
        }
        "enqueued"
    }
    pub fn observe(&mut self, snapshot: Snapshot, now: u64) {
        // Gameplay consumes the request even when a different song was chosen.
        if snapshot.phase == Phase::Playing
            && let Some(c) = self.current.take()
        {
            self.notice(now, format!("已结束本次点歌：{}", c.request.label()));
        }
        self.snapshot = snapshot;
        self.expire(now);
    }
    pub fn expire(&mut self, now: u64) {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| now >= p.until)
            .map(|(u, _)| u.clone())
            .collect();
        for user in expired {
            let p = self.pending.remove(&user).unwrap();
            self.notice(now, format!("{}：选择超时，请重新点歌", p.name));
        }
        if self.current.as_ref().is_some_and(|c| now >= c.until) {
            let c = self.current.take().unwrap();
            self.notice(now, format!("点歌已超时跳过：{}", c.request.label()));
        }
        self.cooldowns.retain(|_, until| now < *until);
        self.messages
            .retain(|(when, _)| now.saturating_sub(*when) < self.config.output.message_seconds);
    }
    pub fn next_jump(&mut self, now: u64) -> Option<Jump> {
        self.expire(now);
        if self.snapshot.phase != Phase::Select
            || self.current.is_some()
            || self.in_flight.is_some()
        {
            return None;
        }
        while let Some(r) = self.queue.front() {
            if Some(r.mode) == self.snapshot.mode {
                break;
            }
            let r = self.queue.pop_front().unwrap();
            self.notice(now, format!("模式不符，已跳过：{}", r.label()));
        }
        let jump = Jump {
            request: self.queue.front()?.clone(),
            epoch: self.snapshot.epoch,
        };
        self.in_flight = Some(jump.clone());
        Some(jump)
    }
    /// None = selection changed before execution; retry on a later selection frame.
    pub fn jump_result(&mut self, token: u64, result: Option<Result<(), String>>, now: u64) {
        if self.in_flight.as_ref().map(|j| j.request.token) != Some(token) {
            return;
        }
        self.in_flight = None;
        let Some(result) = result else {
            return;
        };
        if self.queue.front().map(|r| r.token) != Some(token) {
            return;
        }
        let request = self.queue.pop_front().unwrap();
        match result {
            Ok(()) => {
                self.notice(now, format!("已定位：{}", request.label()));
                self.current = Some(Current {
                    request,
                    until: now + self.config.requests.current_timeout_seconds,
                });
            }
            Err(e) => self.notice(now, format!("无法定位 {}：{}", request.song.title, e)),
        }
    }
    pub fn render(&self, now: u64) -> (String, String) {
        let mut queue = String::from("当前点歌\n");
        if let Some(c) = &self.current {
            queue.push_str(&format!(
                "{}\n剩余 {} 秒\n",
                c.request.label(),
                c.until.saturating_sub(now)
            ));
        } else {
            queue.push_str("暂无\n");
        }
        queue.push_str(&format!(
            "\n等待队列 ({}/{})\n",
            self.queue.len(),
            self.config.requests.queue_capacity
        ));
        for (i, r) in self.queue.iter().enumerate() {
            queue.push_str(&format!("{}. {}\n", i + 1, r.label()));
        }
        let mut interaction = format!("{}\n", self.status);
        for (uid, p) in &self.pending {
            interaction.push_str(&format!(
                "\n{} [{}]：请回复编号（剩余 {} 秒）\n",
                p.name,
                clean(uid, 64),
                p.until.saturating_sub(now)
            ));
            for (i, s) in p.songs.iter().enumerate() {
                interaction.push_str(&format!(
                    "{}. {}{}\n",
                    i + 1,
                    s.title,
                    if s.supports(p.mode, p.chart) {
                        ""
                    } else {
                        "（所请求谱面不存在）"
                    }
                ));
            }
        }
        for (_, m) in &self.messages {
            interaction.push_str(&format!("\n{m}"));
        }
        (queue, interaction)
    }
}
fn clean(s: &str, limit: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(limit).collect()
}
