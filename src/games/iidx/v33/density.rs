//! Immutable MusicDetailDataAnalyzer snapshots. See docs/game-analysis.md.
//! The caller must hold the game's shared_ptr lock while copying these pointers.
use crate::game::{LaneCounts, LaneSideCounts, NoteDensity, SongInfo};
use crate::games::iidx::CHARTS;
use anyhow::{Result, ensure};

pub const ANALYZER_SIZE: usize = 256;
pub const DETAIL_SIZE: usize = 116;
pub const BIN_MS: u32 = 1000;
const MAX_DURATION_MS: u32 = 3_600_000;

pub struct Snapshot {
    pub song_id: u32,
    pub charts: [Option<NoteDensity>; 10],
    pub lane_counts: [Option<LaneCounts>; 10],
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
pub(crate) fn pointer(bytes: &[u8], offset: usize) -> usize {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize
}

fn vector(
    header: &[u8],
    offset: usize,
    max_len: usize,
    read: &impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Vec<u32>> {
    let start = pointer(header, offset);
    let end = pointer(header, offset + 8);
    let capacity = pointer(header, offset + 16);
    ensure!(
        start != 0 && start.is_multiple_of(4),
        "Invalid density pointer"
    );
    ensure!(end >= start && capacity >= end, "Invalid density vector");
    let size = end - start;
    ensure!(
        size.is_multiple_of(4) && (1..=max_len * 4).contains(&size),
        "Invalid density length"
    );
    let data = read(start, size)?;
    ensure!(data.len() == size, "Incomplete density copy");
    let values: Vec<_> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    ensure!(values.iter().all(|&n| n <= 10000), "Invalid density count");
    Ok(values)
}

pub fn read_chart(
    address: usize,
    read: &impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<NoteDensity> {
    ensure!(address != 0, "Missing detail data");
    let header = read(address, DETAIL_SIZE)?;
    read_density(&header, read)
}

fn read_density(
    header: &[u8],
    read: &impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<NoteDensity> {
    ensure!(header.len() == DETAIL_SIZE, "Incomplete detail header");
    let duration_ms = word(header, 112);
    ensure!(
        (1..=MAX_DURATION_MS).contains(&duration_ms),
        "Invalid chart duration"
    );
    let max_len = (duration_ms / BIN_MS + 1) as usize;
    let notes = vector(header, 0, max_len, read)?;
    let scratch = vector(header, 24, max_len, read)?;
    ensure!(notes.len() == scratch.len(), "Density vectors disagree");
    ensure!(
        scratch.iter().zip(&notes).all(|(s, n)| s <= n),
        "Scratch exceeds total"
    );
    let total: u64 = notes.iter().map(|&n| u64::from(n)).sum();
    ensure!((1..=1000000).contains(&total), "Empty or invalid histogram");
    Ok(NoteDensity {
        bin_ms: BIN_MS,
        duration_ms,
        notes,
        scratch,
    })
}

/// 632d20 accumulates two arrays of eight i32 at +48/+80. 64f440 passes
/// these arrays to 650030's key1..7/scratch widgets. Copy before RANDOM.
pub fn read_lane_counts(header: &[u8], double: bool) -> Result<LaneCounts> {
    ensure!(header.len() == DETAIL_SIZE, "Incomplete detail header");
    let values: [u32; 16] = std::array::from_fn(|i| word(header, 48 + i * 4));
    let total: u64 = values.iter().map(|&n| u64::from(n)).sum();
    ensure!(
        (1..=1_000_000).contains(&total),
        "Empty or invalid lane totals"
    );
    ensure!(
        double || values[8..].iter().all(|&n| n == 0),
        "Unexpected SP second side"
    );
    let sides = (0..if double { 2 } else { 1 })
        .map(|i| LaneSideCounts {
            side: i as u8 + 1,
            keys: values[i * 8..i * 8 + 7].try_into().unwrap(),
            scratch: values[i * 8 + 7],
        })
        .collect();
    Ok(LaneCounts { sides })
}

pub fn read_snapshot(
    address: usize,
    song_id: u32,
    vtable: usize,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Snapshot> {
    ensure!(address != 0, "Analyzer not ready");
    let header = read(address, ANALYZER_SIZE)?;
    ensure!(header.len() == ANALYZER_SIZE, "Incomplete analyzer header");
    ensure!(
        pointer(&header, 0) == vtable && word(&header, 8) == song_id,
        "Analyzer identity mismatch"
    );
    let mut lane_counts = std::array::from_fn(|_| None);
    let charts = std::array::from_fn(|index| {
        let entry = 16 + index * 24;
        if header[entry + 16] != 1 || pointer(&header, entry) == 0 {
            return None;
        }
        let detail = read(pointer(&header, entry), DETAIL_SIZE).ok()?;
        let density = read_density(&detail, &read).ok();
        lane_counts[index] = read_lane_counts(&detail, index >= 5).ok().filter(|counts| {
            // Both native accumulators count the same events. A mismatch must
            // not publish plausible but corrupt lane totals or break density.
            density.as_ref().is_none_or(|d| {
                let total: u64 = counts
                    .sides
                    .iter()
                    .flat_map(|s| s.keys.iter().chain(std::iter::once(&s.scratch)))
                    .map(|&n| u64::from(n))
                    .sum();
                let scratch: u64 = counts.sides.iter().map(|s| u64::from(s.scratch)).sum();
                total == d.notes.iter().map(|&n| u64::from(n)).sum::<u64>()
                    && scratch == d.scratch.iter().map(|&n| u64::from(n)).sum::<u64>()
            })
        });
        density
    });
    Ok(Snapshot {
        song_id,
        charts,
        lane_counts,
    })
}

impl Snapshot {
    pub fn apply(&self, song: &mut SongInfo) {
        if song.id != self.song_id {
            return;
        }
        for info in &mut song.charts {
            if let Some(index) = CHARTS.iter().position(|&chart| chart == info.chart) {
                info.density = self.charts[index].clone();
                info.lane_counts = self.lane_counts[index].clone();
            }
        }
    }
}
