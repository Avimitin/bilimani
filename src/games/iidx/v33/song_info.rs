//! IIDX 33 live music-record display fields. See docs/game-analysis.md.
//! On-disk records can have zero BPM/notes/radar until the game loads chart data.
use crate::game::{
    BpmRange, ChartInfo, GameRules, Mode, NowPlaying, PlayerChart, Radar, SongInfo, SongPhase,
};
use crate::games::iidx::{CHARTS, DP, RULES, SP};
use anyhow::{Result, ensure};

pub const RECORD_SIZE: usize = 0x7f8;

pub fn decode(
    record: &[u8],
    mode: Mode,
    difficulties: [Option<u32>; 2],
    phase: SongPhase,
) -> Result<NowPlaying> {
    ensure!(record.len() == RECORD_SIZE, "Invalid music record size");
    ensure!(mode == SP || mode == DP, "Invalid play mode");
    let u32_at = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
    let id = u32_at(0x67c);
    ensure!(id > 0 && id < 100000, "Invalid music ID");
    let title = utf16(&record[..0x100]);
    ensure!(!title.is_empty(), "Empty music title");
    let levels = &record[0x3ec..0x3f6];
    ensure!(
        levels.iter().all(|&level| level <= 12),
        "Invalid chart levels"
    );
    let charts: Vec<_> = CHARTS
        .iter()
        .enumerate()
        .filter_map(|(index, &chart)| {
            let level = levels[index];
            if level == 0 {
                return None;
            }
            let max = u32_at(0x3fc + index * 8);
            let min = u32_at(0x400 + index * 8);
            let bpm = (max > 0 && max <= 100000 && min <= max).then_some(BpmRange {
                min: if min == 0 { max } else { min },
                max,
            });
            let notes = u32_at(0x47c + index * 4);
            let note_count = (notes > 0 && notes <= 1000000).then_some(notes);
            let values: [u32; 6] =
                std::array::from_fn(|axis| u32_at(0x4fc + index * 24 + axis * 4));
            // Missing data uses zeros or a negative sentinel; never draw a fake radar.
            let radar = (values.iter().any(|&v| v > 0) && values.iter().all(|&v| v <= 1000000))
                .then(|| Radar {
                    notes: f64::from(values[0]) / 100.0,
                    peak: f64::from(values[1]) / 100.0,
                    scratch: f64::from(values[2]) / 100.0,
                    soflan: f64::from(values[3]) / 100.0,
                    charge: f64::from(values[4]) / 100.0,
                    chord: f64::from(values[5]) / 100.0,
                });
            Some(ChartInfo {
                chart,
                difficulty: ["BEGINNER", "NORMAL", "HYPER", "ANOTHER", "LEGGENDARIA"][index % 5],
                level: level.to_string(),
                style: RULES.chart_style(chart),
                bpm,
                note_count,
                radar,
            })
        })
        .collect();
    let players = difficulties
        .into_iter()
        .enumerate()
        .filter_map(|(side, difficulty)| {
            let difficulty = difficulty? as usize;
            if difficulty >= 5 {
                return None;
            }
            let chart = CHARTS[difficulty + if mode == DP { 5 } else { 0 }];
            charts
                .iter()
                .any(|c| c.chart == chart)
                .then_some(PlayerChart {
                    side: side as u8 + 1,
                    chart,
                })
        })
        .collect();
    let version = u16::from_le_bytes(record[0x3dc..0x3de].try_into().unwrap());
    Ok(NowPlaying {
        phase,
        song: Some(SongInfo {
            id,
            title,
            artist: utf16(&record[0x1c0..0x2c0]),
            genre: utf16(&record[0x140..0x1c0]),
            game_version: (version <= 33).then_some(version),
            charts,
        }),
        players,
    })
}

fn utf16(bytes: &[u8]) -> String {
    String::from_utf16_lossy(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| u16::from_le_bytes(b))
            .take_while(|&c| c != 0)
            .collect::<Vec<_>>(),
    )
}
