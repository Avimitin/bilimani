use crate::{ApiError, Command, Commands, Message, QueueInput, SelectionInput, search_query};
use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::SocketAddr,
    path::Path,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};

type Snapshots = Arc<RwLock<BTreeMap<String, Bytes>>>;
pub struct Server {
    pub address: SocketAddr,
    snapshots: Snapshots,
    task: JoinHandle<()>,
}
impl Server {
    pub async fn start(address: SocketAddr, static_root: &Path) -> Result<(Self, Commands)> {
        let listener = TcpListener::bind(address)
            .await
            .context("Cannot listen for game API")?;
        let address = listener.local_addr()?;
        let root = Arc::new(static_root.to_path_buf());
        let snapshots: Snapshots = Arc::default();
        let published = snapshots.clone();
        let (tx, rx) = mpsc::channel::<Message>(64);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(12))
            .build()?;
        let task = tokio::spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let Ok((stream, _)) = result else { break };
                        let local = stream.local_addr().unwrap_or(address);
                        if clients.len() >= 32 { continue; }
                        let (snapshots, root, tx, http) = (published.clone(), root.clone(), tx.clone(), http.clone());
                        clients.spawn(async move {
                            let service = service_fn(move |req| {
                                let (snapshots, root, tx, http) = (snapshots.clone(), root.clone(), tx.clone(), http.clone());
                                async move { Ok::<_, Infallible>(route(req, &snapshots, &root, &tx, &http, local).await) }
                            });
                            let _ = tokio::time::timeout(Duration::from_secs(15),
                                http1::Builder::new().keep_alive(false).serve_connection(TokioIo::new(stream), service)).await;
                        });
                    }
                    _ = clients.join_next(), if !clients.is_empty() => {}
                }
            }
        });
        Ok((
            Self {
                address,
                snapshots,
                task,
            },
            rx,
        ))
    }
    /// Serialize before acquiring the lock. All routes then observe one publication.
    pub fn publish(&self, snapshots: impl IntoIterator<Item = (String, Value)>) -> Result<()> {
        let next = snapshots
            .into_iter()
            .map(|(path, value)| serde_json::to_vec(&value).map(|bytes| (path, Bytes::from(bytes))))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        *self.snapshots.write().expect("snapshot lock") = next;
        Ok(())
    }
    pub async fn stop(self) {
        self.task.abort();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn same_origin(req: &Request<Incoming>, address: SocketAddr) -> bool {
    let address = SocketAddr::new(address.ip().to_canonical(), address.port());
    let authority = if address.port() == 80 {
        match address.ip() {
            std::net::IpAddr::V4(ip) => ip.to_string(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        }
    } else {
        address.to_string()
    };
    let mut hosts = vec![authority];
    if address.ip().is_loopback() {
        hosts.push(if address.port() == 80 {
            "localhost".into()
        } else {
            format!("localhost:{}", address.port())
        });
    }
    let host = req.headers().get("host").and_then(|v| v.to_str().ok());
    host.is_some_and(|h| hosts.iter().any(|allowed| allowed == h))
        && req.headers().get("origin").is_none_or(|origin| {
            origin
                .to_str()
                .is_ok_and(|o| hosts.iter().any(|h| o == format!("http://{h}")))
        })
}

async fn route(
    req: Request<Incoming>,
    snapshots: &Snapshots,
    root: &Path,
    tx: &mpsc::Sender<Message>,
    http: &reqwest::Client,
    local: SocketAddr,
) -> Response<Full<Bytes>> {
    let head = req.method() == Method::HEAD;
    let result = if same_origin(&req, local) {
        handle(req, snapshots, root, tx, http).await
    } else {
        Err(ApiError::Forbidden)
    };
    match result {
        Ok((status, mime, bytes, location)) => {
            response(status, mime, bytes, location.as_deref(), head)
        }
        Err(error) => response(
            StatusCode::from_u16(error.status()).expect("HTTP status"),
            "application/json; charset=utf-8",
            Bytes::from(
                json!({"error": {"code": error.code(), "message": error.to_string()}}).to_string(),
            ),
            None,
            head,
        ),
    }
}
type Payload = (StatusCode, &'static str, Bytes, Option<String>);
fn json_response(status: StatusCode, value: Value) -> Payload {
    (
        status,
        "application/json; charset=utf-8",
        Bytes::from(value.to_string()),
        None,
    )
}
async fn handle(
    req: Request<Incoming>,
    snapshots: &Snapshots,
    root: &Path,
    tx: &mpsc::Sender<Message>,
    http: &reqwest::Client,
) -> Result<Payload, ApiError> {
    let path = req.uri().path().to_owned();
    if path.contains("/_proxy/") {
        return proxy(req, root, http).await;
    }
    if req.method() == Method::POST
        && matches!(path.as_str(), "/api/v1/selections" | "/api/v1/queue")
    {
        if !req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            })
        {
            return Err(ApiError::InvalidRequest("需要 application/json".into()));
        }
        let bytes = Limited::new(req.into_body(), 8192)
            .collect()
            .await
            .map_err(|_| ApiError::TooLarge)?
            .to_bytes();
        if path == "/api/v1/queue" {
            let input: QueueInput = serde_json::from_slice(&bytes)
                .map_err(|_| ApiError::InvalidRequest("队列 JSON 字段无效".into()))?;
            return Ok(json_response(
                StatusCode::CREATED,
                command(tx, Command::Enqueue(input)).await?,
            ));
        }
        let input: SelectionInput = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::InvalidRequest("选曲 JSON 字段无效".into()))?;
        return Ok(json_response(
            StatusCode::ACCEPTED,
            command(tx, Command::Select(input)).await?,
        ));
    }
    let mutation = if req.method() == Method::POST && path == "/api/v1/aliases/reload" {
        Some(Command::ReloadAliases)
    } else if let Some(tail) = path.strip_prefix("/api/v1/queue/") {
        if let Some(id) = tail
            .strip_suffix("/priority")
            .filter(|_| req.method() == Method::POST)
        {
            Some(Command::Priority(resource_id(id)?))
        } else if req.method() == Method::DELETE {
            Some(Command::Remove(resource_id(tail)?))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(mutation) = mutation {
        // Consume a bounded body even though these commands have no payload.
        let bytes = Limited::new(req.into_body(), 8192)
            .collect()
            .await
            .map_err(|_| ApiError::TooLarge)?
            .to_bytes();
        if !bytes.is_empty() {
            return Err(ApiError::InvalidRequest("此操作不接受请求正文".into()));
        }
        return Ok(json_response(StatusCode::OK, command(tx, mutation).await?));
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return Err(ApiError::MethodNotAllowed);
    }
    if path == "/api/v1/songs" {
        return Ok(json_response(
            StatusCode::OK,
            command(tx, search_query(req.uri().query().unwrap_or(""))?).await?,
        ));
    }
    if let Some(id) = path.strip_prefix("/api/v1/selections/") {
        let id = id
            .parse::<u64>()
            .map_err(|_| ApiError::InvalidRequest("操作 ID 无效".into()))?;
        return Ok(json_response(
            StatusCode::OK,
            command(tx, Command::Selection(id)).await?,
        ));
    }
    if path.starts_with("/api/") {
        let bytes = snapshots
            .read()
            .expect("snapshot lock")
            .get(&path)
            .cloned()
            .ok_or(ApiError::NotFound)?;
        return Ok((
            StatusCode::OK,
            "application/json; charset=utf-8",
            bytes,
            None,
        ));
    }
    match read_static(root, &path).await? {
        StaticResource::File { mime, bytes } => Ok((StatusCode::OK, mime, bytes, None)),
        StaticResource::Directory => {
            let mut location = format!("{path}/");
            if let Some(query) = req.uri().query() {
                location.push('?');
                location.push_str(query);
            }
            Ok((
                StatusCode::PERMANENT_REDIRECT,
                "text/plain; charset=utf-8",
                Bytes::new(),
                Some(location),
            ))
        }
    }
}

/// Stateless HTTP forwarding. Only the neutral `proxy` map is read; platform
/// fields, identity codes and session responses are opaque bytes to this layer.
async fn proxy(
    req: Request<Incoming>,
    root: &Path,
    http: &reqwest::Client,
) -> Result<Payload, ApiError> {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::POST) {
        return Err(ApiError::MethodNotAllowed);
    }
    let path = req.uri().path();
    let (app, tail) = path.split_once("/_proxy/").ok_or(ApiError::NotFound)?;
    let (name, relative) = tail.split_once('/').ok_or(ApiError::NotFound)?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(ApiError::NotFound);
    }
    #[derive(serde::Deserialize)]
    struct Config {
        proxy: BTreeMap<String, String>,
    }
    let StaticResource::File { bytes, .. } = read_static(root, &format!("{app}/config.json"))
        .await
        .map_err(|_| ApiError::ProxyConfiguration)?
    else {
        return Err(ApiError::ProxyConfiguration);
    };
    let config: Config =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::ProxyConfiguration)?;
    let upstream = config.proxy.get(name).ok_or(ApiError::ProxyConfiguration)?;
    let base = reqwest::Url::parse(upstream).map_err(|_| ApiError::ProxyConfiguration)?;
    let local_http = base.scheme() == "http"
        && base.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    if (base.scheme() != "https" && !local_http)
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(ApiError::ProxyConfiguration);
    }
    let decoded = percent_encoding::percent_decode_str(relative)
        .decode_utf8()
        .map_err(|_| ApiError::InvalidRequest("代理路径无效".into()))?;
    if decoded.contains('\\')
        || decoded.split('/').any(|s| s == ".." || s == ".")
        || relative.starts_with('/')
    {
        return Err(ApiError::InvalidRequest("代理路径无效".into()));
    }
    let mut target =
        reqwest::Url::parse(&format!("{}/{}", upstream.trim_end_matches('/'), relative))
            .map_err(|_| ApiError::InvalidRequest("代理路径无效".into()))?;
    if target.origin() != base.origin() {
        return Err(ApiError::InvalidRequest("代理目标无效".into()));
    }
    target.set_query(req.uri().query());
    let mut request = http.request(req.method().clone(), target);
    // Do not forward browser cookies, Origin, Host or hop-by-hop headers.
    for key in ["content-type", "accept", "authorization", "user-agent"] {
        if let Some(value) = req.headers().get(key) {
            request = request.header(key, value);
        }
    }
    let bytes = Limited::new(req.into_body(), 1024 * 1024)
        .collect()
        .await
        .map_err(|_| ApiError::TooLarge)?
        .to_bytes();
    let mut upstream = request
        .body(bytes)
        .send()
        .await
        .map_err(|_| ApiError::Upstream)?;
    if upstream.status().is_redirection() {
        return Err(ApiError::Upstream);
    }
    let status = upstream.status();
    // Proxy data is consumed by JS. Never serve upstream HTML under the local origin.
    let mime = if upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
    {
        "application/json; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    let mut output = Vec::new();
    while let Some(chunk) = upstream.chunk().await.map_err(|_| ApiError::Upstream)? {
        if output.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err(ApiError::Upstream);
        }
        output.extend_from_slice(&chunk);
    }
    Ok((status, mime, output.into(), None))
}
async fn command(tx: &mpsc::Sender<Message>, command: Command) -> Result<Value, ApiError> {
    let (reply, response) = oneshot::channel();
    tx.try_send(Message { command, reply })
        .map_err(|_| ApiError::Unavailable)?;
    tokio::time::timeout(Duration::from_secs(2), response)
        .await
        .map_err(|_| ApiError::Unavailable)?
        .map_err(|_| ApiError::Unavailable)?
}
fn response(
    status: StatusCode,
    mime: &str,
    bytes: Bytes,
    location: Option<&str>,
    head: bool,
) -> Response<Full<Bytes>> {
    let mut response = Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Content-Length", bytes.len());
    if let Some(location) = location {
        response = response.header("Location", location);
    }
    response
        .body(Full::new(if head { Bytes::new() } else { bytes }))
        .expect("static response headers")
}
enum StaticResource {
    File { mime: &'static str, bytes: Bytes },
    Directory,
}
async fn read_static(root: &Path, url: &str) -> Result<StaticResource, ApiError> {
    let path = percent_encoding::percent_decode_str(url)
        .decode_utf8()
        .map_err(|_| ApiError::NotFound)?;
    let relative = path.strip_prefix('/').ok_or(ApiError::NotFound)?;
    let relative = if relative.is_empty() || relative.ends_with('/') {
        format!("{relative}index.html")
    } else {
        relative.to_owned()
    };
    if relative
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || relative
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(['.', ' ']))
    {
        return Err(ApiError::NotFound);
    }
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let path = tokio::fs::canonicalize(root.join(relative))
        .await
        .map_err(|_| ApiError::NotFound)?;
    if !path.starts_with(&root) {
        return Err(ApiError::NotFound);
    }
    if !url.ends_with('/')
        && tokio::fs::metadata(&path)
            .await
            .map_err(|_| ApiError::NotFound)?
            .is_dir()
    {
        return Ok(StaticResource::Directory);
    }
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let metadata = file.metadata().await.map_err(|_| ApiError::NotFound)?;
    if !metadata.is_file() {
        return Err(ApiError::NotFound);
    }
    const MAX: u64 = 32 * 1024 * 1024;
    if metadata.len() > MAX {
        return Err(ApiError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ApiError::Unavailable)?;
    if bytes.len() as u64 > MAX {
        return Err(ApiError::TooLarge);
    }
    let mime = match path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    };
    Ok(StaticResource::File {
        mime,
        bytes: bytes.into(),
    })
}

fn resource_id(id: &str) -> Result<u64, ApiError> {
    id.parse()
        .map_err(|_| ApiError::InvalidRequest("队列 ID 无效".into()))
}
