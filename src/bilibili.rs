//! Rust port of the protocol used by xfgryujk/blivedm and blivechat.
//! See THIRD-PARTY-NOTICES.md for upstream licenses and source revisions.
use crate::{config::Bilibili, engine::Chat};
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

#[derive(Debug)]
pub enum Event {
    Chat(Chat),
    Status(String),
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
    let _ = tx.try_send(Event::Status(s.into()));
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
impl Client {
    fn new(cfg: Bilibili) -> Result<Self> {
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
        ensure!(
            !self.cfg.auth_code.is_empty(),
            "Set bilibili.auth_code in chart-requester.toml"
        );
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
    ensure!(code == 0, "Bilibili API returned code {code}");
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
    let client = match Client::new(cfg) {
        Ok(c) => c,
        Err(_) => {
            status(&tx, "无法初始化网络客户端");
            return;
        }
    };
    if (client.cfg.mode == "open_live" && client.cfg.auth_code.is_empty())
        || (client.cfg.mode == "web" && client.cfg.room_id == 0)
    {
        status(
            &tx,
            "请在 chart-requester.toml 中填写 bilibili 身份码或房间号后重启",
        );
        return;
    }
    let mut retries = 0u64;
    if client.cfg.mode == "open_live"
        && client.cfg.relay_url.is_empty()
        && (client.cfg.app_id == 0
            || client.cfg.access_key_id.is_empty()
            || client.cfg.access_key_secret.is_empty())
    {
        status(
            &tx,
            "直连 Open Live 需要配置 app_id、access_key_id 和 access_key_secret 后重启",
        );
        return;
    }
    let mut dedup = Dedup::default();
    loop {
        if *stop.borrow() {
            break;
        }
        status(&tx, "正在连接弹幕…");
        let started = tokio::select! { _=stop.changed()=>break, result=client.start()=>result };
        // Never put authentication bodies, cookies or credentials in OBS/logs.
        if let Ok(session) = started {
            let began = Instant::now();
            let _ = socket_session(
                &client,
                &session,
                retries as usize,
                &tx,
                &mut stop,
                &mut dedup,
            )
            .await;
            if let Some(id) = &session.game_id {
                let _ = client
                    .api("end", json!({"app_id":client.cfg.app_id,"game_id":id}))
                    .await;
            }
            if began.elapsed() > Duration::from_secs(60) {
                retries = 0;
            }
        }
        if *stop.borrow() {
            break;
        }
        retries = retries.saturating_add(1);
        let jitter = uuid::Uuid::new_v4().as_bytes()[0] as u64 % 1000;
        let delay = (1 + retries * 2).min(30);
        status(
            &tx,
            format!("弹幕已断开或认证失败，{delay} 秒后重试；请检查身份码和网络"),
        );
        tokio::select! { _=stop.changed()=>break, _=tokio::time::sleep(Duration::from_millis(delay*1000+jitter))=>{} }
    }
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
    timeout(
        Duration::from_secs(5),
        ws.send(Message::Binary(packet(7, session.auth.as_bytes()).into())),
    )
    .await??;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut game_heartbeat = tokio::time::interval(Duration::from_secs(20));
    let mut authenticated = false;
    let mut last_reply = Instant::now();
    loop {
        tokio::select! {
            _=stop.changed()=>{ let _=timeout(Duration::from_secs(2),ws.close(None)).await; return Ok(()); }
            _=heartbeat.tick()=>{
                ensure!(last_reply.elapsed()<Duration::from_secs(if authenticated {45} else {15}),"Danmu heartbeat timed out");
                timeout(Duration::from_secs(5),ws.send(Message::Binary(packet(2,b"{}").into()))).await??;
            }
            _=game_heartbeat.tick(), if session.game_id.is_some()=>{
                client.api("heartbeat",json!({"game_id":session.game_id})).await?;
            }
            message=ws.next()=>{
                match message.context("Websocket closed")?? {
                    Message::Binary(data)=>for p in decode(&data)? {
                        match p {
                            Packet::Auth(code)=>{ ensure!(code==0,"Danmu authentication rejected");authenticated=true;last_reply=Instant::now();status(tx,"弹幕已连接 · 点歌 <曲名> [难度]"); }
                            Packet::Heartbeat=>last_reply=Instant::now(),
                            Packet::Command(v)=>{
                                if v["cmd"]=="LIVE_OPEN_PLATFORM_INTERACTION_END" { bail!("Open Live session ended"); }
                                if authenticated
                                    && let Some((chat,key))=chat_from_command(&v)
                                    && dedup.accept(key)
                                    && tx.try_send(Event::Chat(chat)).is_err() {
                                    status(tx,"弹幕请求过多，部分消息未处理，请稍后重试");
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
