use anyhow::{Result, ensure};
use bilimani::game::SongPhase;
use bilimani::games::iidx::{
    self,
    v33::{density, song_info},
};

struct Memory(Vec<u8>);
impl Memory {
    fn new() -> Self {
        Self(vec![0; 8192])
    }
    fn u32(&mut self, at: usize, n: u32) {
        self.0[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, n: usize) {
        self.0[at..at + 8].copy_from_slice(&(n as u64).to_le_bytes());
    }
    fn read(&self, at: usize, size: usize) -> Result<Vec<u8>> {
        ensure!(size <= 4 * 3601, "Unbounded read");
        Ok(self
            .0
            .get(at..at.checked_add(size).unwrap())
            .ok_or_else(|| anyhow::anyhow!("Unmapped"))?
            .to_vec())
    }
    fn detail(&mut self, at: usize, notes: &[u32], scratch: &[u32], duration: u32) {
        for (offset, values, address) in [(0, notes, at + 128), (24, scratch, at + 256)] {
            self.ptr(at + offset, address);
            self.ptr(at + offset + 8, address + values.len() * 4);
            self.ptr(at + offset + 16, address + values.len() * 4);
            for (i, &n) in values.iter().enumerate() {
                self.u32(address + i * 4, n);
            }
        }
        self.u32(at + 112, duration);
        self.u32(
            at + 48,
            notes.iter().sum::<u32>() - scratch.iter().sum::<u32>(),
        );
        self.u32(at + 76, scratch.iter().sum());
    }
}

#[test]
fn preserves_native_buckets_scratch_subset_long_note_weight_and_silent_tail() {
    let mut memory = Memory::new();
    // Already-native counts: 2 at t=0 is one CN's native weight, 42 is not
    // clipped to the game's visual maximum of 30. Silent ending is not removed.
    memory.detail(512, &[2, 0, 42, 3], &[0, 0, 2, 1], 6500);
    let density = density::read_chart(512, &|a, n| memory.read(a, n)).unwrap();
    let json = serde_json::to_value(density).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"bin_ms":1000,"duration_ms":6500,"notes":[2,0,42,3],"scratch":[0,0,2,1]})
    );
}

#[test]
fn maps_all_ten_optional_charts_by_identity_and_rejects_stale_song() {
    let mut memory = Memory::new();
    memory.ptr(256, 1234);
    memory.u32(264, 33001);
    for index in 0..10 {
        let detail = 512 + index * 512;
        memory.ptr(256 + 16 + index * 24, detail);
        memory.0[256 + 32 + index * 24] = 1;
        memory.detail(detail, &[index as u32 + 1], &[0], 1000);
    }
    let snapshot = density::read_snapshot(256, 33001, 1234, |a, n| memory.read(a, n)).unwrap();
    let mut record = vec![0; song_info::RECORD_SIZE];
    record[0] = b'A';
    record[0x67c..0x680].copy_from_slice(&33001u32.to_le_bytes());
    record[0x3ec..0x3f6].fill(10);
    let mut song = song_info::decode(&record, iidx::DP, [None, Some(4)], SongPhase::Playing)
        .unwrap()
        .song
        .unwrap();
    snapshot.apply(&mut song);
    for (i, chart) in song.charts.iter().enumerate() {
        assert_eq!(chart.density.as_ref().unwrap().notes, [i as u32 + 1]);
        let sides = &chart.lane_counts.as_ref().unwrap().sides;
        assert_eq!(sides.len(), if i < 5 { 1 } else { 2 });
        assert_eq!(sides[0].keys, [i as u32 + 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(sides[0].side, 1);
    }
    song.id = 33002;
    for chart in &mut song.charts {
        chart.density = None;
        chart.lane_counts = None;
    }
    snapshot.apply(&mut song);
    assert!(song.charts.iter().all(|c| c.density.is_none()));
    assert!(song.charts.iter().all(|c| c.lane_counts.is_none()));
    assert!(density::read_snapshot(256, 33002, 1234, |a, n| memory.read(a, n)).is_err());
    assert!(density::read_snapshot(256, 33001, 9999, |a, n| memory.read(a, n)).is_err());
    // An absent/invalid chart must not discard a different valid difficulty.
    memory.0[256 + 32 + 3 * 24] = 0;
    memory.u32(512 + 8 * 512 + 112, 0);
    let snapshot = density::read_snapshot(256, 33001, 1234, |a, n| memory.read(a, n)).unwrap();
    assert!(snapshot.charts[3].is_none() && snapshot.charts[8].is_none());
    assert!(snapshot.charts[9].is_some());
}

#[test]
fn lane_totals_preserve_both_chart_sides_and_native_scratch_counts() {
    let mut header = vec![0; density::DETAIL_SIZE];
    let values: [u32; 16] = [10, 20, 30, 40, 50, 60, 70, 8, 11, 21, 31, 41, 51, 61, 71, 9];
    for (i, n) in values.iter().enumerate() {
        header[48 + i * 4..52 + i * 4].copy_from_slice(&n.to_le_bytes());
    }
    let counts = density::read_lane_counts(&header, true).unwrap();
    assert_eq!(
        serde_json::to_value(counts).unwrap(),
        serde_json::json!({"sides": [
            {"side": 1, "keys": [10,20,30,40,50,60,70], "scratch": 8},
            {"side": 2, "keys": [11,21,31,41,51,61,71], "scratch": 9}
        ]})
    );
    assert!(density::read_lane_counts(&header, false).is_err());
    header[80..112].fill(0);
    let counts = density::read_lane_counts(&header, false).unwrap();
    assert_eq!(counts.sides.len(), 1);
    assert_eq!(counts.sides[0].keys, [10, 20, 30, 40, 50, 60, 70]);
    header[48..52].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(density::read_lane_counts(&header, false).is_err());
    assert!(density::read_lane_counts(&header[..112], false).is_err());
    header.fill(0);
    assert!(density::read_lane_counts(&header, true).is_err());
}

#[test]
fn lane_counts_and_density_fail_independently_without_accepting_disagreeing_totals() {
    let mut memory = Memory::new();
    memory.ptr(256, 1234);
    memory.u32(264, 33001);
    memory.ptr(272, 512);
    memory.0[288] = 1;
    memory.detail(512, &[2, 4], &[0, 2], 2000);
    let capture =
        |m: &Memory| density::read_snapshot(256, 33001, 1234, |a, n| m.read(a, n)).unwrap();
    assert_eq!(
        capture(&memory).lane_counts[0].as_ref().unwrap().sides[0].scratch,
        2
    );
    memory.u32(512 + 48, 5); // Plausible values, but sum no longer agrees.
    let bad = capture(&memory);
    assert!(bad.charts[0].is_some() && bad.lane_counts[0].is_none());
    memory.u32(512 + 48, 4);
    memory.u32(512 + 76, u32::MAX);
    assert!(capture(&memory).lane_counts[0].is_none());
    memory.u32(512 + 76, 2);
    memory.ptr(512, 0); // Invalid vector must not hide independently valid counts.
    let bad = capture(&memory);
    assert!(bad.charts[0].is_none() && bad.lane_counts[0].is_some());
    memory.0[288] = 0;
    let missing = capture(&memory);
    assert!(missing.charts[0].is_none() && missing.lane_counts[0].is_none());
}

#[test]
fn malformed_unloaded_and_unbounded_native_data_is_rejected() {
    for case in 0..9 {
        let mut memory = Memory::new();
        memory.detail(512, &[1, 2], &[0, 1], 2000);
        match case {
            0 => memory.ptr(512, 0),
            1 => memory.ptr(520, 639), // end before start
            2 => memory.ptr(528, 640), // capacity before end
            3 => memory.ptr(520, usize::MAX),
            4 => memory.u32(624, 0),
            5 => memory.u32(624, u32::MAX),
            6 => memory.u32(768, 2), // scratch > notes
            7 => memory.u32(640, u32::MAX),
            _ => memory.ptr(544, 772), // lengths differ
        }
        assert!(
            density::read_chart(512, &|a, n| memory.read(a, n)).is_err(),
            "case {case}"
        );
    }
    let mut memory = Memory::new();
    memory.detail(512, &[0], &[0], 2000);
    assert!(density::read_chart(512, &|a, n| memory.read(a, n)).is_err());
    assert!(density::read_chart(512, &|_, _| Ok(vec![])).is_err());
    assert!(density::read_snapshot(256, 1, 1, |_, _| Ok(vec![])).is_err());
}
