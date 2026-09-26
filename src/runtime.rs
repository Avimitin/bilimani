use crate::{
    bilibili::{self, Event},
    catalog::{Catalog, parse_database},
    config::Config,
    engine::{Engine, Phase, Snapshot},
    native,
    output::{TextFile, resolved_output},
};
use anyhow::{Result, ensure};
use std::{
    ffi::c_void,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
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

static STOP: AtomicBool = AtomicBool::new(false);
static WORKER: AtomicUsize = AtomicUsize::new(0);

fn log(root: &Path, message: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("chart-requester.log"))
    {
        let _ = writeln!(f, "{message}");
    }
}
fn start(module: HMODULE) -> Result<()> {
    let path = native::module_path(module)?;
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
    let config_path = root.join("chart-requester.toml");
    if !config_path.exists() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)?;
        file.write_all(include_bytes!("../chart-requester.example.toml"))?;
    }
    let config = Config::load(&config_path)?;
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
    if let Err(e) = native::install(&config.game.module) {
        interaction.write(&format!("点歌功能未启用：{e}\n"))?;
        return Err(e);
    }
    log(
        root,
        "Native hooks installed for the verified IIDX 33 build; waiting for song select.",
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel(512);
        let (shutdown, stop) = tokio::sync::watch::channel(false);
        let network = tokio::spawn(bilibili::run(config.bilibili.clone(), tx, stop));
        let mut engine: Option<Engine> = None;
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let start = Instant::now();
        let mut plays = 0;
        let mut status = String::from("等待弹幕连接");
        let mut last_error = String::new();
        loop {
            interval.tick().await;
            if STOP.load(Ordering::Acquire) {
                break;
            }
            if native::DISABLED.load(Ordering::Acquire) {
                let _ = interaction.write("点歌 hook 已停止，请查看日志并重启游戏\n");
                break;
            }
            let now = start.elapsed().as_secs();
            if engine.is_none() {
                let data = if !config.game.database_path.as_os_str().is_empty() {
                    Some(std::fs::read(&config.game.database_path))
                } else {
                    native::LIVE_DATABASE.get().map(|d| Ok(d.clone()))
                };
                if let Some(data) = data {
                    let created = (|| -> Result<Engine> {
                        let catalog = Catalog::new(parse_database(&data?)?, &config.aliases)?;
                        log(
                            root,
                            &format!("Loaded {} canonical songs", catalog.songs.len()),
                        );
                        Ok(Engine::new(config.clone(), catalog))
                    })();
                    match created {
                        Ok(mut e) => {
                            e.status = status.clone();
                            engine = Some(e);
                        }
                        Err(e) => {
                            let _ = interaction.write(&format!("无法读取曲库或别名配置：{e}\n"));
                            log(root, &format!("Catalog initialization failed: {e}"));
                            break;
                        }
                    }
                }
            }
            if let Some(e) = engine.as_mut() {
                // Take acknowledgements before observing gameplay: a jump and a
                // subsequent play can both happen between two worker polls.
                let (snapshot, new_plays, ack) = {
                    let mut m = native::MAILBOX.lock().unwrap();
                    (m.snapshot, m.plays, m.ack.take())
                };
                if let Some(ack) = ack {
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
            }
            for _ in 0..128 {
                let Ok(event) = rx.try_recv() else {
                    break;
                };
                match event {
                    Event::Status(s) => {
                        status = s;
                        if let Some(e) = engine.as_mut() {
                            e.status = status.clone();
                        }
                    }
                    Event::Chat(c) => {
                        if let Some(e) = engine.as_mut() {
                            e.chat(c, now);
                        } else {
                            status = "游戏曲库尚未就绪，请进入选曲后重新点歌".into();
                        }
                    }
                }
            }
            let (q, i) = if let Some(e) = engine.as_mut() {
                if let Some(j) = e.next_jump(now) {
                    native::MAILBOX.lock().unwrap().command = Some(j);
                }
                e.render(now)
            } else {
                (
                    String::from("当前点歌\n暂无\n\n等待游戏进入选曲…\n"),
                    format!("{status}\n等待游戏曲库…\n"),
                )
            };
            let write_result = queue.write(&q).and_then(|_| interaction.write(&i));
            if let Err(err) = write_result {
                let message = format!("{err:#}");
                if message != last_error {
                    log(root, &message);
                    last_error = message;
                }
            } else {
                last_error.clear();
            }
        }
        native::DISABLED.store(true, Ordering::Release);
        let _ = shutdown.send(true);
        if !network.is_finished() {
            let _ = tokio::time::timeout(Duration::from_secs(18), network).await;
        }
        let _ = queue.write("点歌已停止\n");
        if STOP.load(Ordering::Acquire) {
            let _ = interaction.write("弹幕连接已关闭\n");
        }
    });
    Ok(())
}
unsafe extern "system" fn worker(module: *mut c_void) -> u32 {
    let result = std::panic::catch_unwind(|| start(module));
    if let Ok(path) = native::module_path(module) {
        match result {
            Ok(Err(e)) => log(path.parent().unwrap(), &format!("Startup failed: {e}")),
            Err(_) => log(path.parent().unwrap(), "Worker panic; hook disabled"),
            _ => {}
        }
    }
    native::DISABLED.store(true, Ordering::Release);
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
    native::DISABLED.store(true, Ordering::Release);
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
/// Spice SDK v0.1 ABI, used solely for orderly shutdown. Older Spice versions
/// without this entry point still load through DllMain; sessions then expire by TTL.
#[repr(C)]
struct SdkV0 {
    size: u32,
    functions: [usize; 13],
}
type SdkInit = unsafe extern "C" fn(u32, extern "C" fn(), *mut c_void) -> i32;
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spice_sdk_entry_point(init: Option<SdkInit>) -> i32 {
    let Some(init) = init else {
        return 1;
    };
    let mut api = SdkV0 {
        size: std::mem::size_of::<SdkV0>() as u32,
        functions: [0; 13],
    };
    unsafe { init(0, spice_destroy, (&mut api as *mut SdkV0).cast()) }
}
