//! Standalone live-chat worker: real transport, database and request engine.
use anyhow::{Context, Result, ensure};
use chart_requester::{
    catalog::Catalog,
    engine::{Engine, Phase, Snapshot},
    games::iidx,
    gui::{Action, Bridge, Command, View, record_chat, record_processing},
    live_config::Store,
    logging::Logger,
    platforms::{Connection, Event, session::Session},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct Backend {
    store: Store,
    config: chart_requester::config::Config,
    engine: Engine,
    pub config_path: PathBuf,
}
impl Backend {
    pub fn open(args: &[String]) -> Result<Self> {
        let option = |name| {
            args.windows(2)
                .find(|a| a[0] == name)
                .map(|a| a[1].as_str())
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let config_path = if let Some(path) = option("--config") {
            std::path::absolute(path)?
        } else {
            root.join("analysis/menu-preview/preview.db")
        };
        let store = Store::open(&config_path)?;
        let config = store.raw.clone().resolve(&config_path)?;
        let database = if let Some(path) = option("--database") {
            PathBuf::from(path)
        } else if !config.game.database_path.as_os_str().is_empty() {
            config.game.database_path.clone()
        } else {
            [
                root.join("analysis/music_data_1.bin"),
                root.join("analysis/music_data.bin"),
            ]
            .into_iter()
            .find(|p| p.is_file())
            .context("独立预览需要曲库：请用 --database 指定 IIDX 33 的 music_data.bin")?
        };
        let songs = iidx::v33::catalog::parse_database(
            &std::fs::read(&database).context("无法读取预览曲库文件")?,
        )?;
        let catalog = Catalog::new(songs, &config.aliases)?;
        let mode = match option("--mode")
            .unwrap_or("SP")
            .to_ascii_uppercase()
            .as_str()
        {
            "SP" => iidx::SP,
            "DP" => iidx::DP,
            _ => anyhow::bail!("--mode 仅支持 SP 或 DP"),
        };
        println!(
            "曲库：{}（{} 首歌曲，模式 {mode:?}）",
            database.display(),
            catalog.songs.len()
        );
        let mut engine = Engine::new(config.clone(), catalog, &iidx::RULES);
        engine.observe(
            Snapshot {
                phase: Phase::Select,
                mode: Some(mode),
                epoch: 1,
                can_skip: false,
            },
            0,
        );
        Ok(Self {
            store,
            config,
            engine,
            config_path,
        })
    }
    pub fn view(&self) -> View {
        let mut view = View::new(self.store.raw.clone());
        view.revision = self.store.revision;
        view.update_engine(Some(&self.engine));
        view
    }
    pub fn spawn(self, bridge: Arc<Bridge>, commands: Receiver<Command>) -> Worker {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(self.run(bridge, commands, signal))
        });
        Worker {
            stop,
            thread: Some(thread),
        }
    }
    async fn run(
        mut self,
        bridge: Arc<Bridge>,
        commands: Receiver<Command>,
        stop: Arc<AtomicBool>,
    ) -> Result<()> {
        let log_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("analysis/menu-preview");
        std::fs::create_dir_all(&log_root)?;
        let mut logger = Logger::new(&log_root, &self.config);
        let executable = std::env::current_exe()?;
        let mut network = Session::new(self.config.source());
        let mut view = self.view();
        let clock = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        logger.info(
            "preview",
            "Standalone live chat started; no game module or song jumps",
        );
        while !stop.load(Ordering::Acquire) {
            tick.tick().await;
            let now = clock.elapsed().as_secs();
            network.poll();
            self.engine.expire(now);
            for _ in 0..8 {
                let Ok(command) = commands.try_recv() else {
                    break;
                };
                view.loaded_draft = None;
                let result: Result<&str> = (|| match command.action {
                    Action::ImportJson(path) => {
                        view.loaded_draft = Some(self.store.import_json(&path)?);
                        Ok("备份已载入编辑区；检查后点击「应用并保存」")
                    }
                    Action::ExportJson(path) => {
                        self.store.export_json(&path)?;
                        Ok("已导出保存的设置；备份包含直播凭据，请妥善保管")
                    }
                    Action::Remove(token) => {
                        ensure!(
                            self.engine.remove_queued(token, now),
                            "该条目已变化，请重试"
                        );
                        Ok("已删除等待点歌")
                    }
                    Action::Skip { .. } => {
                        anyhow::bail!("独立模式不向游戏跳歌，没有正在游玩的曲目")
                    }
                    Action::Select { .. } => {
                        anyhow::bail!("独立模式无法向游戏跳歌，请在游戏内选择队列曲目")
                    }
                    action => {
                        let prepared = match action {
                            Action::Apply {
                                config,
                                revision,
                                bind_card,
                            } => {
                                chart_requester::profiles::check_login(
                                    bind_card.as_ref(),
                                    view.player_card.as_ref(),
                                )?;
                                self.store.prepare(*config, revision, &executable)?
                            }
                            Action::Reload => self.store.reload(&executable)?,
                            _ => unreachable!(),
                        };
                        Catalog::new(self.engine.catalog.songs.clone(), &prepared.raw.aliases)?;
                        let reconnect = prepared.resolved.source() != self.config.source();
                        let next = self.store.commit(prepared)?;
                        self.engine.catalog.update_aliases(&next.aliases)?;
                        self.engine.config = next.clone().into();
                        logger.reconfigure(&log_root, &next);
                        if reconnect {
                            network.replace(next.source());
                            view.room = None;
                            view.connection = Connection {
                                connected: false,
                                text: "配置已更新，正在重新连接弹幕…".into(),
                            };
                            self.engine.status = view.connection.text.clone();
                        }
                        self.config = next;
                        view.config = self.store.raw.clone();
                        view.loaded_draft = Some(self.store.raw.clone());
                        view.revision = self.store.revision;
                        Ok(if self.store.restart_required() {
                            "配置已保存；游戏模块或曲库路径将在下次启动生效"
                        } else {
                            "配置已应用并保存；独立模式仅运行弹幕、匹配和队列"
                        })
                    }
                })();
                view.reply = (
                    command.id,
                    result
                        .map(str::to_owned)
                        .unwrap_or_else(|e| format!("未应用：{e}")),
                );
            }
            for _ in 0..128 {
                let Some(event) = network.try_recv() else {
                    break;
                };
                match event {
                    Event::RoomInfo(room) => view.room = Some(room),
                    Event::Status(status) => {
                        logger.info("connection", &status.text);
                        self.engine.status = status.text.clone();
                        view.connection = status;
                    }
                    Event::Chat(chat) => {
                        record_chat(&mut view.chats, &chat, now);
                        logger.danmu(&format!(
                            "user={:?} name={:?} text={:?}",
                            chat.user, chat.name, chat.text
                        ));
                        let result = self.engine.chat(chat.clone(), now);
                        record_processing(&mut view.processing, &chat, result, now);
                        logger.debug("request", result);
                    }
                    Event::Diagnostic(text) => logger.debug("transport", &text),
                }
            }
            for message in self.engine.take_diagnostics() {
                logger.info("request", &message);
            }
            view.update_engine(Some(&self.engine));
            bridge.publish(view.clone());
        }
        network.stop().await;
        logger.info("preview", "Standalone live chat stopped");
        Ok(())
    }
}

pub struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
}
impl Worker {
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.thread.as_ref().is_some_and(|t| t.is_finished()),
            "独立预览的弹幕工作线程已停止"
        );
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("Preview worker: {error}"),
                Err(_) => eprintln!("Preview worker panicked"),
            }
        }
    }
}
