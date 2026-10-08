//! Render every real egui page to a hidden D3D9 target, then exercise device reset.
#[cfg(windows)]
mod support;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use bilimani::{
        gui::{Bridge, Menu, Page},
        host::menu,
    };
    use std::{path::PathBuf, sync::atomic::Ordering};
    use windows::{
        Win32::{Foundation::HWND, Graphics::Direct3D9::*, UI::WindowsAndMessaging::*},
        core::w,
    };
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "analysis/menu-preview".into());
    std::fs::create_dir_all(&output)?;
    let screen_size = if std::env::args().any(|arg| arg == "--narrow") {
        egui::vec2(900.0, 720.0)
    } else {
        egui::vec2(1280.0, 800.0)
    };
    let mut view = support::fixture();
    if std::env::args().any(|arg| arg == "--standalone") {
        view = bilimani::gui::View::new(Default::default());
        view.standalone = true;
        view.connection.text = "独立配置 · bilimani.db".into();
        view.catalog_status =
            "尚无曲库缓存：请启动新版 DLL 并进入一次选曲，或在「游戏适配」填写 music_data.bin 路径"
                .into();
    }
    if std::env::args().any(|arg| arg == "--profiles") {
        use bilimani::profiles::{CardId, StreamProfile};
        let card = CardId::parse("E0040123456789AB")?;
        let mut profile = StreamProfile::new(card.clone());
        profile.name = "主播的直播间".into();
        profile.bilibili.auth_code = "preview-only".into();
        profile.cards.push(CardId::parse("E0040123456789CD")?);
        view.player_card = Some(card);
        view.active_profile = profile.id.clone();
        view.config.profiles.push(profile);
        if std::env::args().any(|arg| arg == "--unbound") {
            view.player_card = Some(CardId::parse("E0040123456789EF")?);
            view.active_profile = bilimani::profiles::GLOBAL.into();
        }
    }
    let fonts = menu::fonts();
    let (bridge, _rx) = Bridge::new(view.clone(), fonts.clone());
    bridge.visible.store(true, Ordering::Release);
    let context = egui::Context::default();
    context.set_fonts(fonts);
    context.set_global_style(menu::style());
    let mut ui = Menu::new(&view);
    let mut painter = menu::painter::Painter::default();
    unsafe {
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("bilimani rendering check"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            1280,
            800,
            None,
            None,
            None,
            None,
        )?;
        let d3d =
            Direct3DCreate9(D3D_SDK_VERSION).ok_or_else(|| anyhow::anyhow!("D3D9 unavailable"))?;
        let mut parameters = D3DPRESENT_PARAMETERS {
            BackBufferWidth: 1280,
            BackBufferHeight: 800,
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
        for (index, page) in [
            Page::Live,
            Page::Aliases,
            Page::Requests,
            Page::Bilibili,
            Page::Output,
            Page::Controls,
            Page::Logging,
            Page::Game,
            Page::Data,
        ]
        .into_iter()
        .enumerate()
        {
            ui.page = page;
            for frame in 0..20 {
                let result = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen_size)),
                        time: Some((index * 20 + frame) as f64 / 60.0),
                        ..Default::default()
                    },
                    |root| ui.show(root, &bridge),
                );
                let meshes = context.tessellate(result.shapes, result.pixels_per_point);
                device.Clear(
                    0,
                    std::ptr::null(),
                    D3DCLEAR_TARGET as u32,
                    0xFF293440,
                    1.0,
                    0,
                )?;
                device.BeginScene()?;
                painter.paint(&device, result.textures_delta, &meshes, 1.0, 1280, 800)?;
                device.EndScene()?;
            }
            let back = device.GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO)?;
            let mut copy = None;
            device.CreateOffscreenPlainSurface(
                1280,
                800,
                D3DFMT_A8R8G8B8,
                D3DPOOL_SYSTEMMEM,
                &mut copy,
                std::ptr::null_mut(),
            )?;
            let copy = copy.unwrap();
            device.GetRenderTargetData(&back, &copy)?;
            let mut rect = D3DLOCKED_RECT::default();
            copy.LockRect(&mut rect, std::ptr::null(), D3DLOCK_READONLY as u32)?;
            let mut bmp = Vec::new();
            bmp.extend(b"BM");
            bmp.extend((54u32 + 1280 * 800 * 4).to_le_bytes());
            bmp.extend([0u8; 4]);
            bmp.extend(54u32.to_le_bytes());
            bmp.extend(40u32.to_le_bytes());
            bmp.extend(1280i32.to_le_bytes());
            bmp.extend((-800i32).to_le_bytes());
            bmp.extend(1u16.to_le_bytes());
            bmp.extend(32u16.to_le_bytes());
            bmp.extend([0u8; 24]);
            for y in 0..800 {
                bmp.extend(std::slice::from_raw_parts(
                    (rect.pBits as *const u8).offset(y * rect.Pitch as isize),
                    1280 * 4,
                ));
            }
            copy.UnlockRect()?;
            assert!(
                bmp[54..]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[..3] != [0x40, 0x34, 0x29])
                    .count()
                    > 10000,
                "Empty rendering"
            );
            std::fs::write(output.join(format!("page-{index}.bmp")), bmp)?;
            drop(back);
            drop(copy);
            // SDK INVALIDATE releases every default-pool texture before Reset.
            painter.invalidate();
            device.Reset(&mut parameters)?;
        }
        drop(painter);
        drop(device);
        drop(d3d);
        DestroyWindow(HWND(window.0))?;
    }
    println!("PASS: nine egui pages and D3D9 reset; {}", output.display());
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The D3D9 check requires Windows");
}
