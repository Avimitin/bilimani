//! Standalone live-chat preview using the production desktop host and menu.
#[cfg(windows)]
mod preview_live;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use chart_requester::{
        gui::Bridge,
        host::{desktop, menu},
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    let options = desktop::Options {
        hidden: args.iter().any(|a| a == "--hidden"),
        frames: args
            .windows(2)
            .find(|a| a[0] == "--frames")
            .map(|a| a[1].parse())
            .transpose()?,
        preview_shortcuts: true,
    };
    options.validate()?;
    let backend = preview_live::Backend::open(&args)?;
    println!("Configuration: {}", backend.config_path.display());
    let (bridge, commands) = Bridge::new(backend.view(), menu::fonts());
    let worker = backend.spawn(bridge.clone(), commands);
    println!(
        "Live UI preview. F1 toggle; F2/3 next/previous; F4/5 left/right; F6 confirm; F7 back."
    );
    desktop::run(bridge, options, || worker.check())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The D3D9 preview requires Windows");
}
