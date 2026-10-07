//! Spice SDK v0.4 D3D9 renderer. No game addresses or controller IDs live here.
//! SDK serializes callbacks, brackets BeginScene/EndScene, and restores all state.
mod input;
pub mod painter;
use crate::gui::{Bridge, Menu};
use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    time::Instant,
};
use windows::{
    Win32::{Foundation::HWND, Graphics::Direct3D9::IDirect3DDevice9},
    core::Interface,
};

pub static BRIDGE: OnceLock<Arc<Bridge>> = OnceLock::new();
static RENDERER: Mutex<Option<Renderer>> = Mutex::new(None);
static STOPPED: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicI32 = AtomicI32::new(-1);
static AVAILABLE: AtomicBool = AtomicBool::new(false);
struct Renderer {
    context: egui::Context,
    menu: Menu,
    painter: painter::Painter,
    clock: Instant,
}
// Texture interfaces are touched only in serialized SDK graphics callbacks,
// including reset/teardown; the worker never acquires or destroys this state.
unsafe impl Send for Renderer {}
#[repr(C)]
pub struct Frame {
    pub size: u32,
    pub device: *mut c_void,
    pub window: *mut c_void,
    pub width: u32,
    pub height: u32,
}
pub type Register =
    unsafe extern "C" fn(extern "C" fn(u32, *const Frame, *mut c_void), *mut c_void) -> i32;
pub fn register(register: Register) {
    if !STOPPED.load(Ordering::Acquire) {
        let status = unsafe { register(callback, std::ptr::null_mut()) };
        STATUS.store(status, Ordering::Release);
        AVAILABLE.store(status == 0, Ordering::Release);
    }
}
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Acquire) && !STOPPED.load(Ordering::Acquire)
}
pub fn status() -> i32 {
    STATUS.load(Ordering::Acquire)
}
pub fn stop() {
    STOPPED.store(true, Ordering::Release);
    if let Some(b) = BRIDGE.get() {
        b.visible.store(false, Ordering::Release);
    }
    input::restore();
}
pub fn fonts() -> egui::FontDefinitions {
    let mut fonts = crate::gui::design::fonts();
    let root = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:/Windows".into())
        .join("Fonts");
    // Read once on the worker. System fonts are never copied into our release.
    for name in ["msyh.ttc", "msgothic.ttc", "YuGothM.ttc", "simsun.ttc"] {
        if let Ok(bytes) = std::fs::read(root.join(name)) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            for family in fonts.families.values_mut() {
                family.push(name.into());
            }
        }
    }
    fonts
}
pub fn style() -> egui::Style {
    crate::gui::design::style()
}
extern "C" fn callback(event: u32, frame: *const Frame, _: *mut c_void) {
    let result = std::panic::catch_unwind(|| {
        // The host guarantees serialization, but try_lock also rejects reentry.
        let mut state = match RENDERER.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        if event == 2 || event == 3 {
            if let Some(renderer) = state.as_mut() {
                renderer.painter.invalidate();
            }
            if event == 3 {
                *state = None;
                input::restore();
            }
            return;
        }
        if STOPPED.load(Ordering::Acquire) {
            return;
        }
        let Some(bridge) = BRIDGE.get() else {
            return;
        };
        let Some(frame) = (unsafe { frame.as_ref() }) else {
            return;
        };
        if frame.size < std::mem::size_of::<Frame>() as u32
            || frame.device.is_null()
            || frame.window.is_null()
            || frame.width == 0
            || frame.height == 0
        {
            return;
        }
        // Borrow without AddRef/Release: the SDK owns the device for this callback.
        let Some(device) = (unsafe { IDirect3DDevice9::from_raw_borrowed(&frame.device) }) else {
            return;
        };
        if state.is_none() {
            let context = egui::Context::default();
            context.set_fonts(bridge.fonts.clone());
            context.set_global_style(style());
            *state = Some(Renderer {
                context,
                menu: Menu::new(&bridge.snapshot()),
                painter: painter::Painter::default(),
                clock: Instant::now(),
            });
        }
        let renderer = state.as_mut().unwrap();
        if event != 1 {
            return;
        }
        if let Err(error) = input::install(HWND(frame.window)) {
            STATUS.store(error, Ordering::Release);
            return;
        }
        if !bridge.visible.load(Ordering::Acquire) {
            input::clear();
            bridge.take_navigation();
            renderer.menu.hidden();
            return;
        }
        let standalone = bridge.snapshot().standalone;
        let scale = if standalone {
            1.0
        } else {
            (frame.height as f32 / 900.0).clamp(0.8, 2.0)
        };
        renderer.context.set_pixels_per_point(scale);
        let mut raw = input::take(
            HWND(frame.window),
            frame.width,
            frame.height,
            scale,
            renderer.clock.elapsed().as_secs_f64(),
        );
        if standalone {
            raw.viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .maximized = Some(unsafe {
                windows::Win32::UI::WindowsAndMessaging::IsZoomed(HWND(frame.window)).as_bool()
            });
        }
        renderer
            .menu
            .controller_input(&renderer.context, &mut raw, bridge.take_navigation());
        let output = renderer.context.run_ui(raw, |ui| {
            renderer.menu.show(ui, bridge);
            let ctx = ui.ctx();
            // Game windows often hide the OS cursor. Paint a cursor in the same target.
            if !standalone && let Some(pos) = ctx.pointer_hover_pos() {
                ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("menu-cursor"),
                ))
                .add(egui::Shape::convex_polygon(
                    vec![
                        pos,
                        pos + egui::vec2(0.0, 18.0),
                        pos + egui::vec2(12.0, 12.0),
                    ],
                    egui::Color32::WHITE,
                    egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
                ));
            }
        });
        if standalone {
            crate::host::desktop::viewport_commands(HWND(frame.window), &output);
        }
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                let _ = clipboard_win::set_clipboard_string(text);
            }
        }
        input::ime_position(
            HWND(frame.window),
            output.platform_output.ime.as_ref(),
            scale,
        );
        let meshes = renderer
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let result = unsafe {
            renderer.painter.paint(
                device,
                output.textures_delta,
                &meshes,
                output.pixels_per_point,
                frame.width,
                frame.height,
            )
        };
        if let Err(e) = result {
            STATUS.store(e.code().0, Ordering::Release);
        } else {
            STATUS.store(1, Ordering::Release);
        } // registered=0, drawing=1
    });
    if result.is_err() {
        STATUS.store(-3, Ordering::Release);
        STOPPED.store(true, Ordering::Release);
        if let Some(b) = BRIDGE.get() {
            b.visible.store(false, Ordering::Release);
        }
        input::restore();
    }
}
