//! Window messages feed a bounded queue; WndProc never locks renderer/engine state.
use egui::{Event, Key, Modifiers, PointerButton};
use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Globalization::{CP_UTF8, GetACP, IsDBCSLeadByteEx, MB_ERR_INVALID_CHARS, MultiByteToWideChar},
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::{Ime::*, KeyboardAndMouse::*},
        WindowsAndMessaging::*,
    },
};
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static PROCEDURES: OnceLock<Mutex<HashMap<usize, Procedure>>> = OnceLock::new();
#[derive(Clone, Copy)]
struct Procedure {
    original: isize,
    unicode: bool,
}
static INPUT: Mutex<Input> = Mutex::new(Input {
    events: Vec::new(),
    high_surrogate: None,
    ansi: Vec::new(),
});
#[derive(Default)]
struct Input {
    events: Vec<Event>,
    high_surrogate: Option<u16>,
    ansi: Vec<u8>,
}
impl Input {
    fn ansi_character(&mut self, value: u16, code_page: u32) {
        // WM_CHAR can carry a DBCS pair together or one byte at a time.
        let bytes = value.to_be_bytes();
        for &byte in if value > 0xff {
            &bytes[..]
        } else {
            &bytes[1..]
        } {
            self.ansi.push(byte);
            if code_page == CP_UTF8 {
                if let Err(error) = std::str::from_utf8(&self.ansi) {
                    if error.error_len().is_none() && self.ansi.len() < 4 {
                        continue;
                    }
                    self.ansi.clear();
                    continue;
                }
            } else if self.ansi.len() == 1 && unsafe { IsDBCSLeadByteEx(code_page, byte) }.is_ok() {
                continue;
            }
            let mut wide = [0u16; 2];
            let count = unsafe {
                MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, &self.ansi, Some(&mut wide))
            };
            self.ansi.clear();
            if count > 0 {
                let text: String = String::from_utf16_lossy(&wide[..count as usize])
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                if !text.is_empty() {
                    self.events.push(Event::Text(text));
                }
            }
        }
    }
}
pub fn install(window: HWND) -> Result<(), i32> {
    if WINDOW.load(Ordering::Acquire) == window.0 as usize {
        return Ok(());
    }
    restore();
    let mut procedures = PROCEDURES.get_or_init(Default::default).lock().unwrap();
    unsafe {
        // Changing an ANSI window to a W procedure makes GetWindowLongPtrA
        // return a conversion token. Spice's touch emulator calls that value
        // directly, so preserve the host's window encoding when subclassing.
        let unicode = IsWindowUnicode(window).as_bool();
        let (get, set) = window_functions(unicode);
        let old = get(window, GWLP_WNDPROC);
        if old == 0 {
            return Err(-4);
        }
        // Keep old entries alive for later hooks that may still chain through us.
        procedures.insert(
            window.0 as usize,
            Procedure {
                original: old,
                unicode,
            },
        );
        if set(window, GWLP_WNDPROC, callback_address(unicode)) == 0 {
            return Err(-4);
        }
    }
    WINDOW.store(window.0 as usize, Ordering::Release);
    Ok(())
}
pub fn restore() {
    let raw = WINDOW.swap(0, Ordering::AcqRel);
    if raw == 0 {
        return;
    }
    if let Some(procedures) = PROCEDURES.get() {
        let old = procedures.lock().unwrap().get(&raw).copied();
        if let Some(old) = old {
            unsafe {
                let window = HWND(raw as _);
                let (get, set) = window_functions(old.unicode);
                if get(window, GWLP_WNDPROC) == callback_address(old.unicode) {
                    set(window, GWLP_WNDPROC, old.original);
                }
            }
        }
    }
    clear();
}
pub fn clear() {
    if let Ok(mut input) = INPUT.try_lock() {
        input.events.clear();
        input.high_surrogate = None;
        input.ansi.clear();
    }
}
fn active(window: HWND) -> bool {
    WINDOW.load(Ordering::Acquire) == window.0 as usize
        && super::BRIDGE
            .get()
            .is_some_and(|b| b.visible.load(Ordering::Acquire))
        && unsafe { GetForegroundWindow() == window }
}
fn modifiers() -> Modifiers {
    let down = |key: VIRTUAL_KEY| unsafe { GetKeyState(key.0 as i32) < 0 };
    Modifiers {
        alt: down(VK_MENU),
        ctrl: down(VK_CONTROL),
        shift: down(VK_SHIFT),
        mac_cmd: false,
        command: down(VK_CONTROL),
    }
}
type GetProcedure = unsafe fn(HWND, WINDOW_LONG_PTR_INDEX) -> isize;
type SetProcedure = unsafe fn(HWND, WINDOW_LONG_PTR_INDEX, isize) -> isize;
fn window_functions(unicode: bool) -> (GetProcedure, SetProcedure) {
    if unicode {
        (GetWindowLongPtrW, SetWindowLongPtrW)
    } else {
        (GetWindowLongPtrA, SetWindowLongPtrA)
    }
}
fn callback_address(unicode: bool) -> isize {
    if unicode {
        wndproc_w as *const () as isize
    } else {
        wndproc_a as *const () as isize
    }
}
unsafe extern "system" fn wndproc_a(window: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    wndproc(window, msg, w, l, false)
}
unsafe extern "system" fn wndproc_w(window: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    wndproc(window, msg, w, l, true)
}
fn wndproc(window: HWND, msg: u32, w: WPARAM, l: LPARAM, unicode: bool) -> LRESULT {
    let captured =
        std::panic::catch_unwind(|| message(window, msg, w, l, unicode)).unwrap_or(false);
    if captured {
        return LRESULT(0);
    }
    let old = PROCEDURES
        .get()
        .and_then(|p| p.lock().ok()?.get(&(window.0 as usize)).copied());
    unsafe {
        if let Some(old) = old {
            let call = if unicode {
                CallWindowProcW
            } else {
                CallWindowProcA
            };
            call(
                Some(std::mem::transmute::<
                    isize,
                    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
                >(old.original)),
                window,
                msg,
                w,
                l,
            )
        } else if unicode {
            DefWindowProcW(window, msg, w, l)
        } else {
            DefWindowProcA(window, msg, w, l)
        }
    }
}
fn message(window: HWND, msg: u32, w: WPARAM, l: LPARAM, unicode: bool) -> bool {
    if !active(window) {
        return false;
    }
    let mods = modifiers();
    let Ok(mut input) = INPUT.try_lock() else {
        return false;
    };
    if input.events.len() >= 1024 {
        input.events.clear();
    }
    match msg {
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            let pressed = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            if pressed && mods.ctrl {
                match w.0 as u32 {
                    0x43 => input.events.push(Event::Copy),
                    0x58 => input.events.push(Event::Cut),
                    0x56 => {
                        if let Ok(text) = clipboard_win::get_clipboard_string() {
                            input
                                .events
                                .push(Event::Paste(text.chars().take(65536).collect()));
                        }
                    }
                    _ => {}
                }
            }
            if let Some(key) = key(w.0 as u32) {
                input.events.push(Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: l.0 & (1 << 30) != 0,
                    modifiers: mods,
                });
            }
            // Preserve system combinations (Alt+F4 etc.). Controller SDK remains read-only.
            return !mods.alt;
        }
        WM_CHAR => {
            if !unicode {
                input.ansi_character(w.0 as u16, unsafe { GetACP() });
                return true;
            }
            let c = w.0 as u16;
            if (0xD800..=0xDBFF).contains(&c) {
                input.high_surrogate = Some(c);
            } else {
                let code = if (0xDC00..=0xDFFF).contains(&c) {
                    input
                        .high_surrogate
                        .take()
                        .map(|h| 0x10000 + ((h as u32 - 0xD800) << 10) + c as u32 - 0xDC00)
                } else {
                    input.high_surrogate = None;
                    Some(c as u32)
                };
                if let Some(c) = code.and_then(char::from_u32).filter(|c| !c.is_control()) {
                    input.events.push(Event::Text(c.to_string()));
                }
            }
            return true;
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let delta = ((w.0 >> 16) as u16 as i16) as f32 / 120.0;
            input.events.push(Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Line,
                delta: if msg == WM_MOUSEWHEEL {
                    egui::vec2(0.0, delta)
                } else {
                    egui::vec2(delta, 0.0)
                },
                modifiers: mods,
            });
            return true;
        }
        _ => {}
    }
    // Buttons and pointer are sampled in take(), including games consuming mouse messages.
    matches!(
        msg,
        WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_LBUTTONDBLCLK
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
    )
}
static BUTTONS: Mutex<[bool; 3]> = Mutex::new([false; 3]);
pub fn take(window: HWND, width: u32, height: u32, scale: f32, time: f64) -> egui::RawInput {
    let focused = active(window);
    let mut events = std::mem::take(&mut INPUT.lock().unwrap().events);
    let mods = modifiers();
    let mut point = POINT::default();
    let mut rect = RECT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
        let _ = ScreenToClient(window, &mut point);
        let _ = GetClientRect(window, &mut rect);
    }
    let pos = egui::pos2(
        point.x as f32 * width as f32 / (rect.right - rect.left).max(1) as f32 / scale,
        point.y as f32 * height as f32 / (rect.bottom - rect.top).max(1) as f32 / scale,
    );
    if focused {
        events.push(Event::PointerMoved(pos));
    } else {
        events.push(Event::PointerGone);
    }
    let mut buttons = BUTTONS.lock().unwrap();
    for (i, (key, button)) in [
        (VK_LBUTTON, PointerButton::Primary),
        (VK_RBUTTON, PointerButton::Secondary),
        (VK_MBUTTON, PointerButton::Middle),
    ]
    .into_iter()
    .enumerate()
    {
        let down = focused && unsafe { GetAsyncKeyState(key.0 as i32) < 0 };
        if buttons[i] != down {
            events.push(Event::PointerButton {
                pos,
                button,
                pressed: down,
                modifiers: mods,
            });
            buttons[i] = down;
        }
    }
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32 / scale, height as f32 / scale),
        )),
        time: Some(time),
        events,
        modifiers: mods,
        focused,
        ..Default::default()
    }
}
pub fn ime_position(window: HWND, ime: Option<&egui::output::IMEOutput>, scale: f32) {
    if let Some(ime) = ime {
        unsafe {
            let context = ImmGetContext(window);
            if !context.0.is_null() {
                let form = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: POINT {
                        x: (ime.cursor_rect.min.x * scale) as i32,
                        y: (ime.cursor_rect.max.y * scale) as i32,
                    },
                    ..Default::default()
                };
                let _ = ImmSetCompositionWindow(context, &form);
                let _ = ImmReleaseContext(window, context);
            }
        }
    }
}
fn key(code: u32) -> Option<Key> {
    Some(match code {
        8 => Key::Backspace,
        9 => Key::Tab,
        13 => Key::Enter,
        27 => Key::Escape,
        32 => Key::Space,
        33 => Key::PageUp,
        34 => Key::PageDown,
        35 => Key::End,
        36 => Key::Home,
        37 => Key::ArrowLeft,
        38 => Key::ArrowUp,
        39 => Key::ArrowRight,
        40 => Key::ArrowDown,
        45 => Key::Insert,
        46 => Key::Delete,
        0x41 => Key::A,
        0x43 => Key::C,
        0x56 => Key::V,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5A => Key::Z,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{s, w};

    #[test]
    fn ansi_text_decodes_dbcs_packed_bytes_and_utf8_without_control_characters() {
        for (code_page, values, expected) in [
            (932, vec![0x82, 0xa0], "あ"),
            (936, vec![0xd6d0], "中"),
            (1252, vec![0xe9, 0x0d], "é"),
            (CP_UTF8, vec![0xf0, 0x9f, 0x8e, 0xb5], "🎵"),
            (CP_UTF8, vec![0xff, 0x41], "A"),
        ] {
            let mut input = Input::default();
            for value in values {
                input.ansi_character(value, code_page);
            }
            assert_eq!(input.events, vec![Event::Text(expected.into())]);
            assert!(input.ansi.is_empty());
        }
    }

    unsafe extern "system" fn touch_sink(_: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_TOUCH && w.0 == 1 && l.0 == 0x1234 {
            LRESULT(0x5678)
        } else {
            LRESULT(0)
        }
    }

    #[test]
    fn subclass_preserves_window_encoding_and_direct_touch_dispatch() {
        unsafe {
            // Hidden windows; no game, live input, or display changes required.
            let ansi = CreateWindowExA(
                WINDOW_EX_STYLE(0),
                s!("STATIC"),
                s!("Input regression"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let unicode = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Input regression"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            for (window, wide) in [(ansi, false), (unicode, true)] {
                let get = if wide {
                    GetWindowLongPtrW
                } else {
                    GetWindowLongPtrA
                };
                let set = if wide {
                    SetWindowLongPtrW
                } else {
                    SetWindowLongPtrA
                };
                let original = set(window, GWLP_WNDPROC, touch_sink as *const () as isize);
                install(window).unwrap();
                let current = get(window, GWLP_WNDPROC);
                // Spice's touch emulator calls this value directly. Check it
                // before calling: a mismatched A/W subclass returns a thunk token.
                assert!(
                    crate::host::windows::executable_address(current as usize),
                    "touch dispatcher received a non-executable WndProc: {current:#x}"
                );
                assert_eq!(IsWindowUnicode(window).as_bool(), wide);
                let call: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                    std::mem::transmute(current);
                assert_eq!(
                    call(window, WM_TOUCH, WPARAM(1), LPARAM(0x1234)),
                    LRESULT(0x5678)
                );
                install(window).unwrap(); // Repeated frames must not chain to ourselves.
                restore();
                assert_eq!(get(window, GWLP_WNDPROC), touch_sink as *const () as isize);
                assert_eq!(IsWindowUnicode(window).as_bool(), wide);
                set(window, GWLP_WNDPROC, original);
                DestroyWindow(window).unwrap();
            }
        }
    }
}
