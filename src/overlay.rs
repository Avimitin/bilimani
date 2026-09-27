//! Read-only loopback overlay. Network requests only access owned snapshots;
//! they never call the game or hold the engine's state across an await.
use crate::{
    engine::{Engine, Request as SongRequest},
    platforms::Connection,
};
use anyhow::{Context, Result, ensure};
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
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

fn request(e: &Engine, r: &SongRequest) -> Value {
    json!({"token": r.token, "title": r.song.title, "requester": r.name,
        "mode": r.mode, "chart": r.chart.map(|c| c.label()), "chart_style": e.chart_style(r.chart)})
}

/// Deliberately excludes configuration, cookies, identity codes and sender IDs.
pub fn snapshot(engine: Option<&Engine>, status: &Connection, now: u64) -> Value {
    let mut state = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "connected": status.connected, "status": status.text,
        "ready": engine.is_some(), "current": null, "queue": [],
        "capacity": 0, "pending": [], "notices": []
    });
    if let Some(e) = engine {
        state["capacity"] = json!(e.config.requests.queue_capacity);
        if let Some(c) = &e.current {
            let mut current = request(e, &c.request);
            current["remaining"] = json!(c.until.saturating_sub(now));
            current["duration"] = json!(e.config.requests.current_timeout_seconds);
            state["current"] = current;
        }
        state["queue"] = e.queue.iter().map(|r| request(e, r)).collect();
        state["pending"] = e.pending.values().map(|p| json!({
            "requester": p.name, "mode": p.mode, "chart": p.chart.map(|c| c.label()),
            "chart_style": e.chart_style(p.chart),
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

const MAX_STATIC_BYTES: u64 = 32 * 1024 * 1024;

/// A validated public asset directory, prepared before committing live settings.
#[derive(Clone)]
pub struct StaticFiles {
    root: PathBuf,
}
impl StaticFiles {
    pub async fn open(path: &Path) -> Result<Self> {
        let root = tokio::fs::canonicalize(path)
            .await
            .with_context(|| format!("无法打开网页静态目录 {}", path.display()))?;
        ensure!(
            tokio::fs::metadata(&root).await?.is_dir(),
            "网页静态目录必须是文件夹"
        );
        let files = Self { root };
        files.read("index.html").await.map_err(|status| {
            anyhow::anyhow!(
                "网页静态目录 {} 中的 index.html 无法读取：{status}",
                path.display()
            )
        })?;
        Ok(files)
    }

    async fn read(&self, relative: &str) -> Result<(&'static str, Bytes), StatusCode> {
        let path = tokio::fs::canonicalize(self.root.join(relative))
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        // Resolve junctions/symlinks as well as textual path components.
        if !path.starts_with(&self.root) {
            return Err(StatusCode::NOT_FOUND);
        }
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        let metadata = file.metadata().await.map_err(|_| StatusCode::NOT_FOUND)?;
        if !metadata.is_file() {
            return Err(StatusCode::NOT_FOUND);
        }
        if metadata.len() > MAX_STATIC_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let mut bytes = Vec::new();
        file.take(MAX_STATIC_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if bytes.len() as u64 > MAX_STATIC_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        Ok((content_type(&path), bytes.into()))
    }
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn asset_path(url: &str) -> Option<String> {
    let path = percent_encoding::percent_decode_str(url)
        .decode_utf8()
        .ok()?;
    let path = path.strip_prefix('/')?;
    if path.is_empty()
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || path
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(['.', ' ']))
    {
        return None;
    }
    Some(path.to_owned())
}

pub struct Server {
    pub address: SocketAddr,
    state: Arc<RwLock<Bytes>>,
    files: Arc<RwLock<StaticFiles>>,
    task: JoinHandle<Result<()>>,
}
impl Server {
    pub async fn start(port: u16, static_dir: &Path, initial: &Value) -> Result<Self> {
        let files = Arc::new(RwLock::new(StaticFiles::open(static_dir).await?));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(RwLock::new(Bytes::from(serde_json::to_vec(initial)?)));
        let shared = state.clone();
        let shared_files = files.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if connections.len() < 32 => {
                        let (stream, _) = accepted?;
                        let state = shared.clone();
                        let files = shared_files.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |req| {
                                let state = state.clone();
                                let files = files.read().unwrap().clone();
                                async move {
                                    Ok::<_, Infallible>(route(req, &state, address.port(), &files).await)
                                }
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
            files,
            task,
        })
    }
    pub fn publish(&self, value: &Value) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            *self.state.write().unwrap() = bytes.into();
        }
    }
    pub fn set_static_files(&self, files: StaticFiles) {
        *self.files.write().unwrap() = files;
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

async fn route(
    req: Request<Incoming>,
    state: &RwLock<Bytes>,
    port: u16,
    files: &StaticFiles,
) -> Response<Full<Bytes>> {
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
            "/" => (StatusCode::TEMPORARY_REDIRECT, "text/plain", Bytes::new()),
            "/api/state" => (
                StatusCode::OK,
                "application/json; charset=utf-8",
                state.read().unwrap().clone(),
            ),
            path => {
                let asset = match path {
                    "/queue" | "/queue/" => Some("index.html".to_owned()),
                    path if path.starts_with("/api/") => None,
                    path => asset_path(path),
                };
                let result = match asset {
                    Some(asset) => files.read(&asset).await,
                    None => Err(StatusCode::NOT_FOUND),
                };
                match result {
                    Ok((mime, body)) => (StatusCode::OK, mime, body),
                    Err(status) => (
                        status,
                        "text/plain; charset=utf-8",
                        Bytes::from(status.canonical_reason().unwrap_or("Error")),
                    ),
                }
            }
        }
    };
    let mut response = Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Length", body.len());
    if status == StatusCode::TEMPORARY_REDIRECT {
        response = response.header("Location", "/queue");
    }
    response
        .body(Full::new(if req.method() == Method::HEAD {
            Bytes::new()
        } else {
            body
        }))
        .unwrap()
}
