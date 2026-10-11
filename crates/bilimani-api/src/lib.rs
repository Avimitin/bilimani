//! Transport shared by game DLLs. No game, platform or viewer policy lives here.
mod server;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use server::Server;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    NotReady,
    NotFound,
    Busy,
    ModeUnavailable,
    Unavailable,
    Forbidden,
    TooLarge,
    MethodNotAllowed,
    ProxyConfiguration,
    Upstream,
    QueueFull,
    AliasConfiguration,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("游戏曲库尚未就绪")]
    NotReady,
    #[error("请求的资源不存在")]
    NotFound,
    #[error("已有选曲操作等待完成")]
    Busy,
    #[error("游戏模式尚未就绪或与请求不符")]
    ModeUnavailable,
    #[error("游戏服务暂时不可用")]
    Unavailable,
    #[error("请求来源不被允许")]
    Forbidden,
    #[error("请求或文件超过大小限制")]
    TooLarge,
    #[error("此资源不支持该 HTTP 方法")]
    MethodNotAllowed,
    #[error("页面的 proxy 配置缺失或无效")]
    ProxyConfiguration,
    #[error("上游请求失败或响应超过限制")]
    Upstream,
    #[error("点歌队列已满")]
    QueueFull,
    #[error("{0}")]
    AliasConfiguration(String),
}
impl ApiError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest(_) => ErrorCode::InvalidRequest,
            Self::NotReady => ErrorCode::NotReady,
            Self::NotFound => ErrorCode::NotFound,
            Self::Busy => ErrorCode::Busy,
            Self::ModeUnavailable => ErrorCode::ModeUnavailable,
            Self::Unavailable => ErrorCode::Unavailable,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::TooLarge => ErrorCode::TooLarge,
            Self::MethodNotAllowed => ErrorCode::MethodNotAllowed,
            Self::ProxyConfiguration => ErrorCode::ProxyConfiguration,
            Self::Upstream => ErrorCode::Upstream,
            Self::QueueFull => ErrorCode::QueueFull,
            Self::AliasConfiguration(_) => ErrorCode::AliasConfiguration,
        }
    }
    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidRequest(_) => 400,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::Busy | Self::ModeUnavailable => 409,
            Self::TooLarge => 413,
            Self::NotReady | Self::Unavailable => 503,
            Self::ProxyConfiguration => 503,
            Self::Upstream => 502,
            Self::QueueFull => 409,
            Self::AliasConfiguration(_) => 422,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionInput {
    pub song_id: u32,
    pub mode: Option<String>,
    pub chart: Option<String>,
}

#[derive(Debug)]
pub enum Command {
    Enqueue(QueueInput),
    Remove(u64),
    Priority(u64),
    ReloadAliases,
    Search { query: String, limit: usize },
    Select(SelectionInput),
    Selection(u64),
}
pub struct Message {
    pub command: Command,
    pub reply: oneshot::Sender<Result<Value, ApiError>>,
}
pub type Commands = mpsc::Receiver<Message>;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueInput {
    pub song_id: u32,
    pub mode: Option<String>,
    pub chart: Option<String>,
    #[serde(default)]
    pub requester: String,
}

/// Decode a bounded search request without accessing a catalog or IO.
pub fn search_query(query: &str) -> Result<Command, ApiError> {
    let invalid = || ApiError::InvalidRequest("需要 q 和可选的 limit（1–100）".into());
    if query.len() > 4096 {
        return Err(invalid());
    }
    let mut text = None;
    let mut limit = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        let value = value.replace('+', " ");
        let decoded = percent_encoding::percent_decode_str(&value)
            .decode_utf8()
            .map_err(|_| invalid())?;
        match key {
            "q" if text.is_none() => text = Some(decoded.trim().to_owned()),
            "limit" if limit.is_none() => {
                limit = Some(decoded.parse::<usize>().map_err(|_| invalid())?)
            }
            _ => return Err(invalid()),
        }
    }
    let query = text
        .filter(|q| !q.is_empty() && q.len() <= 1024)
        .ok_or_else(invalid)?;
    let limit = limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(invalid());
    }
    Ok(Command::Search { query, limit })
}
