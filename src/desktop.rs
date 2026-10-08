//! Offline configuration using the same Store, commands and egui pages as the DLL.
use crate::{
    catalog::Catalog,
    config::{Config, Game},
    game::Song,
    gui::{Action, Bridge, Command, View},
    live_config::Store,
};
use anyhow::{Context, Result, ensure};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    thread::JoinHandle,
    time::Duration,
};

pub struct Backend {
    store: Store,
    executable: PathBuf,
    config_path: PathBuf,
    view: View,
}

struct OfflineCatalog {
    songs: Option<Vec<Song>>,
    status: String,
}

impl Backend {
    /// Defaults to the EXE's directory, never the shortcut's working directory.
    pub fn open(executable: &Path, config_path: Option<&Path>) -> Result<Self> {
        let executable = std::path::absolute(executable)?;
        let config_path = std::path::absolute(config_path.map_or_else(
            || executable.parent().unwrap().join("bilimani.db"),
            Path::to_path_buf,
        ))?;
        let store = Store::open(&config_path)?;
        let mut view = View::new(store.raw.clone());
        view.standalone = true;
        view.revision = store.revision;
        view.connection.text = format!("独立配置 · {}", config_path.display());
        let mut backend = Self {
            store,
            executable,
            config_path,
            view,
        };
        let catalog = backend.catalog(&backend.store.raw.game);
        backend.update_catalog(&catalog);
        Ok(backend)
    }

    pub fn view(&self) -> View {
        self.view.clone()
    }

    fn catalog(&self, game: &Game) -> OfflineCatalog {
        let loaded = (|| -> Result<(Vec<Song>, String)> {
            if game.database_path.as_os_str().is_empty() {
                let songs = self.store.cached_catalog(game)?.context(
                    "尚无曲库缓存：请启动新版 DLL 并进入一次选曲，或在「游戏适配」填写 music_data.bin 路径",
                )?;
                Ok((songs, "上次游戏曲库缓存".into()))
            } else {
                ensure!(
                    game.module.eq_ignore_ascii_case("bm2dx.dll"),
                    "离线曲库读取仅支持 bm2dx.dll"
                );
                let path = self.config_path.parent().unwrap().join(&game.database_path);
                let mut data = Vec::new();
                const MAX: u64 = 32 * 1024 * 1024;
                std::fs::File::open(&path)
                    .with_context(|| format!("无法读取曲库 {}", path.display()))?
                    .take(MAX + 1)
                    .read_to_end(&mut data)?;
                ensure!(data.len() as u64 <= MAX, "曲库超过 32 MiB");
                let songs = crate::games::iidx::v33::catalog::parse_database(&data)?;
                Ok((songs, format!("曲库：{}", path.display())))
            }
        })();
        match loaded.and_then(|(songs, source)| {
            let catalog = Catalog::new(songs, &Default::default())?;
            Ok((catalog.songs, source))
        }) {
            Ok((songs, source)) => OfflineCatalog {
                status: format!("{source}（{} 首）；保存时校验别名。", songs.len()),
                songs: Some(songs),
            },
            Err(error) => OfflineCatalog {
                songs: None,
                status: format!("{error:#}"),
            },
        }
    }

    fn update_catalog(&mut self, catalog: &OfflineCatalog) {
        self.view.ready = catalog.songs.is_some();
        self.view.catalog_status = catalog.status.clone();
    }

    fn apply(&mut self, config: Config, revision: u64) -> Result<String> {
        let prepared = self.store.prepare(config, revision, &self.executable)?;
        let catalog = self.catalog(&prepared.raw.game);
        // Missing game files must not prevent room setup or removal of bad aliases.
        // New/changed targets need the same unambiguous resolution as the DLL.
        if prepared.raw.aliases != self.store.raw.aliases
            || prepared.raw.game != self.store.raw.game
        {
            if let Some(songs) = &catalog.songs {
                Catalog::new(songs.clone(), &prepared.raw.aliases)?;
            } else {
                ensure!(
                    prepared
                        .raw
                        .aliases
                        .iter()
                        .all(|(key, value)| self.store.raw.aliases.get(key) == Some(value))
                        && (prepared.raw.aliases.is_empty()
                            || prepared.raw.game == self.store.raw.game),
                    "无法校验新别名：{}",
                    catalog.status
                );
            }
        }
        self.store.commit(prepared)?;
        self.update_catalog(&catalog);
        self.sync_saved();
        Ok("已保存；下次启动游戏生效。游戏已运行时，请在游戏内刷新已保存设置。".into())
    }

    fn sync_saved(&mut self) {
        self.view.config = self.store.raw.clone();
        self.view.loaded_draft = Some(self.store.raw.clone());
        self.view.revision = self.store.revision;
    }

    pub fn handle(&mut self, command: Command) {
        self.view.loaded_draft = None;
        let result: Result<String> = (|| match command.action {
            Action::Apply {
                config,
                revision,
                bind_card,
            } => {
                crate::profiles::check_login(bind_card.as_ref(), None)?;
                self.apply(*config, revision)
            }
            Action::Reload => {
                let prepared = self.store.reload(&self.executable)?;
                self.store.commit(prepared)?;
                let catalog = self.catalog(&self.store.raw.game);
                self.update_catalog(&catalog);
                self.sync_saved();
                Ok("已刷新保存的设置与曲库".into())
            }
            Action::ImportJson(path) => {
                self.view.loaded_draft = Some(self.store.import_json(&path)?);
                Ok("备份已载入编辑区；检查后点击「应用并保存」".into())
            }
            Action::ExportJson(path) => {
                self.store.export_json(&path)?;
                Ok("已导出保存的设置；备份包含直播凭据，请妥善保管".into())
            }
            Action::Remove(_) | Action::Select { .. } | Action::Skip { .. } => {
                anyhow::bail!("独立配置窗口没有运行中的点歌队列")
            }
        })();
        self.view.reply = (
            command.id,
            result.unwrap_or_else(|error| format!("未应用：{error:#}")),
        );
    }

    pub fn spawn(mut self, bridge: Arc<Bridge>, commands: Receiver<Command>) -> Worker {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = std::thread::spawn(move || {
            while !signal.load(Ordering::Acquire) {
                match commands.recv_timeout(Duration::from_millis(100)) {
                    Ok(command) => {
                        self.handle(command);
                        bridge.publish(self.view());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Worker {
            stop,
            thread: Some(thread),
        }
    }
}

pub struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.thread.as_ref().is_some_and(|t| t.is_finished()),
            "配置线程已停止"
        );
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
