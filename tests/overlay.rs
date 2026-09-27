use chart_requester::platforms::Connection;
use chart_requester::{
    catalog::Catalog,
    config::Config,
    engine::{Chat, Engine, Phase, Snapshot},
    game::Song,
    games::iidx,
    overlay::{Server, StaticFiles, snapshot},
};
use std::{collections::BTreeMap, path::PathBuf};
struct StaticFixture {
    root: PathBuf,
    public: PathBuf,
}
impl StaticFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("requester-static-{}", uuid::Uuid::new_v4()));
        let public = root.join("chart_request_static");
        std::fs::create_dir_all(&public).unwrap();
        for name in ["index.html", "overlay.css", "overlay.js"] {
            std::fs::copy(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("web")
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
fn public_snapshot_preserves_choices_and_timers_without_credentials_or_sender_ids() {
    let mut config = Config::default();
    config.bilibili.auth_code = "private-identity-code".into();
    config.bilibili.sessdata = "private-cookie".into();
    let songs = ["AA", "AA -rebuild-", "冥"]
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
    let state = snapshot(Some(&engine), &connection("弹幕已连接"), 20);
    assert_eq!(state["current"]["title"], "冥");
    assert_eq!(state["current"]["chart"], "SPA");
    assert_eq!(state["current"]["chart_style"], "red");
    assert_eq!(state["pending"][0]["chart_style"], "neutral");
    assert_eq!(state["current"]["remaining"], 590);
    assert_eq!(state["pending"][0]["remaining"], 40);
    assert_eq!(
        state["pending"][0]["candidates"].as_array().unwrap().len(),
        2
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

#[tokio::test]
async fn serves_static_assets_live_snapshots_and_releases_port_on_shutdown() {
    let files = StaticFixture::new();
    let mut server = Server::start(
        0,
        &files.public,
        &snapshot(None, &connection("等待弹幕连接"), 0),
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
    let updated = snapshot(None, &connection("新连接状态"), 1);
    server.publish(&updated);
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
    for private in [
        "chart-requester.toml",
        "chart-requester.db",
        "chart-requester.db-journal",
        "chart-requester-backup.json",
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
        Server::start(server.address.port(), &files.public, &updated)
            .await
            .is_err()
    );
    server.stop().await;
    let rebound = tokio::net::TcpListener::bind(server.address).await.unwrap();
    drop(rebound);
}

#[tokio::test]
async fn editing_files_and_switching_directories_updates_pages_without_rebinding() {
    let files = StaticFixture::new();
    let alternate = files.root.join("custom-theme");
    std::fs::create_dir_all(&alternate).unwrap();
    std::fs::write(alternate.join("index.html"), "alternate page").unwrap();
    let mut server = Server::start(0, &files.public, &serde_json::json!({"version": "test"}))
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
    let mut server = Server::start(0, &files.public, &serde_json::json!({}))
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
    assert_eq!(
        config.overlay.static_dir,
        PathBuf::from("chart_request_static")
    );
    let old: Config = toml::from_str("[overlay]\nenabled = true\nport = 32133").unwrap();
    assert_eq!(old.overlay.static_dir, config.overlay.static_dir);
    let mut invalid = config;
    invalid.overlay.port = 0;
    assert!(invalid.validate().is_err());
    invalid.overlay.port = 32133;
    invalid.overlay.static_dir = PathBuf::new();
    assert!(invalid.validate().is_err());
}
