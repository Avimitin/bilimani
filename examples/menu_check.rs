//! Render every real egui page to a hidden D3D9 target, then exercise device reset.
#[cfg(windows)]
mod support;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use chart_requester::{
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
    let view = support::fixture();
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
            w!("Chart Requester rendering check"),
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
        ]
        .into_iter()
        .enumerate()
        {
            ui.page = page;
            for frame in 0..20 {
                let result = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        time: Some((index * 20 + frame) as f64 / 60.0),
                        ..Default::default()
                    },
                    |root| ui.show(root.ctx(), &bridge),
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
    println!(
        "PASS: eight egui pages and D3D9 reset; {}",
        output.display()
    );
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The D3D9 check requires Windows");
}
