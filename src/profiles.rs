//! Stream profiles and card matching. The global connection remains compatible
//! with existing configuration files; personal profiles override it by card ID.
use crate::{
    config::{Bilibili, Config},
    platforms::SourceConfig,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const GLOBAL: &str = "global";

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CardId(String);
impl CardId {
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        ensure!(
            value.len() == 16 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "卡号应为 16 位十六进制字符"
        );
        ensure!(
            value != "0000000000000000" && !value.eq_ignore_ascii_case("FFFFFFFFFFFFFFFF"),
            "卡号无效"
        );
        Ok(Self(value.to_ascii_uppercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn masked(&self) -> String {
        format!("•••• {}", &self.0[12..])
    }
}
impl std::fmt::Debug for CardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.masked())
    }
}
impl TryFrom<String> for CardId {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}
impl From<CardId> for String {
    fn from(value: CardId) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StreamProfile {
    pub id: String,
    pub name: String,
    pub cards: Vec<CardId>,
    pub bilibili: Bilibili,
}
impl StreamProfile {
    pub fn new(card: CardId) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "新直播间".into(),
            cards: vec![card],
            bilibili: Bilibili::default(),
        }
    }
}

pub fn validate(profiles: &[StreamProfile]) -> Result<()> {
    ensure!(profiles.len() <= 100, "最多支持 100 个直播档案");
    let mut ids = HashSet::new();
    let mut cards = HashSet::new();
    for profile in profiles {
        ensure!(
            uuid::Uuid::parse_str(&profile.id).is_ok() && ids.insert(&profile.id),
            "直播档案标识无效或重复"
        );
        ensure!(
            !profile.name.trim().is_empty()
                && profile.name.chars().count() <= 80
                && !profile.name.chars().any(char::is_control),
            "档案名称需为 1–80 个字符且不能包含控制字符"
        );
        ensure!(profile.cards.len() <= 100, "每个直播档案最多绑定 100 张卡");
        for card in &profile.cards {
            ensure!(
                cards.insert(card),
                "同一卡号只能绑定一个直播档案，请先解除原绑定"
            );
        }
        ensure!(
            matches!(profile.bilibili.mode.as_str(), "open_live" | "web"),
            "直播档案连接方式无效"
        );
        ensure!(
            if profile.bilibili.mode == "web" {
                profile.bilibili.room_id > 0
            } else {
                !profile.bilibili.auth_code.trim().is_empty()
            },
            "请先填写个人档案的身份码或直播间号"
        );
    }
    Ok(())
}

pub fn for_card<'a>(config: &'a Config, card: Option<&CardId>) -> Option<&'a StreamProfile> {
    let card = card?;
    config.profiles.iter().find(|p| p.cards.contains(card))
}

/// Compare destinations independently of GUI revision or player-side changes.
pub struct ActiveStream {
    pub id: String,
    source: Bilibili,
}
pub struct StreamChange {
    pub profile_changed: bool,
    pub source: SourceConfig,
}
impl ActiveStream {
    pub fn new(config: &Config) -> Self {
        Self {
            id: GLOBAL.into(),
            source: config.bilibili.clone(),
        }
    }
    pub fn update(&mut self, config: &Config, card: Option<&CardId>) -> Option<StreamChange> {
        let profile = for_card(config, card);
        let id = profile.map_or(GLOBAL, |p| p.id.as_str());
        let source = profile.map_or(&config.bilibili, |p| &p.bilibili);
        let profile_changed = self.id != id;
        if !profile_changed && self.source == *source {
            return None;
        }
        self.id = id.to_owned();
        self.source = source.clone();
        Some(StreamChange {
            profile_changed,
            source: SourceConfig::Bilibili(source.clone()),
        })
    }
}

/// A login-bound draft must not silently bind a different player after a card swap.
pub fn check_login(expected: Option<&CardId>, current: Option<&CardId>) -> Result<()> {
    ensure!(
        expected.is_none() || expected == current,
        "登录卡号已变化，请撤销修改后为当前玩家重新创建或绑定档案"
    );
    Ok(())
}
