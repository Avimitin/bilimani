//! Transport-neutral messages and a single platform selection boundary.
use std::{future::Future, pin::Pin};
use tokio::sync::{mpsc, watch};
pub mod bilibili;
pub mod session;

#[derive(Clone, Debug)]
pub struct Chat {
    /// Stable identity within the selected source, never a display name.
    /// The source validates its own anonymous/invalid IDs before emitting chat.
    pub user: String,
    pub name: String,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Connection {
    pub connected: bool,
    pub text: String,
}
impl Connection {
    pub fn waiting() -> Self {
        Self {
            connected: false,
            text: "等待弹幕连接".into(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomInfo {
    pub room_id: u64,
    pub name: String,
    pub title: String,
}
#[derive(Debug)]
pub enum Event {
    Chat(Chat),
    Status(Connection),
    RoomInfo(RoomInfo),
    Diagnostic(String),
}
pub trait ChatSource: Send {
    fn shutdown_message(&self) -> &'static str {
        "Stopping chat worker"
    }
    fn run(
        self: Box<Self>,
        tx: mpsc::Sender<Event>,
        stop: watch::Receiver<bool>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
#[derive(Clone, PartialEq, Eq)]
pub enum SourceConfig {
    Bilibili(crate::config::Bilibili),
}
impl SourceConfig {
    pub fn redaction_secrets(&self) -> Vec<String> {
        match self {
            Self::Bilibili(config) => bilibili::redaction_secrets(config),
        }
    }
}
pub fn create(config: SourceConfig) -> Box<dyn ChatSource> {
    match config {
        SourceConfig::Bilibili(config) => Box::new(bilibili::Source(config)),
    }
}
