use bilimani::{
    game::SongPhase,
    games::iidx::{self, v33::song_info},
};

fn put(record: &mut [u8], offset: usize, value: u32) {
    record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn text(record: &mut [u8], offset: usize, value: &str) {
    for (i, c) in value.encode_utf16().enumerate() {
        record[offset + i * 2..offset + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
}
fn record() -> Vec<u8> {
    let mut record = vec![0; song_info::RECORD_SIZE];
    put(&mut record, 0x67c, 11040);
    text(&mut record, 0, "Fixture 曲名 🥁");
    text(&mut record, 0x140, "TEST GENRE");
    text(&mut record, 0x1c0, "テスト Artist");
    record[0x3dc] = 11;
    record[0x3ec..0x3f6].copy_from_slice(&[0, 5, 10, 12, 0, 0, 5, 9, 12, 0]);
    // Distinct values for every chart detect mode/difficulty stride mistakes.
    for chart in 0..10 {
        put(&mut record, 0x3fc + chart * 8, 200 + chart as u32);
        put(&mut record, 0x400 + chart * 8, 100 + chart as u32);
        put(&mut record, 0x47c + chart * 4, 1000 + chart as u32);
        for axis in 0..6 {
            put(
                &mut record,
                0x4fc + chart * 24 + axis * 4,
                10000 + chart as u32 * 100 + axis as u32,
            );
        }
    }
    record
}

#[test]
fn live_record_maps_both_sides_and_every_difficulty_without_request_state() {
    let data = record();
    let current =
        song_info::decode(&data, iidx::SP, [Some(2), Some(3)], SongPhase::Selecting).unwrap();
    assert_eq!(current.phase, SongPhase::Selecting);
    assert_eq!(current.players[0].side, 1);
    assert_eq!(current.players[0].chart.id, "SPH");
    assert_eq!(current.players[1].side, 2);
    assert_eq!(current.players[1].chart.id, "SPA");
    let song = current.song.unwrap();
    assert_eq!(
        (
            song.id,
            song.title.as_str(),
            song.genre.as_str(),
            song.artist.as_str()
        ),
        (11040, "Fixture 曲名 🥁", "TEST GENRE", "テスト Artist")
    );
    assert_eq!(song.game_version, Some(11));
    assert_eq!(song.charts.len(), 6);
    let dp = song.charts.iter().find(|c| c.chart.id == "DPA").unwrap();
    assert_eq!(dp.level, "12");
    assert_eq!(dp.difficulty, "ANOTHER");
    assert_eq!(dp.bpm.unwrap().min, 108);
    assert_eq!(dp.bpm.unwrap().max, 208);
    assert_eq!(dp.note_count, Some(1008));
    let radar = serde_json::to_value(dp.radar.unwrap()).unwrap();
    for (axis, name) in ["notes", "peak", "scratch", "soflan", "charge", "chord"]
        .iter()
        .enumerate()
    {
        assert_eq!(radar[name].as_f64().unwrap(), (10800 + axis) as f64 / 100.0);
    }
    let playing = song_info::decode(&data, iidx::DP, [None, Some(3)], SongPhase::Playing).unwrap();
    assert_eq!(playing.players.len(), 1);
    assert_eq!(playing.players[0].side, 2);
    assert_eq!(playing.players[0].chart.id, "DPA");
}

#[test]
fn unloaded_invalid_and_absent_data_is_not_presented_as_real_metrics() {
    let mut data = record();
    data[0x3fc..0x67c].fill(0);
    let current =
        song_info::decode(&data, iidx::SP, [Some(0), Some(99)], SongPhase::Selecting).unwrap();
    assert!(current.players.is_empty());
    for chart in current.song.unwrap().charts {
        assert!(chart.bpm.is_none() && chart.note_count.is_none() && chart.radar.is_none());
        assert!(chart.density.is_none());
        assert!(chart.lane_counts.is_none());
    }
    put(&mut data, 0x3fc + 3 * 8, 154);
    let song = song_info::decode(&data, iidx::SP, [Some(3), None], SongPhase::Playing)
        .unwrap()
        .song
        .unwrap();
    let bpm = song
        .charts
        .iter()
        .find(|c| c.chart.id == "SPA")
        .unwrap()
        .bpm
        .unwrap();
    assert_eq!((bpm.min, bpm.max), (154, 154));
    put(&mut data, 0x4fc + 3 * 24, u32::MAX);
    put(&mut data, 0x47c + 3 * 4, u32::MAX);
    put(&mut data, 0x400 + 3 * 8, 999);
    let song = song_info::decode(&data, iidx::SP, [Some(3), None], SongPhase::Playing)
        .unwrap()
        .song
        .unwrap();
    let chart = song.charts.iter().find(|c| c.chart.id == "SPA").unwrap();
    assert!(chart.radar.is_none() && chart.note_count.is_none() && chart.bpm.is_none());
    assert!(song_info::decode(&data[..100], iidx::SP, [None; 2], SongPhase::Idle).is_err());
    data[0x3ec] = 13;
    assert!(song_info::decode(&data, iidx::SP, [None; 2], SongPhase::Idle).is_err());
}
