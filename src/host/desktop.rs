//! Native desktop host for the shared egui menu.
use crate::{
    gui::Bridge,
    host::menu::{self, Frame},
};
use anyhow::{Result, ensure};
use std::{
    cell::RefCell,
    ffi::c_void,
    sync::{Arc, OnceLock, atomic::Ordering},
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{Direct3D9::*, Gdi::*},
        UI::WindowsAndMessaging::*,
    },
    core::{Interface, w},
};

type Draw = extern "C" fn(u32, *const Frame, *mut c_void);
static DRAW: OnceLock<Draw> = OnceLock::new();
thread_local! {
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
}

#[derive(Default)]
pub struct Options {
    pub hidden: bool,
    pub frames: Option<u64>,
    pub preview_shortcuts: bool,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.hidden || self.frames.is_some(),
            "--hidden requires --frames N"
        );
        ensure!(self.frames != Some(0), "--frames must be positive");
        Ok(())
    }
}

fn standalone() -> bool {
    menu::BRIDGE
        .get()
        .is_some_and(|bridge| bridge.snapshot().standalone)
}

/// Post native commands after rendering, never enter a modal loop while the
/// renderer lock / BeginScene is still held.
pub(super) fn viewport_commands(window: HWND, output: &egui::FullOutput) {
    if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
        for command in &viewport.commands {
            let command = match command {
                egui::ViewportCommand::Minimized(true) => SC_MINIMIZE,
                egui::ViewportCommand::Maximized(true) => SC_MAXIMIZE,
                egui::ViewportCommand::Maximized(false) => SC_RESTORE,
                _ => continue,
            };
            unsafe {
                let _ = PostMessageW(
                    Some(window),
                    WM_SYSCOMMAND,
                    WPARAM(command as usize),
                    LPARAM(0),
                );
            }
        }
    }
}

fn hit_test(window: HWND, position: LPARAM) -> LRESULT {
    let x = (position.0 as u16 as i16) as i32;
    let y = ((position.0 >> 16) as u16 as i16) as i32;
    let mut rect = RECT::default();
    unsafe {
        if GetWindowRect(window, &mut rect).is_err() {
            return LRESULT(HTCLIENT as isize);
        }
        if !IsZoomed(window).as_bool() {
            let left = x < rect.left + 6;
            let right = x >= rect.right - 6;
            let top = y < rect.top + 6;
            let bottom = y >= rect.bottom - 6;
            let edge = match (left, right, top, bottom) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ => HTCLIENT,
            };
            if edge != HTCLIENT {
                return LRESULT(edge as isize);
            }
        }
    }
    let point = egui::pos2((x - rect.left) as f32, (y - rect.top) as f32);
    LRESULT(if menu::BRIDGE
        .get()
        .is_some_and(|bridge| bridge.is_title_bar(point))
    {
        HTCAPTION
    } else {
        HTCLIENT
    } as isize)
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // Native moves/resizes run a nested Windows message loop. Its timer keeps
    // painting and resetting the back buffer, without borrowing Surface twice.
    unsafe {
        match message {
            WM_NCCALCSIZE if standalone() => return LRESULT(0),
            WM_NCHITTEST if standalone() => return hit_test(window, l),
            WM_GETMINMAXINFO if standalone() => {
                if let Some(info) = (l.0 as *mut MINMAXINFO).as_mut() {
                    info.ptMinTrackSize.x = 1000;
                    info.ptMinTrackSize.y = 680;
                    let mut monitor = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if GetMonitorInfoW(
                        MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                        &mut monitor,
                    )
                    .as_bool()
                    {
                        info.ptMaxPosition.x = monitor.rcWork.left - monitor.rcMonitor.left;
                        info.ptMaxPosition.y = monitor.rcWork.top - monitor.rcMonitor.top;
                        info.ptMaxSize.x = monitor.rcWork.right - monitor.rcWork.left;
                        info.ptMaxSize.y = monitor.rcWork.bottom - monitor.rcWork.top;
                    }
                }
                return LRESULT(0);
            }
            WM_ENTERSIZEMOVE => {
                SetTimer(Some(window), 1, 16, None);
                return LRESULT(0);
            }
            WM_EXITSIZEMOVE => {
                let _ = KillTimer(Some(window), 1);
                return LRESULT(0);
            }
            WM_TIMER if w.0 == 1 => {
                paint_frame();
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let _ = BeginPaint(window, &mut paint);
                paint_frame();
                let _ = EndPaint(window, &paint);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(window, message, w, l)
    }
}

struct Surface {
    window: HWND,
    device: IDirect3DDevice9,
    parameters: D3DPRESENT_PARAMETERS,
    draw: Draw,
    frames: u64,
    reset: bool,
    error: Option<anyhow::Error>,
}
impl Surface {
    fn paint(&mut self) -> Result<()> {
        unsafe {
            if IsIconic(self.window).as_bool() || !IsWindow(Some(self.window)).as_bool() {
                return Ok(());
            }
            let mut rect = RECT::default();
            GetClientRect(self.window, &mut rect)?;
            if rect.right == 0 || rect.bottom == 0 {
                return Ok(());
            }
            if self.parameters.BackBufferWidth != rect.right as u32
                || self.parameters.BackBufferHeight != rect.bottom as u32
                || self.reset
            {
                (self.draw)(2, std::ptr::null(), std::ptr::null_mut());
                self.parameters.BackBufferWidth = rect.right as u32;
                self.parameters.BackBufferHeight = rect.bottom as u32;
                if self.device.Reset(&mut self.parameters).is_err() {
                    self.reset = true;
                    return Ok(());
                }
                self.reset = false;
            }
            let frame = Frame {
                size: std::mem::size_of::<Frame>() as u32,
                device: self.device.as_raw(),
                window: self.window.0,
                width: self.parameters.BackBufferWidth,
                height: self.parameters.BackBufferHeight,
            };
            self.device.Clear(
                0,
                std::ptr::null(),
                D3DCLEAR_TARGET as u32,
                0xFF0A0A0A,
                1.0,
                0,
            )?;
            self.device.BeginScene()?;
            (self.draw)(1, &frame, std::ptr::null_mut());
            self.device.EndScene()?;
            ensure!(menu::status() == 1, "Renderer failed: {}", menu::status());
            if self
                .device
                .Present(
                    std::ptr::null(),
                    std::ptr::null(),
                    HWND::default(),
                    std::ptr::null(),
                )
                .is_err()
            {
                self.reset = true;
            }
            self.frames += 1;
        }
        Ok(())
    }
}

fn paint_frame() {
    SURFACE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut()
            && let Some(surface) = state.as_mut()
            && surface.error.is_none()
            && let Err(error) = surface.paint()
        {
            surface.error = Some(error);
        }
    });
}

pub fn run(
    bridge: Arc<Bridge>,
    options: Options,
    mut check: impl FnMut() -> Result<()>,
) -> Result<()> {
    use crate::game::Navigation;
    unsafe extern "C" fn register(draw: Draw, _: *mut c_void) -> i32 {
        if DRAW.set(draw).is_ok() { 0 } else { -1 }
    }
    options.validate()?;
    bridge.visible.store(true, Ordering::Release);
    menu::BRIDGE
        .set(bridge.clone())
        .map_err(|_| anyhow::anyhow!("GUI already initialized"))?;
    menu::register(register);
    let draw = *DRAW
        .get()
        .ok_or_else(|| anyhow::anyhow!("Callback missing"))?;
    unsafe {
        let instance = HINSTANCE(windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            std::ptr::null(),
        ));
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("ChartRequesterDesktop"),
            ..Default::default()
        };
        ensure!(
            RegisterClassW(&class) != 0,
            "Cannot register desktop window: {}",
            windows::core::Error::from_win32()
        );
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class.lpszClassName,
            w!("Chart Requester"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1120,
            780,
            None,
            None,
            Some(instance),
            None,
        )?;
        struct Cleanup {
            window: HWND,
            draw: Draw,
            instance: HINSTANCE,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                menu::stop();
                (self.draw)(3, std::ptr::null(), std::ptr::null_mut());
                SURFACE.with(|state| {
                    state.borrow_mut().take();
                });
                unsafe {
                    if IsWindow(Some(self.window)).as_bool() {
                        let _ = DestroyWindow(self.window);
                    }
                    let _ = UnregisterClassW(w!("ChartRequesterDesktop"), Some(self.instance));
                }
            }
        }
        let cleanup = Cleanup {
            window,
            draw,
            instance,
        };
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
        SURFACE.with(|state| {
            *state.borrow_mut() = Some(Surface {
                window,
                device: device.unwrap(),
                parameters,
                draw,
                frames: 0,
                reset: false,
                error: None,
            })
        });
        paint_frame();
        if !options.hidden {
            let _ = ShowWindow(window, SW_SHOW);
        }
        loop {
            check()?;
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if options.preview_shortcuts
                    && matches!(message.message, WM_KEYDOWN | WM_KEYUP)
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
            let count = SURFACE.with(|state| -> Result<u64> {
                let mut state = state.borrow_mut();
                let surface = state.as_mut().unwrap();
                if let Some(error) = surface.error.take() {
                    return Err(error);
                }
                Ok(surface.frames)
            })?;
            if !IsWindow(Some(window)).as_bool()
                || options.frames.is_some_and(|n| count >= n)
                || (!options.preview_shortcuts && !bridge.visible.load(Ordering::Acquire))
            {
                break;
            }
            paint_frame();
            std::thread::sleep(Duration::from_millis(16));
        }
        drop(cleanup);
    }
    Ok(())
}
