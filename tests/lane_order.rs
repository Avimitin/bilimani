use anyhow::{Result, bail};
use bilimani::{
    game::{LaneOrderStatus, PlayerChart, RandomMode, SongPhase},
    games::iidx::{CHARTS, v33::lane_order},
};
use std::collections::BTreeMap;

const BASE: usize = 0x400000;
const OPTIONS: usize = 0x10000;
struct Memory(BTreeMap<usize, Vec<u8>>);
impl Memory {
    fn new() -> Self {
        let mut memory = Self(BTreeMap::new());
        memory.0.insert(
            BASE + lane_order::OPTIONS_RVA,
            (OPTIONS as u64).to_le_bytes().to_vec(),
        );
        memory.0.insert(OPTIONS, vec![0; lane_order::OPTIONS_SIZE]);
        memory.0.get_mut(&OPTIONS).unwrap()[..8]
            .copy_from_slice(&((BASE + lane_order::OPTIONS_VTABLE) as u64).to_le_bytes());
        memory.0.insert(BASE + lane_order::CONTEXT_RVA, vec![0; 24]);
        memory.0.insert(BASE + lane_order::TABLE_RVA, vec![0; 64]);
        memory.set(BASE + lane_order::CONTEXT_RVA, 16, 1);
        for side in 0..2 {
            memory.table(side, [0, 1, 2, 3, 4, 5, 6, 7]);
        }
        memory
    }
    fn set(&mut self, at: usize, offset: usize, value: u32) {
        self.0.get_mut(&at).unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn table(&mut self, side: usize, values: [u32; 8]) {
        for (i, value) in values.into_iter().enumerate() {
            self.set(BASE + lane_order::TABLE_RVA, side * 32 + i * 4, value);
        }
    }
    fn options(&mut self, side: usize, dp: bool, random: u32, mirror: u32) {
        let block = 8 + 180 * if dp { 2 } else { side };
        self.set(OPTIONS, block + 48 + side * 4, random);
        self.set(OPTIONS, block + 56 + side * 4, mirror);
    }
    fn read(&self, at: usize, size: usize) -> Result<Vec<u8>> {
        assert!(size <= lane_order::OPTIONS_SIZE);
        if let Some((&start, bytes)) = self.0.range(..=at).next_back()
            && let Some(part) = bytes.get(at - start..at - start + size)
        {
            return Ok(part.to_vec());
        }
        bail!("Unmapped snapshot")
    }
    fn capture(&self, phase: SongPhase, players: &[PlayerChart]) -> Vec<bilimani::game::LaneOrder> {
        lane_order::capture(BASE, phase, players, |a, n| self.read(a, n)).unwrap()
    }
}
fn player(side: u8, index: usize) -> PlayerChart {
    PlayerChart {
        side,
        chart: CHARTS[index],
    }
}

#[test]
fn inverts_native_mapping_and_never_mistakes_source_order_for_display_order() {
    let mut m = Memory::new();
    m.options(0, false, 1, 0);
    m.table(0, [4, 3, 0, 1, 2, 5, 6, 7]);
    let lanes = m.capture(SongPhase::Playing, &[player(1, 3)]);
    assert_eq!(lanes[0].keys, Some([3, 4, 5, 2, 1, 6, 7]));
    assert_eq!(lanes[0].status, LaneOrderStatus::Ready);
    assert_eq!(lanes[0].random, RandomMode::Random);
    let json = serde_json::to_value(&lanes[0]).unwrap();
    assert_eq!(json["keys"], serde_json::json!([3, 4, 5, 2, 1, 6, 7]));
    assert_eq!(json["random"], "random");
}

#[test]
fn normal_loading_and_gameplay_transitions_keep_the_current_lane_order() {
    let mut m = Memory::new();
    let players = [player(2, 3)];
    m.set(BASE + lane_order::CONTEXT_RVA, 16, 0);
    m.set(BASE + lane_order::CONTEXT_RVA, 20, 1);
    m.options(1, false, 1, 0);
    m.table(1, [4, 3, 0, 1, 2, 5, 6, 7]);
    // IDA: 90ffc0 sets this byte on successful sound loading; 9103a0 also
    // sets it during normal stage flow. It is not a chart-load error flag.
    for transition in [0, 1, 1, 0, 1] {
        m.0.insert(BASE + 0xaab19ae, vec![transition]);
        let lanes = m.capture(SongPhase::Playing, &players);
        assert_eq!(lanes[0].status, LaneOrderStatus::Ready);
        assert_eq!(lanes[0].keys, Some([3, 4, 5, 2, 1, 6, 7]));
    }
    // A retry must still replace the map, rather than holding an old snapshot.
    m.table(1, [6, 5, 4, 3, 2, 1, 0, 7]);
    assert_eq!(
        m.capture(SongPhase::Playing, &players)[0].keys,
        Some([7, 6, 5, 4, 3, 2, 1])
    );
    m.table(1, [0; 8]);
    assert_eq!(
        m.capture(SongPhase::Playing, &players)[0].status,
        LaneOrderStatus::Unavailable
    );
    assert!(m.capture(SongPhase::Idle, &players).is_empty());
}

#[test]
fn selection_distinguishes_normal_mirror_pending_random_and_dynamic_srandom() {
    let mut m = Memory::new();
    let players = [player(1, 3)];
    assert_eq!(
        m.capture(SongPhase::Selecting, &players)[0].keys,
        Some([1, 2, 3, 4, 5, 6, 7])
    );
    m.options(0, false, 0, 1);
    assert_eq!(
        m.capture(SongPhase::Selecting, &players)[0].keys,
        Some([7, 6, 5, 4, 3, 2, 1])
    );
    m.options(0, false, 2, 1);
    let pending = m.capture(SongPhase::Selecting, &players);
    assert_eq!(pending[0].status, LaneOrderStatus::Pending);
    assert_eq!(pending[0].keys, None);
    m.options(0, false, 3, 0);
    for phase in [SongPhase::Selecting, SongPhase::Playing] {
        let lanes = m.capture(phase, &players);
        assert_eq!(lanes[0].status, LaneOrderStatus::Dynamic);
        assert_eq!(lanes[0].keys, None);
    }
    assert!(m.capture(SongPhase::Idle, &players).is_empty());
    assert!(m.capture(SongPhase::Playing, &[]).is_empty());
}

#[test]
fn dp_uses_both_physical_sides_and_the_dp_options_without_double_applying_mirror() {
    let mut m = Memory::new();
    m.set(BASE + lane_order::CONTEXT_RVA, 4, 1);
    m.options(0, true, 1, 0);
    m.options(1, true, 2, 1);
    m.table(0, [4, 3, 0, 1, 2, 5, 6, 7]);
    m.table(1, [6, 5, 4, 3, 2, 1, 0, 7]);
    let lanes = m.capture(SongPhase::Playing, &[player(2, 8)]);
    assert_eq!(lanes.iter().map(|p| p.side).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(lanes[0].random, RandomMode::Random);
    assert_eq!(lanes[1].random, RandomMode::RRandom);
    assert!(lanes[1].mirror);
    assert_eq!(lanes[1].keys, Some([7, 6, 5, 4, 3, 2, 1]));
    m.set(BASE + lane_order::CONTEXT_RVA, 4, 0);
    m.options(1, false, 1, 0);
    let sp = m.capture(SongPhase::Playing, &[player(2, 3)]);
    assert_eq!(sp.len(), 1);
    assert_eq!(sp[0].side, 2);
    assert_eq!(sp[0].random, RandomMode::Random);
}

#[test]
fn native_special_and_battle_reflections_happen_after_the_generated_permutation() {
    let mut m = Memory::new();
    m.options(1, false, 1, 0);
    m.set(BASE + lane_order::CONTEXT_RVA, 0, 7);
    m.set(BASE + lane_order::CONTEXT_RVA, 16, 0);
    m.set(BASE + lane_order::CONTEXT_RVA, 20, 1);
    assert_eq!(
        m.capture(SongPhase::Playing, &[player(2, 3)])[0].keys,
        Some([7, 6, 5, 4, 3, 2, 1])
    );
    m.set(BASE + lane_order::CONTEXT_RVA, 0, 0);
    m.set(BASE + lane_order::CONTEXT_RVA, 16, 1);
    m.set(OPTIONS, 8 + 180 + 156, 1);
    let lanes = m.capture(SongPhase::Playing, &[player(1, 3), player(2, 3)]);
    assert_eq!(lanes[0].keys, Some([1, 2, 3, 4, 5, 6, 7]));
    assert_eq!(lanes[1].keys, Some([7, 6, 5, 4, 3, 2, 1]));
}

#[test]
fn corrupt_missing_and_beginner_helper_never_publish_a_fake_permutation() {
    let mut m = Memory::new();
    for bad in [
        [0, 0, 2, 3, 4, 5, 6, 7],
        [0, 1, 2, 3, 4, 5, 8, 7],
        [0, 1, 2, 3, 4, 5, 6, 0],
    ] {
        m.table(0, bad);
        let lanes = m.capture(SongPhase::Playing, &[player(1, 3)]);
        assert_eq!(lanes[0].keys, None);
        assert_eq!(lanes[0].status, LaneOrderStatus::Unavailable);
    }
    m.table(0, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(m.capture(SongPhase::Playing, &[player(1, 0)])[0].keys, None);
    m.0.insert(BASE + 0xaab3c20, 0x20000u64.to_le_bytes().to_vec());
    m.0.insert(0x20008, vec![0]);
    assert_eq!(
        m.capture(SongPhase::Playing, &[player(1, 0)])[0].keys,
        Some([1, 2, 3, 4, 5, 6, 7])
    );
    m.0.get_mut(&0x20008).unwrap()[0] = 1;
    assert_eq!(m.capture(SongPhase::Playing, &[player(1, 0)])[0].keys, None);
    m.0.remove(&(BASE + lane_order::TABLE_RVA));
    assert_eq!(m.capture(SongPhase::Playing, &[player(1, 3)])[0].keys, None);
    assert!(lane_order::invert(&[0; 31], false).is_err());
    m.set(OPTIONS, 0, 0);
    assert!(
        lane_order::capture(BASE, SongPhase::Playing, &[player(1, 3)], |a, n| m
            .read(a, n))
        .is_err()
    );
}
