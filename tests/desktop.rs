use bilimani::{
    config::{Config, Game},
    desktop::Backend,
    game::Song,
    gui::{Action, Command},
    live_config::Store,
    profiles::{self, CardId, StreamProfile},
};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("bilimani-desktop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("bilimani.exe"), "fixture").unwrap();
        Self(root)
    }
    fn exe(&self) -> PathBuf {
        self.0.join("bilimani.exe")
    }
    fn db(&self) -> PathBuf {
        self.0.join("bilimani.db")
    }
    fn backend(&self) -> Backend {
        Backend::open(&self.exe(), None).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn apply(backend: &mut Backend, config: Config) {
    backend.handle(Command {
        id: 1,
        action: Action::Apply {
            config: Box::new(config),
            revision: backend.view().revision,
            bind_card: None,
        },
    });
}
fn songs() -> Vec<Song> {
    vec![Song {
        id: 5,
        title: "测试歌曲".into(),
        search_terms: vec![],
        charts: vec![],
    }]
}

#[test]
fn double_click_uses_exe_directory_and_offline_profiles_round_trip_to_dll_store() {
    let f = Fixture::new();
    let mut backend = f.backend();
    assert!(f.db().is_file());
    assert!(backend.view().standalone);
    assert!(!backend.view().ready);
    assert!(backend.view().player_card.is_none());
    let mut config = backend.view().config;
    config.bilibili.auth_code = "global-test".into();
    let mut profile = StreamProfile::unbound();
    profile.bilibili.auth_code = "personal-test".into();
    profile
        .cards
        .push(CardId::parse("E0040123456789AB").unwrap());
    config.profiles.push(profile);
    apply(&mut backend, config.clone());
    assert_eq!(backend.view().revision, 1, "{}", backend.view().reply.1);
    let dll_store = Store::open(&f.db()).unwrap();
    assert_eq!(dll_store.raw, config);
    assert_eq!(
        profiles::for_card(&dll_store.raw, config.profiles[0].cards.first())
            .unwrap()
            .bilibili
            .auth_code,
        "personal-test"
    );
    assert!(!f.0.join("obs/queue.txt").exists());
    assert!(!f.0.join("obs/interaction.txt").exists());
    assert!(!f.0.join("bilimani.log").exists());
}

#[test]
fn cached_alias_validation_and_concurrent_game_edits_preserve_both_sides() {
    let f = Fixture::new();
    let mut dll = Store::open(&f.db()).unwrap();
    let mut desktop = f.backend();
    let mut config = desktop.view().config;
    config.aliases.insert("测试".into(), "5".into());
    apply(&mut desktop, config.clone());
    assert_eq!(desktop.view().revision, 0);
    assert!(desktop.view().reply.1.contains("无法校验新别名"));
    dll.cache_catalog(&songs()).unwrap();
    assert_eq!(Store::open(&f.db()).unwrap().revision, 0); // Cache is not a settings edit.
    apply(&mut desktop, config.clone());
    assert_eq!(desktop.view().revision, 1, "{}", desktop.view().reply.1);
    assert!(desktop.view().ready);
    config.aliases.insert("错误".into(), "missing song".into());
    apply(&mut desktop, config);
    assert_eq!(desktop.view().revision, 1);
    assert!(desktop.view().reply.1.contains("exactly one song"));
    // Game reloads EXE settings, then independently changes another setting.
    dll.commit(dll.reload(&f.exe()).unwrap()).unwrap();
    assert_eq!(dll.raw.aliases["测试"], "5");
    let mut next = dll.raw.clone();
    next.requests.queue_capacity = 42;
    dll.commit(dll.prepare(next.clone(), dll.revision, &f.exe()).unwrap())
        .unwrap();
    let stale = desktop.view().config;
    apply(&mut desktop, stale);
    assert!(desktop.view().reply.1.contains("另一个窗口"));
    assert_eq!(Store::open(&f.db()).unwrap().raw, next);
    desktop.handle(Command {
        id: 2,
        action: Action::Reload,
    });
    assert_eq!(desktop.view().config, next);
    assert_eq!(desktop.view().revision, 2);
}

#[test]
fn explicit_relative_catalog_validates_new_aliases_in_the_same_save() {
    let f = Fixture::new();
    // A single genuine-layout record; parse_database verifies its ID lookup.
    let mut data = vec![0u8; 16 + 8 * 4 + 0x7f8];
    data[..4].copy_from_slice(b"IIDX");
    for (offset, value) in [(4, 33u32), (8, 1), (12, 8)] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let record = &mut data[16 + 8 * 4..];
    record[0x67c..0x680].copy_from_slice(&5u32.to_le_bytes());
    for (index, unit) in "测试歌曲".encode_utf16().enumerate() {
        record[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(f.0.join("music_data.bin"), data).unwrap();
    let mut desktop = f.backend();
    let mut config = desktop.view().config;
    config.game.database_path = "music_data.bin".into();
    config.aliases.insert("测试".into(), "测试歌曲".into());
    apply(&mut desktop, config.clone());
    assert_eq!(desktop.view().revision, 1, "{}", desktop.view().reply.1);
    assert!(desktop.view().ready);
    assert_eq!(Store::open(&f.db()).unwrap().raw, config);
    // Never use a stale cache as fallback for an explicitly broken file.
    std::fs::write(f.0.join("music_data.bin"), "broken").unwrap();
    config.aliases.insert("新别名".into(), "5".into());
    apply(&mut desktop, config);
    assert_eq!(desktop.view().revision, 1);
    assert!(desktop.view().reply.1.contains("无法校验新别名"));
}

#[test]
fn cache_stays_bound_to_running_game_after_next_startup_settings_are_saved() {
    let f = Fixture::new();
    let mut dll = Store::open(&f.db()).unwrap();
    let mut config = dll.raw.clone();
    config.game.module = "other.dll".into();
    let changed_game = config.game.clone();
    dll.commit(dll.prepare(config, 0, &f.exe()).unwrap())
        .unwrap();
    dll.cache_catalog(&songs()).unwrap();
    assert!(dll.cached_catalog(&Game::default()).unwrap().is_some());
    assert!(dll.cached_catalog(&changed_game).unwrap().is_none());
    let desktop = f.backend();
    assert!(!desktop.view().ready);
}

#[test]
fn no_catalog_still_allows_bad_alias_removal_and_backup_import_remains_a_draft() {
    let f = Fixture::new();
    let mut store = Store::open(&f.db()).unwrap();
    let mut config = store.raw.clone();
    config.aliases.insert("bad".into(), "missing".into());
    store
        .commit(store.prepare(config, 0, &f.exe()).unwrap())
        .unwrap();
    let mut desktop = f.backend();
    let mut next = desktop.view().config;
    next.aliases.clear();
    apply(&mut desktop, next.clone());
    assert_eq!(desktop.view().revision, 2);
    desktop.handle(Command {
        id: 2,
        action: Action::ExportJson("backup.json".into()),
    });
    next.requests.queue_capacity = 41;
    apply(&mut desktop, next.clone());
    desktop.handle(Command {
        id: 3,
        action: Action::ImportJson("backup.json".into()),
    });
    assert_eq!(desktop.view().config, next);
    assert_eq!(
        desktop.view().loaded_draft.unwrap().requests.queue_capacity,
        20
    );
    assert_eq!(Store::open(&f.db()).unwrap().raw, next);
}
