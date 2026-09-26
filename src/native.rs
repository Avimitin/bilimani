//! IIDX 33 adapter. All game calls execute from the native selection update.
//! ABI/offset evidence and supported file hash are recorded in docs/game-analysis.md.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{
    catalog::Mode,
    engine::{Jump, Phase, Snapshot},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::HMODULE,
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
        Memory::{PAGE_READWRITE, VirtualProtect},
        Threading::GetCurrentProcess,
    },
};

pub const SUPPORTED_SHA256: &str =
    "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d";
const SELECT_VTABLE: usize = 0xd84788;
const DATABASE: usize = 0xacd8900;
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

#[derive(Clone, Debug)]
pub struct Ack {
    pub token: u64,
    pub result: Option<std::result::Result<(), String>>,
}
#[derive(Default)]
pub struct Mailbox {
    pub snapshot: Snapshot,
    pub plays: u64,
    pub command: Option<Jump>,
    pub ack: Option<Ack>,
}
pub static MAILBOX: Mutex<Mailbox> = Mutex::new(Mailbox {
    snapshot: Snapshot {
        phase: Phase::Other,
        mode: None,
        epoch: 0,
    },
    plays: 0,
    command: None,
    ack: None,
});

// HMODULE is an opaque OS handle; this function never dereferences it in Rust.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn module_path(module: HMODULE) -> Result<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    ensure!(len > 0 && len < buf.len(), "Cannot determine module path");
    use std::os::windows::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_wide(&buf[..len]).into())
}
pub fn install(module_name: &str) -> Result<()> {
    ensure!(cfg!(target_arch = "x86_64"), "Only x64 IIDX is supported");
    let name: Vec<u16> = module_name.encode_utf16().chain(Some(0)).collect();
    let module = unsafe { GetModuleHandleW(name.as_ptr()) };
    ensure!(!module.is_null(), "Game DLL not loaded: {module_name}");
    let path = module_path(module)?;
    let file = std::fs::read(&path).context("Cannot fingerprint game DLL")?;
    ensure!(
        hex::encode(Sha256::digest(&file)) == SUPPORTED_SHA256,
        "Unsupported bm2dx.dll build; hook disabled (see docs/game-analysis.md)"
    );
    let base = module as usize;
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
        (0x806f60, "4883ec28e8f7feffff85c07517e84eff"),
    ] {
        ensure!(
            read_memory(base + rva, 16)? == hex::decode(expected)?,
            "Native function at RVA {rva:x} was changed; hook disabled"
        );
    }
    let mut hooks = Vec::new();
    for (table, slots) in std::iter::once((SELECT_VTABLE, &[13usize, 14, 15][..]))
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
        0 => Some(Mode::SP),
        1 => Some(Mode::DP),
        _ => None,
    }
}
unsafe fn reserve() -> usize {
    let f: unsafe extern "system" fn() -> usize =
        std::mem::transmute(ADAPTER.get().unwrap().base + 0x7d60e0);
    f()
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
unsafe extern "system" fn select_init(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    let result = original(this, 13)(this, a2, a3, a4);
    guard(|| {
        ACTIVE_SCENE.store(this, Ordering::Release);
        if LIVE_DATABASE.get().is_none() {
            let _ = capture_database();
        }
        let mut m = MAILBOX.lock().unwrap();
        m.snapshot.epoch += 1;
        m.snapshot.mode = get_mode();
        m.snapshot.phase = Phase::Other;
    });
    result
}
fn capture_database() -> Result<()> {
    let address = ADAPTER.get().context("Adapter not installed")?.base + DATABASE;
    let header = read_memory(address, 16)?;
    ensure!(
        &header[..4] == b"IIDX" && u32::from_le_bytes(header[4..8].try_into()?) == 33,
        "Database not ready"
    );
    let count = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    let ids = u32::from_le_bytes(header[12..16].try_into()?) as usize;
    ensure!(
        (1..=2250).contains(&count) && (1..=100000).contains(&ids),
        "Invalid live database dimensions"
    );
    let size = 16 + ids * 4 + count * 0x7f8;
    ensure!(size <= 0x400000, "Live database too large");
    let _ = LIVE_DATABASE.set(read_memory(address, size)?);
    Ok(())
}
fn cancel_command(m: &mut Mailbox) {
    if let Some(j) = m.command.take() {
        m.ack = Some(Ack {
            token: j.request.token,
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
    });
    original(this, 13)(this, a2, a3, a4)
}
unsafe extern "system" fn select_update(this: usize, a2: usize, a3: usize, a4: usize) -> usize {
    let mut executing: Option<Jump> = None;
    guard(|| {
        let mode = get_mode();
        let is_ready = ready(this);
        if is_ready
            && LIVE_DATABASE.get().is_none()
            && DATABASE_RETRY
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(120)
        {
            let _ = capture_database();
        }
        let mut m = MAILBOX.lock().unwrap();
        m.snapshot.mode = mode;
        m.snapshot.phase = if is_ready {
            Phase::Select
        } else {
            Phase::Other
        };
        if let Some(j) = m.command.as_ref() {
            if j.epoch != m.snapshot.epoch || Some(j.request.mode) != mode {
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
            let difficulty = j.request.chart.map(|c| c.difficulty as i32).unwrap_or(-1);
            let optional = j
                .request
                .chart
                .map(|c| (1u64 << 32) | c.difficulty as u64)
                .unwrap_or(0);
            let mode = if j.request.mode == Mode::DP { 1 } else { 0 };
            if can(r, j.request.song.id, mode, optional) == 0 {
                error = Some("谱面未解锁或当前选曲模式不可用".to_owned());
                return;
            }
            let set: unsafe extern "system" fn(usize, u32, u32, i32) =
                std::mem::transmute(base + 0x7d6150);
            set(r, j.request.song.id, mode, difficulty);
            submitted = true;
        }
    });
    let result = original(this, 15)(this, a2, a3, a4);
    guard(|| {
        if let Some(j) = executing {
            let outcome = if let Some(e) = error {
                Some(Err(e))
            } else if submitted {
                let base = ADAPTER.get().unwrap().base;
                let r = reserve();
                if *((r + 9) as *const u8) == 0 {
                    // A modal/transition won the frame. Withdraw only our reservation.
                    if *((r + 16) as *const u32) == j.request.song.id {
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
                        && *((music + 1660) as *const u32) == j.request.song.id;
                    if let Some(c) = j.request.chart {
                        let player: unsafe extern "system" fn() -> u32 =
                            std::mem::transmute(base + 0x949230);
                        let difficulty: unsafe extern "system" fn(usize, u32) -> u32 =
                            std::mem::transmute(base + 0x607030);
                        success &= difficulty(this + 408, player()) == c.difficulty as u32;
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
                token: j.request.token,
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
