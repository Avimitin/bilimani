use bilimani::{
    config::Config,
    gui::View,
    live_config::Store,
    profiles::{self, ActiveStream, CardId, GLOBAL, StreamProfile},
};
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bilimani-profiles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("plugin.dll"), "fixture").unwrap();
        Self(path)
    }
    fn db(&self) -> PathBuf {
        self.0.join("config.db")
    }
    fn dll(&self) -> PathBuf {
        self.0.join("plugin.dll")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn card(n: u8) -> CardId {
    CardId::parse(&format!("E0040123456789{n:02X}")).unwrap()
}
fn profile(n: u8) -> StreamProfile {
    let mut profile = StreamProfile::new(card(n));
    profile.name = format!("主播 {n}");
    profile.bilibili.auth_code = format!("profile-secret-{n}");
    profile.bilibili.enabled = false;
    profile
}

#[test]
fn version_one_migrates_existing_room_to_global_without_changing_credentials_or_revision() {
    let f = Fixture::new();
    let mut config = Config::default();
    config.bilibili.auth_code = "legacy-global-secret".into();
    config.requests.queue_capacity = 42;
    let mut global = serde_json::to_value(&config).unwrap();
    let source = global.as_object_mut().unwrap().remove("bilibili").unwrap();
    global.as_object_mut().unwrap().remove("profiles");
    let db = rusqlite::Connection::open(f.db()).unwrap();
    db.execute_batch("CREATE TABLE stream_profiles (id INTEGER PRIMARY KEY, name TEXT NOT NULL, platform TEXT NOT NULL, settings TEXT NOT NULL);
        CREATE TABLE settings (id INTEGER PRIMARY KEY, revision INTEGER NOT NULL, default_profile INTEGER NOT NULL REFERENCES stream_profiles(id), settings TEXT NOT NULL);
        PRAGMA user_version = 1; PRAGMA application_id = 1129468243;").unwrap();
    db.execute(
        "INSERT INTO stream_profiles VALUES (17, '默认直播档案', 'bilibili', ?1)",
        [source.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO settings VALUES (1, 7, 17, ?1)",
        [global.to_string()],
    )
    .unwrap();
    drop(db);
    let store = Store::open(&f.db()).unwrap();
    assert_eq!(store.raw, config);
    assert_eq!(store.revision, 7);
    let db = rusqlite::Connection::open(f.db()).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT profile_key FROM stream_profiles WHERE id = 17",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        GLOBAL
    );
    let mut active = ActiveStream::new(&store.raw);
    assert!(active.update(&store.raw, Some(&card(99))).is_none());
    assert_eq!(active.id, GLOBAL);
}

#[test]
fn profiles_cards_backup_and_deletion_round_trip_with_atomic_conflict_detection() {
    let f = Fixture::new();
    let mut store = Store::open(&f.db()).unwrap();
    let mut config = store.raw.clone();
    let mut first = profile(1);
    first.cards.push(card(2));
    config.profiles = vec![first, profile(3)];
    store
        .commit(store.prepare(config.clone(), 0, &f.dll()).unwrap())
        .unwrap();
    assert_eq!(Store::open(&f.db()).unwrap().raw, config);
    let db = rusqlite::Connection::open(f.db()).unwrap();
    let global: String = db
        .query_row("SELECT settings FROM settings", [], |r| r.get(0))
        .unwrap();
    assert!(!global.contains("profile-secret") && !global.contains("E004"));
    assert_eq!(
        db.query_row("SELECT count(*) FROM stream_cards", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let path = store.export_json(Path::new("backup.json")).unwrap();
    assert_eq!(store.import_json(&path).unwrap(), config);

    let mut stale = Store::open(&f.db()).unwrap();
    let stale_save = stale.prepare(config.clone(), 1, &f.dll()).unwrap();
    let mut next = config.clone();
    next.profiles[0].name = "更名".into();
    next.profiles.remove(1);
    store
        .commit(store.prepare(next.clone(), 1, &f.dll()).unwrap())
        .unwrap();
    assert!(stale.commit(stale_save).is_err());
    assert_eq!(Store::open(&f.db()).unwrap().raw, next);
    assert!(profiles::for_card(&next, Some(&card(3))).is_none());
    assert_eq!(
        db.query_row("SELECT count(*) FROM stream_cards", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );

    db.execute_batch("CREATE TRIGGER fail BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    assert!(
        store
            .commit(store.prepare(config.clone(), 2, &f.dll()).unwrap())
            .is_err()
    );
    assert_eq!(Store::open(&f.db()).unwrap().raw, next);
    assert_eq!(store.raw, next);
}

#[test]
fn duplicate_cards_invalid_rooms_and_stale_login_drafts_are_rejected() {
    let f = Fixture::new();
    let store = Store::open(&f.db()).unwrap();
    let mut config = Config {
        profiles: vec![profile(1), profile(2)],
        ..Default::default()
    };
    config.profiles[1]
        .cards
        .push(CardId::parse("e004012345678901").unwrap());
    let err = match store.prepare(config, 0, &f.dll()) {
        Ok(_) => panic!("duplicate accepted"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("同一卡号"));
    assert!(!err.contains("E004"));
    let mut config = Config::default();
    config.profiles.push(StreamProfile::new(card(1)));
    assert!(store.prepare(config, 0, &f.dll()).is_err());
    assert_eq!(Store::open(&f.db()).unwrap().raw, Config::default());
    assert!(profiles::check_login(Some(&card(1)), Some(&card(1))).is_ok());
    assert!(profiles::check_login(Some(&card(1)), Some(&card(2))).is_err());
    assert!(profiles::check_login(Some(&card(1)), None).is_err());
    assert!(profiles::check_login(None, None).is_ok());
    for invalid in [
        "",
        "0000000000000000",
        "FFFFFFFFFFFFFFFF",
        "12345678",
        "not-a-card-value",
        "E0040123456789ZZ",
    ] {
        assert!(CardId::parse(invalid).is_err());
    }
}

#[test]
fn selection_uses_global_fallback_and_shared_cards_do_not_clear_or_reconnect() {
    let mut config = Config::default();
    let mut first = profile(1);
    first.cards.push(card(2));
    config.profiles = vec![first, profile(3)];
    let mut active = ActiveStream::new(&config);
    let mut view = View::new(config.clone());
    let room = bilimani::platforms::RoomInfo {
        room_id: 123,
        name: "主播".into(),
        title: "今晚点歌".into(),
    };
    view.room = Some(room.clone());
    assert!(view.sync_stream(&config, &mut active, None).is_none());
    view.player_card = Some(card(1));
    assert!(
        view.sync_stream(&config, &mut active, None)
            .unwrap()
            .profile_changed
    );
    assert!(view.room.is_none());
    view.room = Some(room.clone());
    view.player_card = Some(card(2));
    assert!(view.sync_stream(&config, &mut active, None).is_none());
    assert_eq!(view.room, Some(room.clone()));
    config.profiles[1].bilibili.auth_code = "unrelated-room-change".into();
    config.bilibili.auth_code = "unrelated-global-change".into();
    config.profiles[0].name = "rename without reconnect".into();
    assert!(view.sync_stream(&config, &mut active, None).is_none());
    assert_eq!(view.room, Some(room));
    config.profiles[0].bilibili.auth_code = "active-room-change".into();
    assert!(
        !view
            .sync_stream(&config, &mut active, None)
            .unwrap()
            .profile_changed
    );
    assert!(view.room.is_none());
    view.player_card = Some(card(3));
    assert!(
        view.sync_stream(&config, &mut active, None)
            .unwrap()
            .profile_changed
    );
    view.player_card = None;
    assert!(
        view.sync_stream(&config, &mut active, None)
            .unwrap()
            .profile_changed
    );
    assert_eq!(view.active_profile, GLOBAL);
    view.player_card = Some(card(99));
    assert!(view.sync_stream(&config, &mut active, None).is_none());
}

#[test]
fn version_one_backup_import_keeps_global_connection() {
    let f = Fixture::new();
    let store = Store::open(&f.db()).unwrap();
    let backup = f.0.join("old.json");
    std::fs::write(
        &backup,
        r#"{"format":"bilimani","version":1,"config":{"bilibili":{"auth_code":"legacy-backup"}}}"#,
    )
    .unwrap();
    let draft = store.import_json(&backup).unwrap();
    assert!(draft.profiles.is_empty());
    assert_eq!(draft.bilibili.auth_code, "legacy-backup");
    assert!(store.raw.bilibili.auth_code.is_empty());
}
