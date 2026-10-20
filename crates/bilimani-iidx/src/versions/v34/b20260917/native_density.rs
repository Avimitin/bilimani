//! Sample the native job's published analyzer on the scene thread.
//! The selection widget and stage initialization already enqueue analysis in 34.
use super::{density, native::read_memory};
use crate::game::SongInfo;
use anyhow::Result;
use std::sync::{
    Mutex,
    atomic::{AtomicU32, Ordering},
};

static CACHE: Mutex<Option<density::Snapshot>> = Mutex::new(None);
const MODEL: usize = 0xb3e9fa0;
const PUBLICATION_LOCK: usize = 0xc51b324;
const ANALYZER_VTABLE: usize = 0xc8b2d8;

struct PublicationLock(&'static AtomicU32);
impl Drop for PublicationLock {
    fn drop(&mut self) {
        self.0.fetch_and(!1, Ordering::Release);
    }
}

fn capture(base: usize, song_id: u32) -> Result<Option<density::Snapshot>> {
    let model = density::pointer(&read_memory(base + MODEL, 8)?, 0);
    if model == 0 {
        return Ok(None);
    }
    // 623210 publishes model +144 under the process-wide shared_ptr spin lock.
    // A try-lock pins the immutable analyzer and vectors for bounded copies.
    let lock = unsafe { &*((base + PUBLICATION_LOCK) as *const AtomicU32) };
    if lock
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(None);
    }
    let _held = PublicationLock(lock);
    let analyzer = density::pointer(&read_memory(model + 144, 8)?, 0);
    Ok(density::read_snapshot(analyzer, song_id, base + ANALYZER_VTABLE, read_memory).ok())
}

pub fn apply(base: usize, song: &mut SongInfo) {
    let mut cache = CACHE.lock().unwrap();
    if cache.as_ref().is_some_and(|s| s.song_id != song.id) {
        *cache = None;
    }
    // Difficulty changes publish another pair for the same song. Merge new
    // immutable chart data rather than freezing the first analyzer we observe.
    if let Ok(Some(snapshot)) = capture(base, song.id) {
        match cache.as_mut() {
            Some(previous) => {
                for index in 0..10 {
                    if snapshot.charts[index].is_some() {
                        previous.charts[index] = snapshot.charts[index].clone();
                    }
                    if snapshot.lane_counts[index].is_some() {
                        previous.lane_counts[index] = snapshot.lane_counts[index].clone();
                    }
                }
            }
            None => *cache = Some(snapshot),
        }
    }
    if let Some(snapshot) = cache.as_ref() {
        snapshot.apply(song);
    }
}
