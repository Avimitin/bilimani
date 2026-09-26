use chart_requester::platforms::Connection;
use chart_requester::{
    catalog::Catalog,
    config::Config,
    engine::{Chat, Engine, Phase, Snapshot},
    game::Song,
    games::iidx,
    overlay::{Server, snapshot},
};
use std::collections::BTreeMap;
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
async fn serves_embedded_assets_live_snapshots_and_releases_port_on_shutdown() {
    let mut server = Server::start(0, &snapshot(None, &connection("等待弹幕连接"), 0))
        .await
        .unwrap();
    assert!(server.address.ip().is_loopback());
    let root = format!("http://{}", server.address);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, mime) in [
        ("/queue", "text/html"),
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
    assert_eq!(
        client
            .get(format!("{root}/chart-requester.toml"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
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
        Server::start(server.address.port(), &updated)
            .await
            .is_err()
    );
    server.stop().await;
    let rebound = tokio::net::TcpListener::bind(server.address).await.unwrap();
    drop(rebound);
}

#[test]
fn old_configs_enable_overlay_and_invalid_ports_are_rejected() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.overlay.enabled);
    assert_eq!(config.overlay.port, 32133);
    let mut invalid = config;
    invalid.overlay.port = 0;
    assert!(invalid.validate().is_err());
}
