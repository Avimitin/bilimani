//! Validated, comment-preserving configuration transactions. Never log TOML bodies.
use crate::{
    config::Config,
    output::{atomic_write, resolved_output},
};
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

pub struct Store {
    path: PathBuf,
    source: String,
    pub raw: Config,
    pub revision: u64,
}
pub struct Prepared {
    pub raw: Config,
    pub resolved: Config,
    source: String,
    expected: String,
    save: bool,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let source = std::fs::read_to_string(path)?;
        let raw = Config::parse(&source)?;
        Ok(Self {
            path: path.into(),
            source,
            raw,
            revision: 0,
        })
    }
    pub fn prepare(&self, raw: Config, revision: u64, dll: &Path) -> Result<Prepared> {
        ensure!(revision == self.revision, "配置已更新，请重新载入后再编辑");
        ensure!(
            std::fs::read_to_string(&self.path)? == self.source,
            "配置文件已被其他程序修改，请先重新载入，避免覆盖"
        );
        let resolved = self.validate(&raw, dll)?;
        let mut document: DocumentMut = self
            .source
            .parse()
            .map_err(|_| anyhow::anyhow!("配置文件格式错误"))?;
        let new: DocumentMut = toml::to_string(&raw)?.parse()?;
        for (section, item) in new.iter() {
            if let Some(table) = item.as_table() {
                if !document.get(section).is_some_and(Item::is_table) {
                    document[section] = Item::Table(toml_edit::Table::new());
                }
                if section == "controls" {
                    document[section]
                        .as_table_mut()
                        .unwrap()
                        .remove("skip_enabled");
                }
                if section == "aliases" {
                    document[section]
                        .as_table_mut()
                        .unwrap()
                        .retain(|key, _| table.contains_key(key));
                }
                for (key, value) in table.iter() {
                    let mut value = value.clone();
                    if let (Some(old), Some(new)) = (
                        document[section].get(key).and_then(Item::as_value),
                        value.as_value_mut(),
                    ) {
                        *new.decor_mut() = old.decor().clone();
                    }
                    document[section][key] = value;
                }
            }
        }
        Ok(Prepared {
            raw,
            resolved,
            source: document.to_string(),
            expected: self.source.clone(),
            save: true,
        })
    }
    pub fn reload(&self, dll: &Path) -> Result<Prepared> {
        let source = std::fs::read_to_string(&self.path)?;
        let raw = Config::parse(&source)?;
        let resolved = self.validate(&raw, dll)?;
        Ok(Prepared {
            raw,
            resolved,
            expected: source.clone(),
            source,
            save: false,
        })
    }
    fn validate(&self, raw: &Config, dll: &Path) -> Result<Config> {
        ensure!(
            raw.game == self.raw.game,
            "游戏模块与曲库路径需重启游戏后生效；本次未应用任何修改"
        );
        let mut config = raw.clone().resolve(&self.path)?;
        let queue = resolved_output(&config.output.queue_path)?;
        let interaction = resolved_output(&config.output.interaction_path)?;
        ensure!(
            queue.to_string_lossy().to_lowercase() != interaction.to_string_lossy().to_lowercase(),
            "两个 OBS 文件不能相同"
        );
        for path in [&queue, &interaction] {
            ensure!(
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("txt")),
                "OBS 文件必须使用 .txt 扩展名"
            );
            ensure!(
                ![self.path.canonicalize()?, dll.canonicalize()?].contains(path),
                "OBS 文件不能覆盖配置或 DLL"
            );
        }
        config.output.queue_path = queue;
        config.output.interaction_path = interaction;
        Ok(config)
    }
    pub fn commit(&mut self, prepared: Prepared) -> Result<Config> {
        ensure!(
            std::fs::read_to_string(&self.path)? == prepared.expected,
            "保存期间配置文件被修改，请重新载入"
        );
        if prepared.save {
            atomic_write(&self.path, prepared.source.as_bytes())?;
        }
        self.source = prepared.source;
        self.raw = prepared.raw;
        self.revision += 1;
        Ok(prepared.resolved)
    }
}
