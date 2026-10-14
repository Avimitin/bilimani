use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Cannot read DLL configuration: {0}")]
    Read(#[from] std::io::Error),
    #[error("Invalid DLL configuration: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Invalid listener address")]
    Address,
    #[error("Queue capacity must be 1..=1000, timeout 1..=86400 seconds")]
    Bounds,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    pub enabled: bool,
}
impl Default for Controls {
    fn default() -> Self {
        Self { enabled: true }
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueueSettings {
    pub capacity: usize,
    pub current_timeout_seconds: u64,
}
impl Default for QueueSettings {
    fn default() -> Self {
        Self {
            capacity: 20,
            current_timeout_seconds: 600,
        }
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub static_root: PathBuf,
    pub module: String,
    pub aliases_path: PathBuf,
    pub queue: QueueSettings,
    pub controls: Controls,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            listen: ([127, 0, 0, 1], 32133).into(),
            static_root: "bilimani_web".into(),
            module: "gamemdx.dll".into(),
            aliases_path: "bilimani_ddr_aliases.csv".into(),
            queue: QueueSettings::default(),
            controls: Controls::default(),
        }
    }
}
impl Config {
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let mut config: Self = match std::fs::read(root.join("bilimani_ddr.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        if config.listen.ip().is_multicast() {
            return Err(ConfigError::Address);
        }
        config.static_root = root.join(config.static_root);
        config.aliases_path = root.join(config.aliases_path);
        if !(1..=1000).contains(&config.queue.capacity)
            || !(1..=86400).contains(&config.queue.current_timeout_seconds)
        {
            return Err(ConfigError::Bounds);
        }
        Ok(config)
    }
}
