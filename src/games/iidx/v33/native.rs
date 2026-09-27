//! IIDX 33 adapter. All game calls execute from the native selection update.
//! ABI/offset evidence and supported file hash are recorded in docs/game-analysis.md.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{
    config::Controls,
    game::{Mode, Navigation, Phase, Selection, SelectionResult as Ack, Snapshot},
    games::iidx::{
        self,
        controls::{DoubleTap, Target, opposite_start, single_side},
    },
    host::windows::ModuleImage,
};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::c_void,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    Memory::{PAGE_READWRITE, VirtualProtect},
    Threading::GetCurrentProcess,
};

use super::{SUPPORTED_SHA256, search_index};

const SELECT_VTABLE: usize = 0xd84788;
const TITLE_DICTIONARY_VTABLE: usize = 0xce9f40;
const INPUT_VTABLE: usize = 0xdd05c0;
const DATABASE_GETTER: usize = 0x951fd0;
const STAGES: &[usize] = &[
    0xda50a8, 0xda5188, 0xda5268, 0xda5348, 0xda5428, 0xda5508, 0xda55e8, 0xda56c8, 0xdae1e8,
    0xdae2c8, 0xdae3a8, 0xdae488, 0xdae728,
];
type SceneFn = unsafe extern "system" fn(usize, usize, usize, usize) -> usize;
struct Hook {
    table: usize,
    slot: usize,
    original: usize,
}
struct Adapter {
    base: usize,
    hooks: Vec<Hook>,
}
static ADAPTER: OnceLock<Adapter> = OnceLock::new();
pub static DISABLED: AtomicBool = AtomicBool::new(false);
pub static LIVE_DATABASE: OnceLock<Vec<u8>> = OnceLock::new();
static ACTIVE_SCENE: AtomicUsize = AtomicUsize::new(0);
static DATABASE_RETRY: AtomicUsize = AtomicUsize::new(0);
static SELECT_UPDATES: AtomicUsize = AtomicUsize::new(0);
static DATABASE_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static DATABASE_ERROR: Mutex<Option<String>> = Mutex::new(None);
pub static SEARCH_INDEX: Mutex<Option<Vec<(u32, String)>>> = Mutex::new(None);
static INDEX_LOADS: AtomicUsize = AtomicUsize::new(0);
static INDEX_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static INDEX_ERROR: Mutex<Option<String>> = Mutex::new(None);
static CONTROLS: Mutex<Option<Controls>> = Mutex::new(None);
static INPUT_CLOCK: OnceLock<Instant> = OnceLock::new();
pub static MENU_OPEN: AtomicBool = AtomicBool::new(false);
static NAVIGATOR: OnceLock<Mutex<super::super::navigation::Navigator>> = OnceLock::new();

pub fn configure_controls(config: Controls) {
    *CONTROLS.lock().unwrap() = Some(config);
    MAILBOX.lock().unwrap().taps.reset();
}

pub fn player_card() -> Option<crate::profiles::CardId> {
    super::player::snapshot(ADAPTER.get()?.base, read_memory)
        .ok()
        .flatten()
}

/// Read-only diagnostics. Native callbacks update counters; disk IO stays on the worker.
pub fn diagnostics() -> String {
    let slots_intact = ADAPTER.get().is_some_and(|a| {
        [
            (13, select_init as *const () as usize),
            (14, select_exit as *const () as usize),
            (15, select_update as *const () as usize),
        ]
        .iter()
        .all(|&(slot, expected)| {
            read_memory(a.base + SELECT_VTABLE + slot * 8, 8)
                .is_ok_and(|b| usize::from_le_bytes(b.try_into().unwrap()) == expected)
        })
    });
    format!(
        "select_updates={} active_scene={} select_hooks_intact={} database_attempts={} database_bytes={} database_error={:?} index_loads={} index_entries={} index_error={:?}",
        SELECT_UPDATES.load(Ordering::Relaxed),
        ACTIVE_SCENE.load(Ordering::Acquire) != 0,
        slots_intact,
        DATABASE_ATTEMPTS.load(Ordering::Relaxed),
        LIVE_DATABASE.get().map_or(0, Vec::len),
        DATABASE_ERROR.lock().unwrap().as_deref(),
        INDEX_LOADS.load(Ordering::Relaxed),
        INDEX_ENTRIES.load(Ordering::Relaxed),
        INDEX_ERROR.lock().unwrap().as_deref()
    )
}

fn try_capture_database() {
    DATABASE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    *DATABASE_ERROR.lock().unwrap() = capture_database().err().map(|e| e.to_string());
}

#[derive(Default)]
pub struct Mailbox {
    pub snapshot: Snapshot,
    pub plays: u64,
    pub command: Option<Selection>,
    pub ack: Option<Ack>,
    pub menu_event: Option<Target>,
    pub menu_side: Option<u8>,
    pub navigation: std::collections::VecDeque<Navigation>,
    taps: DoubleTap,
}
pub static MAILBOX: Mutex<Mailbox> = Mutex::new(Mailbox {
    snapshot: Snapshot {
        phase: Phase::Other,
        mode: None,
        epoch: 0,
        can_skip: false,
    },
    plays: 0,
    command: None,
    ack: None,
    menu_event: None,
    menu_side: None,
    navigation: std::collections::VecDeque::new(),
    taps: DoubleTap::new(),
});

pub fn install(image: ModuleImage) -> Result<()> {
    ensure!(cfg!(target_arch = "x86_64"), "Only x64 IIDX is supported");
    ensure!(image.sha256 == SUPPORTED_SHA256, "Wrong IIDX 33 profile");
    let base = image.base;
    // Guard native entry points against incompatible in-memory patches as well.
    for (rva, expected) in [
        (0x7d60e0, "4883ec288b051e2f010aa801755483c8"),
        (0x7d6150, "80790800741089511044894114448949"),
        (0x7d5eb0, "4c894c2420534154415641574883ec48"),
        (0x82ded0, "e95bb61100cccccccccccccccccccccc"),
        (0x606fd0, "48895c2408574883ec20488bd9b9e803"),
        (0x607030, "40534883ec204881c1280300008bdae8"),
        (0x606e60, "4883ec284881c128030000e880cdffff"),
        (0x949230, "4883ec28e84702000083f801751533c9"),
        (0x9493e0, "85c9781783f90273124863c1488d0dbd"),
        (0x806f60, "4883ec28e8f7feffff85c07517e84eff"),
        (0xa7a2f0, "48894c24085553565741544155415641"),
        (0x5c4480, "833d614caf00007411833d5c4caf0000"),
        (0x5ad900, "48895c240848896c2410488974241857"),
        (0x5ad8a0, "4883ec28e8d76b010085c075484863c9"),
    ] {
        ensure!(
            read_memory(base + rva, 16)? == hex::decode(expected)?,
            "Native function at RVA {rva:x} was changed; hook disabled"
        );
    }
    let mut hooks = Vec::new();
    for (table, slots) in std::iter::once((SELECT_VTABLE, &[13usize, 14, 15][..]))
        .chain(std::iter::once((TITLE_DICTIONARY_VTABLE, &[1usize][..])))
        .chain(std::iter::once((INPUT_VTABLE, &[3usize][..])))
        .chain(STAGES.iter().map(|&r| (r, &[13usize][..])))
    {
        for &slot in slots {
            let bytes = read_memory(base + table + slot * 8, 8)?;
            let original = usize::from_le_bytes(bytes.try_into().unwrap());
            ensure!(
                (base + 0x1000..base + 0xc90000).contains(&original),
                "Unexpected scene vtable entry"
            );
            hooks.push(Hook {
                table: base + table,
                slot,
                original,
            });
        }
    }
    ADAPTER
        .set(Adapter { base, hooks })
        .map_err(|_| anyhow::anyhow!("Hooks already installed"))?;
    let adapter = ADAPTER.get().unwrap();
    for (installed, hook) in adapter.hooks.iter().enumerate() {
        let replacement = if hook.table == base + SELECT_VTABLE {
            match hook.slot {
                13 => select_init as *const () as usize,
                14 => select_exit as *const () as usize,
                _ => select_update as *const () as usize,
            }
        } else if hook.table == base + TITLE_DICTIONARY_VTABLE {
            title_dictionary_load as *const () as usize
        } else if hook.table == base + INPUT_VTABLE {
            input_poll as *const () as usize
        } else {
            stage_init as *const () as usize
        };
        if let Err(e) = unsafe { replace_slot(hook.table + hook.slot * 8, replacement) } {
            DISABLED.store(true, Ordering::Release);
            for old in &adapter.hooks[..installed] {
                let _ = unsafe { replace_slot(old.table + old.slot * 8, old.original) };
            }
            return Err(e);
        }
    }
    Ok(())
}
unsafe fn replace_slot(slot: usize, value: usize) -> Result<()> {
    let mut old = 0;
    ensure!(
        VirtualProtect(slot as *const c_void, 8, PAGE_READWRITE, &mut old) != 0,
        "Cannot make vtable writable"
    );
    (*(slot as *const AtomicUsize)).store(value, Ordering::SeqCst);
    let mut unused = 0;
    // Pointer is already installed; failure to restore protection must not make
    // the caller lose track of a live hook.
    if VirtualProtect(slot as *const c_void, 8, old, &mut unused) == 0 {
        DISABLED.store(true, Ordering::Release);
    }
    Ok(())
}
pub fn read_memory(address: usize, size: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; size];
    let mut read = 0;
    ensure!(
        unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const c_void,
                out.as_mut_ptr().cast(),
                size,
                &mut read,
            )
        } != 0
            && read == size,
        "Cannot read game memory"
    );
    Ok(out)
}
unsafe fn original(this: usize, slot: usize) -> SceneFn {
    let table = *(this as *const usize);
    let hook = ADAPTER
        .get()
        .unwrap()
        .hooks
        .iter()
        .find(|h| h.table == table && h.slot == slot)
        .expect("Known scene vtable");
    std::mem::transmute(hook.original)
}
unsafe fn get_mode() -> Option<Mode> {
    let f: unsafe extern "system" fn() -> u32 =
        std::mem::transmute(ADAPTER.get().unwrap().base + 0x82ded0);
    match f() {
        0 => Some(iidx::SP),
        1 => Some(iidx::DP),
        _ => None,
    }
}
unsafe fn reserve() -> usize {
    let f: unsafe extern "system" fn() -> usize =
        std::mem::transmute(ADAPTER.get().unwrap().base + 0x7d60e0);
    f()
}
unsafe fn menu_side(mode: Option<Mode>) -> Option<u8> {
    if mode != Some(iidx::SP) {
        return None;
    }
    let joined: unsafe extern "system" fn(u32) -> u8 =
        std::mem::transmute(ADAPTER.get().unwrap().base + 0x9493e0);
    single_side(true, [joined(0) != 0, joined(1) != 0])
}

fn reset_input(m: &mut Mailbox) {
    m.taps.reset();
    m.menu_event = None;
    m.menu_side = None;
    m.snapshot.can_skip = false;
    m.navigation.clear();
}

unsafe extern "system" fn input_poll(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    // Verified native layout: only button words +08..+17 and turntables +58..+67.
    // Never copy the container headers between these regions.
    let previous = [
        *((this + 0x58) as *const i32),
        *((this + 0x60) as *const i32),
    ];
    let result = original(this, 3)(this, a2, a3, a4);
    guard(|| {
        let mut mailbox = MAILBOX.lock().unwrap();
        let context =
            if MENU_OPEN.load(Ordering::Acquire) && mailbox.snapshot.phase == Phase::Select {
                mailbox.menu_side.map(|side| (mailbox.snapshot.epoch, side))
            } else {
                None
            };
        let buttons = *((this + 8) as *const u32);
        let scratch = context.map_or(0, |(_, side)| {
            *((this + 0x5c + usize::from(side) * 8) as *const i32)
        });
        let (events, mask) = NAVIGATOR
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .sample(
                context,
                buttons,
                scratch,
                INPUT_CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64,
            );
        for event in events {
            if mailbox.navigation.len() < 32 {
                mailbox.navigation.push_back(event);
            }
        }
        mask_navigation_input(
            this,
            mask,
            context.map(|(_, side)| (side, previous[usize::from(side)])),
        );
    });
    result
}
unsafe fn mask_navigation_input(this: usize, mask: u32, turntable: Option<(u8, i32)>) {
    for offset in [8, 12, 16, 20] {
        let field = (this + offset) as *mut u32;
        field.write_unaligned(field.read_unaligned() & !mask);
    }
    if let Some((side, previous)) = turntable {
        ((this + 0x58 + usize::from(side) * 8) as *mut i32).write_unaligned(previous);
        ((this + 0x5c + usize::from(side) * 8) as *mut i32).write_unaligned(0);
    }
}

unsafe fn sample_input(m: &mut Mailbox, mode: Option<Mode>, ready: bool) -> Option<Target> {
    let config = CONTROLS.lock().unwrap().clone();
    let Some(config) = config.filter(|c| c.skip_enabled) else {
        reset_input(m);
        return None;
    };
    m.menu_side = if ready { menu_side(mode) } else { None };
    m.snapshot.can_skip = m.menu_side.is_some();
    let target = m.menu_side.map(|side| Target {
        side,
        epoch: m.snapshot.epoch,
    });
    let pressed = target.and_then(|t| opposite_start(t.side));
    if target.is_none() || pressed.is_none() || m.menu_event.is_some_and(|e| Some(e) != target) {
        m.menu_event = None;
    }
    m.taps.sample(
        target,
        pressed,
        INPUT_CLOCK.get_or_init(Instant::now).elapsed(),
        Duration::from_millis(config.double_tap_ms),
    )
}
unsafe fn ready(this: usize) -> bool {
    if ACTIVE_SCENE.load(Ordering::Acquire) != this || DISABLED.load(Ordering::Acquire) {
        return false;
    }
    let base = ADAPTER.get().unwrap().base;
    let controller = *((this + 144) as *const usize);
    if controller == 0 || *((this + 80) as *const u32) != 3 || *((this + 128) as *const i32) <= 0 {
        return false;
    }
    if *((controller + 8) as *const usize) != base + 0x8ee190
        || *((controller + 16) as *const u32) != 0
        || *((controller + 56) as *const usize) != 0
    {
        return false;
    }
    let modal: unsafe extern "system" fn() -> i32 = std::mem::transmute(base + 0x806f60);
    modal() == 0 && *((reserve() + 8) as *const u8) != 0
}
fn guard(f: impl FnOnce()) {
    if DISABLED.load(Ordering::Acquire) {
        return;
    }
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        DISABLED.store(true, Ordering::Release);
    }
}
unsafe extern "system" fn title_dictionary_load(this: usize, path: usize, callback: usize) -> u8 {
    let load: unsafe extern "system" fn(usize, usize, usize) -> u8 =
        std::mem::transmute(original(this, 1));
    let result = load(this, path, callback);
    // The native loader destroys path/callback. Only inspect the populated map,
    // while still on its owning thread, and pass owned strings to our worker.
    guard(|| {
        INDEX_LOADS.fetch_add(1, Ordering::Relaxed);
        let captured = if result != 0 {
            search_index::snapshot(this, read_memory)
        } else {
            Err(anyhow::anyhow!("Native title dictionary XML load failed"))
        };
        match captured {
            Ok(entries) => {
                INDEX_ENTRIES.store(entries.len(), Ordering::Relaxed);
                *SEARCH_INDEX.lock().unwrap() = Some(entries);
                *INDEX_ERROR.lock().unwrap() = None;
            }
            Err(e) => *INDEX_ERROR.lock().unwrap() = Some(e.to_string()),
        }
    });
    result
}
unsafe extern "system" fn select_init(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    let result = original(this, 13)(this, a2, a3, a4);
    guard(|| {
        ACTIVE_SCENE.store(this, Ordering::Release);
        if LIVE_DATABASE.get().is_none() {
            try_capture_database();
        }
        let mut m = MAILBOX.lock().unwrap();
        m.snapshot.epoch += 1;
        reset_input(&mut m);
        m.snapshot.mode = get_mode();
        m.snapshot.phase = Phase::Other;
    });
    result
}
fn capture_database() -> Result<()> {
    let entry = ADAPTER.get().context("Adapter not installed")?.base + DATABASE_GETTER;
    // The game/search dictionary uses this accessor. Omnifix changes LEA to MOV
    // so the old global becomes a pointer to its larger allocation. Accept only
    // these two verified instruction forms, then call the game's current accessor.
    let code = read_memory(entry, 8)?;
    validate_database_getter(&code)?;
    let getter: unsafe extern "system" fn() -> usize = unsafe { std::mem::transmute(entry) };
    let _ = LIVE_DATABASE.set(snapshot_database(getter)?);
    Ok(())
}
fn validate_database_getter(code: &[u8]) -> Result<()> {
    ensure!(
        code.len() == 8
            && code[0] == 0x48
            && matches!(code[1], 0x8d | 0x8b)
            && code[2..] == [0x05, 0x29, 0x69, 0x38, 0x0a, 0xc3],
        "Unsupported native database getter; expected original or relocated-buffer accessor"
    );
    Ok(())
}
fn snapshot_database(getter: unsafe extern "system" fn() -> usize) -> Result<Vec<u8>> {
    let address = unsafe { getter() };
    ensure!(
        address != 0,
        "Native database getter returned null; database not ready"
    );
    let header = read_memory(address, 16)?;
    ensure!(
        &header[..4] == b"IIDX" && u32::from_le_bytes(header[4..8].try_into()?) == 33,
        "Database header not ready: magic={} version={}",
        hex::encode(&header[..4]),
        u32::from_le_bytes(header[4..8].try_into()?)
    );
    let count = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    let ids = u32::from_le_bytes(header[12..16].try_into()?) as usize;
    ensure!(
        (1..=10000).contains(&count) && (1..=100000).contains(&ids),
        "Invalid live database dimensions: records={count} ids={ids}"
    );
    let size = 16 + ids * 4 + count * 0x7f8;
    ensure!(
        size <= 32 * 1024 * 1024,
        "Live database too large: bytes={size}"
    );
    let data = read_memory(address, size)?;
    ensure!(
        data[..16] == header,
        "Live database header changed during snapshot"
    );
    Ok(data)
}
fn cancel_command(m: &mut Mailbox) {
    if let Some(j) = m.command.take() {
        m.ack = Some(Ack {
            token: j.token,
            result: None,
        });
    }
}
unsafe extern "system" fn select_exit(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    guard(|| {
        ACTIVE_SCENE.store(0, Ordering::Release);
        let mut m = MAILBOX.lock().unwrap();
        m.snapshot.phase = Phase::Other;
        cancel_command(&mut m);
        reset_input(&mut m);
    });
    original(this, 14)(this, a2, a3, a4)
}
unsafe extern "system" fn stage_init(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    guard(|| {
        ACTIVE_SCENE.store(0, Ordering::Release);
        let mut m = MAILBOX.lock().unwrap();
        m.plays += 1;
        m.snapshot.phase = Phase::Playing;
        cancel_command(&mut m);
        reset_input(&mut m);
    });
    original(this, 13)(this, a2, a3, a4)
}
unsafe extern "system" fn select_update(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    SELECT_UPDATES.fetch_add(1, Ordering::Relaxed);
    let mut executing: Option<Selection> = None;
    let mut detected_menu = None;
    guard(|| {
        let mode = get_mode();
        let is_ready = ready(this);
        if is_ready
            && LIVE_DATABASE.get().is_none()
            && DATABASE_RETRY
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(120)
        {
            try_capture_database();
        }
        let mut m = MAILBOX.lock().unwrap();
        m.snapshot.mode = mode;
        m.snapshot.phase = if is_ready {
            Phase::Select
        } else {
            Phase::Other
        };
        detected_menu = sample_input(&mut m, mode, is_ready);
        if let Some(j) = m.command.as_ref() {
            if j.epoch != m.snapshot.epoch || Some(j.mode) != mode {
                cancel_command(&mut m);
            } else if is_ready && m.ack.is_none() {
                executing = m.command.take();
            }
        }
    });
    // Schedule the game's own reservation, then let the original update consume
    // it through the same path as the touchscreen's song-jump button.
    let mut submitted = false;
    let mut error = None;
    guard(|| {
        if let Some(j) = &executing {
            let base = ADAPTER.get().unwrap().base;
            let r = reserve();
            if *((r + 9) as *const u8) == 0 {
                return;
            } // Do not replace a touchscreen request.
            let can: unsafe extern "system" fn(usize, u32, u32, u64) -> u8 =
                std::mem::transmute(base + 0x7d5eb0);
            let difficulty = j
                .chart
                .map(|c| iidx::difficulty(c).expect("IIDX chart") as i32)
                .unwrap_or(-1);
            let optional = j
                .chart
                .map(|c| (1u64 << 32) | iidx::difficulty(c).expect("IIDX chart") as u64)
                .unwrap_or(0);
            let mode = if j.mode == iidx::DP { 1 } else { 0 };
            if can(r, j.song_id, mode, optional) == 0 {
                error = Some("谱面未解锁或当前选曲模式不可用".to_owned());
                return;
            }
            let set: unsafe extern "system" fn(usize, u32, u32, i32) =
                std::mem::transmute(base + 0x7d6150);
            set(r, j.song_id, mode, difficulty);
            submitted = true;
        }
    });
    let result = original(this, 15)(this, a2, a3, a4);
    guard(|| {
        // The original update can open a modal, change sides or enter gameplay.
        // Withdraw an unconsumed gesture if its context changed in this frame.
        let is_ready = ready(this);
        let mode = get_mode();
        let mut mailbox = MAILBOX.lock().unwrap();
        if !is_ready || menu_side(mode) != mailbox.menu_side {
            reset_input(&mut mailbox);
        } else if let Some(event) = detected_menu
            && mailbox.snapshot.epoch == event.epoch
        {
            mailbox.menu_event = Some(event);
        }
        mailbox.snapshot.mode = mode;
        if is_ready {
            mailbox.snapshot.phase = Phase::Select;
        } else if mailbox.snapshot.phase == Phase::Select {
            mailbox.snapshot.phase = Phase::Other;
        }
        drop(mailbox);
        if let Some(j) = executing {
            let outcome = if let Some(e) = error {
                Some(Err(e))
            } else if submitted {
                let base = ADAPTER.get().unwrap().base;
                let r = reserve();
                if *((r + 9) as *const u8) == 0 {
                    // A modal/transition won the frame. Withdraw only our reservation.
                    if *((r + 16) as *const u32) == j.song_id {
                        *((r + 9) as *mut u8) = 1;
                    }
                    None
                } else if ACTIVE_SCENE.load(Ordering::Acquire) != this {
                    // Cleanup may have run synchronously inside the original update.
                    // The old widget must no longer be inspected.
                    None
                } else {
                    let selected: unsafe extern "system" fn(usize) -> usize =
                        std::mem::transmute(base + 0x606fd0);
                    let music = selected(this + 408);
                    let bar_type: unsafe extern "system" fn(usize) -> u32 =
                        std::mem::transmute(base + 0x606e60);
                    let mut success = bar_type(this + 408) == 1
                        && music != 0
                        && *((music + 1660) as *const u32) == j.song_id;
                    if let Some(c) = j.chart {
                        let player: unsafe extern "system" fn() -> u32 =
                            std::mem::transmute(base + 0x949230);
                        let difficulty: unsafe extern "system" fn(usize, u32) -> u32 =
                            std::mem::transmute(base + 0x607030);
                        success &= difficulty(this + 408, player())
                            == iidx::difficulty(c).expect("IIDX chart");
                    }
                    Some(if success {
                        Ok(())
                    } else {
                        Err("游戏未接受该选曲请求".into())
                    })
                }
            } else {
                None
            };
            let mut m = MAILBOX.lock().unwrap();
            m.ack = Some(Ack {
                token: j.token,
                result: outcome,
            });
            if ACTIVE_SCENE.load(Ordering::Acquire) == this {
                m.snapshot.phase = if ready(this) {
                    Phase::Select
                } else {
                    Phase::Other
                };
                m.snapshot.mode = get_mode();
            }
        }
    });
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn menu_capture_changes_only_button_words_and_selected_turntable() {
        let mut object = [0xFFu8; 0xC0];
        unsafe {
            super::mask_navigation_input(object.as_mut_ptr() as usize, 0x7f << 7, Some((1, 123)));
        }
        assert!(object[..8].iter().all(|b| *b == 0xFF));
        assert!(object[0x18..0x60].iter().all(|b| *b == 0xFF));
        assert!(object[0x68..].iter().all(|b| *b == 0xFF));
        for offset in [8, 12, 16, 20] {
            assert_eq!(
                u32::from_le_bytes(object[offset..offset + 4].try_into().unwrap()),
                !(0x7f << 7)
            );
        }
        assert_eq!(
            i32::from_le_bytes(object[0x60..0x64].try_into().unwrap()),
            123
        );
        assert_eq!(&object[0x64..0x68], &[0; 4]);
    }
    use super::*;
    #[test]
    fn native_database_accessor_accepts_relocation_but_rejects_unknown_patches() {
        let mut code = [0x48, 0x8d, 0x05, 0x29, 0x69, 0x38, 0x0a, 0xc3];
        validate_database_getter(&code).unwrap();
        code[1] = 0x8b;
        validate_database_getter(&code).unwrap();
        code[3] ^= 1;
        assert!(validate_database_getter(&code).is_err());
        assert!(validate_database_getter(&[]).is_err());
    }

    #[test]
    fn snapshot_uses_native_returned_buffer_and_supports_expanded_database() {
        static BUFFER: OnceLock<Vec<u8>> = OnceLock::new();
        unsafe extern "system" fn getter() -> usize {
            BUFFER.get().unwrap().as_ptr() as usize
        }
        unsafe extern "system" fn empty_getter() -> usize {
            0
        }
        let count = 2300u32;
        let ids = 34000u32;
        let mut data = vec![0; 16 + ids as usize * 4 + count as usize * 0x7f8];
        data[..4].copy_from_slice(b"IIDX");
        data[4..8].copy_from_slice(&33u32.to_le_bytes());
        data[8..12].copy_from_slice(&count.to_le_bytes());
        data[12..16].copy_from_slice(&ids.to_le_bytes());
        BUFFER.set(data).unwrap();
        let snapshot = snapshot_database(getter).unwrap();
        assert!(snapshot.len() > 0x400000);
        assert_eq!(&snapshot, BUFFER.get().unwrap());
        assert!(snapshot_database(empty_getter).is_err());
    }
}
