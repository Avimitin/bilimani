//! Queue controls only. The render thread observes owned data and sends commands;
//! it never owns the queue, reads CSV, or calls game/HTTP functions.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

#[cfg(windows)]
pub mod renderer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    Down,
    Up,
    Left,
    Right,
    Confirm,
    Back,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Remove(u64),
    Priority(u64),
    ReloadAliases,
}
#[derive(Clone, Debug)]
pub struct View {
    pub queue: bilimani_queue::View,
    pub can_select: bool,
    pub mode: Option<String>,
    pub aliases_path: String,
    pub aliases_count: usize,
    pub notice: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_navigation_uses_queue_ids_and_never_selects_executing_rows() {
        let (bridge, commands) = Bridge::new(Default::default());
        let mut queue = bilimani_queue::Queue::default();
        let id = queue
            .add(bilimani_queue::SongRequest {
                song_id: 1,
                title: "Song".into(),
                mode: "SP".into(),
                chart: None,
                requester: "Viewer".into(),
            })
            .unwrap()
            .id;
        bridge.publish(View {
            queue: queue.view(0),
            can_select: true,
            mode: Some("SP".into()),
            ..Default::default()
        });
        let ctx = egui::Context::default();
        let mut menu = Menu {
            navigation: vec![Navigation::Confirm],
            ..Default::default()
        };
        let _ = ctx.run_ui(Default::default(), |ui| menu.show(ui, &bridge));
        assert_eq!(commands.try_recv().unwrap(), Command::Priority(id));
        menu.navigation = vec![Navigation::Down, Navigation::Confirm];
        let _ = ctx.run_ui(Default::default(), |ui| menu.show(ui, &bridge));
        assert_eq!(commands.try_recv().unwrap(), Command::Remove(id));
        queue.next("SP", false).unwrap();
        bridge.publish(View {
            queue: queue.view(0),
            can_select: true,
            mode: Some("SP".into()),
            ..Default::default()
        });
        menu.navigation = vec![Navigation::Confirm];
        let _ = ctx.run_ui(Default::default(), |ui| menu.show(ui, &bridge));
        assert_eq!(commands.try_recv().unwrap(), Command::ReloadAliases);
    }
}
impl Default for View {
    fn default() -> Self {
        Self {
            queue: bilimani_queue::Queue::default().view(0),
            can_select: false,
            mode: None,
            aliases_path: String::new(),
            aliases_count: 0,
            notice: String::new(),
        }
    }
}
pub struct Bridge {
    pub visible: AtomicBool,
    pub fonts: egui::FontDefinitions,
    view: RwLock<View>,
    commands: SyncSender<Command>,
    navigation: Mutex<VecDeque<Navigation>>,
}
impl Bridge {
    pub fn new(fonts: egui::FontDefinitions) -> (Arc<Self>, Receiver<Command>) {
        let (commands, rx) = sync_channel(32);
        (
            Arc::new(Self {
                visible: AtomicBool::new(false),
                fonts,
                view: RwLock::new(View::default()),
                commands,
                navigation: Mutex::default(),
            }),
            rx,
        )
    }
    pub fn snapshot(&self) -> View {
        self.view.read().expect("menu snapshot").clone()
    }
    pub fn publish(&self, view: View) {
        *self.view.write().expect("menu snapshot") = view;
    }
    pub fn send(&self, command: Command) {
        let _ = self.commands.try_send(command);
    }
    pub fn navigate(&self, navigation: Vec<Navigation>) {
        let mut queue = self.navigation.lock().expect("menu input");
        for event in navigation {
            if queue.len() < 32 {
                queue.push_back(event);
            }
        }
    }
    pub fn take_navigation(&self) -> Vec<Navigation> {
        self.navigation
            .lock()
            .expect("menu input")
            .drain(..)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Command(Command),
    Close,
}
fn actions(view: &View) -> Vec<Action> {
    let busy = view.queue.items.iter().any(|item| item.executing);
    let mut actions = Vec::new();
    // First confirmation on opening selects the first available waiting song.
    for item in &view.queue.items {
        if view.can_select
            && !busy
            && view.mode.as_deref() == Some(item.entry.request.mode.as_str())
        {
            actions.push(Action::Command(Command::Priority(item.entry.id)));
        }
        if !item.executing {
            actions.push(Action::Command(Command::Remove(item.entry.id)));
        }
    }
    if let Some(current) = &view.queue.current {
        actions.push(Action::Command(Command::Remove(current.entry.id)));
    }
    actions.extend([Action::Command(Command::ReloadAliases), Action::Close]);
    actions
}
#[derive(Default)]
pub struct Menu {
    focused: Option<Action>,
    navigation: Vec<Navigation>,
}
impl Menu {
    pub fn new(_: &View) -> Self {
        Self::default()
    }
    pub fn hidden(&mut self) {
        self.focused = None;
        self.navigation.clear();
    }
    pub fn controller_input(
        &mut self,
        _: &egui::Context,
        raw: &mut egui::RawInput,
        mut navigation: Vec<Navigation>,
    ) {
        raw.events.retain(|event| {
            if let egui::Event::Key {
                key, pressed: true, ..
            } = event
            {
                let direction = match key {
                    egui::Key::ArrowDown | egui::Key::Tab => Some(Navigation::Down),
                    egui::Key::ArrowUp => Some(Navigation::Up),
                    egui::Key::ArrowLeft => Some(Navigation::Left),
                    egui::Key::ArrowRight => Some(Navigation::Right),
                    egui::Key::Enter => Some(Navigation::Confirm),
                    egui::Key::Escape => Some(Navigation::Back),
                    _ => None,
                };
                if let Some(direction) = direction {
                    navigation.push(direction);
                    return false;
                }
            }
            true
        });
        self.navigation = navigation;
    }
    fn dispatch(&self, bridge: &Bridge, action: Action) {
        match action {
            Action::Command(command) => bridge.send(command),
            Action::Close => bridge.visible.store(false, Ordering::Release),
        }
    }
    pub fn show(&mut self, root: &mut egui::Ui, bridge: &Bridge) {
        let view = bridge.snapshot();
        let actions = actions(&view);
        if !self.focused.is_some_and(|focus| actions.contains(&focus)) {
            self.focused = actions.first().copied();
        }
        let mut moved = false;
        for event in std::mem::take(&mut self.navigation) {
            let index = self
                .focused
                .and_then(|focus| actions.iter().position(|action| *action == focus))
                .unwrap_or(0);
            match event {
                Navigation::Down | Navigation::Right => {
                    self.focused = Some(actions[(index + 1) % actions.len()]);
                    moved = true;
                }
                Navigation::Up | Navigation::Left => {
                    self.focused = Some(actions[(index + actions.len() - 1) % actions.len()]);
                    moved = true;
                }
                Navigation::Confirm => {
                    if let Some(action) = self.focused {
                        self.dispatch(bridge, action);
                    }
                }
                Navigation::Back => self.dispatch(bridge, Action::Close),
            }
        }
        egui::Window::new("点歌队列")
            .id(egui::Id::new("request-queue-menu"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size([760.0, 540.0])
            .collapsible(false)
            .resizable(false)
            .show(root.ctx(), |ui| {
                ui.label("B1 / B2 移动 · 转盘切换操作 · B6 执行 · B7 关闭");
                ui.label("菜单打开时暂停自动选曲；优先选曲成功后自动关闭。");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.heading(format!(
                        "等待 {} / {}",
                        view.queue.items.len(),
                        view.queue.capacity
                    ));
                    if !view.can_select {
                        ui.label("当前场景不可选曲");
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("request-rows")
                    .max_height(305.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if view.queue.items.is_empty() {
                            ui.label("队列为空，等待观众点歌");
                        }
                        for item in &view.queue.items {
                            ui.push_id(item.entry.id, |ui| {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    let request = &item.entry.request;
                                    ui.horizontal_wrapped(|ui| {
                                        ui.strong(&request.title);
                                        ui.label(request.chart.as_deref().unwrap_or(&request.mode));
                                        if !request.requester.is_empty() {
                                            ui.label(format!("由 {} 点歌", request.requester));
                                        }
                                    });
                                    if item.executing {
                                        ui.label("正在定位…");
                                    }
                                    if let Some(error) = &item.entry.error {
                                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                                    }
                                    ui.horizontal(|ui| {
                                        self.button(
                                            ui,
                                            bridge,
                                            &actions,
                                            Action::Command(Command::Priority(item.entry.id)),
                                            "优先选这首",
                                            moved,
                                        );
                                        self.button(
                                            ui,
                                            bridge,
                                            &actions,
                                            Action::Command(Command::Remove(item.entry.id)),
                                            "删除",
                                            moved,
                                        );
                                    });
                                });
                            });
                        }
                        if let Some(current) = &view.queue.current {
                            ui.separator();
                            ui.label(format!(
                                "当前点歌：{} · {} 秒后结束等待",
                                current.entry.request.title, current.remaining_seconds
                            ));
                            self.button(
                                ui,
                                bridge,
                                &actions,
                                Action::Command(Command::Remove(current.entry.id)),
                                "跳过当前点歌",
                                moved,
                            );
                        }
                    });
                ui.separator();
                ui.label(format!(
                    "别名：{} 条 · {}",
                    view.aliases_count, view.aliases_path
                ));
                ui.horizontal(|ui| {
                    self.button(
                        ui,
                        bridge,
                        &actions,
                        Action::Command(Command::ReloadAliases),
                        "重新加载别名 CSV",
                        moved,
                    );
                    self.button(ui, bridge, &actions, Action::Close, "关闭菜单", moved);
                });
                if !view.notice.is_empty() {
                    ui.label(&view.notice);
                }
            });
    }
    fn button(
        &mut self,
        ui: &mut egui::Ui,
        bridge: &Bridge,
        actions: &[Action],
        action: Action,
        label: &str,
        moved: bool,
    ) {
        let focused = self.focused == Some(action);
        let response = ui.add_enabled(
            actions.contains(&action),
            egui::Button::new(label)
                .selected(focused)
                .min_size(egui::vec2(110.0, 32.0)),
        );
        if focused && moved {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.clicked() {
            self.focused = Some(action);
            self.dispatch(bridge, action);
        }
    }
}
