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
fn fresh_install_uses_defaults_without_creating_toml() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.raw, Config::default());
    assert_eq!(store.revision, 0);
    assert!(!f.0.join("chart-requester.toml").exists());
    assert!(
        std::fs::read(&path)
            .unwrap()
            .starts_with(b"SQLite format 3\0")
    );
}

#[test]
fn static_directory_defaults_for_old_settings_and_round_trips_relative_to_dll() {
    let f = Fixture::new();
    let database = f.0.join("settings/chart-requester.db");
    let dll = f.0.join("plugin.dll");
    let store = Store::open(&database).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let settings: String = connection
        .query_row("SELECT settings FROM settings", [], |r| r.get(0))
        .unwrap();
    let mut settings: serde_json::Value = serde_json::from_str(&settings).unwrap();
    settings["overlay"]
        .as_object_mut()
        .unwrap()
        .remove("static_dir");
    settings["overlay"]
        .as_object_mut()
        .unwrap()
        .remove("history_limit");
    connection
        .execute("UPDATE settings SET settings = ?1", [settings.to_string()])
        .unwrap();
    drop(connection);
    let mut store = Store::open(&database).unwrap();
    assert_eq!(store.raw.overlay.history_limit, 10);
    assert_eq!(
        store.raw.overlay.static_dir,
        PathBuf::from("chart_request_static")
    );
    let resolved = store.commit(store.reload(&dll).unwrap()).unwrap();
    assert_eq!(
        resolved.overlay.static_dir,
        f.0.join("chart_request_static")
    );
    let mut config = store.raw.clone();
    config.overlay.static_dir = "themes/custom".into();
    config.overlay.history_limit = 15;
    let resolved = store
        .commit(store.prepare(config.clone(), store.revision, &dll).unwrap())
        .unwrap();
    assert_eq!(resolved.overlay.static_dir, f.0.join("themes/custom"));
    assert_eq!(
        Store::open(&database).unwrap().raw.overlay.history_limit,
        15
    );
    assert_eq!(
        Store::open(&database).unwrap().raw.overlay.static_dir,
        PathBuf::from("themes/custom")
    );
    config.overlay.static_dir = f.0.join("absolute-theme");
    let resolved = store
        .commit(store.prepare(config.clone(), store.revision, &dll).unwrap())
        .unwrap();
    assert_eq!(resolved.overlay.static_dir, config.overlay.static_dir);
    store
        .export_json(std::path::Path::new("static-backup.json"))
        .unwrap();
    assert_eq!(
        store
            .import_json(std::path::Path::new("static-backup.json"))
            .unwrap()
            .overlay
            .history_limit,
        15
    );
    assert_eq!(
        store
            .import_json(std::path::Path::new("static-backup.json"))
            .unwrap()
            .overlay
            .static_dir,
        config.overlay.static_dir
    );
}

#[test]
fn legacy_config_migrates_once_with_credentials_aliases_and_relative_paths() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let legacy = path.with_extension("toml");
    let text = include_str!("fixtures/legacy-config.toml").replace("menu_enabled", "skip_enabled");
    std::fs::write(&legacy, &text).unwrap();
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.raw, Config::parse(&text).unwrap());
    let mut next = store.raw.clone();
    next.bilibili.auth_code = "test-secret-only".into();
    next.requests.queue_capacity = 30;
    next.aliases.insert("测试别名".into(), "123".into());
    let prepared = store
        .prepare(next.clone(), 0, &f.0.join("plugin.dll"))
        .unwrap();
    let resolved = store.commit(prepared).unwrap();
    assert!(resolved.output.queue_path.is_absolute());
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), text);
    std::fs::write(&legacy, "invalid legacy file after migration").unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.raw, next);
    assert_eq!(reopened.revision, 1);
    assert_eq!(next.output.queue_path, PathBuf::from("obs/queue.txt"));
    let db = rusqlite::Connection::open(&path).unwrap();
    let global: String = db
        .query_row("SELECT settings FROM settings", [], |r| r.get(0))
        .unwrap();
    assert!(!global.contains("test-secret-only"));
    let profile: String = db
        .query_row("SELECT settings FROM stream_profiles", [], |r| r.get(0))
        .unwrap();
    assert!(profile.contains("test-secret-only"));
}

#[test]
fn concurrent_windows_detect_stale_prepares_commits_and_reloads() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let dll = f.0.join("plugin.dll");
    let mut a = Store::open(&path).unwrap();
    let mut b = Store::open(&path).unwrap();
    let stale = b.prepare(b.raw.clone(), 0, &dll).unwrap();
    let stale_reload = b.reload(&dll).unwrap();
    let mut config = a.raw.clone();
    config.requests.queue_capacity = 42;
    a.commit(a.prepare(config.clone(), 0, &dll).unwrap())
        .unwrap();
    assert!(b.commit(stale).is_err());
    assert!(b.commit(stale_reload).is_err());
    assert!(b.prepare(b.raw.clone(), 0, &dll).is_err());
    assert_eq!(b.revision, 0);
    b.commit(b.reload(&dll).unwrap()).unwrap();
    assert_eq!(b.revision, 1);
    assert_eq!(b.raw, config);
    assert!(b.prepare(b.raw.clone(), 0, &dll).is_err());
    assert_eq!(Store::open(&path).unwrap().revision, 1);
}

#[test]
fn transaction_failure_rolls_back_profile_and_global_settings() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let mut store = Store::open(&path).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_save BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    let mut next = store.raw.clone();
    next.bilibili.auth_code = "must-rollback".into();
    next.requests.queue_capacity = 88;
    let prepared = store.prepare(next, 0, &f.0.join("plugin.dll")).unwrap();
    assert!(store.commit(prepared).is_err());
    assert_eq!(store.raw, Config::default());
    assert_eq!(store.revision, 0);
    assert_eq!(Store::open(&path).unwrap().raw, Config::default());
}

#[test]
fn invalid_settings_leave_database_unchanged_and_game_changes_wait_for_restart() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let dll = f.0.join("plugin.dll");
    let mut store = Store::open(&path).unwrap();
    for kind in 0..4 {
        let mut config = store.raw.clone();
        match kind {
            0 => config.requests.queue_capacity = 0,
            1 => config.output.queue_path = config.output.interaction_path.clone(),
            2 => config.output.queue_path = "plugin.dll".into(),
            _ => config.game.module.clear(),
        }
        assert!(store.prepare(config, 0, &dll).is_err());
    }
    assert_eq!(Store::open(&path).unwrap().raw, Config::default());
    let mut next = store.raw.clone();
    next.game.database_path = "new-catalog.bin".into();
    let effective = store
        .commit(store.prepare(next.clone(), 0, &dll).unwrap())
        .unwrap();
    assert_eq!(effective.game, Config::default().game);
    assert!(store.restart_required());
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.raw.game, next.game);
    assert!(!reopened.restart_required());
}

#[test]
fn json_round_trip_stages_a_draft_and_never_overwrites_backups() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let dll = f.0.join("plugin.dll");
    let mut store = Store::open(&path).unwrap();
    let mut config = store.raw.clone();
    config.bilibili.auth_code = "backup-test-secret".into();
    config.bilibili.room_id = u64::MAX;
    config.aliases.insert("冥".into(), "123".into());
    store
        .commit(store.prepare(config.clone(), 0, &dll).unwrap())
        .unwrap();
    let exported = store
        .export_json(std::path::Path::new("backup.json"))
        .unwrap();
    assert!(store.export_json(&exported).is_err());
    let before = std::fs::read(&exported).unwrap();
    assert!(store.export_json(&path).is_err());
    let changed = Config::default();
    store
        .commit(store.prepare(changed.clone(), 1, &dll).unwrap())
        .unwrap();
    let draft = store.import_json(&exported).unwrap();
    assert_eq!(draft, config);
    assert_eq!(store.raw, changed);
    assert_eq!(Store::open(&path).unwrap().raw, changed);
    assert_eq!(std::fs::read(&exported).unwrap(), before);
    store
        .commit(store.prepare(draft, 2, &dll).unwrap())
        .unwrap();
    assert_eq!(store.raw, config);
}

#[test]
fn bad_imports_do_not_leak_values_or_modify_settings() {
    let f = Fixture::new();
    let store = Store::open(&f.0.join("chart-requester.db")).unwrap();
    let path = f.0.join("bad.json");
    for json in [
        r#"{"format":"chart-requester","version":99,"config":{}}"#,
        r#"{"format":"other-app","version":1,"config":{}}"#,
        r#"{"format":"chart-requester","version":1,"config":{"requests":{"queue_capacity":0}}}"#,
        r#"{"format":"chart-requester","version":1,"config":{"bilibili":{"auth_code":12345678901234}}}"#,
        r#"{"format":"chart-requester","version":1,"config":{"secret-value-here":true}}"#,
    ] {
        std::fs::write(&path, json).unwrap();
        let error = format!("{:#}", store.import_json(&path).unwrap_err());
        assert!(!error.contains("12345678901234"));
        assert!(!error.contains("secret-value-here"));
    }
    std::fs::write(&path, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(store.import_json(&path).is_err());
    assert_eq!(store.raw, Config::default());
    assert_eq!(store.revision, 0);
}

#[test]
fn future_foreign_and_corrupt_databases_are_not_reset() {
    let f = Fixture::new();
    for (name, sql) in [
        ("future.db", "PRAGMA user_version = 99;"),
        ("foreign.db", "CREATE TABLE other (id INTEGER);"),
    ] {
        let path = f.0.join(name);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(Store::open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let path = f.0.join("corrupt.db");
    std::fs::write(&path, b"not a sqlite database").unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not a sqlite database");
}

#[test]
fn failed_legacy_migration_can_be_retried_without_losing_original() {
    let f = Fixture::new();
    let path = f.0.join("chart-requester.db");
    let legacy = path.with_extension("toml");
    let invalid = "[bilibili]\nauth_code = 12345678901234\n";
    std::fs::write(&legacy, invalid).unwrap();
    let error = match Store::open(&path) {
        Ok(_) => panic!("invalid migration accepted"),
        Err(e) => format!("{e:#}"),
    };
    assert!(!error.contains("12345678901234"));
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), invalid);
    std::fs::write(&legacy, "").unwrap();
    assert_eq!(Store::open(&path).unwrap().raw, Config::default());
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
        Page::Data,
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
            |root| menu.show(root, &bridge),
        );
        assert!(!output.shapes.is_empty());
    }
}
