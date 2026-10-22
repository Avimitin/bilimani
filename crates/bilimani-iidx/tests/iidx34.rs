use anyhow::{Result, ensure};
use bilimani_iidx::{
    game::SongPhase,
    iidx,
    service::Service,
    versions::{
        self, Build, v33,
        v34::{b010_0916 as profile_010_0916, b010_0917 as profile_010_0917},
    },
};

#[test]
fn fingerprint_selects_profile_and_api_reports_detected_build() {
    for (hash, build, major, label) in [
        (
            v33::b20260812::SUPPORTED_SHA256,
            Build::V33_20260812,
            33,
            "20260812",
        ),
        (
            profile_010_0916::SUPPORTED_SHA256,
            Build::V34_010_0916,
            34,
            "010-0916",
        ),
        (
            profile_010_0917::SUPPORTED_SHA256,
            Build::V34_010_0917,
            34,
            "010-0917",
        ),
    ] {
        assert_eq!(versions::resolve(hash).unwrap(), build);
        let mut service = Service::default();
        service.build = build;
        let snapshots = service.snapshots();
        let meta = &snapshots
            .iter()
            .find(|(key, _)| key == "/api/v1/meta")
            .unwrap()
            .1;
        assert_eq!(meta["major_version"], major);
        assert_eq!(meta["build"], label);
    }
    assert!(versions::resolve("unknown").is_err());
}

fn record() -> Vec<u8> {
    let mut record = vec![0; profile_010_0916::song_info::RECORD_SIZE];
    record[0] = b'A';
    record[0x67c..0x680].copy_from_slice(&34001u32.to_le_bytes());
    record[0x3dc..0x3de].copy_from_slice(&34u16.to_le_bytes());
    record[0x3ec..0x3f6].fill(12);
    record
}

#[test]
fn database_requires_34_and_canonical_lookup_but_preserves_record_layout() {
    let mut data = b"IIDX".to_vec();
    for n in [34u32, 1, 34002] {
        data.extend(n.to_le_bytes());
    }
    data.resize(16 + 34002 * 4, 0xff);
    data[16 + 34001 * 4..16 + 34002 * 4].fill(0);
    data.extend(record());
    let songs = profile_010_0916::catalog::parse_database(&data).unwrap();
    assert_eq!(songs[0].id, 34001);
    assert_eq!(
        profile_010_0917::catalog::parse_database(&data).unwrap()[0].id,
        34001
    );
    assert_eq!(songs[0].charts.len(), 10);
    assert!(v33::b20260812::catalog::parse_database(&data).is_err());
    data[4] = 33;
    assert_eq!(
        profile_010_0916::catalog::parse_database(&data).unwrap_err(),
        profile_010_0916::catalog::DatabaseError::Version
    );
    data[4] = 34;
    data[16 + 34001 * 4] = 1;
    assert_eq!(
        profile_010_0916::catalog::parse_database(&data).unwrap_err(),
        profile_010_0916::catalog::DatabaseError::Lookup(34001)
    );
    let song = profile_010_0916::song_info::decode(&record(), iidx::DP, [None, Some(3)], SongPhase::Playing)
        .unwrap();
    assert_eq!(song.song.unwrap().game_version, Some(34));
    assert_eq!(song.players[0].chart.id, "DPA");
    let old =
        v33::b20260812::song_info::decode(&record(), iidx::SP, [Some(3), None], SongPhase::Playing)
            .unwrap();
    assert_eq!(old.song.unwrap().game_version, None);
}

#[test]
fn analyzer_maps_two_explicit_indices_and_isolates_invalid_entries() {
    let mut memory = vec![0u8; 4096];
    fn u32_at(m: &mut [u8], at: usize, value: u32) {
        m[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn ptr(m: &mut [u8], at: usize, value: usize) {
        m[at..at + 8].copy_from_slice(&(value as u64).to_le_bytes());
    }
    ptr(&mut memory, 256, 1234);
    u32_at(&mut memory, 264, 34001);
    for (side, index) in [(0, 3), (1, 8)] {
        u32_at(&mut memory, 268 + side * 4, index);
        let detail = 512 + side * 1024;
        ptr(&mut memory, 280 + side * 24, detail);
        memory[296 + side * 24] = 1;
        for (offset, value) in [(0, 5), (24, 2)] {
            let start = detail + 128 + offset;
            ptr(&mut memory, detail + offset, start);
            ptr(&mut memory, detail + offset + 8, start + 4);
            ptr(&mut memory, detail + offset + 16, start + 4);
            u32_at(&mut memory, start, value);
        }
        u32_at(&mut memory, detail + 48, 3);
        u32_at(&mut memory, detail + 76, 2);
        u32_at(&mut memory, detail + 112, 2000);
    }
    let capture = |m: &[u8], song, table| {
        profile_010_0916::density::read_snapshot(256, song, table, |at, n| -> Result<Vec<u8>> {
            ensure!(n <= 4 * 3601, "Unbounded read");
            Ok(m.get(at..at.checked_add(n).unwrap())
                .ok_or_else(|| anyhow::anyhow!("Unmapped"))?
                .to_vec())
        })
    };
    let snapshot = capture(&memory, 34001, 1234).unwrap();
    assert_eq!(snapshot.charts.iter().filter(|c| c.is_some()).count(), 2);
    assert_eq!(snapshot.charts[3].as_ref().unwrap().notes, [5]);
    assert_eq!(snapshot.lane_counts[8].as_ref().unwrap().sides.len(), 2);
    assert!(capture(&memory, 34002, 1234).is_err());
    assert!(capture(&memory, 34001, 1235).is_err());
    // Sentinel is a chart index, not a pointer or a third side.
    u32_at(&mut memory, 268, 11);
    ptr(&mut memory, 280, usize::MAX);
    let snapshot = capture(&memory, 34001, 1234).unwrap();
    assert!(snapshot.charts[3].is_none());
    assert!(snapshot.charts[8].is_some());
    // A corrupt vector does not discard independently valid lane totals.
    ptr(&mut memory, 1536, usize::MAX);
    let snapshot = capture(&memory, 34001, 1234).unwrap();
    assert!(snapshot.charts[8].is_none());
    assert!(snapshot.lane_counts[8].is_some());
    memory[320] = 0;
    let snapshot = capture(&memory, 34001, 1234).unwrap();
    assert!(snapshot.charts.iter().all(Option::is_none));
    assert!(snapshot.lane_counts.iter().all(Option::is_none));
}
