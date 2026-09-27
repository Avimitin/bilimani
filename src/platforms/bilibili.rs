//! Rust port of the protocol used by xfgryujk/blivedm and blivechat.
//! See THIRD-PARTY-NOTICES.md for upstream licenses and source revisions.
use super::{Chat, ChatSource, Connection, Event};
use crate::config::Bilibili;
use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{HashSet, VecDeque},
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

const MAX_PACKET: usize = 4 * 1024 * 1024;
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36";

pub struct Source(pub Bilibili);
pub(super) fn redaction_secrets(config: &Bilibili) -> Vec<String> {
    [
        &config.auth_code,
        &config.access_key_id,
        &config.access_key_secret,
        &config.sessdata,
        &config.buvid3,
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .cloned()
    .collect()
}
impl ChatSource for Source {
    fn shutdown_message(&self) -> &'static str {
        "Stopping chat worker and closing the Open Live session"
    }
    fn run(
        self: Box<Self>,
        tx: mpsc::Sender<Event>,
        stop: watch::Receiver<bool>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(run(self.0, tx, stop))
    }
}
#[derive(Debug)]
pub enum Packet {
    Auth(i64),
    Heartbeat,
    Command(Value),
}
pub fn packet(op: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + body.len());
    out.extend_from_slice(&((16 + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&op.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(body);
    out
}
pub fn decode(data: &[u8]) -> Result<Vec<Packet>> {
    let mut result = Vec::new();
    let mut budget = MAX_PACKET;
    decode_inner(data, 0, &mut budget, &mut result)?;
    Ok(result)
}
fn decode_inner(
    mut data: &[u8],
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<Packet>,
) -> Result<()> {
    ensure!(
        depth <= 4 && data.len() <= MAX_PACKET,
        "Danmu packet exceeds limits"
    );
    while !data.is_empty() {
        ensure!(data.len() >= 16, "Truncated danmu header");
        let len = u32::from_be_bytes(data[0..4].try_into()?) as usize;
        let header = u16::from_be_bytes(data[4..6].try_into()?) as usize;
        let ver = u16::from_be_bytes(data[6..8].try_into()?);
        let op = u32::from_be_bytes(data[8..12].try_into()?);
        ensure!(
            header >= 16 && len >= header && len <= data.len(),
            "Invalid danmu packet length"
        );
        ensure!(out.len() < 4096, "Too many danmu messages");
        let body = &data[header..len];
        match (op, ver) {
            (5, 2 | 3) => {
                let reader: Box<dyn Read + '_> = if ver == 2 {
                    Box::new(flate2::read::ZlibDecoder::new(body))
                } else {
                    Box::new(brotli::Decompressor::new(body, 4096))
                };
                let mut expanded = Vec::new();
                reader
                    .take((*budget + 1) as u64)
                    .read_to_end(&mut expanded)?;
                ensure!(
                    expanded.len() <= *budget,
                    "Danmu decompression limit exceeded"
                );
                *budget -= expanded.len();
                decode_inner(&expanded, depth + 1, budget, out)?;
            }
            (5, 0 | 1) if !body.is_empty() => {
                out.push(Packet::Command(serde_json::from_slice(body)?))
            }
            (8, _) => out.push(Packet::Auth(
                serde_json::from_slice::<Value>(body)?["code"]
                    .as_i64()
                    .unwrap_or(-1),
            )),
            (3, _) => {
                out.push(Packet::Heartbeat); /* Some servers append the echoed heartbeat outside pack_len. */
                if &data[len..] == b"{}" {
                    return Ok(());
                }
            }
            _ => {}
        }
        data = &data[len..];
    }
    Ok(())
}

pub fn chat_from_command(v: &Value) -> Option<(Chat, String)> {
    let cmd = v["cmd"].as_str()?.split(':').next()?;
    let (user, name, text, key) = match cmd {
        "LIVE_OPEN_PLATFORM_DM" => {
            let d = &v["data"];
            if d["is_mirror"].as_bool().unwrap_or(false) {
                return None;
            }
            let user = d["open_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| format!("open:{s}"))
                .or_else(|| {
                    d["uid"]
                        .as_u64()
                        .filter(|&x| x > 0)
                        .map(|n| format!("uid:{n}"))
                })?;
            (
                user,
                d["uname"].as_str()?.to_owned(),
                d["msg"].as_str()?.to_owned(),
                d["msg_id"].as_str().unwrap_or("").to_owned(),
            )
        }
        "DANMU_MSG" => {
            let info = &v["info"];
            let uid = info[2][0].as_u64().filter(|&x| x > 0)?;
            let text = info[1].as_str()?.to_owned();
            let stamp = info[0][4].as_u64().unwrap_or(0);
            let key = if stamp > 0 {
                format!("web:{uid}:{stamp}:{}:{text}", info[0][5])
            } else {
                String::new()
            };
            (
                format!("uid:{uid}"),
                info[2][1].as_str()?.to_owned(),
                text,
                key,
            )
        }
        _ => return None,
    };
    if text.len() > 1024 {
        return None;
    }
    Some((Chat { user, name, text }, key))
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn status(tx: &mpsc::Sender<Event>, s: impl Into<String>) {
    let text = s.into();
    let connected = text.starts_with("弹幕已连接");
    let _ = tx.try_send(Event::Status(Connection { connected, text }));
}
fn diagnostic(tx: &mpsc::Sender<Event>, s: impl Into<String>) {
    // Leave channel space for real chat/status events during bursts.
    if tx.capacity() > 16 {
        let _ = tx.try_send(Event::Diagnostic(s.into()));
    }
}

struct Session {
    urls: Vec<String>,
    auth: String,
    game_id: Option<String>,
}
struct Client {
    cfg: Bilibili,
    http: reqwest::Client,
}

// The public website is a frontend; its backend lives on these separate hosts.
// Keep existing configurations working without sending credentials to arbitrary fallbacks.
fn relay_endpoint(configured: &str, attempt: u64) -> String {
    let base = configured.trim_end_matches('/');
    let first = match base {
        "https://blive.chat" | "https://api1.blive.chat" => 0,
        "https://api2.blive.chat" => 1,
        _ => return configured.to_owned(),
    };
    ["https://api1.blive.chat", "https://api2.blive.chat"][(first + attempt as usize % 2) % 2]
        .to_owned()
}

#[derive(Debug)]
struct ApiFailure(i64);
impl std::fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bilibili API returned code {}", self.0)
    }
}
impl std::error::Error for ApiFailure {}

/// Only expose known error types/codes, never arbitrary response bodies or URLs.
fn failure_reason(error: &anyhow::Error) -> String {
    for cause in error.chain() {
        if let Some(e) = cause.downcast_ref::<ApiFailure>() {
            let hint = match e.0 {
                7007 => "身份码无效",
                7010 => "同一直播间的应用会话数已达上限",
                4009 => "接口请求过于频繁",
                _ => "接口拒绝请求",
            };
            return format!("{hint}（API {}）", e.0);
        }
        if let Some(e) = cause.downcast_ref::<reqwest::Error>() {
            return if let Some(code) = e.status() {
                format!("服务返回 HTTP {}", code.as_u16())
            } else if e.is_timeout() {
                "HTTP 请求超时".into()
            } else if e.is_connect() {
                "无法连接 API 服务（网络、DNS 或 TLS 错误）".into()
            } else {
                "HTTP 连接中断或响应读取失败".into()
            };
        }
        if let Some(e) = cause.downcast_ref::<tokio_tungstenite::tungstenite::Error>() {
            use tokio_tungstenite::tungstenite::Error;
            return match e {
                Error::Http(response) => {
                    format!("WebSocket 握手返回 HTTP {}", response.status().as_u16())
                }
                Error::Tls(_) => "WebSocket TLS 认证失败".into(),
                Error::Io(_) => "WebSocket 网络连接失败".into(),
                _ => "WebSocket 连接关闭或协议错误".into(),
            };
        }
        if cause.is::<tokio::time::error::Elapsed>() {
            return "弹幕连接超时".into();
        }
    }
    match error.to_string().as_str() {
        "Danmu authentication rejected" => "弹幕服务器拒绝认证",
        "Danmu heartbeat timed out" => "弹幕心跳超时",
        "Open Live session ended" => "B 站已结束弹幕会话",
        "Websocket closed" => "弹幕服务器已关闭连接",
        _ => "响应格式错误或连接异常",
    }
    .into()
}

impl Client {
    fn new(mut cfg: Bilibili) -> Result<Self> {
        cfg.relay_url = relay_endpoint(&cfg.relay_url, 0);
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { cfg, http })
    }
    async fn api(&self, action: &str, body: Value) -> Result<Value> {
        let bytes = serde_json::to_vec(&body)?;
        let request = if self.cfg.relay_url.is_empty() {
            let url = format!("https://live-open.biliapi.com/v2/app/{action}");
            let headers = signing_headers(
                &self.cfg,
                &bytes,
                unix_time(),
                &uuid::Uuid::new_v4().simple().to_string(),
            )?;
            let mut req = self.http.post(url);
            for (k, v) in headers {
                req = req.header(k, v);
            }
            req
        } else {
            let action = match action {
                "start" => "start_game",
                "end" => "end_game",
                "heartbeat" => "game_heartbeat",
                _ => bail!("Invalid API action"),
            };
            let url = format!(
                "{}/api/open_live/{action}",
                self.cfg.relay_url.trim_end_matches('/')
            );
            ensure!(url.starts_with("https://"), "relay_url must use HTTPS");
            self.http.post(url)
        };
        let response = request
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(bytes)
            .send()
            .await?;
        read_api(response).await
    }
    async fn start(&self) -> Result<Session> {
        if self.cfg.mode == "web" {
            return self.start_web().await;
        }
        ensure!(!self.cfg.auth_code.is_empty(), "尚未设置主播身份码");
        if self.cfg.relay_url.is_empty() {
            ensure!(
                self.cfg.app_id > 0
                    && !self.cfg.access_key_id.is_empty()
                    && !self.cfg.access_key_secret.is_empty(),
                "Direct Open Live requires app credentials"
            );
        }
        let d = self
            .api(
                "start",
                json!({"code":self.cfg.auth_code,"app_id":self.cfg.app_id}),
            )
            .await?;
        let game_id = d["game_info"]["game_id"]
            .as_str()
            .context("Missing game ID")?
            .to_owned();
        let parsed = (|| -> Result<_> {
            let urls = d["websocket_info"]["wss_link"]
                .as_array()
                .context("Missing websocket hosts")?
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            ensure!(
                !urls.is_empty() && urls.iter().all(|u| u.starts_with("wss://")),
                "Invalid websocket hosts"
            );
            let auth = d["websocket_info"]["auth_body"]
                .as_str()
                .context("Missing websocket authentication")?
                .to_owned();
            Ok(Session {
                urls,
                auth,
                game_id: Some(game_id.clone()),
            })
        })();
        if parsed.is_err() {
            let _ = self
                .api("end", json!({"app_id":self.cfg.app_id,"game_id":game_id}))
                .await;
        }
        parsed
    }
    fn web_get(&self, url: &str) -> reqwest::RequestBuilder {
        let cookie = format!("SESSDATA={}; buvid3={}", self.cfg.sessdata, self.cfg.buvid3);
        self.http
            .get(url)
            .header("Referer", "https://live.bilibili.com/")
            .header("Cookie", cookie)
    }
    async fn start_web(&self) -> Result<Session> {
        ensure!(self.cfg.room_id > 0, "Set bilibili.room_id for web mode");
        let nav: Value = self
            .web_get("https://api.bilibili.com/x/web-interface/nav")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut key = String::new();
        for field in ["img_url", "sub_url"] {
            let url = nav["data"]["wbi_img"][field]
                .as_str()
                .context("Missing WBI signing keys")?;
            key.push_str(
                url.rsplit('/')
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .next()
                    .unwrap_or(""),
            );
        }
        let shuffled = wbi_key(&key)?;
        let room = read_api(
            self.web_get("https://api.live.bilibili.com/room/v1/Room/get_info")
                .query(&[("room_id", self.cfg.room_id)])
                .send()
                .await?,
        )
        .await?;
        let room_id = room["room_id"].as_u64().context("Missing room ID")?;
        let params = format!("id={room_id}&type=0&wts={}", unix_time());
        let signed = format!(
            "{params}&w_rid={}",
            hex::encode(Md5::digest(format!("{params}{shuffled}")))
        );
        let url =
            format!("https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?{signed}");
        let server = read_api(self.web_get(&url).send().await?).await?;
        let urls = server["host_list"]
            .as_array()
            .context("Missing danmu hosts")?
            .iter()
            .filter_map(|h| {
                let host = h["host"].as_str()?;
                if !host.ends_with(".bilibili.com") || host.contains('/') {
                    return None;
                }
                Some(format!("wss://{host}:{}/sub", h["wss_port"].as_u64()?))
            })
            .collect::<Vec<_>>();
        ensure!(!urls.is_empty(), "No secure danmu hosts");
        let token = server["token"].as_str().context("Missing danmu token")?;
        let auth = json!({"uid":nav["data"]["mid"].as_u64().unwrap_or(0),"roomid":room_id,
            "protover":3,"platform":"web","type":2,"buvid":self.cfg.buvid3,"key":token})
        .to_string();
        Ok(Session {
            urls,
            auth,
            game_id: None,
        })
    }
}
async fn read_api(response: reqwest::Response) -> Result<Value> {
    let mut response = response.error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_PACKET,
            "API response too large"
        );
        body.extend_from_slice(&chunk);
    }
    let v: Value = serde_json::from_slice(&body)?;
    let code = v["code"].as_i64().context("Missing API result code")?;
    if code != 0 {
        return Err(ApiFailure(code).into());
    }
    Ok(v["data"].clone())
}
pub fn wbi_key(key: &str) -> Result<String> {
    const ORDER: [usize; 32] = [
        46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19,
        29, 28, 14, 39, 12, 38, 41, 13,
    ];
    ensure!(key.is_ascii() && key.len() >= 64, "Invalid WBI key");
    Ok(ORDER.iter().map(|&i| key.as_bytes()[i] as char).collect())
}
pub fn signing_headers(
    cfg: &Bilibili,
    body: &[u8],
    timestamp: u64,
    nonce: &str,
) -> Result<Vec<(String, String)>> {
    let mut h = vec![
        ("x-bili-accesskeyid".into(), cfg.access_key_id.clone()),
        ("x-bili-content-md5".into(), hex::encode(Md5::digest(body))),
        ("x-bili-signature-method".into(), "HMAC-SHA256".into()),
        ("x-bili-signature-nonce".into(), nonce.into()),
        ("x-bili-signature-version".into(), "1.0".into()),
        ("x-bili-timestamp".into(), timestamp.to_string()),
    ];
    let signed = h
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut mac = Hmac::<Sha256>::new_from_slice(cfg.access_key_secret.as_bytes())?;
    mac.update(signed.as_bytes());
    h.push((
        "Authorization".into(),
        hex::encode(mac.finalize().into_bytes()),
    ));
    Ok(h)
}

#[derive(Default)]
struct Dedup {
    seen: HashSet<String>,
    order: VecDeque<String>,
}
impl Dedup {
    fn accept(&mut self, key: String) -> bool {
        if key.is_empty() {
            return true;
        }
        if !self.seen.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        if self.order.len() > 4096 {
            self.seen.remove(&self.order.pop_front().unwrap());
        }
        true
    }
}

pub async fn run(cfg: Bilibili, tx: mpsc::Sender<Event>, mut stop: watch::Receiver<bool>) {
    if !cfg.enabled {
        status(&tx, "弹幕连接已禁用");
        return;
    }
    let mut client = match Client::new(cfg) {
        Ok(c) => c,
        Err(_) => {
            status(&tx, "无法初始化网络客户端");
            return;
        }
    };
    if (client.cfg.mode == "open_live" && client.cfg.auth_code.is_empty())
        || (client.cfg.mode == "web" && client.cfg.room_id == 0)
    {
        status(&tx, "等待设置直播连接");
        return;
    }
    let mut retries = 0u64;
    if client.cfg.mode == "open_live"
        && client.cfg.relay_url.is_empty()
        && (client.cfg.app_id == 0
            || client.cfg.access_key_id.is_empty()
            || client.cfg.access_key_secret.is_empty())
    {
        status(&tx, "开放平台应用凭据尚未设置完整");
        return;
    }
    let mut dedup = Dedup::default();
    let configured_relay = client.cfg.relay_url.clone();
    loop {
        if *stop.borrow() {
            break;
        }
        client.cfg.relay_url = relay_endpoint(&configured_relay, retries);
        diagnostic(
            &tx,
            format!(
                "connect attempt={} mode={} relay_host={}",
                retries + 1,
                client.cfg.mode,
                reqwest::Url::parse(&client.cfg.relay_url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "direct".into())
            ),
        );
        status(&tx, "正在连接弹幕…");
        let started = tokio::select! { _=stop.changed()=>break, result=client.start()=>result };
        // Never put authentication bodies, cookies or credentials in OBS/logs.
        let failure = match started {
            Ok(session) => {
                diagnostic(
                    &tx,
                    format!("session_created websocket_endpoints={}", session.urls.len()),
                );
                let began = Instant::now();
                let result = socket_session(
                    &client,
                    &session,
                    retries as usize,
                    &tx,
                    &mut stop,
                    &mut dedup,
                )
                .await;
                if let Some(id) = &session.game_id {
                    let ended = client
                        .api("end", json!({"app_id":client.cfg.app_id,"game_id":id}))
                        .await;
                    diagnostic(
                        &tx,
                        match ended {
                            Ok(_) => "session_closed".into(),
                            Err(e) => format!("session_close_failed reason={}", failure_reason(&e)),
                        },
                    );
                }
                if began.elapsed() > Duration::from_secs(60) {
                    retries = 0;
                }
                result
                    .err()
                    .map(|e| format!("弹幕连接失败：{}", failure_reason(&e)))
                    .unwrap_or_else(|| "弹幕服务器已关闭连接".into())
            }
            Err(e) => format!("创建弹幕会话失败：{}", failure_reason(&e)),
        };
        if *stop.borrow() {
            break;
        }
        retries = retries.saturating_add(1);
        let jitter = uuid::Uuid::new_v4().as_bytes()[0] as u64 % 1000;
        let delay = (1 + retries * 2).min(30);
        status(&tx, format!("{failure}；{delay} 秒后重试"));
        tokio::select! { _=stop.changed()=>break, _=tokio::time::sleep(Duration::from_millis(delay*1000+jitter))=>{} }
    }
    diagnostic(&tx, "network_worker_stopped");
}
async fn socket_session(
    client: &Client,
    session: &Session,
    host: usize,
    tx: &mpsc::Sender<Event>,
    stop: &mut watch::Receiver<bool>,
    dedup: &mut Dedup,
) -> Result<()> {
    let ws_cfg = WebSocketConfig::default()
        .max_message_size(Some(MAX_PACKET))
        .max_frame_size(Some(MAX_PACKET));
    let (mut ws, _) = tokio::select! {
        _=stop.changed()=>return Ok(()),
        r=timeout(Duration::from_secs(15),tokio_tungstenite::connect_async_with_config(&session.urls[host%session.urls.len()],Some(ws_cfg),false))=>r??
    };
    diagnostic(tx, "websocket_connected; sending_authentication");
    timeout(
        Duration::from_secs(5),
        ws.send(Message::Binary(packet(7, session.auth.as_bytes()).into())),
    )
    .await??;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut game_heartbeat = tokio::time::interval(Duration::from_secs(20));
    let mut authenticated = false;
    let mut last_reply = Instant::now();
    let mut received_danmu = 0u64;
    let mut forwarded_danmu = 0u64;
    let mut dropped_danmu = 0u64;
    loop {
        tokio::select! {
            _=stop.changed()=>{ let _=timeout(Duration::from_secs(2),ws.close(None)).await; return Ok(()); }
            _=heartbeat.tick()=>{
                ensure!(last_reply.elapsed()<Duration::from_secs(if authenticated {45} else {15}),"Danmu heartbeat timed out");
                timeout(Duration::from_secs(5),ws.send(Message::Binary(packet(2,b"{}").into()))).await??;
                diagnostic(tx, "websocket_heartbeat_sent");
                diagnostic(tx, format!("danmu_counts received={received_danmu} forwarded={forwarded_danmu} dropped={dropped_danmu}"));
            }
            _=game_heartbeat.tick(), if session.game_id.is_some()=>{
                client.api("heartbeat",json!({"game_id":session.game_id})).await?;
                diagnostic(tx, "open_live_heartbeat_ok");
            }
            message=ws.next()=>{
                match message.context("Websocket closed")?? {
                    Message::Binary(data)=>for p in decode(&data)? {
                        match p {
                            Packet::Auth(code)=>{ diagnostic(tx, format!("websocket_auth_result code={code}")); ensure!(code==0,"Danmu authentication rejected");authenticated=true;last_reply=Instant::now();status(tx,"弹幕已连接 · 点歌 <曲名> [难度]"); }
                            Packet::Heartbeat=>{last_reply=Instant::now(); diagnostic(tx,"websocket_heartbeat_received");},
                            Packet::Command(v)=>{
                                if v["cmd"]=="LIVE_OPEN_PLATFORM_INTERACTION_END" { bail!("Open Live session ended"); }
                                let cmd = v["cmd"].as_str().unwrap_or("").split(':').next().unwrap_or("");
                                if !matches!(cmd, "LIVE_OPEN_PLATFORM_DM" | "DANMU_MSG") { continue; }
                                received_danmu += 1;
                                if !authenticated {
                                    diagnostic(tx, "danmu_ignored reason=not_authenticated");
                                } else if let Some((chat,key))=chat_from_command(&v) {
                                    if !dedup.accept(key) {
                                        diagnostic(tx, "danmu_ignored reason=duplicate");
                                    } else if tx.try_send(Event::Chat(chat)).is_err() {
                                        dropped_danmu += 1;
                                        status(tx,"弹幕请求过多，部分消息未处理，请稍后重试");
                                    } else {
                                        forwarded_danmu += 1;
                                    }
                                } else {
                                    diagnostic(tx, "danmu_ignored reason=invalid_sender_mirrored_or_malformed_message");
                                }
                            }
                        }
                    },
                    Message::Ping(p)=>{ timeout(Duration::from_secs(5),ws.send(Message::Pong(p))).await??; }
                    Message::Close(_)=>return Ok(()),
                    _=>{}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_relay_uses_api_hosts_and_preserves_custom_or_direct_access() {
        assert_eq!(
            relay_endpoint("https://blive.chat/", 0),
            "https://api1.blive.chat"
        );
        assert_eq!(
            relay_endpoint("https://blive.chat", 1),
            "https://api2.blive.chat"
        );
        assert_eq!(
            relay_endpoint("https://api2.blive.chat", 0),
            "https://api2.blive.chat"
        );
        assert_eq!(
            relay_endpoint("https://api2.blive.chat", 1),
            "https://api1.blive.chat"
        );
        assert_eq!(
            relay_endpoint("https://example.com/relay/", 5),
            "https://example.com/relay/"
        );
        assert_eq!(relay_endpoint("", 1), "");
    }

    #[tokio::test]
    async fn api_error_feedback_is_specific_without_exposing_response_secrets() {
        let response: reqwest::Response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(200)
            .body(r#"{"code":7007,"message":"sensitive-server-message","data":{"code":"private-auth-code"}}"#)
            .unwrap()
            .into();
        let error = read_api(response).await.unwrap_err();
        assert_eq!(failure_reason(&error), "身份码无效（API 7007）");
        assert!(!format!("{error:#}").contains("sensitive"));
        assert_eq!(
            failure_reason(&anyhow::anyhow!("private-auth-code")),
            "响应格式错误或连接异常"
        );
        let response: reqwest::Response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(503)
            .body("private-server-body")
            .unwrap()
            .into();
        assert_eq!(
            failure_reason(&read_api(response).await.unwrap_err()),
            "服务返回 HTTP 503"
        );
    }

    /// Opt-in live validation: secrets are supplied only through the process environment.
    #[tokio::test]
    #[ignore = "requires CHART_REQUESTER_AUTH_CODE and a live Open Live session"]
    async fn live_open_live_authentication_and_heartbeats() {
        let cfg = Bilibili {
            auth_code: std::env::var("CHART_REQUESTER_AUTH_CODE")
                .expect("Set identity code in environment"),
            // Exercise compatibility with configurations written by the first release.
            relay_url: "https://blive.chat".into(),
            ..Bilibili::default()
        };
        let client = Client::new(cfg).unwrap();
        let session = match client.start().await {
            Ok(session) => session,
            Err(e) => panic!("Create session: {}", failure_reason(&e)),
        };
        println!("Live API session created successfully");
        let (tx, mut rx) = mpsc::channel(512);
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let observer = tokio::spawn(async move {
            let deadline = tokio::time::sleep(Duration::from_secs(35));
            tokio::pin!(deadline);
            let mut authenticated = false;
            loop {
                tokio::select! {
                    _ = &mut deadline => break,
                    event = rx.recv() => match event {
                        Some(Event::Status(s)) if s.connected => {
                            authenticated = true;
                            println!("Live WebSocket authentication succeeded");
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
            let _ = stop_tx.send(true);
            authenticated
        });
        let result = socket_session(
            &client,
            &session,
            0,
            &tx,
            &mut stop_rx,
            &mut Dedup::default(),
        )
        .await;
        // Always release the session, including after authentication/heartbeat failures.
        let ended = client
            .api(
                "end",
                json!({"app_id":client.cfg.app_id,"game_id":session.game_id}),
            )
            .await;
        drop(tx);
        let authenticated = observer.await.unwrap();
        assert!(ended.is_ok(), "Session cleanup failed");
        println!("Live session closed successfully");
        if let Err(e) = result {
            panic!("WebSocket: {}", failure_reason(&e));
        }
        assert!(authenticated, "WebSocket did not authenticate");
    }

    #[tokio::test]
    async fn websocket_auth_compression_dedup_and_shutdown() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let Message::Binary(auth) = ws.next().await.unwrap().unwrap() else {
                panic!("Expected binary auth")
            };
            assert_eq!(u32::from_be_bytes(auth[8..12].try_into().unwrap()), 7);
            assert_eq!(&auth[16..], b"{\"test\":true}");
            ws.send(Message::Binary(packet(8, b"{\"code\":0}").into()))
                .await
                .unwrap();
            let command = json!({"cmd":"LIVE_OPEN_PLATFORM_DM","data":{"open_id":"alice","uname":"Alice","msg":"点歌 AA SPA","msg_id":"unique-message"}});
            let mut both = packet(5, command.to_string().as_bytes());
            both.extend(packet(5, command.to_string().as_bytes()));
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            use std::io::Write;
            encoder.write_all(&both).unwrap();
            let mut p = packet(5, &encoder.finish().unwrap());
            p[6..8].copy_from_slice(&2u16.to_be_bytes());
            ws.send(Message::Binary(p.into())).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                match m {
                    Message::Binary(p) if u32::from_be_bytes(p[8..12].try_into().unwrap()) == 2 => {
                        ws.send(Message::Binary(packet(3, &0u32.to_be_bytes()).into()))
                            .await
                            .unwrap();
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        let connection = tokio::spawn(async move {
            let client = Client::new(Bilibili::default()).unwrap();
            let session = Session {
                urls: vec![format!("ws://{address}")],
                auth: "{\"test\":true}".into(),
                game_id: None,
            };
            socket_session(
                &client,
                &session,
                0,
                &tx,
                &mut stop_rx,
                &mut Dedup::default(),
            )
            .await
        });
        let message = timeout(Duration::from_secs(5), async {
            loop {
                if let Some(Event::Chat(c)) = rx.recv().await {
                    break c;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(message.user, "open:alice");
        assert_eq!(message.text, "点歌 AA SPA");
        stop_tx.send(true).unwrap();
        timeout(Duration::from_secs(5), connection)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        while let Ok(event) = rx.try_recv() {
            assert!(!matches!(event, Event::Chat(_)), "Duplicate chat delivered");
        }
    }
    #[test]
    fn signing_covers_exact_body_bytes_and_stable_header_order() {
        let cfg = Bilibili {
            access_key_id: "test-key".into(),
            access_key_secret: "test-secret".into(),
            ..Default::default()
        };
        let body = br#"{"game_id":"test"}"#;
        let h = signing_headers(&cfg, body, 1234567890, "nonce").unwrap();
        // Independently generated with Python hashlib/hmac against the upstream
        // ordered-header signing recipe, including the exact compact JSON body.
        assert_eq!(
            h[6].1,
            "9617e0f731c32a481330a81e248ef8dfafd1d870b8897be50237c50faf4aca57"
        );
        assert_eq!(h[1].1, hex::encode(Md5::digest(body)));
        let base = h[..6]
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!base.ends_with('\n'));
        assert_eq!(h[6].0, "Authorization");
        assert_ne!(
            h[6].1,
            signing_headers(&cfg, b"{}", 1234567890, "nonce").unwrap()[6].1
        );
    }
}
