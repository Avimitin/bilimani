use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub requests: Requests,
    pub output: Output,
    pub bilibili: Bilibili,
    pub game: Game,
    /// Alias -> exact song title or numeric music ID.
    pub aliases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Requests {
    pub queue_capacity: usize,
    pub candidates: usize,
    pub selection_timeout_seconds: u64,
    pub current_timeout_seconds: u64,
    pub cooldown_seconds: u64,
    pub max_pending_users: usize,
}
impl Default for Requests {
    fn default() -> Self {
        Self {
            queue_capacity: 20,
            candidates: 5,
            selection_timeout_seconds: 60,
            current_timeout_seconds: 600,
            cooldown_seconds: 0,
            max_pending_users: 100,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub queue_path: PathBuf,
    pub interaction_path: PathBuf,
    pub message_seconds: u64,
    pub recent_messages: usize,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            queue_path: "obs/queue.txt".into(),
            interaction_path: "obs/interaction.txt".into(),
            message_seconds: 30,
            recent_messages: 8,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bilibili {
    pub enabled: bool,
    /// open_live (identity code) or web (room ID and optional cookies).
    pub mode: String,
    pub auth_code: String,
    /// Empty uses direct signed Open Live requests; otherwise use blivechat's public API.
    pub relay_url: String,
    pub app_id: u64,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub room_id: u64,
    pub sessdata: String,
    pub buvid3: String,
}
impl Default for Bilibili {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "open_live".into(),
            auth_code: String::new(),
            relay_url: "https://api1.blive.chat".into(),
            app_id: 0,
            access_key_id: String::new(),
            access_key_secret: String::new(),
            room_id: 0,
            sessdata: String::new(),
            buvid3: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Game {
    pub module: String,
    /// Optional explicit database path. Empty uses the live game database.
    pub database_path: PathBuf,
}
impl Default for Game {
    fn default() -> Self {
        Self {
            module: "bm2dx.dll".into(),
            database_path: PathBuf::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut result: Self = toml::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("Cannot read {}", path.display()))?,
        )
        // TOML errors normally include the source line, which could contain a
        // cookie or access key. Report only the location to startup logs.
        .map_err(|e: toml::de::Error| {
            anyhow::anyhow!(
                "Invalid chart-requester.toml near byte {}; compare chart-requester.example.toml",
                e.span().map(|s| s.start).unwrap_or(0)
            )
        })?;
        result.validate()?;
        let root = path
            .parent()
            .context("Configuration has no parent directory")?;
        result.output.queue_path = root.join(&result.output.queue_path);
        result.output.interaction_path = root.join(&result.output.interaction_path);
        if !result.game.database_path.as_os_str().is_empty() {
            result.game.database_path = root.join(&result.game.database_path);
        }
        ensure!(
            result.output.queue_path != path && result.output.interaction_path != path,
            "OBS output must not overwrite the configuration"
        );
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=1000).contains(&self.requests.queue_capacity),
            "queue_capacity must be 1..1000"
        );
        ensure!(
            (1..=20).contains(&self.requests.candidates),
            "candidates must be 1..20"
        );
        ensure!(
            (1..=1000).contains(&self.requests.max_pending_users),
            "max_pending_users must be 1..1000"
        );
        ensure!(
            (1..=86400).contains(&self.requests.selection_timeout_seconds),
            "selection_timeout_seconds must be 1..86400"
        );
        ensure!(
            (1..=86400).contains(&self.requests.current_timeout_seconds),
            "current_timeout_seconds must be 1..86400"
        );
        ensure!(
            self.requests.cooldown_seconds <= 86400,
            "cooldown_seconds must be <=86400"
        );
        ensure!(
            (1..=100).contains(&self.output.recent_messages),
            "recent_messages must be 1..100"
        );
        ensure!(
            (1..=86400).contains(&self.output.message_seconds),
            "message_seconds must be 1..86400"
        );
        ensure!(
            !self.output.queue_path.as_os_str().is_empty()
                && !self.output.interaction_path.as_os_str().is_empty(),
            "OBS paths must not be empty"
        );
        ensure!(
            self.output.queue_path != self.output.interaction_path,
            "OBS paths must differ"
        );
        ensure!(
            matches!(self.bilibili.mode.as_str(), "open_live" | "web"),
            "bilibili.mode must be open_live or web"
        );
        Ok(())
    }
}
