use chart_requester::{
    config::Config,
    gui::{self, Bridge, Menu, Page, View},
    live_config::Store,
    platforms::Chat,
};
use std::{collections::VecDeque, path::PathBuf, sync::atomic::Ordering};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("requester-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("chart-requester.toml"),
            include_str!("../chart-requester.example.toml"),
        )
        .unwrap();
        std::fs::write(root.join("plugin.dll"), "fixture").unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn transactions_preserve_comments_relative_paths_and_detect_conflicts() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.toml");
    let mut store = Store::open(&path).unwrap();
    let mut next = store.raw.clone();
    next.requests.queue_capacity = 30;
    next.aliases.insert("测试别名".into(), "123".into());
    let prepared = store
        .prepare(next.clone(), 0, &f.0.join("plugin.dll"))
        .unwrap();
    let resolved = store.commit(prepared).unwrap();
    assert!(resolved.output.queue_path.is_absolute());
    assert_eq!(Config::read(&path).unwrap(), next);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# 等待队列"));
    assert!(text.contains("obs/queue.txt"));
    assert!(text.contains("menu_enabled"));
    assert!(!text.contains("\nskip_enabled ="));
    assert!(
        store
            .prepare(next.clone(), 0, &f.0.join("plugin.dll"))
            .is_err()
    );
    let prepared = store
        .prepare(next.clone(), 1, &f.0.join("plugin.dll"))
        .unwrap();
    std::fs::write(&path, format!("{text}\n# external edit\n")).unwrap();
    assert!(store.commit(prepared).is_err());
    assert_eq!(store.revision, 1);
    assert!(store.prepare(next, 1, &f.0.join("plugin.dll")).is_err());
    let reloaded = store.reload(&f.0.join("plugin.dll")).unwrap();
    store.commit(reloaded).unwrap();
    assert_eq!(store.revision, 2);
}
#[test]
fn rejected_settings_leave_disk_unchanged_and_old_controls_migrate() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.toml");
    let original = std::fs::read_to_string(&path)
        .unwrap()
        .replace("menu_enabled", "skip_enabled");
    std::fs::write(&path, &original).unwrap();
    let store = Store::open(&path).unwrap();
    for kind in 0..4 {
        let mut config = store.raw.clone();
        match kind {
            0 => config.requests.queue_capacity = 0,
            1 => config.output.queue_path = config.output.interaction_path.clone(),
            2 => config.output.queue_path = "plugin.dll".into(),
            _ => config.game.module = "different.dll".into(),
        }
        assert!(store.prepare(config, 0, &f.0.join("plugin.dll")).is_err());
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}
#[test]
fn alias_filter_is_fuzzy_and_editor_rejects_ambiguous_rows() {
    let rows = vec![
        (1, "向日葵".into(), "ヒマワリ".into()),
        (2, "Another AA".into(), "12345".into()),
    ];
    assert_eq!(gui::filter_aliases(&rows, "aa"), vec![2]);
    assert_eq!(gui::filter_aliases(&rows, "１３５"), vec![2]);
    assert_eq!(gui::filter_aliases(&rows, "向葵"), vec![1]);
    assert_eq!(gui::alias_map(&rows).unwrap().len(), 2);
    assert!(
        gui::alias_map(&[(1, "ＡＡ".into(), "1".into()), (2, "aa".into(), "2".into())]).is_err()
    );
    assert!(gui::alias_map(&[(1, "".into(), "1".into())]).is_err());
}
#[test]
fn every_page_renders_and_chat_is_bounded_independent_of_request_syntax() {
    let mut chats = VecDeque::new();
    let mut processing = VecDeque::new();
    for i in 0..600 {
        gui::record_chat(
            &mut chats,
            &Chat {
                user: "viewer".into(),
                name: "观众".into(),
                text: format!("普通聊天 {i}"),
            },
            i,
        );
        gui::record_processing(
            &mut processing,
            &Chat {
                user: "viewer".into(),
                name: "观众\n".into(),
                text: format!("普通聊天 {i}"),
            },
            "ignored_not_a_request",
            i,
        );
    }
    assert_eq!(chats.len(), gui::CHAT_LIMIT);
    assert_eq!(chats.front().unwrap().at, 100);
    assert_eq!(processing.len(), gui::PROCESSING_LIMIT);
    assert_eq!(processing.front().unwrap().at, 100);
    assert_eq!(processing.back().unwrap().name, "观众");
    assert_eq!(processing.back().unwrap().outcome, "ignored_not_a_request");
    let mut view = View::new(Config::default());
    view.chats = chats;
    view.processing = processing;
    let (bridge, _commands) = Bridge::new(view.clone(), egui::FontDefinitions::default());
    bridge.visible.store(true, Ordering::Release);
    let mut menu = Menu::new(&view);
    let context = egui::Context::default();
    context.set_fonts(gui::design::fonts());
    context.set_global_style(gui::design::style());
    for page in [
        Page::Live,
        Page::Aliases,
        Page::Requests,
        Page::Bilibili,
        Page::Output,
        Page::Controls,
        Page::Logging,
        Page::Game,
    ] {
        menu.page = page;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |root| menu.show(root.ctx(), &bridge),
        );
        assert!(!output.shapes.is_empty());
    }
}
