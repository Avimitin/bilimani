use bilimani_api::{ApiError, Command, Server, search_query};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bilimani-api-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("app")).unwrap();
        std::fs::write(path.join("app/index.html"), "<p>test</p>").unwrap();
        std::fs::write(path.join("app/config.json"), "{\"value\":1}").unwrap();
        Self(path)
    }
}

#[tokio::test]
async fn proxy_is_page_scoped_stateless_and_does_not_forward_browser_credentials() {
    use http_body_util::{BodyExt, Full};
    use hyper::{
        Request, Response,
        body::{Bytes, Incoming},
        server::conn::http1,
        service::service_fn,
    };
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", upstream.local_addr().unwrap());
    let upstream_task = tokio::spawn(async move {
        loop {
            let (socket, _) = upstream.accept().await.unwrap();
            tokio::spawn(async move {
                let service = service_fn(|req: Request<Incoming>| async move {
                    let path = req.uri().to_string();
                    let browser_headers = ["cookie", "origin", "referer"]
                        .iter()
                        .any(|key| req.headers().contains_key(*key));
                    let auth = req
                        .headers()
                        .get("authorization")
                        .map(|v| v.to_str().unwrap().to_owned());
                    let user_agent = req
                        .headers()
                        .get("user-agent")
                        .map(|v| v.to_str().unwrap().to_owned());
                    let body = req.into_body().collect().await.unwrap().to_bytes();
                    let mut response =
                        Response::builder().header("Content-Type", "application/json");
                    if path.ends_with("redirect") {
                        response = response
                            .status(302)
                            .header("Location", "https://example.com/");
                    }
                    Ok::<_, Infallible>(response.body(Full::new(Bytes::from(json!({"path":path,"body":String::from_utf8_lossy(&body),"browser_headers":browser_headers,"authorization":auth,"user_agent":user_agent}).to_string()))).unwrap())
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    let files = Files::new();
    let config = |target: &str| {
        std::fs::write(
            files.0.join("app/config.json"),
            json!({"proxy":{"example":target},"platform":{"arbitrary":"ignored"}}).to_string(),
        )
        .unwrap()
    };
    config(&origin);
    let (server, _commands) = Server::start(([127, 0, 0, 1], 0).into(), &files.0)
        .await
        .unwrap();
    let base = format!("http://{}/app/_proxy/example", server.address);
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/send?q=1"))
        .header("Origin", format!("http://{}", server.address))
        .header("Cookie", "local=private")
        .header("Referer", "https://example.test/?authCode=private")
        .header("User-Agent", "Browser test")
        .header("Authorization", "test-only")
        .json(&json!({"opaque":123}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response: serde_json::Value = response.json().await.unwrap();
    assert_eq!(response["path"], "/send?q=1");
    assert_eq!(response["body"], "{\"opaque\":123}");
    assert_eq!(response["browser_headers"], false);
    assert_eq!(response["authorization"], "test-only");
    assert_eq!(response["user_agent"], "Browser test");
    config(&format!("{origin}/changed"));
    let changed: serde_json::Value = client
        .get(format!("{base}/read"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        changed["path"], "/changed/read",
        "no server-side configuration cache"
    );
    assert_eq!(
        client
            .get(format!("{base}/redirect"))
            .send()
            .await
            .unwrap()
            .status(),
        502
    );
    assert_eq!(
        client
            .get(format!("{base}/%2e%2e%2fescape"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .get(format!("{base}/read"))
            .header("Origin", "https://example.com")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    config("http://remote.invalid/");
    assert_eq!(
        client
            .get(format!("{base}/read"))
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    server.stop().await;
    upstream_task.abort();
}
impl Drop for Files {
    fn drop(&mut self) {
        assert!(self.0.starts_with(std::env::temp_dir()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("bilimani-api-test-")
        );
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn search_input_is_bounded_decoded_and_unambiguous() {
    let Command::Search { query, limit } = search_query("q=%E5%86%A5+SPA&limit=10").unwrap() else {
        panic!()
    };
    assert_eq!((query.as_str(), limit), ("冥 SPA", 10));
    for query in [
        "",
        "q=",
        "q=a&q=b",
        "q=a&limit=0",
        "q=a&limit=101",
        "q=a&unknown=1",
        "q=%FF",
    ] {
        assert!(search_query(query).is_err(), "{query}");
    }
}
#[tokio::test]
async fn static_configuration_and_json_commands_obey_http_boundaries() {
    let files = Files::new();
    let (server, mut commands) = Server::start(([127, 0, 0, 1], 0).into(), &files.0)
        .await
        .unwrap();
    server
        .publish([(
            "/api/v1/meta".into(),
            json!({"game":"test","api_version":1}),
        )])
        .unwrap();
    let worker = tokio::spawn(async move {
        while let Some(message) = commands.recv().await {
            let result = match message.command {
                Command::Search { query, .. } => Ok(json!({"songs":[{"id":1,"title":query}]})),
                Command::Select(input) => {
                    Ok(json!({"id":7,"status":"waiting","song_id":input.song_id}))
                }
                Command::Selection(_) => Err(ApiError::NotFound),
                Command::Enqueue(input) => {
                    Ok(json!({"id":8,"song_id":input.song_id,"requester":input.requester}))
                }
                Command::Remove(id) | Command::Priority(id) => Ok(json!({"id":id})),
                Command::ReloadAliases => Ok(json!({"count":3})),
            };
            let _ = message.reply.send(result);
        }
    });
    let client = reqwest::Client::new();
    let base = format!("http://{}", server.address);
    let no_redirect = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for method in [reqwest::Method::GET, reqwest::Method::HEAD] {
        let redirect = no_redirect
            .request(
                method,
                format!("{base}/app?authCode=fixture%2Bcode&layout=wide"),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(redirect.status(), 308);
        assert_eq!(
            redirect.headers()["location"],
            "/app/?authCode=fixture%2Bcode&layout=wide"
        );
        assert_eq!(redirect.headers()["referrer-policy"], "no-referrer");
        assert!(redirect.bytes().await.unwrap().is_empty());
    }
    let followed = client
        .get(format!("{base}/app?authCode=fixture"))
        .send()
        .await
        .unwrap();
    assert_eq!(followed.status(), 200);
    assert_eq!(followed.url().path(), "/app/");
    assert_eq!(
        followed.url().join("./config.json").unwrap().path(),
        "/app/config.json"
    );
    assert_eq!(followed.text().await.unwrap(), "<p>test</p>");
    for path in ["/missing", "/%2fapp", "/app%5c..", "/%2e%2e/private"] {
        let missing = no_redirect
            .get(format!("{base}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), 404);
        assert!(!missing.headers().contains_key("location"));
    }
    let page = client.get(format!("{base}/app/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert_eq!(page.headers()["referrer-policy"], "no-referrer");
    assert_eq!(page.text().await.unwrap(), "<p>test</p>");
    let head = client
        .head(format!("{base}/app/index.html"))
        .send()
        .await
        .unwrap();
    assert_eq!(head.headers()["content-length"], "11");
    assert!(head.bytes().await.unwrap().is_empty());
    std::fs::write(files.0.join("app/config.json"), "{\"value\":2}").unwrap();
    let config: serde_json::Value = client
        .get(format!("{base}/app/config.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(config["value"], 2);
    assert_eq!(
        client
            .get(format!("{base}/api/v1/meta"))
            .header("Origin", "https://example.com")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{base}/api/v1/meta"))
            .header("Host", "other.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let search: serde_json::Value = client
        .get(format!("{base}/api/v1/songs?q=AA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search["songs"][0]["title"], "AA");
    let selected = client
        .post(format!("{base}/api/v1/selections"))
        .json(&json!({"song_id":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(selected.status(), 202);
    let queued = client
        .post(format!("{base}/api/v1/queue"))
        .json(&json!({"song_id":1,"requester":"Alice"}))
        .send()
        .await
        .unwrap();
    assert_eq!(queued.status(), 201);
    assert_eq!(
        queued.json::<serde_json::Value>().await.unwrap()["requester"],
        "Alice"
    );
    for response in [
        client
            .delete(format!("{base}/api/v1/queue/8"))
            .send()
            .await
            .unwrap(),
        client
            .post(format!("{base}/api/v1/queue/8/priority"))
            .send()
            .await
            .unwrap(),
        client
            .post(format!("{base}/api/v1/aliases/reload"))
            .send()
            .await
            .unwrap(),
    ] {
        assert_eq!(response.status(), 200);
    }
    assert_eq!(
        client
            .post(format!("{base}/api/v1/selections"))
            .json(&json!({"song_id":1,"user":"unexpected"}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .post(format!("{base}/api/v1/selections"))
            .header("Content-Type", "application/json")
            .body("x".repeat(9000))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let missing: serde_json::Value = client
        .get(format!("{base}/api/v1/selections/99"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(missing["error"]["code"], "not_found");
    for path in [
        "/api/state",
        "/%2e%2e/secret",
        "/app/%2econfig",
        "/app/C%3Asecret",
        "/app/index.html%20",
    ] {
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            404,
            "{path}"
        );
    }
    server.stop().await;
    worker.abort();
}
