//! Platform-independent egui pages. The render thread only sends owned commands.
use crate::{
    config::{Config, LogLevel},
    engine::Engine,
    game::Navigation,
    platforms::{Chat, Connection},
};
use egui::{RichText, ScrollArea, TextEdit, Ui};
pub mod design;
mod live;
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ouroboros_ui::{
    Theme,
    atoms::{Badge, Button, Heading, Input, Kbd, Surface, Switch, Text},
    egui_phosphor::light as icons,
    molecules::{Card, SearchField},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

pub const CHAT_LIMIT: usize = 500;
pub const PROCESSING_LIMIT: usize = 500;
#[derive(Clone)]
pub struct ChatLine {
    pub at: u64,
    pub name: String,
    pub text: String,
}
#[derive(Clone)]
pub struct ProcessingLine {
    pub at: u64,
    pub name: String,
    pub text: String,
    pub outcome: &'static str,
}
pub fn record_processing(
    history: &mut VecDeque<ProcessingLine>,
    chat: &Chat,
    outcome: &'static str,
    at: u64,
) {
    history.push_back(ProcessingLine {
        at,
        name: chat
            .name
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect(),
        text: chat
            .text
            .chars()
            .filter(|c| !c.is_control())
            .take(2000)
            .collect(),
        outcome,
    });
    while history.len() > PROCESSING_LIMIT {
        history.pop_front();
    }
}
pub fn processing_label(outcome: &str) -> &str {
    match outcome {
        "enqueued" => "已处理：加入队列",
        "awaiting_selection" => "已处理：等待观众选择",
        "ignored_not_a_request" => "已忽略：普通聊天",
        "ignored_no_pending_selection" => "已忽略：没有待选择的请求",
        "ignored_invalid_sender_or_oversized_message" => "已忽略：用户无效或消息过长",
        "ignored_catalog_not_ready" => "未处理：曲库尚未就绪",
        "rejected_missing_song" => "已拒绝：缺少曲名",
        "rejected_game_mode_not_ready" => "已拒绝：游戏模式尚未就绪",
        "rejected_opposite_mode" => "已拒绝：SP / DP 模式不符",
        "rejected_mode_changed" => "已拒绝：游戏模式已改变",
        "rejected_queue_full" => "已拒绝：队列已满",
        "rejected_no_matches" => "已拒绝：未找到歌曲",
        "rejected_pending_limit" => "已拒绝：待选择请求已满",
        "rejected_invalid_selection" => "已拒绝：选择编号无效",
        "rejected_chart_missing" => "已拒绝：所请求谱面不存在",
        "rejected_cooldown" => "已拒绝：点歌冷却中",
        _ => outcome,
    }
}
pub fn record_chat(history: &mut VecDeque<ChatLine>, chat: &Chat, at: u64) {
    fn clean(s: &str, n: usize) -> String {
        s.chars().filter(|c| !c.is_control()).take(n).collect()
    }
    history.push_back(ChatLine {
        at,
        name: clean(&chat.name, 80),
        text: clean(&chat.text, 2000),
    });
    while history.len() > CHAT_LIMIT {
        history.pop_front();
    }
}
#[derive(Clone)]
pub struct QueueRow {
    pub token: u64,
    pub text: String,
    pub removable: bool,
}
#[derive(Clone)]
pub struct View {
    pub config: Config,
    pub loaded_draft: Option<Config>,
    pub revision: u64,
    pub connection: Connection,
    pub chats: VecDeque<ChatLine>,
    pub current: Option<QueueRow>,
    pub queue: Vec<QueueRow>,
    pub epoch: u64,
    pub ready: bool,
    pub reply: (u64, String),
    pub processing: VecDeque<ProcessingLine>,
}
impl View {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            loaded_draft: None,
            revision: 0,
            connection: Connection::waiting(),
            chats: VecDeque::new(),
            current: None,
            queue: vec![],
            epoch: 0,
            ready: false,
            reply: (0, String::new()),
            processing: VecDeque::new(),
        }
    }
    pub fn update_engine(&mut self, engine: Option<&Engine>) {
        self.ready = engine.is_some();
        self.current = engine.and_then(|e| {
            e.current.as_ref().map(|c| QueueRow {
                token: c.request.token,
                text: c.request.label(),
                removable: e.snapshot.phase == crate::game::Phase::Select,
            })
        });
        self.queue = engine.map_or_else(Vec::new, |e| {
            e.queue
                .iter()
                .map(|r| QueueRow {
                    token: r.token,
                    text: r.label(),
                    removable: e.can_remove(r.token),
                })
                .collect()
        });
        self.epoch = engine.map_or(0, |e| e.snapshot.epoch);
    }
}
pub enum Action {
    Apply { config: Box<Config>, revision: u64 },
    Reload,
    ImportJson(std::path::PathBuf),
    ExportJson(std::path::PathBuf),
    Remove(u64),
    Skip { token: u64, epoch: u64 },
}
pub struct Command {
    pub id: u64,
    pub action: Action,
}
pub struct Bridge {
    pub visible: AtomicBool,
    view: RwLock<Arc<View>>,
    commands: SyncSender<Command>,
    pub fonts: egui::FontDefinitions,
    navigation: Mutex<VecDeque<Navigation>>,
}
impl Bridge {
    pub fn new(view: View, fonts: egui::FontDefinitions) -> (Arc<Self>, Receiver<Command>) {
        let (commands, rx) = sync_channel(8);
        (
            Arc::new(Self {
                visible: AtomicBool::new(false),
                view: RwLock::new(Arc::new(view)),
                commands,
                fonts,
                navigation: Mutex::new(VecDeque::new()),
            }),
            rx,
        )
    }
    pub fn publish(&self, view: View) {
        *self.view.write().unwrap() = Arc::new(view);
    }
    pub fn snapshot(&self) -> Arc<View> {
        self.view.read().unwrap().clone()
    }
    pub fn toggle(&self) {
        self.visible.fetch_xor(true, Ordering::AcqRel);
    }
    pub fn navigate(&self, events: Vec<Navigation>) {
        let mut queue = self.navigation.lock().unwrap();
        if !self.visible.load(Ordering::Acquire) {
            queue.clear();
            return;
        }
        for event in events {
            if queue.len() < 32 {
                queue.push_back(event);
            }
        }
    }
    pub fn take_navigation(&self) -> Option<Navigation> {
        let mut queue = self.navigation.lock().unwrap();
        if !self.visible.load(Ordering::Acquire) {
            queue.clear();
            return None;
        }
        queue.pop_front()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Live,
    Aliases,
    Requests,
    Bilibili,
    Output,
    Controls,
    Logging,
    Game,
    Data,
}
const PAGES: [(Page, &str); 9] = [
    (Page::Live, "弹幕与队列"),
    (Page::Aliases, "歌曲别名"),
    (Page::Requests, "点歌规则"),
    (Page::Bilibili, "直播连接"),
    (Page::Output, "OBS 显示"),
    (Page::Controls, "按键操作"),
    (Page::Logging, "日志"),
    (Page::Game, "游戏适配"),
    (Page::Data, "备份与恢复"),
];
pub struct Menu {
    pub page: Page,
    draft: Config,
    aliases: Vec<(u64, String, String)>,
    query: String,
    revision: u64,
    next_id: u64,
    pending: Option<u64>,
    feedback: String,
    follow: bool,
    back: bool,
    was_open: bool,
    sidebar: Vec<egui::Id>,
    chat_offset: f32,
    processing_offset: f32,
    follow_processing: bool,
    backup_path: std::path::PathBuf,
}
impl Menu {
    /// Translate adapter-owned controller events; no IIDX button IDs enter egui.
    pub fn controller_input(
        &mut self,
        ctx: &egui::Context,
        input: &mut egui::RawInput,
        event: Option<Navigation>,
    ) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("controller-adjust"), 0i32));
        let Some(event) = event else {
            return;
        };
        if event == Navigation::Back {
            self.back = true;
            return;
        }
        let numeric = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::new("numeric-focus")));
        if matches!(event, Navigation::Left | Navigation::Right)
            && numeric.is_some()
            && numeric == ctx.memory(|m| m.focused())
        {
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("controller-adjust"),
                    if event == Navigation::Left {
                        -1i32
                    } else {
                        1i32
                    },
                )
            });
            return;
        }
        let (key, shift) = match event {
            Navigation::Down => (egui::Key::Tab, false),
            Navigation::Up => (egui::Key::Tab, true),
            Navigation::Left => (egui::Key::ArrowLeft, false),
            Navigation::Right => (egui::Key::ArrowRight, false),
            Navigation::Confirm => (egui::Key::Enter, false),
            Navigation::Back => unreachable!(),
        };
        for pressed in [true, false] {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            });
        }
    }
    pub fn hidden(&mut self) {
        self.was_open = false;
        self.back = false;
    }
    pub fn new(view: &View) -> Self {
        let mut menu = Self {
            page: Page::Live,
            draft: view.config.clone(),
            aliases: vec![],
            query: String::new(),
            revision: view.revision,
            next_id: 1,
            pending: None,
            feedback: String::new(),
            follow: true,
            back: false,
            was_open: false,
            sidebar: vec![],
            chat_offset: 0.0,
            processing_offset: 0.0,
            follow_processing: true,
            backup_path: "chart-requester-backup.json".into(),
        };
        menu.reset(view);
        menu
    }
    fn reset(&mut self, view: &View) {
        self.draft = view.config.clone();
        self.revision = view.revision;
        self.aliases = self
            .draft
            .aliases
            .iter()
            .enumerate()
            .map(|(i, (a, b))| (i as u64, a.clone(), b.clone()))
            .collect();
        self.next_id = self.next_id.max(self.aliases.len() as u64 + 1);
    }
    fn send(&mut self, bridge: &Bridge, action: Action) {
        let id = self.next_id;
        self.next_id += 1;
        if bridge.commands.try_send(Command { id, action }).is_ok() {
            self.pending = Some(id);
            self.feedback = "正在处理…".into();
        } else {
            self.feedback = "操作队列忙，请稍后再试".into();
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, bridge: &Bridge) {
        let view = bridge.snapshot();
        if self.revision != view.revision {
            self.reset(&view);
        }
        if self.pending == Some(view.reply.0) {
            self.pending = None;
            self.feedback = view.reply.1.clone();
            if let Some(config) = &view.loaded_draft {
                let mut loaded = (*view).clone();
                loaded.config = config.clone();
                self.reset(&loaded);
            }
        }
        let mut open = bridge.visible.load(Ordering::Acquire);
        if !open {
            self.hidden();
            return;
        }
        let opening = !self.was_open;
        self.was_open = true;
        if std::mem::take(&mut self.back) {
            let focused = ctx.memory(|m| m.focused());
            if focused.is_some_and(|id| self.sidebar.contains(&id)) {
                bridge.visible.store(false, Ordering::Release);
                self.hidden();
                return;
            }
            if let Some(index) = PAGES.iter().position(|p| p.0 == self.page)
                && let Some(id) = self.sidebar.get(index)
            {
                ctx.memory_mut(|m| m.request_focus(*id));
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            bridge.visible.store(false, Ordering::Release);
            return;
        }
        let screen = ctx.content_rect();
        let window_size = egui::vec2(
            1000.0_f32.min(screen.width() - 64.0),
            650.0_f32.min(screen.height() - 80.0),
        );
        self.sidebar.clear();
        egui::Window::new("Chart Requester  /  直播控制台")
            .id(egui::Id::new("requester-menu"))
            .title_bar(false)
            .fixed_size(window_size)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        Heading::new("Chart Requester").h2().show(ui);
                        Text::new("直播控制台").caption().muted().show(ui);
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Button::new("")
                            .icon_left(icons::X)
                            .icon_only()
                            .ghost()
                            .sm()
                            .show(ui)
                            .on_hover_text("关闭控制台")
                            .clicked()
                        {
                            open = false;
                        }
                        let badge = if view.connection.connected {
                            Badge::new("弹幕已连接").success()
                        } else {
                            Badge::new("弹幕未连接").secondary()
                        };
                        badge
                            .dot()
                            .sm()
                            .show(ui)
                            .on_hover_text(&view.connection.text);
                        if !view.ready {
                            Badge::new("等待曲库").warning().sm().show(ui);
                        }
                    });
                });
                ui.add_space(8.0);
                ui.separator();
                let body_height = (window_size.y
                    - if self.feedback.is_empty() {
                        120.0
                    } else {
                        152.0
                    })
                .max(200.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.set_min_height(body_height);
                        ui.vertical(|ui| {
                            ui.set_width(130.0);
                            ui.add_space(8.0);
                            Text::new("工作台").caption().muted().show(ui);
                            for (index, (page, label)) in PAGES.into_iter().enumerate() {
                                if index == 2 {
                                    ui.add_space(12.0);
                                    Text::new("设置").caption().muted().show(ui);
                                }
                                let icon = [
                                    icons::CHATS,
                                    icons::TAG,
                                    icons::LIST_NUMBERS,
                                    icons::BROADCAST,
                                    icons::MONITOR,
                                    icons::GAME_CONTROLLER,
                                    icons::FILE_TEXT,
                                    icons::CUBE,
                                    icons::DATABASE,
                                ][index];
                                let button = Button::new(label).icon_left(icon).sm();
                                let response = if self.page == page {
                                    button.secondary()
                                } else {
                                    button.ghost()
                                }
                                .show(ui);
                                self.sidebar.push(response.id);
                                if opening && self.page == page {
                                    response.request_focus();
                                }
                                if response.clicked() {
                                    self.page = page;
                                }
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_min_width((ui.available_width()).max(400.0));
                            let height = body_height;
                            ui.add_enabled_ui(self.pending.is_none(), |ui| {
                                if self.page == Page::Live {
                                    self.live(ui, bridge, &view, height);
                                } else {
                                    ScrollArea::vertical()
                                        .id_salt("settings-page")
                                        .max_height(height)
                                        .show(ui, |ui| {
                                            Card::new().sm().show(ui, |ui| {
                                                ui.set_min_width((ui.available_width()).max(360.0));
                                                self.settings(ui, &view.connection, bridge);
                                            });
                                        });
                                }
                            });
                        });
                    },
                );
                ui.separator();
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        if Button::new("应用并保存")
                            .icon_left(icons::CHECK)
                            .sm()
                            .show(ui)
                            .clicked()
                        {
                            match alias_map(&self.aliases) {
                                Ok(aliases) => {
                                    self.draft.aliases = aliases;
                                    self.send(
                                        bridge,
                                        Action::Apply {
                                            config: Box::new(self.draft.clone()),
                                            revision: self.revision,
                                        },
                                    );
                                }
                                Err(e) => self.feedback = e,
                            }
                        }
                        if Button::new("撤销修改").outline().sm().show(ui).clicked() {
                            self.reset(&view);
                            self.feedback = "已恢复当前配置".into();
                        }
                        if Button::new("刷新已保存设置")
                            .ghost()
                            .sm()
                            .show(ui)
                            .on_hover_text("放弃未保存修改，从数据库读取并应用设置")
                            .clicked()
                        {
                            self.send(bridge, Action::Reload);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Text::new("转盘 ← →").caption().muted().show(ui);
                            for (key, label) in [("B7", "返回"), ("B6", "确认"), ("B1/2", "上下")]
                            {
                                Text::new(label).caption().muted().show(ui);
                                Kbd::new(key).show(ui);
                            }
                        });
                    })
                });
                if !self.feedback.is_empty() {
                    ui.label(&self.feedback);
                }
            });
        bridge.visible.store(open, Ordering::Release);
    }
    fn settings(&mut self, ui: &mut Ui, connection: &Connection, bridge: &Bridge) {
        match self.page {
            Page::Live => {}
            Page::Aliases => {
                Heading::new("歌曲别名").h2().show(ui);
                SearchField::new(&mut self.query)
                    .placeholder("搜索别名、曲名或歌曲 ID…")
                    .show(ui);
                ui.weak("左列填写观众使用的别名，右列填写完整曲名或歌曲 ID。");
                if Button::new("添加别名")
                    .icon_left(icons::PLUS)
                    .outline()
                    .sm()
                    .show(ui)
                    .clicked()
                {
                    self.aliases
                        .push((self.next_id, String::new(), String::new()));
                    self.next_id += 1;
                    self.query.clear();
                }
                let visible = filter_aliases(&self.aliases, &self.query);
                let mut remove = None;
                egui::Grid::new("alias-table")
                    .num_columns(2)
                    .min_col_width(230.0)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("别名");
                        ui.strong("曲名 / ID");
                        ui.end_row();
                        for (id, alias, target) in &mut self.aliases {
                            if !visible.contains(id) {
                                continue;
                            }
                            ui.push_id((*id, "alias"), |ui| {
                                input(ui, alias, 200.0);
                            });
                            ui.push_id((*id, "target"), |ui| {
                                ui.horizontal(|ui| {
                                    input(ui, target, 240.0);
                                    if Button::new("")
                                        .icon_left(icons::TRASH)
                                        .icon_only()
                                        .ghost()
                                        .sm()
                                        .show(ui)
                                        .on_hover_text("删除别名")
                                        .clicked()
                                    {
                                        remove = Some(*id);
                                    }
                                })
                            });
                            ui.end_row();
                        }
                    });
                if let Some(id) = remove {
                    self.aliases.retain(|r| r.0 != id);
                }
            }
            Page::Requests => {
                Heading::new("点歌规则").h2().show(ui);
                let c = &mut self.draft.requests;
                number(ui, "队列容量", &mut c.queue_capacity, 1..=1000);
                number(ui, "候选歌曲数", &mut c.candidates, 1..=20);
                number(
                    ui,
                    "候选选择超时（秒）",
                    &mut c.selection_timeout_seconds,
                    1..=86400,
                );
                number(
                    ui,
                    "当前点歌超时（秒）",
                    &mut c.current_timeout_seconds,
                    1..=86400,
                );
                number(
                    ui,
                    "每人点歌冷却（秒，0 关闭）",
                    &mut c.cooldown_seconds,
                    0..=86400,
                );
                number(
                    ui,
                    "同时等待选择的观众数",
                    &mut c.max_pending_users,
                    1..=1000,
                );
                ui.weak("新规则用于后续请求；已有队列、候选和倒计时继续保留。");
            }
            Page::Bilibili => {
                Heading::new("直播连接").h2().show(ui);
                Text::new(&connection.text)
                    .caption()
                    .muted()
                    .wrap()
                    .show(ui);
                let c = &mut self.draft.bilibili;
                toggle(ui, &mut c.enabled, "启用弹幕连接");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut c.mode, "open_live".into(), "主播身份码");
                    ui.selectable_value(&mut c.mode, "web".into(), "直播间网页");
                });
                if c.mode == "open_live" {
                    field(ui, "身份码", &mut c.auth_code, true);
                    field(ui, "会话接口地址", &mut c.relay_url, false);
                    ui.collapsing("自有开放平台应用（直连）", |ui| {
                        number(ui, "App ID", &mut c.app_id, 0..=u64::MAX);
                        field(ui, "Access Key ID", &mut c.access_key_id, true);
                        field(ui, "Access Key Secret", &mut c.access_key_secret, true);
                    });
                } else {
                    number(ui, "直播间号", &mut c.room_id, 0..=u64::MAX);
                    field(ui, "SESSDATA", &mut c.sessdata, true);
                    field(ui, "buvid3", &mut c.buvid3, true);
                }
                ui.weak("应用后自动关闭旧会话并重新连接，队列不清空。凭据始终隐藏显示。");
            }
            Page::Output => {
                Heading::new("OBS 显示").h2().show(ui);
                toggle(ui, &mut self.draft.overlay.enabled, "启用浏览器来源");
                number(ui, "本机端口", &mut self.draft.overlay.port, 1..=65535);
                ui.label(format!(
                    "http://127.0.0.1:{}/queue",
                    self.draft.overlay.port
                ));
                ui.separator();
                path_field(ui, "队列文本文件", &mut self.draft.output.queue_path);
                path_field(ui, "交互文本文件", &mut self.draft.output.interaction_path);
                number(
                    ui,
                    "提示保留（秒）",
                    &mut self.draft.output.message_seconds,
                    1..=86400,
                );
                number(
                    ui,
                    "最近提示条数",
                    &mut self.draft.output.recent_messages,
                    1..=100,
                );
                ui.weak("相对路径以 DLL 目录为准。修改端口或文件后，也请更新 OBS 来源。");
            }
            Page::Controls => {
                Heading::new("按键操作").h2().show(ui);
                toggle(
                    ui,
                    &mut self.draft.controls.skip_enabled,
                    "对侧 Start 双击开关控制台",
                );
                number(
                    ui,
                    "双击间隔（毫秒）",
                    &mut self.draft.controls.double_tap_ms,
                    100..=2000,
                );
                ui.label("单人 SP 普通选曲：1P 登录双击 2P Start；2P 登录双击 1P Start。");
                ui.label(
                    "当前登录侧：B1 下一项、B2 上一项、B6 确认、B7 返回；转盘左右移动 / 调整数值。",
                );
                ui.label("文字可用键盘输入或粘贴，Esc 关闭。Start 不会直接跳过点歌。");
            }
            Page::Logging => {
                Heading::new("日志").h2().show(ui);
                let c = &mut self.draft.logging;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut c.level, LogLevel::Off, "关闭");
                    ui.selectable_value(&mut c.level, LogLevel::Info, "普通");
                    ui.selectable_value(&mut c.level, LogLevel::Debug, "详细");
                });
                toggle(ui, &mut c.danmu, "详细日志记录弹幕正文（不影响弹幕页）");
                number(ui, "单个文件上限（MiB）", &mut c.max_file_mb, 1..=100);
                number(ui, "备份个数", &mut c.backups, 1..=10);
                number(
                    ui,
                    "状态记录间隔（秒）",
                    &mut c.status_interval_seconds,
                    5..=3600,
                );
            }
            Page::Game => {
                Heading::new("游戏适配").h2().show(ui);
                field(ui, "游戏模块", &mut self.draft.game.module, false);
                path_field(ui, "曲库路径", &mut self.draft.game.database_path);
                ui.weak("通常保持默认即可。曲库路径留空时自动读取；本页修改保存后在下次启动生效。");
            }
            Page::Data => {
                Heading::new("备份与恢复").h2().show(ui);
                Text::new("设置自动保存在本机数据库中，无需编辑配置文件。")
                    .muted()
                    .wrap()
                    .show(ui);
                ui.add_space(12.0);
                path_field(ui, "JSON 文件", &mut self.backup_path);
                ui.weak("相对路径以配置数据库所在目录为准。文件名需以 .json 结尾。");
                ui.horizontal(|ui| {
                    if Button::new("导出已保存设置").outline().show(ui).clicked() {
                        self.send(bridge, Action::ExportJson(self.backup_path.clone()));
                    }
                    if Button::new("导入到编辑区").outline().show(ui).clicked() {
                        self.send(bridge, Action::ImportJson(self.backup_path.clone()));
                    }
                });
                ui.add_space(12.0);
                ui.label("导入会替换尚未保存的编辑内容；确认设置后点击「应用并保存」。");
                ui.label(
                    "导出仅包含已保存设置，已有同名文件不会被覆盖。队列和弹幕不包含在备份中。",
                );
                ui.weak("备份包含直播身份码等登录信息，请自行保管，不要公开分享。");
            }
        }
    }
}
fn number<T: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.x = 80.0;
        let id = ui.next_auto_id();
        if ui.memory(|m| m.has_focus(id)) {
            ui.data_mut(|d| d.insert_temp(egui::Id::new("numeric-focus"), id));
            let adjust = ui
                .data(|d| d.get_temp::<i32>(egui::Id::new("controller-adjust")))
                .unwrap_or(0);
            if adjust != 0 {
                *value = T::from_f64(
                    (value.to_f64() + f64::from(adjust))
                        .clamp(range.start().to_f64(), range.end().to_f64()),
                );
                ui.data_mut(|d| {
                    d.remove::<String>(id);
                    d.insert_temp(egui::Id::new("controller-adjust"), 0i32);
                });
            }
        }
        let response = ui.add(egui::DragValue::new(value).range(range));
        if response.has_focus() {
            response.scroll_to_me(None);
        }
        ui.label(label);
    });
}
fn field(ui: &mut Ui, label: &str, value: &mut String, secret: bool) {
    Text::new(label).label().show(ui);
    if secret {
        // Ouroboros Input has no password mode. Keep egui's masking rather than
        // passing credentials to a plain-text component.
        ui.add_sized(
            [ui.available_width().min(480.0), 32.0],
            TextEdit::singleline(value)
                .password(true)
                .margin(egui::vec2(10.0, 8.0)),
        );
    } else {
        input(ui, value, ui.available_width().min(480.0));
    }
}
fn input(ui: &mut Ui, value: &mut String, width: f32) {
    ui.allocate_ui(egui::vec2(width, 32.0), |ui| {
        Input::new(value).sm().show(ui);
    });
}
fn toggle(ui: &mut Ui, value: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        Switch::new(value).sm().show(ui).on_hover_text(label);
        Text::new(label).label().show(ui);
    });
}
fn path_field(ui: &mut Ui, label: &str, value: &mut std::path::PathBuf) {
    let mut text = value.to_string_lossy().to_string();
    field(ui, label, &mut text, false);
    *value = text.into();
}
pub fn alias_map(rows: &[(u64, String, String)]) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    let mut normalized = std::collections::HashSet::new();
    for (_, alias, target) in rows {
        if alias.trim().is_empty() || target.trim().is_empty() {
            return Err("别名与曲名 / ID 均不能为空".into());
        }
        if !normalized.insert(crate::catalog::normalize(alias)) {
            return Err("存在重复别名（忽略大小写与全半角）".into());
        }
        map.insert(alias.trim().into(), target.trim().into());
    }
    Ok(map)
}
pub fn filter_aliases(rows: &[(u64, String, String)], query: &str) -> Vec<u64> {
    let query = crate::catalog::normalize(query);
    let pattern = Pattern::new(
        &query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut buffer = Vec::new();
    rows.iter()
        .filter(|(_, a, b)| {
            query.is_empty()
                || pattern
                    .score(
                        Utf32Str::new(&crate::catalog::normalize(&format!("{a} {b}")), &mut buffer),
                        &mut matcher,
                    )
                    .is_some()
        })
        .map(|r| r.0)
        .collect()
}

#[cfg(test)]
mod controller_tests {
    use super::*;
    fn frame(menu: &mut Menu, ctx: &egui::Context, bridge: &Bridge, event: Option<Navigation>) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        menu.controller_input(ctx, &mut input, event);
        let _ = ctx.run_ui(input, |root| menu.show(root.ctx(), bridge));
    }
    #[test]
    fn controller_can_choose_page_adjust_number_and_back_out_without_mouse() {
        let view = View::new(Config::default());
        let (bridge, _rx) = Bridge::new(view.clone(), egui::FontDefinitions::default());
        let ctx = egui::Context::default();
        ctx.set_fonts(design::fonts());
        ctx.set_global_style(design::style());
        let mut menu = Menu::new(&view);
        bridge.visible.store(true, Ordering::Release);
        for _ in 0..3 {
            frame(&mut menu, &ctx, &bridge, None);
        }
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Down));
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Confirm));
        assert!(menu.page == Page::Aliases);
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Down));
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Confirm));
        assert!(menu.page == Page::Requests);
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Right));
        for _ in 0..2 {
            frame(&mut menu, &ctx, &bridge, None);
        }
        let before = menu.draft.requests.clone();
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Right));
        assert_ne!(menu.draft.requests, before);
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Left));
        assert_eq!(menu.draft.requests, before);
        // The Ouroboros switch must respond to the same controller confirm key.
        menu.page = Page::Bilibili;
        frame(&mut menu, &ctx, &bridge, None);
        ctx.memory_mut(|m| m.request_focus(menu.sidebar[3]));
        for _ in 3..PAGES.len() {
            frame(&mut menu, &ctx, &bridge, Some(Navigation::Down));
        }
        let enabled = menu.draft.bilibili.enabled;
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Confirm));
        assert_ne!(menu.draft.bilibili.enabled, enabled);
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Back));
        assert!(bridge.visible.load(Ordering::Acquire));
        assert_eq!(ctx.memory(|m| m.focused()), Some(menu.sidebar[3]));
        frame(&mut menu, &ctx, &bridge, Some(Navigation::Back));
        assert!(!bridge.visible.load(Ordering::Acquire));
    }

    #[test]
    fn import_reply_updates_draft_without_applying_and_refresh_can_discard_it() {
        let mut view = View::new(Config::default());
        view.revision = 7;
        let (bridge, rx) = Bridge::new(view.clone(), design::fonts());
        let ctx = egui::Context::default();
        ctx.set_fonts(design::fonts());
        let mut menu = Menu::new(&view);
        menu.send(&bridge, Action::ImportJson("backup.json".into()));
        let command = rx.try_recv().unwrap();
        let mut draft = view.config.clone();
        draft.requests.queue_capacity = 42;
        draft.aliases.insert("imported alias".into(), "123".into());
        view.loaded_draft = Some(draft.clone());
        view.reply = (command.id, "imported".into());
        bridge.publish(view.clone());
        frame(&mut menu, &ctx, &bridge, None);
        assert_eq!(menu.draft, draft);
        assert_eq!(alias_map(&menu.aliases).unwrap(), draft.aliases);
        assert_eq!(bridge.snapshot().config, Config::default());
        assert_eq!(menu.revision, 7);
        assert!(rx.try_recv().is_err());
        menu.send(&bridge, Action::Reload);
        let command = rx.try_recv().unwrap();
        view.reply = (command.id, "refreshed".into());
        view.loaded_draft = Some(view.config.clone());
        bridge.publish(view);
        frame(&mut menu, &ctx, &bridge, None);
        assert_eq!(menu.draft, Config::default());
    }
}
