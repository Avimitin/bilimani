//! SQLite persistence; game/render code only sees owned configuration values.
//! All mutations are transactional and compare persistent revisions.
use crate::{config::Config, output::resolved_output, profiles::StreamProfile};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const SCHEMA: i64 = 2;
// Keep the database identity stable so renamed databases remain readable.
const APPLICATION_ID: i64 = 0x43525153;
const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024;

pub struct Store {
    path: PathBuf,
    connection: Connection,
    active_game: crate::config::Game,
    pub raw: Config,
    pub revision: u64,
}
pub struct Prepared {
    pub raw: Config,
    pub resolved: Config,
    expected: u64,
    save: bool,
}

/// Portable backup, not a second live configuration source.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    format: String,
    version: u32,
    config: Config,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("db")),
            "配置存储路径必须使用 .db 扩展名"
        );
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path).context("无法打开配置数据库")?;
        connection.busy_timeout(Duration::from_millis(500))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let application: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        ensure!(version <= SCHEMA, "配置数据库来自更新版本，请升级插件");
        let mut initial_profiles = Vec::new();
        if version == 0 {
            let tables: i64 = tx.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                tables == 0 && application == 0,
                "该文件不是 bilimani 配置数据库"
            );
            let legacy = path.with_extension("toml");
            let raw = if legacy.exists() {
                Config::read(&legacy).context("旧 TOML 配置迁移失败，原文件保持不变")?
            } else {
                Config::default()
            };
            raw.validate()?;
            tx.execute_batch(
                "CREATE TABLE stream_profiles (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    platform TEXT NOT NULL,
                    settings TEXT NOT NULL
                );
                CREATE TABLE settings (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    revision INTEGER NOT NULL CHECK(revision >= 0),
                    default_profile INTEGER NOT NULL REFERENCES stream_profiles(id),
                    settings TEXT NOT NULL
                );",
            )?;
            let (global, source) = encode(&raw)?;
            tx.execute(
                "INSERT INTO stream_profiles VALUES (1, '默认直播档案', 'bilibili', ?1)",
                [source],
            )?;
            tx.execute("INSERT INTO settings VALUES (1, 0, 1, ?1)", [global])?;
            initial_profiles = raw.profiles;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        } else {
            ensure!(
                application == APPLICATION_ID,
                "该文件不是 bilimani 配置数据库"
            );
        }
        if version < 2 {
            tx.execute_batch(
                "ALTER TABLE stream_profiles ADD COLUMN profile_key TEXT;
                 CREATE UNIQUE INDEX profile_keys ON stream_profiles(profile_key);
                 UPDATE stream_profiles SET profile_key = 'global', name = '全局档案'
                    WHERE id = (SELECT default_profile FROM settings WHERE id = 1);
                 CREATE TABLE stream_cards (
                    card_id TEXT PRIMARY KEY,
                    profile_id INTEGER NOT NULL REFERENCES stream_profiles(id) ON DELETE CASCADE
                 );",
            )?;
            tx.pragma_update(None, "user_version", SCHEMA)?;
        }
        if !initial_profiles.is_empty() {
            write_profiles(&tx, &initial_profiles)?;
        }
        // Derived data only: old schema-2 readers may ignore this optional table.
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS song_catalog (
                game TEXT PRIMARY KEY,
                songs TEXT NOT NULL
            );",
        )?;
        // Validate inside the migration transaction, so corrupt input rolls it back.
        read_config(&tx)?;
        tx.commit()?;
        let (raw, revision) = read(&connection)?;
        Ok(Self {
            path: path.to_owned(),
            connection,
            active_game: raw.game.clone(),
            raw,
            revision,
        })
    }

    pub fn prepare(&self, raw: Config, revision: u64, dll: &Path) -> Result<Prepared> {
        ensure!(
            revision == self.revision && revision == revision_of(&self.connection)?,
            "配置已被另一个窗口更新，请刷新已保存设置后再编辑"
        );
        let resolved = self.validate(&raw, dll)?;
        Ok(Prepared {
            raw,
            resolved,
            expected: revision,
            save: true,
        })
    }

    pub fn reload(&self, dll: &Path) -> Result<Prepared> {
        let (raw, revision) = read(&self.connection)?;
        let resolved = self.validate(&raw, dll)?;
        Ok(Prepared {
            raw,
            resolved,
            expected: revision,
            save: false,
        })
    }

    fn validate(&self, raw: &Config, dll: &Path) -> Result<Config> {
        let mut config = raw.clone().resolve(&self.path)?;
        config.overlay.static_dir = dll
            .parent()
            .context("DLL has no parent directory")?
            .join(&raw.overlay.static_dir);
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
        // Native modules/catalogs remain bound until restart, but can be edited in GUI.
        config.game = self.active_game.clone();
        if !config.game.database_path.as_os_str().is_empty() {
            config.game.database_path =
                self.path.parent().unwrap().join(&config.game.database_path);
        }
        Ok(config)
    }

    pub fn commit(&mut self, prepared: Prepared) -> Result<Config> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            revision_of(&tx)? == prepared.expected,
            "保存期间配置已更新，请刷新已保存设置后再编辑"
        );
        let revision = if prepared.save {
            let (global, source) = encode(&prepared.raw)?;
            tx.execute("UPDATE stream_profiles SET settings = ?1 WHERE id = (SELECT default_profile FROM settings WHERE id = 1)", [source])?;
            write_profiles(&tx, &prepared.raw.profiles)?;
            let expected = i64::try_from(prepared.expected)?;
            ensure!(expected < i64::MAX, "配置版本号已超出范围");
            tx.execute("UPDATE settings SET settings = ?1, revision = revision + 1 WHERE id = 1 AND revision = ?2", params![global, expected])?;
            prepared.expected + 1
        } else {
            prepared.expected
        };
        tx.commit()?;
        self.raw = prepared.raw;
        self.revision = revision;
        Ok(prepared.resolved)
    }

    pub fn restart_required(&self) -> bool {
        self.raw.game != self.active_game
    }

    /// Cache titles/IDs for offline alias validation without exposing native data.
    /// Use the bound game, even when a different game is saved for next startup.
    pub fn cache_catalog(&self, songs: &[crate::game::Song]) -> Result<()> {
        let titles: Vec<_> = songs.iter().map(|s| (s.id, &s.title)).collect();
        self.connection.execute(
            "INSERT INTO song_catalog (game, songs) VALUES (?1, ?2)
             ON CONFLICT(game) DO UPDATE SET songs = excluded.songs",
            params![
                serde_json::to_string(&self.active_game)?,
                serde_json::to_string(&titles)?
            ],
        )?;
        Ok(())
    }

    pub fn cached_catalog(
        &self,
        game: &crate::config::Game,
    ) -> Result<Option<Vec<crate::game::Song>>> {
        let data: Option<String> = self
            .connection
            .query_row(
                "SELECT songs FROM song_catalog WHERE game = ?1",
                [serde_json::to_string(game)?],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| {
            let titles: Vec<(u32, String)> = serde_json::from_str(&data)?;
            Ok(titles
                .into_iter()
                .map(|(id, title)| crate::game::Song {
                    id,
                    title,
                    search_terms: vec![],
                    charts: vec![],
                })
                .collect())
        })
        .transpose()
    }

    fn json_path(&self, path: &Path) -> Result<PathBuf> {
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json")),
            "请选择 .json 备份文件"
        );
        Ok(self.path.parent().unwrap().join(path))
    }

    /// Import only reads a draft. Applying it goes through the same validation/transaction.
    pub fn import_json(&self, path: &Path) -> Result<Config> {
        let file = std::fs::File::open(self.json_path(path)?).context("无法打开 JSON 备份文件")?;
        let mut bytes = Vec::new();
        file.take(MAX_IMPORT_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_IMPORT_BYTES,
            "JSON 备份文件超过 4 MiB"
        );
        let backup: Backup = serde_json::from_slice(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "JSON 备份格式错误（第 {} 行，第 {} 列）",
                e.line(),
                e.column()
            )
        })?;
        ensure!(
            matches!(backup.format.as_str(), "bilimani" | "chart-requester")
                && matches!(backup.version, 1 | 2),
            "不支持的 JSON 备份格式或版本"
        );
        backup.config.validate()?;
        Ok(backup.config)
    }

    /// Export committed settings, never unsaved GUI drafts. Never overwrite a file.
    pub fn export_json(&self, path: &Path) -> Result<PathBuf> {
        let path = self.json_path(path)?;
        let (config, _) = read(&self.connection)?;
        let bytes = serde_json::to_vec_pretty(&Backup {
            format: "bilimani".into(),
            version: 2,
            config,
        })?;
        ensure!(
            bytes.len() as u64 <= MAX_IMPORT_BYTES,
            "配置超过 JSON 备份的 4 MiB 上限"
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("无法创建 JSON 备份；若文件已存在，请更换文件名")?;
        let result = file.write_all(&bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = result {
            let _ = std::fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(path)
    }
}

fn revision_of(connection: &Connection) -> Result<u64> {
    let revision: i64 =
        connection.query_row("SELECT revision FROM settings WHERE id = 1", [], |r| {
            r.get(0)
        })?;
    Ok(u64::try_from(revision)?)
}
fn write_profiles(connection: &Connection, profiles: &[StreamProfile]) -> Result<()> {
    connection.execute("DELETE FROM stream_profiles WHERE id != (SELECT default_profile FROM settings WHERE id = 1)", [])?;
    for profile in profiles {
        connection.execute("INSERT INTO stream_profiles (name, platform, settings, profile_key) VALUES (?1, 'bilibili', ?2, ?3)",
            params![profile.name, serde_json::to_string(&profile.bilibili)?, profile.id])?;
        let profile_id = connection.last_insert_rowid();
        for card in &profile.cards {
            connection.execute(
                "INSERT INTO stream_cards (card_id, profile_id) VALUES (?1, ?2)",
                params![card.as_str(), profile_id],
            )?;
        }
    }
    Ok(())
}
fn encode(config: &Config) -> Result<(String, String)> {
    let mut global = serde_json::to_value(config)?;
    let source = global.as_object_mut().unwrap().remove("bilibili").unwrap();
    global.as_object_mut().unwrap().remove("profiles");
    Ok((
        serde_json::to_string(&global)?,
        serde_json::to_string(&source)?,
    ))
}
fn read(connection: &Connection) -> Result<(Config, u64)> {
    let tx = connection.unchecked_transaction()?;
    let result = read_config(&tx)?;
    tx.commit()?;
    Ok(result)
}
fn read_config(connection: &Connection) -> Result<(Config, u64)> {
    // Caller holds a transaction across settings, profiles and card bindings.
    let (global, source, platform, revision): (String, String, String, i64) = connection.query_row(
        "SELECT s.settings, p.settings, p.platform, s.revision FROM settings s JOIN stream_profiles p ON p.id = s.default_profile WHERE s.id = 1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    ensure!(platform == "bilibili", "数据库包含当前版本不支持的直播平台");
    let parse = || -> Result<Config> {
        let mut value: serde_json::Value = serde_json::from_str(&global)?;
        value
            .as_object_mut()
            .context("settings must be an object")?
            .insert("bilibili".into(), serde_json::from_str(&source)?);
        Ok(serde_json::from_value(value)?)
    };
    let mut config = parse().map_err(|_| anyhow::anyhow!("配置数据库内容无效，原文件保持不变"))?;
    let mut query = connection.prepare("SELECT id, profile_key, name, platform, settings FROM stream_profiles WHERE id != (SELECT default_profile FROM settings WHERE id = 1) ORDER BY id")?;
    let rows = query.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (sql_id, id, name, platform, settings) = row?;
        ensure!(platform == "bilibili", "数据库包含当前版本不支持的直播平台");
        let mut cards_query = connection
            .prepare("SELECT card_id FROM stream_cards WHERE profile_id = ?1 ORDER BY rowid")?;
        let cards = cards_query
            .query_map([sql_id], |r| r.get::<_, String>(0))?
            .map(|r| crate::profiles::CardId::parse(&r?))
            .collect::<Result<Vec<_>>>()?;
        let bilibili =
            serde_json::from_str(&settings).map_err(|_| anyhow::anyhow!("直播档案内容无效"))?;
        config.profiles.push(StreamProfile {
            id,
            name,
            cards,
            bilibili,
        });
    }
    config.validate()?;
    Ok((config, u64::try_from(revision)?))
}
