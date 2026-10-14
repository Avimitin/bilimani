//! DDR WORLD MDX-003-2026022400, x64. All native access runs in scene callbacks.
use crate::{config::Controls, ddr, game::*, host::windows::ModuleImage};
use anyhow::{Result, ensure};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    Memory::{PAGE_READWRITE, VirtualProtect},
    Threading::GetCurrentProcess,
};

const HASH: &str = "89756f03a0ca98c2d42776d08569364514472a97ff89f1b09b0b68f40082d62e";
static STATE: OnceLock<Arc<Native>> = OnceLock::new();
type Callback = unsafe extern "system" fn(usize) -> usize;
#[derive(Default)]
struct Data {
    update: GameUpdate,
    songs: Option<Vec<Song>>,
    pending: Option<Selection>,
    scene: usize,
    playing: u8,
    keys: u8,
}
struct Native {
    base: usize,
    data: Mutex<Data>,
    stopped: AtomicBool,
    menu: AtomicBool,
    controls: bool,
}
struct Adapter(Arc<Native>);

fn read<T: Copy + Default>(address: usize) -> Option<T> {
    if address < 0x10000 {
        return None;
    }
    let mut value = T::default();
    let mut count = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            (&mut value as *mut T).cast(),
            size_of::<T>(),
            &mut count,
        )
    };
    (ok != 0 && count == size_of::<T>()).then_some(value)
}
fn ptr(address: usize) -> Option<usize> {
    read::<usize>(address).filter(|p| *p >= 0x10000)
}
fn string(address: usize) -> Option<String> {
    let len = read::<usize>(address + 16)?;
    let cap = read::<usize>(address + 24)?;
    if len > 4096 || len > cap {
        return None;
    }
    let data = if cap < 16 { address } else { ptr(address)? };
    let mut bytes = vec![0u8; len];
    let mut count = 0;
    if len > 0
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                data as *const _,
                bytes.as_mut_ptr().cast(),
                len,
                &mut count,
            )
        } == 0
    {
        return None;
    }
    if count != len {
        return None;
    }
    Some(
        String::from_utf8(bytes.clone())
            .unwrap_or_else(|_| encoding_rs::SHIFT_JIS.decode(&bytes).0.into_owned()),
    )
}
fn info(address: usize) -> Option<SongInfo> {
    let id = read::<u32>(address + 8)?;
    if id == 0 {
        return None;
    }
    let title = string(address + 24)?;
    if title.is_empty() {
        return None;
    }
    let mut charts = Vec::new();
    for (i, chart) in ddr::CHARTS.iter().enumerate() {
        let native = if i < 5 { i } else { i + 1 };
        let override_level = read::<i32>(address + 476 + native * 4)?;
        let level = if override_level >= 0 {
            override_level
        } else {
            read::<u8>(address + 272 + native)? as i32
        };
        if (1..=20).contains(&level) {
            charts.push(AvailableChart {
                chart: *chart,
                level: level.to_string(),
            });
        }
    }
    Some(SongInfo {
        id,
        title,
        artist: string(address + 104).unwrap_or_default(),
        charts,
    })
}
fn vector(begin: usize, end: usize, stride: usize) -> Option<usize> {
    let bytes = end.checked_sub(begin)?;
    (begin >= 0x10000 && bytes % stride == 0 && bytes / stride <= 10000).then_some(bytes / stride)
}
impl Native {
    fn session(&self) -> Option<usize> {
        ptr(ptr(self.base + 0x6c80a8)?)
    }
    fn mode(&self) -> Option<Mode> {
        match read::<u32>(self.session()? + 4)? {
            0 => Some(ddr::SP),
            1 => Some(ddr::DP),
            _ => None,
        }
    }
    fn catalog(&self) -> Option<Vec<Song>> {
        let db = ptr(self.base + 0x6c9928)?;
        let begin = ptr(db)?;
        let count = vector(begin, ptr(db + 8)?, 600)?;
        if count == 0 {
            return None;
        }
        let mut songs = Vec::with_capacity(count);
        for i in 0..count {
            let address = begin + i * 600;
            if let Some(song) = info(address)
                && !song.charts.is_empty()
            {
                songs.push(Song {
                    id: song.id,
                    title: song.title,
                    search_terms: string(address + 64).into_iter().collect(),
                    charts: song.charts,
                });
            }
        }
        (!songs.is_empty()).then_some(songs)
    }
    fn select(
        &self,
        scene: usize,
        selection: &Selection,
    ) -> std::result::Result<(), SelectionError> {
        let model = ptr(scene + 136).ok_or(SelectionError::Unavailable)?;
        if read::<u32>(model + 264) != Some(1) {
            return Err(SelectionError::Unavailable);
        }
        let view = ptr(scene + 144).ok_or(SelectionError::Unavailable)?;
        let panel = ptr(view + 1088).ok_or(SelectionError::Unavailable)?;
        if read::<u8>(panel + 297) != Some(1) {
            return Err(SelectionError::Unavailable);
        }
        let list = ptr(panel + 192).ok_or(SelectionError::Unavailable)?;
        let begin = ptr(list + 104).ok_or(SelectionError::Unavailable)?;
        let count = vector(
            begin,
            ptr(list + 112).ok_or(SelectionError::Unavailable)?,
            8,
        )
        .ok_or(SelectionError::Unavailable)?;
        for index in 0..count {
            let Some(item) = ptr(begin + index * 8) else {
                continue;
            };
            if ptr(item + 40) != Some(self.base + 0x35beb0) {
                continue;
            }
            let Some(metadata) = ptr(item + 328) else {
                continue;
            };
            let Some(song) = ptr(metadata).and_then(info) else {
                continue;
            };
            if song.id != selection.song_id {
                continue;
            }
            let valid: unsafe extern "system" fn(usize, u32) -> u8 =
                unsafe { std::mem::transmute(self.base + 0x49700) };
            if unsafe { valid(list, index as u32) } == 0 {
                return Err(SelectionError::Unavailable);
            }
            if let Some(chart) = selection.chart {
                if !song.charts.iter().any(|c| c.chart == chart) {
                    return Err(SelectionError::InvalidChart);
                }
                let difficulty = ddr::CHARTS
                    .iter()
                    .position(|c| *c == chart)
                    .ok_or(SelectionError::InvalidChart)?;
                let difficulty = if difficulty < 5 {
                    difficulty
                } else {
                    difficulty - 4
                };
                let side = self
                    .session()
                    .and_then(|s| read::<u32>(s + 8))
                    .filter(|s| *s < 2)
                    .ok_or(SelectionError::Unavailable)?;
                let native_info = ptr(metadata).ok_or(SelectionError::Unavailable)?;
                if ptr(native_info) != Some(self.base + 0x355338) {
                    return Err(SelectionError::Unavailable);
                }
                let available: unsafe extern "system" fn(usize, u32, u32, u32) -> u8 =
                    unsafe { std::mem::transmute(self.base + 0x19fdc0) };
                if unsafe {
                    available(
                        native_info,
                        u32::from(selection.mode == ddr::DP),
                        difficulty as u32,
                        side,
                    )
                } == 0
                {
                    return Err(SelectionError::Unavailable);
                }
                let set: unsafe extern "system" fn(usize, u32, u32) =
                    unsafe { std::mem::transmute(self.base + 0xf3460) };
                unsafe {
                    set(model, difficulty as u32, side);
                }
            }
            let set: unsafe extern "system" fn(usize, u32) =
                unsafe { std::mem::transmute(self.base + 0x48370) };
            unsafe {
                set(list, index as u32);
            }
            return if self.session().and_then(|s| read::<u32>(s + 24)) == Some(song.id) {
                Ok(())
            } else {
                Err(SelectionError::Rejected)
            };
        }
        Err(SelectionError::Unavailable)
    }
    fn frame(&self, scene: usize) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut data) = self.data.lock() else {
            return;
        };
        if read::<u8>(scene + 104) != Some(1) {
            return;
        }
        if data.scene != scene || data.update.snapshot.phase != Phase::Select {
            data.scene = scene;
            data.update.snapshot.epoch += 1;
        }
        let mode = self.mode();
        if mode != data.update.snapshot.mode {
            data.update.snapshot.epoch += 1;
        }
        data.update.snapshot.mode = mode;
        data.update.snapshot.phase = Phase::Select;
        let unblocked = ptr(self.base + 0x6c9918).and_then(|p| read::<u8>(p + 36)) == Some(0);
        data.update.snapshot.can_open_menu = unblocked;
        data.playing = 0;
        if data.songs.is_none() {
            data.songs = self.catalog();
        }
        if unblocked && let Some(selection) = data.pending.take() {
            let result = if selection.epoch != data.update.snapshot.epoch {
                Err(SelectionError::Expired)
            } else if mode != Some(selection.mode) {
                Err(SelectionError::ModeChanged)
            } else {
                self.select(scene, &selection)
            };
            data.update.selection_result = Some(SelectionResult {
                token: selection.token,
                result: Some(result),
            });
        }
        data.update.now_playing.song = ptr(scene + 136)
            .and_then(|m| ptr(m + 400))
            .and_then(ptr)
            .and_then(info);
        // Native model +4/+8 contains each side's selected difficulty. Keep
        // this owned value through gameplay; never derive it from requests.
        let selected_chart = self
            .session()
            .and_then(|session| read::<u32>(session + 8))
            .filter(|side| *side < 2)
            .and_then(|side| {
                ptr(scene + 136).and_then(|model| read::<u32>(model + 4 + side as usize * 4))
            })
            .and_then(|difficulty| mode.and_then(|mode| ddr::native_chart(mode, difficulty)));
        data.update.now_playing.selected_chart =
            data.update.now_playing.song.as_ref().and_then(|song| {
                song.charts
                    .iter()
                    .find(|entry| Some(entry.chart) == selected_chart)
                    .cloned()
            });
        data.update.user.players = (0..2)
            .filter_map(|side| {
                let player = ptr(self.base + 0x6c9a80 + side * 8).and_then(ptr)?;
                (read::<u8>(player + 4) == Some(1)).then_some(ddr::user::Player {
                    side: side as u8 + 1,
                    name: String::new(),
                })
            })
            .collect();
    }
}
fn callback(scene: usize, original: usize, kind: u8) -> usize {
    let Some(state) = STATE.get() else { return 0 };
    // The selection scene owns its input dispatch. Freeze it while the queue
    // menu is open so menu keyboard presses cannot also navigate/confirm songs.
    if kind == 0 && state.menu.load(Ordering::Acquire) && !state.stopped.load(Ordering::Acquire) {
        if std::panic::catch_unwind(|| state.frame(scene)).is_err() {
            state.stopped.store(true, Ordering::Release);
        }
        return 0;
    }
    if kind == 1
        && let Ok(mut data) = state.data.lock()
    {
        data.scene = 0;
        data.pending = None;
        data.update.snapshot.epoch += 1;
        data.update.snapshot.phase = Phase::Other;
        data.update.snapshot.can_open_menu = false;
    }
    let function: Callback = unsafe { std::mem::transmute(state.base + original) };
    let result = unsafe { function(scene) };
    if !state.stopped.load(Ordering::Acquire) {
        let guarded = std::panic::catch_unwind(|| {
            if kind == 0 {
                state.frame(scene);
            }
            if kind == 2
                && let Some(side) = read::<u32>(scene + 132).filter(|s| *s < 2)
                && ptr(state.base + 0x6c9a80 + side as usize * 8)
                    .and_then(ptr)
                    .and_then(|p| read::<u8>(p + 4))
                    == Some(1)
                && let Ok(mut data) = state.data.lock()
            {
                if data.playing == 0 {
                    data.update.plays += 1;
                    data.update.snapshot.epoch += 1;
                }
                data.playing |= 1 << side;
                data.update.snapshot.phase = Phase::Playing;
                data.update.snapshot.can_open_menu = false;
            }
            if kind == 3
                && let Some(side) = read::<u32>(scene + 132).filter(|s| *s < 2)
                && let Ok(mut data) = state.data.lock()
            {
                data.playing &= !(1 << side);
                if data.playing == 0 {
                    data.update.snapshot.phase = Phase::Other;
                    data.update.snapshot.epoch += 1;
                    data.update.user.players.clear();
                }
            }
        });
        if guarded.is_err() {
            state.stopped.store(true, Ordering::Release);
        }
    }
    result
}
unsafe extern "system" fn select_update(scene: usize) -> usize {
    callback(scene, 0xf1a10, 0)
}
unsafe extern "system" fn select_exit(scene: usize) -> usize {
    callback(scene, 0xf1d40, 1)
}
unsafe extern "system" fn play_update(scene: usize) -> usize {
    callback(scene, 0x588b0, 2)
}
unsafe extern "system" fn play_exit(scene: usize) -> usize {
    callback(scene, 0x59540, 3)
}

pub fn attach(module: &str, controls: &Controls) -> Result<Box<dyn GameAdapter>> {
    let image = ModuleImage::load(module)?;
    ensure!(
        image.sha256 == HASH,
        "Unsupported DDR DLL: expected MDX-003-2026022400 x64 ({HASH}), found {}",
        image.sha256
    );
    let signatures: [(usize, [u8; 16]); 4] = [
        (
            0x48370,
            [
                0x48, 0x89, 0x5c, 0x24, 8, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24,
                0x18, 0x48,
            ],
        ),
        (
            0x49700,
            [
                0x48, 0x89, 0x5c, 0x24, 0x20, 0x57, 0x48, 0x83, 0xec, 0x20, 0x4c, 0x8b, 0x49, 0x68,
                0x4c, 0x8b,
            ],
        ),
        (
            0xf3460,
            [
                0x48, 0x8b, 5, 0x41, 0x4c, 0x5d, 0, 0x45, 0x33, 0xc9, 0x4c, 0x8b, 0xd9, 0x48, 0x8b,
                8,
            ],
        ),
        (
            0x19fdc0,
            [
                0x48, 0x89, 0x5c, 0x24, 8, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24,
                0x18, 0x57,
            ],
        ),
    ];
    for (rva, expected) in signatures {
        ensure!(
            read::<[u8; 16]>(image.base + rva) == Some(expected),
            "DDR native entry mismatch at {rva:x}"
        );
    }
    let hooks = [
        (0x355188, 0xf1a10, select_update as *const () as usize),
        (0x355180, 0xf1d40, select_exit as *const () as usize),
        (0x348468, 0x588b0, play_update as *const () as usize),
        (0x348460, 0x59540, play_exit as *const () as usize),
    ];
    for (slot, original, _) in hooks {
        ensure!(
            ptr(image.base + slot) == Some(image.base + original),
            "DDR scene hook mismatch"
        );
    }
    let state = Arc::new(Native {
        base: image.base,
        data: Mutex::new(Data::default()),
        stopped: AtomicBool::new(false),
        menu: AtomicBool::new(false),
        controls: controls.enabled,
    });
    ensure!(STATE.set(state.clone()).is_ok(), "DDR already attached");
    for (slot, _, replacement) in hooks {
        let address = (image.base + slot) as *mut usize;
        let mut old = 0;
        let writable = unsafe { VirtualProtect(address.cast(), 8, PAGE_READWRITE, &mut old) } != 0;
        if !writable {
            state.stopped.store(true, Ordering::Release);
        }
        ensure!(writable, "Cannot protect DDR hook");
        unsafe {
            std::ptr::write_volatile(address, replacement);
        }
        let mut ignored = 0;
        let restored = unsafe { VirtualProtect(address.cast(), 8, old, &mut ignored) } != 0;
        if !restored {
            state.stopped.store(true, Ordering::Release);
        }
        ensure!(restored, "Cannot restore DDR hook protection");
    }
    Ok(Box::new(Adapter(state)))
}
impl GameAdapter for Adapter {
    fn set_menu_open(&self, open: bool) {
        self.0.menu.store(open, Ordering::Release);
    }
    fn cancel_selection(&self) {
        if let Ok(mut d) = self.0.data.lock() {
            d.pending = None;
        }
    }
    fn startup_messages(&self) -> Vec<(&'static str, String)> {
        vec![(
            "DDR",
            "WORLD 2026022400 x64; song jumps require the song in the current folder/filter".into(),
        )]
    }
    fn catalog(&self, _: &Path) -> Result<Option<Vec<Song>>> {
        Ok(self.0.data.lock().ok().and_then(|d| d.songs.clone()))
    }
    fn poll(&self) -> GameUpdate {
        let Ok(mut d) = self.0.data.lock() else {
            return GameUpdate::default();
        };
        use windows_sys::Win32::UI::{
            Input::KeyboardAndMouse::GetAsyncKeyState,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        let mut process = 0;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), &mut process);
        }
        let foreground =
            process == unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };
        let mut keys = 0;
        if foreground && self.0.controls {
            for (i, key) in [0x77, 0x26, 0x28, 0x25, 0x27, 0x0d, 0x1b]
                .iter()
                .enumerate()
            {
                if unsafe { GetAsyncKeyState(*key) } < 0 {
                    keys |= 1 << i;
                }
            }
        }
        let edges = keys & !d.keys;
        d.keys = keys;
        let navigation = if self.0.menu.load(Ordering::Acquire) {
            [
                Navigation::Up,
                Navigation::Down,
                Navigation::Left,
                Navigation::Right,
                Navigation::Confirm,
                Navigation::Back,
            ]
            .into_iter()
            .enumerate()
            .filter_map(|(i, n)| (edges & (2 << i) != 0).then_some(n))
            .collect()
        } else {
            Vec::new()
        };
        GameUpdate {
            snapshot: d.update.snapshot,
            now_playing: d.update.now_playing.clone(),
            user: d.update.user.clone(),
            plays: d.update.plays,
            selection_result: d.update.selection_result.take(),
            toggle_menu: edges & 1 != 0,
            navigation,
        }
    }
    fn submit(&self, selection: Selection) {
        if let Ok(mut d) = self.0.data.lock() {
            d.pending = Some(selection);
        }
    }
    fn diagnostics(&self) -> String {
        format!("DDR controls enabled: {}", self.0.controls)
    }
    fn disabled(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
        self.cancel_selection();
    }
}
