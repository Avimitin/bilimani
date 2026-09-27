use crate::{
    catalog::Catalog,
    config::Config,
    engine::{Engine, Phase, Snapshot},
    game::GameAdapter,
    games,
    gui::{self, Action, Bridge, View},
    host::{menu, spice as sdk, windows::module_path},
    live_config::Store,
    logging::Logger,
    output::{TextFile, resolved_output},
    overlay,
    platforms::{self, Connection, Event},
};
use anyhow::{Result, ensure};
use std::{
    ffi::c_void,
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HMODULE},
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
        Threading::{CreateThread, WaitForSingleObject},
    },
};

static GAME: OnceLock<Box<dyn GameAdapter>> = OnceLock::new();

static STOP: AtomicBool = AtomicBool::new(false);
static WORKER: AtomicUsize = AtomicUsize::new(0);

fn log(root: &Path, message: &str) {
    Logger::new(root, &Config::default()).info("startup", message);
}
fn start(module: HMODULE) -> Result<()> {
    let path = module_path(module)?;
    let root = path.parent().unwrap();
    // Keep callback code mapped for the process lifetime. Shutdown stops work;
    // unmapping a DLL while another thread is returning through it is unsafe.
    let mut pinned = std::ptr::null_mut();
    ensure!(
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                start as *const () as *const u16,
                &mut pinned,
            )
        } != 0,
        "Cannot pin hook DLL"
    );
    let config_path = root.join("chart-requester.db");
    let mut store = Store::open(&config_path)?;
    let mut config = store.raw.clone().resolve(&config_path)?;
    let mut logger = Logger::new(root, &config);
    let mut view = View::new(store.raw.clone());
    view.revision = store.revision;
    let (bridge, commands) = Bridge::new(view.clone(), menu::fonts());
    let _ = menu::BRIDGE.set(bridge.clone());
    logger.info(
        "startup",
        &format!(
            "chart-requester {} started; logging={:?} danmu={} status_interval={}s",
            env!("CARGO_PKG_VERSION"),
            config.logging.level,
            config.logging.danmu,
            config.logging.status_interval_seconds
        ),
    );
    let queue_path = resolved_output(&config.output.queue_path)?;
    let interaction_path = resolved_output(&config.output.interaction_path)?;
    ensure!(
        queue_path.to_string_lossy().to_lowercase()
            != interaction_path.to_string_lossy().to_lowercase(),
        "Both OBS paths resolve to the same file"
    );
    for p in [&queue_path, &interaction_path] {
        ensure!(
            ![config_path.canonicalize()?, path.canonicalize()?].contains(p),
            "OBS output must not overwrite DLL/config"
        );
        ensure!(
            p.extension().is_some_and(|x| x.eq_ignore_ascii_case("txt")),
            "OBS output paths must end in .txt"
        );
    }
    let mut queue = TextFile::new(queue_path);
    let mut interaction = TextFile::new(interaction_path);
    queue.write("当前点歌\n暂无\n\n等待队列\n暂无\n")?;
    interaction.write("正在初始化 chart-requester…\n")?;
    let game = match games::attach(&config.game, &config.controls) {
        Ok(game) => game,
        Err(e) => {
            interaction.write(&format!("点歌功能未启用：{e}\n"))?;
            return Err(e);
        }
    };
    for (category, message) in game.startup_messages() {
        logger.info(category, &message);
    }
    GAME.set(game)
        .map_err(|_| anyhow::anyhow!("Game adapter already installed"))?;
    let game = GAME.get().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let mut overlay_error = String::new();
        let mut web = if config.overlay.enabled {
            match overlay::Server::start(config.overlay.port, &config.overlay.static_dir, &overlay::snapshot(None, &Connection::waiting(), 0)).await {
                Ok(server) => {
                    logger.info("overlay", &format!("Browser source: http://{}/queue", server.address));
                    Some(server)
                }
                Err(e) => {
                    overlay_error = format!("网页界面启动失败（端口 {}）：{e}；请在「OBS 显示」页检查端口和网页静态目录并应用，文本点歌仍可使用", config.overlay.port);
                    logger.info("overlay", &overlay_error);
                    None
                }
            }
        } else { None };
        let mut network = platforms::session::Session::new(config.source());
        let mut engine: Option<Engine> = None;
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let start = Instant::now();
        let mut plays = 0;
        let mut connection = Connection::waiting();
        let mut status = connection.text.clone();
        let mut catalog_notice = String::new();
        let mut last_error = String::new();
        let mut last_game_state = String::new();
        let mut next_status = 0;
        let mut received = 0u64;
        let mut handled = 0u64;
        let mut last_input_state = String::new();
        let mut menu_status = i32::MIN;
        let mut manual_selection: Option<(u64, u64)> = None;
        loop {
            interval.tick().await;
            if STOP.load(Ordering::Acquire) {
                break;
            }
            if game.disabled() {
                let _ = interaction.write("点歌 hook 已停止，请查看日志并重启游戏\n");
                break;
            }
            let now = start.elapsed().as_secs();
            network.poll();
            if menu_status != menu::status() {
                menu_status = menu::status();
                logger.info("gui", &format!("D3D9 menu status={menu_status} (-1=SDK v0.4 renderer unavailable; 0=registered; 1=drawing; negative=error)"));
            }
            if engine.is_none() {
                let loaded = game.catalog(&config.game.database_path);
                if !matches!(loaded, Ok(None)) {
                    let created = (|| -> Result<Engine> {
                        let catalog = Catalog::new(loaded?.unwrap(), &config.aliases)?;
                        logger.info("catalog", &format!("Loaded {} canonical songs", catalog.songs.len()));
                        Ok(Engine::new(config.clone(), catalog, game.rules()))
                    })();
                    match created {
                        Ok(mut e) => {
                            e.status = status.clone();
                            engine = Some(e);
                        }
                        Err(e) => {
                            let _ = interaction.write(&format!("无法读取曲库或别名配置：{e}\n"));
                            logger.info("catalog", &format!("Catalog initialization failed: {e}"));
                            break;
                        }
                    }
                }
            }
            // Report game state even before the catalog becomes available.
            let update = game.poll();
            let (snapshot, new_plays, ack, skip) = (update.snapshot, update.plays, update.selection_result, update.skip);
            if update.toggle_menu {
                if menu::available() { bridge.toggle(); }
                else { logger.info("gui", "Cannot open menu: upgrade Spice2x to SDK v0.4 with D3D9 callbacks"); }
            }
            bridge.navigate(update.navigation);
            // Never leave an interactive menu above gameplay or a different scene.
            if snapshot.phase != Phase::Select { bridge.visible.store(false, Ordering::Release); }
            let input_state = update.input_status;
            if input_state != last_input_state {
                logger.info("input", &input_state);
                last_input_state = input_state;
            }
            let game_state = format!("phase={:?} mode={:?} select_entries={} plays={new_plays}", snapshot.phase, snapshot.mode, snapshot.epoch);
            if game_state != last_game_state {
                logger.info("game", &game_state);
                last_game_state = game_state;
            }
            if let Some(e) = engine.as_mut() {
                if let Some(entries) = game.take_search_index() {
                    let added = e.catalog.set_search_index(&entries);
                    logger.info("catalog", &format!("Native search index captured: entries={} additional_terms={added}; fuzzy matching enabled", entries.len()));
                }
                // Acknowledge the jump before consuming the request on gameplay start.
                if let Some(ack) = ack {
                    logger.info("jump", &format!("ack token={} result={:?}", ack.token, ack.result));
                    if manual_selection.is_some_and(|(token, _)| token == ack.token) {
                        let (_, command_id) = manual_selection.take().unwrap();
                        let message = match &ack.result {
                            Some(Ok(())) => {
                                bridge.visible.store(false, Ordering::Release);
                                game.set_menu_open(false);
                                "已定位到选中曲目".to_owned()
                            }
                            Some(Err(error)) => format!("无法定位：{error}；曲目仍保留在队列中"),
                            None => "游戏界面已变化，曲目仍保留在队列中，请重新选择".to_owned(),
                        };
                        view.reply = (command_id, message);
                    }
                    e.jump_result(ack.token, ack.result, now);
                }
                if plays != new_plays {
                    e.observe(
                        Snapshot {
                            phase: Phase::Playing,
                            ..snapshot
                        },
                        now,
                    );
                    plays = new_plays;
                }
                e.observe(snapshot, now);
                if let Some(event) = skip {
                    let accepted = e.skip_current(event.token, event.epoch, now);
                    logger.info("input", &format!("{} token={} epoch={} skipped={accepted}", event.description, event.token, event.epoch));
                }
            }
            // Commands are handled only on this worker, never from a graphics callback.
            for _ in 0..8 {
                let Ok(command) = commands.try_recv() else { break; };
                view.loaded_draft = None;
                let result: Result<String> = async {
                    match command.action {
                        Action::ImportJson(path) => {
                            view.loaded_draft = Some(store.import_json(&path)?);
                            Ok("备份已载入编辑区；检查后点击「应用并保存」".into())
                        }
                        Action::ExportJson(path) => {
                            store.export_json(&path)?;
                            Ok("已导出保存的设置；备份包含直播凭据，请妥善保管".into())
                        }
                        Action::Remove(token) => {
                            ensure!(engine.as_mut().is_some_and(|e| e.remove_queued(token, now)), "该条目已变化或正在定位，请刷新后重试");
                            Ok("已删除等待点歌".into())
                        }
                        Action::Select { token, epoch } => {
                            let jump = engine.as_mut()
                                .and_then(|e| e.select_queued(token, epoch))
                                .ok_or_else(|| anyhow::anyhow!("曲目、游戏模式或选曲界面已变化，或正在定位，请重试"))?;
                            logger.info("jump", &format!("manual submit token={} song_id={} song={:?} mode={:?} chart={:?} epoch={}",
                                jump.request.token, jump.request.song.id, jump.request.song.title, jump.request.mode, jump.request.chart, jump.epoch));
                            manual_selection = Some((token, command.id));
                            game.submit(jump.selection());
                            Ok("正在定位选中曲目…".into())
                        }
                        Action::Skip { token, epoch } => {
                            ensure!(engine.as_mut().is_some_and(|e| e.dismiss_current(token, epoch, now)), "当前点歌或游戏界面已变化，请刷新后重试");
                            Ok("已跳过当前点歌".into())
                        }
                        action => {
                            let prepared = match action {
                                Action::Apply { config, revision } => store.prepare(*config, revision, &path)?,
                                Action::Reload => store.reload(&path)?,
                                _ => unreachable!(),
                            };
                            // Validate aliases before touching disk, engine state, or services.
                            if let Some(e) = &engine { Catalog::new(e.catalog.songs.clone(), &prepared.raw.aliases)?; }
                            else { ensure!(prepared.raw.aliases == store.raw.aliases, "曲库尚未就绪，暂时不能验证新的别名"); }
                            let next = &prepared.resolved;
                            let replace_web = next.overlay.enabled != config.overlay.enabled
                                || next.overlay.port != config.overlay.port
                                || (next.overlay.enabled && web.is_none());
                            let replacement = if replace_web && next.overlay.enabled {
                                Some(overlay::Server::start(next.overlay.port, &next.overlay.static_dir, &overlay::snapshot(engine.as_ref(), &connection, now)).await?)
                            } else { None };
                            // Changing only the asset directory reuses the existing listener.
                            let static_files = if !replace_web && next.overlay.enabled
                                && next.overlay.static_dir != config.overlay.static_dir {
                                Some(overlay::StaticFiles::open(&next.overlay.static_dir).await?)
                            } else { None };
                            let source_changed = next.source() != config.source();
                            // A failed atomic save drops the prepared web server, leaving old state intact.
                            let next = store.commit(prepared)?;
                            if let Some(e) = engine.as_mut() {
                                e.catalog.update_aliases(&next.aliases)?;
                                e.config = next.clone().into();
                            }
                            if replace_web {
                                if let Some(server) = web.as_mut() { server.stop().await; }
                                web = replacement;
                                overlay_error.clear();
                            } else if let (Some(server), Some(files)) = (web.as_ref(), static_files) {
                                server.set_static_files(files);
                            }
                            if next.output.queue_path != config.output.queue_path { queue = TextFile::new(next.output.queue_path.clone()); }
                            if next.output.interaction_path != config.output.interaction_path { interaction = TextFile::new(next.output.interaction_path.clone()); }
                            game.configure_controls(&next.controls);
                            logger.reconfigure(root, &next);
                            if source_changed {
                                network.replace(next.source());
                                connection = Connection { connected: false, text: "配置已更新，正在重新连接弹幕…".into() };
                                status = connection.text.clone();
                                if let Some(e) = engine.as_mut() { e.status = status.clone(); }
                            }
                            config = next;
                            view.config = store.raw.clone();
                            view.loaded_draft = Some(store.raw.clone());
                            view.revision = store.revision;
                            next_status = 0;
                            logger.info("config", "Live configuration applied; queue preserved");
                            Ok(if store.restart_required() { "配置已保存；游戏模块或曲库路径将在下次启动生效" } else { "配置已应用；现有队列保持不变" }.into())
                        }
                    }
                }.await;
                // Error text never contains serialized configuration or credential values.
                // Keep the panel pending until the game acknowledges an explicit pick.
                if manual_selection.is_none_or(|(_, id)| id != command.id) {
                    view.reply = (command.id, result.unwrap_or_else(|e| format!("未应用：{e}")));
                }
            }
            for _ in 0..128 {
                let Some(event) = network.try_recv() else {
                    break;
                };
                match event {
                    Event::Status(s) => {
                        logger.info("connection", &s.text);
                        status = s.text.clone();
                        connection = s;
                        if let Some(e) = engine.as_mut() {
                            e.status = status.clone();
                        }
                    }
                    Event::Chat(c) => {
                        gui::record_chat(&mut view.chats, &c, now);
                        received += 1;
                        logger.danmu(&format!("received seq={received} user={:?} name={:?} text={:?}", c.user, c.name, c.text));
                        if let Some(e) = engine.as_mut() {
                            let user = c.user.clone();
                            let outcome = e.chat(c.clone(), now);
                            gui::record_processing(&mut view.processing, &c, outcome, now);
                            if !outcome.starts_with("ignored_") { handled += 1; }
                            logger.debug("request", &format!("seq={received} result={outcome} queued={} pending={}", e.queue.len(), e.pending.len()));
                            if outcome == "awaiting_selection" && let Some(p) = e.pending.get(&user) {
                                logger.info("request", &format!("seq={received} selection_deadline={}s candidates={:?}", p.until.saturating_sub(now),
                                    p.songs.iter().map(|s| (s.id, &s.title)).collect::<Vec<_>>()));
                            }
                        } else {
                            gui::record_processing(&mut view.processing, &c, "ignored_catalog_not_ready", now);
                            logger.info("request", &format!("seq={received} result=ignored_catalog_not_ready"));
                            catalog_notice = "游戏曲库尚未就绪，请进入选曲后重新点歌".into();
                        }
                    }
                    Event::Diagnostic(s) => logger.debug("transport", &s),
                }
            }
            let (q, mut i) = if let Some(e) = engine.as_mut() {
                if !bridge.visible.load(Ordering::Acquire) && let Some(j) = e.next_jump(now) {
                    logger.info("jump", &format!("submit token={} song_id={} song={:?} mode={:?} chart={:?} epoch={}",
                        j.request.token, j.request.song.id, j.request.song.title, j.request.mode, j.request.chart, j.epoch));
                    game.submit(j.selection());
                }
                for message in e.take_diagnostics() { logger.info("request", &message); }
                e.render(now)
            } else {
                (
                    String::from("当前点歌\n暂无\n\n等待游戏进入选曲…\n"),
                    format!("{status}\n{catalog_notice}\n等待游戏曲库…\n"),
                )
            };
            view.connection = connection.clone();
            view.update_engine(engine.as_ref());
            bridge.publish(view.clone());
            game.set_menu_open(bridge.visible.load(Ordering::Acquire));
            game.set_skip_target(engine.as_ref().and_then(|e| e.current.as_ref().map(|c| c.request.token)));
            if web.as_ref().is_some_and(overlay::Server::is_finished) {
                overlay_error = "网页界面服务已停止，请在控制台点击「应用并保存」重试；文本点歌仍可使用".into();
                logger.info("overlay", &overlay_error);
                web = None;
            }
            if let Some(server) = &web {
                server.publish(&overlay::snapshot(engine.as_ref(), &connection, now));
            }
            if !overlay_error.is_empty() { i.push_str(&format!("\n{overlay_error}\n")); }
            if now >= next_status {
                let activity = engine.as_ref().map_or("waiting_for_catalog", Engine::activity);
                logger.info("status", &format!("connection={:?} received={received} handled={handled} activity={activity} queue={} pending={} current={:?}",
                    status, engine.as_ref().map_or(0, |e| e.queue.len()), engine.as_ref().map_or(0, |e| e.pending.len()),
                    engine.as_ref().and_then(|e| e.current.as_ref().map(|c| c.request.token))));
                logger.info("game", &game.diagnostics());
                next_status = now + config.logging.status_interval_seconds;
            }
            let write_result = queue.write(&q).and_then(|_| interaction.write(&i));
            if let Err(err) = write_result {
                let message = format!("{err:#}");
                if message != last_error {
                    logger.info("output", &message);
                    last_error = message;
                }
            } else {
                last_error.clear();
            }
        }
        if let Some(game) = GAME.get() { game.stop(); }
        if let Some(server) = web.as_mut() { server.stop().await; }
        menu::stop();
        network.stop().await;
        logger.info("shutdown", "Network worker stopped");
        let _ = queue.write("点歌已停止\n");
        if STOP.load(Ordering::Acquire) {
            let _ = interaction.write("弹幕连接已关闭\n");
        }
    });
    Ok(())
}
unsafe extern "system" fn worker(module: *mut c_void) -> u32 {
    let result = std::panic::catch_unwind(|| start(module));
    if let Ok(path) = module_path(module) {
        match result {
            Ok(Err(e)) => log(path.parent().unwrap(), &format!("Startup failed: {e}")),
            Err(_) => log(path.parent().unwrap(), "Worker panic; hook disabled"),
            _ => {}
        }
    }
    if let Some(game) = GAME.get() {
        game.stop();
    }
    0
}
/// LoadLibrary entry point. No network, file IO, waits or game calls under loader lock.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == 1 {
        let thread = unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(worker),
                module,
                0,
                std::ptr::null_mut(),
            )
        };
        if thread.is_null() {
            return 0;
        }
        WORKER.store(thread as usize, Ordering::Release);
    } else if reason == 0 {
        STOP.store(true, Ordering::Release);
    }
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn chart_requester_shutdown() {
    STOP.store(true, Ordering::Release);
    if let Some(game) = GAME.get() {
        game.stop();
    }
    sdk::set(None);
    menu::stop();
}
extern "C" fn spice_destroy() {
    chart_requester_shutdown();
    let worker = WORKER.swap(0, Ordering::AcqRel);
    if worker != 0 {
        unsafe {
            WaitForSingleObject(worker as _, 20000);
            CloseHandle(worker as _);
        }
    }
}
/// Spice SDK v0.4 table: shutdown, read-only buttons, and D3D9 registration.
/// Older hosts populate only their supported prefix; absent render callbacks are optional.
#[repr(C)]
struct SdkV0 {
    size: u32,
    functions: [usize; 18],
}
type SdkInit = unsafe extern "C" fn(u32, extern "C" fn(), *mut c_void) -> i32;
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spice_sdk_entry_point(init: Option<SdkInit>) -> i32 {
    let Some(init) = init else {
        return 1;
    };
    let mut api = SdkV0 {
        size: std::mem::size_of::<SdkV0>() as u32,
        functions: [0; 18],
    };
    let status = unsafe { init(0, spice_destroy, (&mut api as *mut SdkV0).cast()) };
    if status == 0 && api.functions[3] != 0 && !STOP.load(Ordering::Acquire) {
        sdk::set(Some(unsafe {
            std::mem::transmute::<usize, sdk::GetButton>(api.functions[3])
        }));
    }
    if status == 0 && api.functions[17] != 0 && !STOP.load(Ordering::Acquire) {
        menu::register(unsafe { std::mem::transmute::<usize, menu::Register>(api.functions[17]) });
    }
    status
}
