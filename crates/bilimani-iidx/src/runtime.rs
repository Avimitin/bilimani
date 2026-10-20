use crate::{
    catalog::Catalog,
    config::Config,
    game::GameAdapter,
    host::windows::module_path,
    service::{GameAction, Service},
    versions,
};
use anyhow::{Result, ensure};
use std::{
    ffi::c_void,
    io::Write,
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
    let path = root.join("bilimani_iidx.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 10 * 1024 * 1024) {
        let _ = std::fs::rename(&path, root.join("bilimani_iidx.log.1"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}
fn start(module: HMODULE) -> Result<()> {
    let path = module_path(module)?;
    let root = path.parent().expect("DLL directory");
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
    let config = Config::load(root)?;
    let adapter = versions::attach(&config.module, &config.controls)?;
    GAME.set(adapter)
        .map_err(|_| anyhow::anyhow!("Game already attached"))?;
    let game = GAME.get().expect("attached game");
    for (area, message) in game.startup_messages() {
        log(root, &format!("[{area}] {message}"));
    }
    let (menu, menu_commands) = bilimani_menu::Bridge::new(bilimani_menu::renderer::fonts());
    let _ = bilimani_menu::renderer::BRIDGE.set(menu.clone());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let (server, mut commands) =
                bilimani_api::Server::start(config.listen, &config.static_root).await?;
            log(
                root,
                &format!(
                    "IIDX {} / {} API: http://{}",
                    game.build().major_version(),
                    game.build().date(),
                    server.address
                ),
            );
            let mut service = Service::default();
            service.build = game.build();
            service.queue = bilimani_queue::Queue::new(
                config.queue.capacity,
                config.queue.current_timeout_seconds * 1000,
            );
            let clock = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut next_catalog = 0;
            let mut notice = String::new();
            let mut last_error = String::new();
            let mut next_diagnostics = 0;
            loop {
                tick.tick().await;
                if STOP.load(Ordering::Acquire) || game.disabled() {
                    break;
                }
                let now = clock.elapsed().as_millis() as u64;
                if service.catalog.is_none() && now >= next_catalog {
                    next_catalog = now + 1000;
                    let loaded = game
                        .catalog(&config.database_path)
                        .and_then(|songs| songs.map(Catalog::new).transpose().map_err(Into::into));
                    match loaded {
                        Ok(Some(catalog)) => {
                            log(root, &format!("Loaded {} songs", catalog.songs.len()));
                            service.catalog = Some(catalog);
                            if config.aliases_path.exists() {
                                notice = match crate::application::execute(
                                    &mut service,
                                    bilimani_api::Command::ReloadAliases,
                                    now,
                                    &config.aliases_path,
                                ) {
                                    Ok(value) => format!("已加载 {} 条别名", value["count"]),
                                    Err(error) => error.to_string(),
                                };
                                log(root, &notice);
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let text = error.to_string();
                            if text != last_error {
                                log(root, &text);
                                last_error = text;
                            }
                        }
                    }
                }
                if let Some(catalog) = service.catalog.as_mut()
                    && let Some(entries) = game.take_search_index()
                {
                    catalog.set_search_index(&entries);
                }
                let mut update = game.poll();
                if !update.snapshot.can_open_menu || !bilimani_menu::renderer::available() {
                    menu.visible.store(false, Ordering::Release);
                } else if update.toggle_menu {
                    menu.visible.fetch_xor(true, Ordering::AcqRel);
                }
                menu.navigate(std::mem::take(&mut update.navigation));
                service.menu_open = menu.visible.load(Ordering::Acquire);
                game.set_menu_open(service.menu_open);
                if let Some(action) = service.update(update, now) {
                    match action {
                        GameAction::Submit(selection) => game.submit(selection),
                        GameAction::Cancel => game.cancel_selection(),
                    }
                }
                if std::mem::take(&mut service.close_menu) {
                    menu.visible.store(false, Ordering::Release);
                    service.menu_open = false;
                    game.set_menu_open(false);
                }
                for command in menu_commands.try_iter().take(32) {
                    let command = match command {
                        bilimani_menu::Command::Remove(id) => bilimani_api::Command::Remove(id),
                        bilimani_menu::Command::Priority(id) => bilimani_api::Command::Priority(id),
                        bilimani_menu::Command::ReloadAliases => {
                            bilimani_api::Command::ReloadAliases
                        }
                    };
                    let reload = matches!(command, bilimani_api::Command::ReloadAliases);
                    notice = match crate::application::execute(
                        &mut service,
                        command,
                        now,
                        &config.aliases_path,
                    ) {
                        Ok(value) if reload => format!("已加载 {} 条别名", value["count"]),
                        Ok(_) => String::new(),
                        Err(error) => error.to_string(),
                    };
                }
                // Bound work per tick so a busy HTTP client cannot starve game polling.
                for _ in 0..64 {
                    let Ok(message) = commands.try_recv() else {
                        break;
                    };
                    if !message.reply.is_closed() {
                        let result = crate::application::execute(
                            &mut service,
                            message.command,
                            now,
                            &config.aliases_path,
                        );
                        let _ = message.reply.send(result);
                    }
                }
                menu.publish(service.menu_view(&config.aliases_path, &notice));
                server.publish(service.snapshots())?;
                if now >= next_diagnostics {
                    log(root, &format!("[game] {}", game.diagnostics()));
                    log(
                        root,
                        &format!(
                            "[menu] renderer_status={} button_status={}",
                            bilimani_menu::renderer::status(),
                            crate::host::spice::status()
                        ),
                    );
                    next_diagnostics = now + 30_000;
                }
            }
            bilimani_menu::renderer::stop();
            game.cancel_selection();
            game.stop();
            server.stop().await;
            Ok::<_, anyhow::Error>(())
        })
}
unsafe extern "system" fn worker(module: *mut c_void) -> u32 {
    let result = std::panic::catch_unwind(|| start(module));
    if let Ok(path) = module_path(module) {
        match result {
            Ok(Err(e)) => log(path.parent().unwrap(), &format!("Startup failed: {e:#}")),
            Err(_) => log(path.parent().unwrap(), "Worker panic; hooks disabled"),
            _ => {}
        }
    }
    bilimani_menu::renderer::stop();
    if let Some(game) = GAME.get() {
        game.stop();
    }
    0
}
/// No file/network IO or waits while the loader lock is held.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _: *mut c_void) -> i32 {
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
pub extern "C" fn bilimani_shutdown() {
    STOP.store(true, Ordering::Release);
    bilimani_menu::renderer::stop();
    if let Some(game) = GAME.get() {
        game.stop();
    }
}
extern "C" fn spice_destroy() {
    crate::host::spice::set(None);
    bilimani_shutdown();
    let worker = WORKER.swap(0, Ordering::AcqRel);
    if worker != 0 {
        unsafe {
            WaitForSingleObject(worker as _, 20_000);
            CloseHandle(worker as _);
        }
    }
}
#[repr(C)]
struct SdkV0 {
    size: u32,
    functions: [usize; 18],
}
type SdkInit = unsafe extern "C" fn(u32, extern "C" fn(), *mut c_void) -> i32;
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spice_sdk_entry_point(init: Option<SdkInit>) -> i32 {
    let Some(init) = init else { return 1 };
    let mut api = SdkV0 {
        size: std::mem::size_of::<SdkV0>() as u32,
        functions: [0; 18],
    };
    let result = unsafe { init(0, spice_destroy, (&mut api as *mut SdkV0).cast()) };
    if result == 0 {
        let get_button = api.functions[3];
        if get_button != 0 {
            crate::host::spice::set(Some(unsafe {
                std::mem::transmute::<usize, crate::host::spice::GetButton>(get_button)
            }));
        }
        let register = api.functions[17];
        if register != 0 {
            bilimani_menu::renderer::register(unsafe {
                std::mem::transmute::<usize, bilimani_menu::renderer::Register>(register)
            });
        }
    }
    result
}
