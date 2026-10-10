use bilimani::platforms::{Connection, RoomInfo};
use bilimani::{
    catalog::Catalog,
    config::Config,
    engine::{Chat, Engine, Phase, Snapshot},
    game::Song,
    games::iidx,
    overlay::{History, Server, StaticFiles, snapshot},
};
use std::{collections::BTreeMap, path::PathBuf};
struct StaticFixture {
    root: PathBuf,
    public: PathBuf,
}
impl StaticFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("bilimani-static-{}", uuid::Uuid::new_v4()));
        let public = root.join("bilimani_web");
        std::fs::create_dir_all(&public).unwrap();
        for name in ["index.html", "overlay.css", "overlay.js"] {
            std::fs::copy(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("web/card")
                    .join(name),
                public.join(name),
            )
            .unwrap();
        }
        Self { root, public }
    }
}
impl Drop for StaticFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn connection(text: &str) -> Connection {
    Connection {
        connected: text == "弹幕已连接",
        text: text.into(),
    }
}

#[test]
fn mixed_history_keeps_order_duplicates_and_survives_notice_expiry() {
    let mut history = History::default();
    let mut engine = Engine::new(
        Config::default(),
        Catalog::new(
            vec![Song {
                id: 1,
                title: "AA".into(),
                search_terms: vec![],
                charts: iidx::available_charts([4; 10]),
            }],
            &BTreeMap::new(),
        )
        .unwrap(),
        &iidx::RULES,
    );
    let chat = Chat {
        user: "private-sender-id".into(),
        name: "观众\n甲".into(),
        text: "晚上好！".into(),
    };
    history.chat(&chat, 1);
    assert_eq!(engine.chat(chat, 1), "ignored_not_a_request");
    // Identical notices in the same second are separate arrivals, not duplicates.
    engine.notice(1, "已加入队列");
    engine.notice(1, "已加入队列");
    for (at, text) in engine.take_notices() {
        history.notice(at, &text);
    }
    assert_eq!(engine.take_notices().count(), 0);
    engine.expire(1000);
    let state = snapshot(
        Some(&engine),
        &connection("弹幕已连接"),
        None,
        1000,
        &history,
    );
    assert!(state["notices"].as_array().unwrap().is_empty());
    assert_eq!(state["feed_limit"], 10);
    let feed = state["feed"].as_array().unwrap();
    assert_eq!(feed.len(), 3);
    assert_eq!(feed[0]["kind"], "chat");
    assert_eq!(feed[0]["name"], "观众甲");
    assert_eq!(feed[0]["text"], "晚上好！");
    assert_eq!(feed[1]["kind"], "event");
    assert_ne!(feed[1]["id"], feed[2]["id"]);
    assert!(!state.to_string().contains("private-sender-id"));
    // Ordinary chat is also available before the game catalog exists.
    assert_eq!(
        snapshot(None, &connection("等待曲库"), None, 2000, &history)["feed"],
        state["feed"]
    );
}

#[test]
fn history_drops_oldest_on_overflow_resize_and_profile_clear() {
    let mut history = History::default();
    for i in 1..=11 {
        history.notice(0, &format!("事件 {i}"));
    }
    let read = |history: &History| {
        snapshot(None, &connection("弹幕已连接"), None, 0, history)["feed"].clone()
    };
    let feed = read(&history);
    assert_eq!(feed.as_array().unwrap().len(), 10);
    assert_eq!(feed[0]["text"], "事件 2");
    assert_eq!(feed[9]["text"], "事件 11");
    history.set_limit(3);
    assert_eq!(read(&history)[0]["text"], "事件 9");
    history.set_limit(10);
    assert_eq!(read(&history).as_array().unwrap().len(), 3); // Evicted entries do not reappear.
    history.clear();
    assert!(read(&history).as_array().unwrap().is_empty());
    history.notice(0, "另一个直播间");
    assert_eq!(read(&history)[0]["id"], 12);
}

#[test]
fn public_snapshot_preserves_choices_and_timers_without_credentials_or_sender_ids() {
    let mut config = Config::default();
    config.bilibili.auth_code = "private-identity-code".into();
    config.bilibili.sessdata = "private-cookie".into();
    config.requests.candidates = 7;
    let songs = [
        "AA",
        "AA -rebuild-",
        "AA 3",
        "AA 4",
        "AA 5",
        "AA 6",
        "AA 7",
        "冥",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, title)| Song {
        id: i as u32 + 1,
        title: title.into(),
        search_terms: vec![],
        charts: iidx::available_charts([4; 10]),
    })
    .collect();
    let mut engine = Engine::new(
        config,
        Catalog::new(songs, &BTreeMap::new()).unwrap(),
        &iidx::RULES,
    );
    engine.observe(
        Snapshot {
            phase: Phase::Select,
            mode: Some(iidx::SP),
            epoch: 1,
            can_skip: true,
        },
        0,
    );
    engine.chat(
        Chat {
            user: "private-user-one".into(),
            name: "观众甲".into(),
            text: "点歌 AA".into(),
        },
        0,
    );
    engine.chat(
        Chat {
            user: "private-user-two".into(),
            name: "观众乙".into(),
            text: "点歌 冥 SPA".into(),
        },
        10,
    );
    let jump = engine.next_jump(10).unwrap();
    engine.jump_result(jump.request.token, Some(Ok(())), 10);
    engine.chat(
        Chat {
            user: "private-user-one".into(),
            name: "观众甲".into(),
            text: "n".into(),
        },
        19,
    );
    let state = snapshot(
        Some(&engine),
        &connection("弹幕已连接"),
        None,
        20,
        &History::default(),
    );
    assert_eq!(state["current"]["title"], "冥");
    assert_eq!(state["current"]["chart"], "SPA");
    assert_eq!(state["current"]["chart_style"], "red");
    assert_eq!(state["pending"][0]["chart_style"], "neutral");
    assert_eq!(state["current"]["remaining"], 590);
    assert_eq!(state["pending"][0]["remaining"], 40);
    assert_eq!(state["pending"][0]["page"], 1);
    assert_eq!(state["pending"][0]["page_size"], 5);
    assert_eq!(state["pending"][0]["page_count"], 2);
    assert_eq!(
        state["pending"][0]["candidates"].as_array().unwrap().len(),
        7
    );
    assert_eq!(state["pending"][0]["candidates"][0]["title"], "AA");
    assert!(state["queue"].as_array().unwrap().is_empty());
    let wire = state.to_string();
    for secret in [
        "private-identity-code",
        "private-cookie",
        "private-user-one",
        "private-user-two",
    ] {
        assert!(!wire.contains(secret));
    }
    assert!(wire.contains("观众甲"));
}

#[test]
fn public_room_tracks_the_connected_profile_even_before_catalog_is_ready() {
    let history = History::default();
    let connected = connection("弹幕已连接");
    let first = RoomInfo {
        room_id: 123,
        name: "主播甲".into(),
        title: "IIDX 点歌".into(),
    };
    let room = snapshot(None, &connected, Some(&first), 0, &history)["room"].clone();
    assert_eq!(
        room,
        serde_json::json!({"room_id": 123, "name": "主播甲", "title": "IIDX 点歌"})
    );
    // Disconnecting or switching profiles must not advertise a stale anchor.
    assert!(snapshot(None, &Connection::waiting(), Some(&first), 1, &history)["room"].is_null());
    assert!(snapshot(None, &connected, None, 2, &history)["room"].is_null());
    let second = RoomInfo {
        room_id: 456,
        name: "主播乙".into(),
        title: "新直播间".into(),
    };
    let next = snapshot(None, &connected, Some(&second), 3, &history);
    assert_eq!(next["room"]["name"], "主播乙");
    assert_eq!(next["room"]["room_id"], 456);
}

#[tokio::test]
async fn serves_static_assets_live_snapshots_and_releases_port_on_shutdown() {
    let files = StaticFixture::new();
    let mut server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        &files.public,
        &snapshot(
            None,
            &connection("等待弹幕连接"),
            None,
            0,
            &History::default(),
        ),
    )
    .await
    .unwrap();
    assert!(server.address.ip().is_loopback());
    let root = format!("http://{}", server.address);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, mime) in [
        ("/queue", "text/html"),
        ("/index.html", "text/html"),
        ("/overlay.css", "text/css"),
        ("/overlay.js", "text/javascript"),
        ("/api/state", "application/json"),
        ("/api/now-playing", "application/json"),
        ("/api/lane-counts", "application/json"),
        ("/api/lane-order", "application/json"),
    ] {
        let response = client.get(format!("{root}{path}")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with(mime)
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(!response.text().await.unwrap().is_empty());
    }
    let redirect = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(format!("{root}/"))
        .send()
        .await
        .unwrap();
    assert_eq!(redirect.status(), 307);
    assert_eq!(redirect.headers()["location"], "/queue");
    assert_eq!(
        client
            .get(format!("{root}/interaction"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let mut updated = snapshot(
        None,
        &connection("新连接状态"),
        None,
        1,
        &History::default(),
    );
    // Game metadata remains available before the request-engine catalog exists.
    updated["now_playing"] = serde_json::json!({
        "phase": "playing", "song": {"id": 11040, "title": "AA", "artist": "D.J.Amuro",
            "charts": [
                {"chart": {"mode": "SP", "id": "SPA"}, "lane_counts": {"sides": [
                    {"side": 1, "keys": [100,200,300,400,500,600,700], "scratch": 80}]}},
                {"chart": {"mode": "SP", "id": "SPH"}, "lane_counts": null}]},
        "players": [{"side": 2, "chart": {"mode": "SP", "id": "SPA"}}],
        "lane_order": [{"side": 2, "random": "random", "mirror": false,
                        "status": "ready", "keys": [3, 4, 5, 2, 1, 6, 7]}]
    });
    server.publish(&updated);
    let playing = client
        .get(format!("{root}/api/now-playing"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        playing.json::<serde_json::Value>().await.unwrap(),
        updated["now_playing"]
    );
    let head = client
        .head(format!("{root}/api/now-playing"))
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), 200);
    assert!(head.text().await.unwrap().is_empty());
    let counts = client
        .get(format!("{root}/api/lane-counts"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        counts,
        serde_json::json!({
            "phase": "playing", "song_id": 11040, "basis": "original_chart", "counting": "native_weighted",
            "charts": updated["now_playing"]["song"]["charts"]
        })
    );
    let order = client
        .get(format!("{root}/api/lane-order"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        order,
        serde_json::json!({
            "phase": "playing", "song_id": 11040,
            "players": updated["now_playing"]["players"], "lane_order": updated["now_playing"]["lane_order"]
        })
    );
    for path in ["lane-counts", "lane-order"] {
        let head = client
            .head(format!("{root}/api/{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(head.status(), 200);
        assert!(head.text().await.unwrap().is_empty());
        assert_eq!(
            client
                .post(format!("{root}/api/{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            405
        );
        assert_eq!(
            client
                .get(format!("{root}/api/{path}"))
                .header("Origin", "https://example.com")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        client
            .get(format!("{root}/api/state"))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        updated
    );
    let idle = snapshot(
        None,
        &connection("新连接状态"),
        None,
        2,
        &History::default(),
    );
    server.publish(&idle);
    for (path, field) in [("lane-counts", "charts"), ("lane-order", "lane_order")] {
        let response = client
            .get(format!("{root}/api/{path}"))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        assert_eq!(response["phase"], "idle");
        assert!(response["song_id"].is_null());
        assert_eq!(response[field], serde_json::json!([]));
    }
    assert_eq!(
        client
            .get(format!("{root}/api/now-playing"))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        serde_json::json!({"phase": "idle", "song": null, "players": [], "lane_order": [], "playback": null})
    );
    for private in [
        "bilimani.toml",
        "bilimani.db",
        "bilimani.db-journal",
        "bilimani-backup.json",
    ] {
        assert_eq!(
            client
                .get(format!("{root}/{private}"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        client
            .post(format!("{root}/api/state"))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    assert_eq!(
        client
            .get(format!("{root}/api/state"))
            .header("Host", "external.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{root}/api/state"))
            .header("Origin", "https://external.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let head = client.head(format!("{root}/queue")).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert!(head.text().await.unwrap().is_empty());
    assert!(
        Server::start(server.address, &files.public, &updated)
            .await
            .is_err()
    );
    server.stop().await;
    let rebound = tokio::net::TcpListener::bind(server.address).await.unwrap();
    drop(rebound);
}

#[tokio::test]
async fn bundled_styles_switch_with_the_same_queue_url_and_live_state() {
    let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    let state = serde_json::json!({"status": "style-switch-test"});
    let mut server = Server::start("127.0.0.1:0".parse().unwrap(), &web.join("card"), &state)
        .await
        .unwrap();
    let root = format!("http://{}", server.address);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for style in ["card", "mecha", "card"] {
        server.set_static_files(StaticFiles::open(&web.join(style)).await.unwrap());
        let page = client
            .get(format!("{root}/queue"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(page.contains("data-layout=\"frame\""), style == "mecha");
        for asset in ["overlay.css", "overlay.js", "frame.css"] {
            let response = client.get(format!("{root}/{asset}")).send().await.unwrap();
            if asset == "frame.css" && style == "card" {
                assert_eq!(response.status(), 404);
            } else {
                assert_eq!(response.status(), 200);
                assert_eq!(
                    response.text().await.unwrap(),
                    std::fs::read_to_string(web.join(style).join(asset)).unwrap()
                );
            }
        }
        assert_eq!(
            client
                .get(format!("{root}/api/state"))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap(),
            state
        );
    }
    server.stop().await;
}

#[tokio::test]
async fn editing_files_and_switching_directories_updates_pages_without_rebinding() {
    let files = StaticFixture::new();
    let alternate = files.root.join("custom-theme");
    std::fs::create_dir_all(&alternate).unwrap();
    std::fs::write(alternate.join("index.html"), "alternate page").unwrap();
    let mut server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        &files.public,
        &serde_json::json!({"version": "test"}),
    )
    .await
    .unwrap();
    let root = format!("http://{}", server.address);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    std::fs::write(files.public.join("index.html"), "replacement 页面").unwrap();
    assert_eq!(
        client
            .get(format!("{root}/queue"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "replacement 页面"
    );
    let head = client
        .head(format!("{root}/index.html"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        head.headers()["content-length"],
        "replacement 页面".len().to_string()
    );
    assert!(head.bytes().await.unwrap().is_empty());
    std::fs::write(files.public.join("overlay.css"), "body { color: red; }").unwrap();
    assert_eq!(
        client
            .get(format!("{root}/overlay.css?v=2"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "body { color: red; }"
    );
    std::fs::remove_file(files.public.join("index.html")).unwrap();
    assert_eq!(
        client
            .get(format!("{root}/queue"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert!(StaticFiles::open(&files.public).await.is_err());
    server.set_static_files(StaticFiles::open(&alternate).await.unwrap());
    assert_eq!(
        client
            .get(format!("{root}/queue"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "alternate page"
    );
    // A failed directory preparation leaves the currently served page intact.
    assert!(
        StaticFiles::open(&files.root.join("missing"))
            .await
            .is_err()
    );
    assert_eq!(
        client
            .get(format!("{root}/queue"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "alternate page"
    );
    assert_eq!(
        client
            .get(format!("{root}/api/state"))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        serde_json::json!({"version": "test"})
    );
    server.stop().await;
}

#[tokio::test]
async fn serves_nested_assets_but_blocks_traversal_hidden_files_and_escaping_links() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let files = StaticFixture::new();
    std::fs::create_dir(files.public.join("assets")).unwrap();
    std::fs::write(files.root.join("secret.txt"), "private-outside-root").unwrap();
    std::fs::write(files.public.join(".hidden"), "private-hidden-file").unwrap();
    std::fs::write(files.public.join("assets/自定义 样式.css"), "custom css").unwrap();
    std::fs::write(files.public.join("assets/icon.svg"), "<svg></svg>").unwrap();
    let large = std::fs::File::create(files.public.join("assets/large.bin")).unwrap();
    large.set_len(32 * 1024 * 1024 + 1).unwrap();
    let mut server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        &files.public,
        &serde_json::json!({}),
    )
    .await
    .unwrap();
    let root = format!("http://{}", server.address);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let name = percent_encoding::utf8_percent_encode(
        "自定义 样式.css",
        percent_encoding::NON_ALPHANUMERIC,
    );
    let css = client
        .get(format!("{root}/assets/{name}"))
        .send()
        .await
        .unwrap();
    assert_eq!(css.headers()["content-type"], "text/css; charset=utf-8");
    assert_eq!(css.text().await.unwrap(), "custom css");
    let svg = client
        .get(format!("{root}/assets/icon.svg"))
        .send()
        .await
        .unwrap();
    assert_eq!(svg.headers()["content-type"], "image/svg+xml");
    assert_eq!(svg.text().await.unwrap(), "<svg></svg>");
    assert_eq!(
        client
            .get(format!("{root}/assets/large.bin"))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/%2e%2e%5csecret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/.hidden",
        "/%2ehidden",
        "/C:/secret.txt",
        "/assets/icon.svg:stream",
        "/assets",
        "/%00index.html",
    ] {
        // A raw request avoids the HTTP client's URL normalization of dot segments.
        let mut stream = tokio::net::TcpStream::connect(server.address)
            .await
            .unwrap();
        stream
            .write_all(
                format!(
                    "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                    server.address
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 404"), "{path}: {response}");
        assert!(!response.contains("private-"));
    }
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_dir(&files.root, files.public.join("escape"));
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&files.root, files.public.join("escape"));
    if link.is_ok() {
        assert_eq!(
            client
                .get(format!("{root}/escape/secret.txt"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        // Remove the link itself before the fixture's recursive cleanup.
        #[cfg(windows)]
        std::fs::remove_dir(files.public.join("escape")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(files.public.join("escape")).unwrap();
    }
    server.stop().await;
}

#[test]
fn old_configs_enable_overlay_and_invalid_ports_are_rejected() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.overlay.enabled);
    assert_eq!(config.overlay.port, 32133);
    assert_eq!(config.overlay.bind_address, "127.0.0.1");
    assert_eq!(config.overlay.history_limit, 10);
    assert_eq!(
        config.overlay.static_dir,
        PathBuf::from("bilimani_web/card")
    );
    let old: Config = toml::from_str("[overlay]\nenabled = true\nport = 32133").unwrap();
    assert_eq!(old.overlay.static_dir, config.overlay.static_dir);
    assert_eq!(old.overlay.history_limit, 10);
    assert_eq!(old.overlay.bind_address, "127.0.0.1");
    let mut invalid = config;
    for limit in [0, 101] {
        invalid.overlay.history_limit = limit;
        assert!(invalid.validate().is_err());
    }
    invalid.overlay.history_limit = 10;
    invalid.overlay.port = 0;
    assert!(invalid.validate().is_err());
    invalid.overlay.port = 32133;
    invalid.overlay.static_dir = PathBuf::new();
    assert!(invalid.validate().is_err());
}

#[test]
fn bind_addresses_validate_and_generate_usable_ipv4_and_ipv6_urls() {
    let mut config = Config::default();
    for (ip, url) in [
        ("127.0.0.1", "http://127.0.0.1:32133/queue"),
        ("0.0.0.0", "http://127.0.0.1:32133/queue"),
        ("192.168.1.10", "http://192.168.1.10:32133/queue"),
        ("::1", "http://[::1]:32133/queue"),
        ("::", "http://[::1]:32133/queue"),
    ] {
        config.overlay.bind_address = ip.into();
        config.validate().unwrap();
        assert_eq!(config.overlay.local_url().unwrap(), url);
    }
    for ip in [
        "",
        "localhost",
        "http://127.0.0.1",
        "127.0.0.1:32133",
        "256.0.0.1",
        "224.0.0.1",
        "ff02::1",
    ] {
        config.overlay.bind_address = ip.into();
        assert!(
            config.validate().is_err(),
            "accepted invalid bind address {ip}"
        );
    }
}

#[tokio::test]
async fn configured_ipv4_and_ipv6_listeners_accept_their_actual_host_and_origin() {
    let files = StaticFixture::new();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for bind in ["127.0.0.2:0", "0.0.0.0:0", "[::1]:0", "[::]:0"] {
        let mut server =
            Server::start(bind.parse().unwrap(), &files.public, &serde_json::json!({}))
                .await
                .unwrap();
        assert_eq!(
            server.address.ip(),
            bind.parse::<std::net::SocketAddr>().unwrap().ip()
        );
        let mut destination = server.address;
        if destination.ip().is_unspecified() {
            destination.set_ip(
                if destination.is_ipv4() {
                    "127.0.0.2"
                } else {
                    "::1"
                }
                .parse()
                .unwrap(),
            );
        }
        let origin = format!("http://{destination}");
        let url = format!("{origin}/api/state");
        assert_eq!(
            client
                .get(&url)
                .header("Origin", &origin)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .get(&url)
                .header("Host", "external.example")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(&url)
                .header("Host", format!("192.0.2.1:{}", destination.port()))
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(&url)
                .header("Origin", "https://external.example")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        server.stop().await;
    }
}

#[tokio::test]
async fn live_bind_changes_keep_state_and_restore_the_listener_on_failure() {
    let files = StaticFixture::new();
    let state = serde_json::json!({"status": "preserved"});
    let mut server = Server::start("127.0.0.1:0".parse().unwrap(), &files.public, &state)
        .await
        .unwrap();
    let original = server.address;
    let wildcard = std::net::SocketAddr::new("0.0.0.0".parse().unwrap(), original.port());
    server.rebind(wildcard).await.unwrap();
    assert_eq!(server.address, wildcard);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{original}/api/state");
    assert_eq!(
        client
            .get(&url)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        state
    );
    server.rebind(original).await.unwrap();

    // A second interface owns the same port: moving there must fail and restore loopback.
    let blocker =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::new(127, 0, 0, 2), original.port()))
            .await
            .unwrap();
    assert!(server.rebind(blocker.local_addr().unwrap()).await.is_err());
    assert_eq!(server.address, original);
    assert_eq!(
        client
            .get(&url)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        state
    );
    drop(blocker);

    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(server.rebind(occupied.local_addr().unwrap()).await.is_err());
    assert_eq!(client.get(&url).send().await.unwrap().status(), 200);
    server.rebind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    assert_ne!(server.address.port(), original.port());
    assert_eq!(
        client
            .get(format!("http://{}/api/state", server.address))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        state
    );
    server.stop().await;
}
