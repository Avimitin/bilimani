//! Read-only loopback overlay. Network requests only access owned snapshots;
//! they never call the game or hold the engine's state across an await.
use crate::engine::{Engine, Request as SongRequest};
use anyhow::Result;
use http_body_util::Full;
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

fn request(r: &SongRequest) -> Value {
    json!({"token": r.token, "title": r.song.title, "requester": r.name,
        "mode": r.mode, "chart": r.chart.map(|c| c.label())})
}

/// Deliberately excludes configuration, cookies, identity codes and sender IDs.
pub fn snapshot(engine: Option<&Engine>, status: &str, now: u64) -> Value {
    let mut state = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "connected": status.starts_with("弹幕已连接"), "status": status,
        "ready": engine.is_some(), "current": null, "queue": [],
        "capacity": 0, "pending": [], "notices": []
    });
    if let Some(e) = engine {
        state["capacity"] = json!(e.config.requests.queue_capacity);
        if let Some(c) = &e.current {
            let mut current = request(&c.request);
            current["remaining"] = json!(c.until.saturating_sub(now));
            current["duration"] = json!(e.config.requests.current_timeout_seconds);
            state["current"] = current;
        }
        state["queue"] = e.queue.iter().map(request).collect();
        state["pending"] = e.pending.values().map(|p| json!({
            "requester": p.name, "mode": p.mode, "chart": p.chart.map(|c| c.label()),
            "remaining": p.until.saturating_sub(now), "duration": e.config.requests.selection_timeout_seconds,
            "candidates": p.songs.iter().map(|s| json!({"title": s.title,
                "available": s.supports(p.mode, p.chart)})).collect::<Vec<_>>()
        })).collect();
        state["notices"] = e
            .messages
            .iter()
            .map(|(at, text)| json!({"at": at, "text": text}))
            .collect();
    }
    state
}

pub struct Server {
    pub address: SocketAddr,
    state: Arc<RwLock<Bytes>>,
    task: JoinHandle<Result<()>>,
}
impl Server {
    pub async fn start(port: u16, initial: &Value) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(RwLock::new(Bytes::from(serde_json::to_vec(initial)?)));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if connections.len() < 32 => {
                        let (stream, _) = accepted?;
                        let state = shared.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |req| {
                                let response = route(req, &state, address.port());
                                async { Ok::<_, Infallible>(response) }
                            });
                            // One bounded request per connection; this also limits slow clients.
                            let mut builder = http1::Builder::new();
                            builder.keep_alive(false).max_buf_size(8192).max_headers(32);
                            let _ = tokio::time::timeout(Duration::from_secs(5),
                                builder.serve_connection(TokioIo::new(stream), service)).await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Ok(Self {
            address,
            state,
            task,
        })
    }
    pub fn publish(&self, value: &Value) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            *self.state.write().unwrap() = bytes.into();
        }
    }
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
    pub async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn route(req: Request<Incoming>, state: &RwLock<Bytes>, port: u16) -> Response<Full<Bytes>> {
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    // Browsers omit the default HTTP port from Host and Origin.
    if port == 80 {
        hosts.extend(["127.0.0.1".into(), "localhost".into()]);
    }
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let allowed_origin = req.headers().get("origin").is_none_or(|origin| {
        origin
            .to_str()
            .is_ok_and(|o| hosts.iter().any(|h| o == format!("http://{h}")))
    });
    let (status, mime, body): (_, _, Bytes) = if !hosts.iter().any(|h| h == host) || !allowed_origin
    {
        (
            StatusCode::FORBIDDEN,
            "text/plain",
            "Local OBS access only".into(),
        )
    } else if req.method() != Method::GET && req.method() != Method::HEAD {
        (
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain",
            "Read-only overlay".into(),
        )
    } else {
        match req.uri().path() {
            "/" | "/queue" | "/interaction" => (
                StatusCode::OK,
                "text/html; charset=utf-8",
                Bytes::from_static(include_bytes!("../web/index.html")),
            ),
            "/overlay.css" => (
                StatusCode::OK,
                "text/css; charset=utf-8",
                Bytes::from_static(include_bytes!("../web/overlay.css")),
            ),
            "/overlay.js" => (
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                Bytes::from_static(include_bytes!("../web/overlay.js")),
            ),
            "/api/state" => (
                StatusCode::OK,
                "application/json; charset=utf-8",
                state.read().unwrap().clone(),
            ),
            _ => (StatusCode::NOT_FOUND, "text/plain", "Not found".into()),
        }
    };
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Length", body.len())
        .body(Full::new(if req.method() == Method::HEAD {
            Bytes::new()
        } else {
            body
        }))
        .unwrap()
}
