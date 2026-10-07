#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        if std::env::args_os().any(|arg| arg == "--hidden") {
            eprintln!("无法打开 Chart Requester：{error:#}");
            std::process::exit(1);
        }
        use windows::{
            Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MessageBoxW},
            core::{PCWSTR, w},
        };
        let text: Vec<u16> = format!("无法打开 Chart Requester：\n{error:#}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                w!("Chart Requester"),
                MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> anyhow::Result<()> {
    use chart_requester::{
        desktop::Backend,
        gui::Bridge,
        host::{desktop, menu},
    };
    let mut args = std::env::args_os().skip(1);
    let mut config = None;
    let mut options = desktop::Options::default();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => {
                config = Some(std::path::PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--config 需要数据库路径"))?,
                ))
            }
            Some("--hidden") => options.hidden = true,
            Some("--frames") => {
                options.frames = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--frames 需要帧数"))?
                        .to_string_lossy()
                        .parse()?,
                )
            }
            _ => anyhow::bail!("未知参数：{}", arg.to_string_lossy()),
        }
    }
    options.validate()?;
    let backend = Backend::open(&std::env::current_exe()?, config.as_deref())?;
    let (bridge, commands) = Bridge::new(backend.view(), menu::fonts());
    let worker = backend.spawn(bridge.clone(), commands);
    desktop::run(bridge, options, || worker.check())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Chart Requester 配置窗口需要 Windows");
}
