//! Runs only on the native scene thread. Never used by HTTP or worker threads.
use super::{density, native::read_memory};
use crate::{game::SongInfo, host::windows::executable_address};
use anyhow::{Result, ensure};
use std::sync::{
    Mutex,
    atomic::{AtomicU32, Ordering},
};

static CACHE: Mutex<Option<density::Snapshot>> = Mutex::new(None);

// MSVC's shared_ptr publication lock, confirmed at af158c/af159c. A try-lock
// keeps a busy analyzer from blocking the selection frame. Holding it pins the
// thread's immutable analyzer and its vectors until all bounded copies finish.
struct PublicationLock(&'static AtomicU32);
impl Drop for PublicationLock {
    fn drop(&mut self) {
        self.0.fetch_and(!1, Ordering::Release);
    }
}

type AvsMutexFn = unsafe extern "system" fn(u32);
struct RequestLock {
    handle: u32,
    unlock: AvsMutexFn,
}
impl Drop for RequestLock {
    fn drop(&mut self) {
        unsafe {
            (self.unlock)(self.handle);
        }
    }
}

fn capture(base: usize, song_id: u32, request: bool) -> Result<Option<density::Snapshot>> {
    let thread = density::pointer(&read_memory(base + 0xa7d33e0, 8)?, 0);
    if thread == 0 {
        return Ok(None);
    }
    // The thread singleton is created/destroyed on this same game thread.
    let header = read_memory(thread, 16)?;
    ensure!(
        density::pointer(&header, 0) == base + 0xcb5cd0,
        "Unknown analyzer thread"
    );
    let lock = unsafe { &*((base + 0xbaac324) as *const AtomicU32) };
    if lock
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(None);
    }
    let held = PublicationLock(lock);
    let analyzer = density::pointer(&read_memory(thread + 40, 8)?, 0);
    let snapshot = density::read_snapshot(analyzer, song_id, base + 0xcb5cf0, read_memory).ok();
    drop(held);
    if snapshot.is_some() || !request {
        return Ok(snapshot);
    }

    // Mirror 650550's request to MusicDetailDataThread. The native worker owns
    // file I/O and the shared chart scratch buffer; never invoke the constructor
    // or analyzer ourselves. No UI tab needs to be opened or changed.
    let lock_address = density::pointer(&read_memory(base + 0xc91fe0, 8)?, 0);
    let unlock_address = density::pointer(&read_memory(base + 0xc91fe8, 8)?, 0);
    ensure!(
        executable_address(lock_address) && executable_address(unlock_address),
        "AVS mutex imports unavailable"
    );
    let lock_fn: AvsMutexFn = unsafe { std::mem::transmute(lock_address) };
    let unlock_fn: AvsMutexFn = unsafe { std::mem::transmute(unlock_address) };
    let handle = u32::from_le_bytes(header[8..12].try_into().unwrap());
    unsafe {
        lock_fn(handle);
    }
    let _held = RequestLock {
        handle,
        unlock: unlock_fn,
    };
    unsafe {
        ((thread + 16) as *mut u64).write(u64::from(song_id) | (1u64 << 32));
    }
    Ok(None)
}

pub fn apply(base: usize, song: &mut SongInfo, request: bool) {
    let mut cache = CACHE.lock().unwrap();
    if !cache.as_ref().is_some_and(|s| s.song_id == song.id)
        && let Ok(Some(snapshot)) = capture(base, song.id, request)
    {
        *cache = Some(snapshot);
    }
    if let Some(snapshot) = cache.as_ref() {
        snapshot.apply(song);
    }
}
