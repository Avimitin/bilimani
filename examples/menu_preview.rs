//! Standalone UI with real live chat, database matching and queue processing.
//! Uses the production egui, Win32 input and D3D9 callback; no game module.
#[cfg(windows)]
mod preview_live;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use chart_requester::{
        game::Navigation,
        gui::Bridge,
        host::menu::{self, Frame},
    };
    use std::{
        ffi::c_void,
        sync::{OnceLock, atomic::Ordering},
        time::Duration,
    };
    use windows::{
        Win32::{
            Foundation::{HWND, RECT},
            Graphics::Direct3D9::*,
            UI::WindowsAndMessaging::*,
        },
        core::{Interface, w},
    };
    type Draw = extern "C" fn(u32, *const Frame, *mut c_void);
    static DRAW: OnceLock<Draw> = OnceLock::new();
    unsafe extern "C" fn register(draw: Draw, _: *mut c_void) -> i32 {
        if DRAW.set(draw).is_ok() { 0 } else { -1 }
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let hidden = args.iter().any(|s| s == "--hidden");
    let frames = args
        .windows(2)
        .find(|a| a[0] == "--frames")
        .map(|a| a[1].parse::<u64>())
        .transpose()?;
    anyhow::ensure!(!hidden || frames.is_some(), "--hidden requires --frames N");
    let backend = preview_live::Backend::open(&args)?;
    println!("Configuration: {}", backend.config_path.display());
    let (bridge, commands) = Bridge::new(backend.view(), menu::fonts());
    let worker = backend.spawn(bridge.clone(), commands);
    bridge.visible.store(true, Ordering::Release);
    menu::BRIDGE
        .set(bridge.clone())
        .map_err(|_| anyhow::anyhow!("GUI already initialized"))?;
    menu::register(register);
    let draw = *DRAW
        .get()
        .ok_or_else(|| anyhow::anyhow!("Callback missing"))?;
    println!(
        "Live UI preview (no game jumps). F1 toggle; F2/F3 next/previous; F4/F5 left/right; F6 confirm; F7 back."
    );
    unsafe {
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(
                "Chart Requester - UI preview | F1 toggle | F2/3 down/up | F4/5 left/right | F6 confirm | F7 back"
            ),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            850,
            None,
            None,
            None,
            None,
        )?;
        let d3d =
            Direct3DCreate9(D3D_SDK_VERSION).ok_or_else(|| anyhow::anyhow!("D3D9 unavailable"))?;
        let mut rect = RECT::default();
        GetClientRect(window, &mut rect)?;
        let mut parameters = D3DPRESENT_PARAMETERS {
            BackBufferWidth: rect.right as u32,
            BackBufferHeight: rect.bottom as u32,
            BackBufferFormat: D3DFMT_A8R8G8B8,
            BackBufferCount: 1,
            SwapEffect: D3DSWAPEFFECT_DISCARD,
            hDeviceWindow: window,
            Windowed: true.into(),
            PresentationInterval: D3DPRESENT_INTERVAL_IMMEDIATE as u32,
            ..Default::default()
        };
        let mut device = None;
        d3d.CreateDevice(
            0,
            D3DDEVTYPE_HAL,
            window,
            D3DCREATE_SOFTWARE_VERTEXPROCESSING as u32,
            &mut parameters,
            &mut device,
        )?;
        let device = device.unwrap();
        if !hidden {
            let _ = ShowWindow(window, SW_SHOW);
        }
        let mut count = 0;
        let mut reset = false;
        loop {
            worker.check()?;
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if matches!(message.message, WM_KEYDOWN | WM_KEYUP)
                    && (0x70..=0x76).contains(&message.wParam.0)
                {
                    if message.message == WM_KEYDOWN && message.lParam.0 & (1 << 30) == 0 {
                        match message.wParam.0 {
                            0x70 => bridge.toggle(),
                            0x71 => bridge.navigate(vec![Navigation::Down]),
                            0x72 => bridge.navigate(vec![Navigation::Up]),
                            0x73 => bridge.navigate(vec![Navigation::Left]),
                            0x74 => bridge.navigate(vec![Navigation::Right]),
                            0x75 => bridge.navigate(vec![Navigation::Confirm]),
                            0x76 => bridge.navigate(vec![Navigation::Back]),
                            _ => {}
                        }
                    }
                    continue;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if !IsWindow(Some(window)).as_bool() || frames.is_some_and(|n| count >= n) {
                break;
            }
            GetClientRect(window, &mut rect)?;
            if rect.right == 0 || rect.bottom == 0 {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            if parameters.BackBufferWidth != rect.right as u32
                || parameters.BackBufferHeight != rect.bottom as u32
                || reset
            {
                draw(2, std::ptr::null(), std::ptr::null_mut());
                parameters.BackBufferWidth = rect.right as u32;
                parameters.BackBufferHeight = rect.bottom as u32;
                if device.Reset(&mut parameters).is_err() {
                    reset = true;
                    std::thread::sleep(Duration::from_millis(30));
                    continue;
                }
                reset = false;
            }
            let frame = Frame {
                size: std::mem::size_of::<Frame>() as u32,
                device: device.as_raw(),
                window: window.0,
                width: parameters.BackBufferWidth,
                height: parameters.BackBufferHeight,
            };
            device.Clear(
                0,
                std::ptr::null(),
                D3DCLEAR_TARGET as u32,
                0xFF293440,
                1.0,
                0,
            )?;
            device.BeginScene()?;
            draw(1, &frame, std::ptr::null_mut());
            device.EndScene()?;
            anyhow::ensure!(menu::status() == 1, "Renderer failed: {}", menu::status());
            if device
                .Present(
                    std::ptr::null(),
                    std::ptr::null(),
                    HWND::default(),
                    std::ptr::null(),
                )
                .is_err()
            {
                reset = true;
            }
            count += 1;
            std::thread::sleep(Duration::from_millis(16));
        }
        menu::stop();
        draw(3, std::ptr::null(), std::ptr::null_mut());
        drop(device);
        drop(d3d);
        if IsWindow(Some(window)).as_bool() {
            DestroyWindow(window)?;
        }
        println!("Preview closed after {count} frames.");
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The D3D9 preview requires Windows");
}
