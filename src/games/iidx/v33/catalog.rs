use crate::game::Song;
use anyhow::{Result, bail, ensure};

/// IIDX 33 on-disk database: header, u32 ID->record table, 0x7f8-byte records.
/// The same record representation is used by the loaded database.
pub fn parse_database(data: &[u8]) -> Result<Vec<Song>> {
    ensure!(
        data.len() >= 16 && &data[..4] == b"IIDX",
        "Not an IIDX music database"
    );
    let get = |o| u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) as usize;
    ensure!(get(4) == 33, "Only IIDX 33 database layout is supported");
    let count = get(8);
    let ids = get(12);
    ensure!(
        (1..=10000).contains(&count) && (1..=100000).contains(&ids),
        "Invalid database dimensions"
    );
    let start = 16 + ids * 4;
    ensure!(
        data.len() == start + count * 0x7f8,
        "Unexpected IIDX 33 record size"
    );
    let mut songs = Vec::with_capacity(count);
    for index in 0..count {
        let record = &data[start + index * 0x7f8..start + (index + 1) * 0x7f8];
        let id = u32::from_le_bytes(record[0x67c..0x680].try_into().unwrap());
        // Historical IDs can alias another record. The native getter rejects IDs
        // that do not equal the canonical ID stored inside that record.
        ensure!(
            (id as usize) < ids && get(16 + id as usize * 4) == index,
            "Invalid canonical song lookup"
        );
        songs.push(parse_record(id, record)?);
    }
    ensure!(songs.len() == count, "Incomplete song lookup table");
    Ok(songs)
}
pub fn parse_record(id: u32, record: &[u8]) -> Result<Song> {
    ensure!(record.len() == 0x7f8, "Invalid music record");
    let title = String::from_utf16_lossy(
        &record[..0x100]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_le_bytes([x[0], x[1]]))
            .take_while(|&x| x != 0)
            .collect::<Vec<_>>(),
    );
    if title.is_empty() {
        bail!("Empty title for music {id}");
    }
    let reading_bytes = &record[0x100..0x140];
    let reading_bytes = &reading_bytes[..reading_bytes
        .iter()
        .position(|&x| x == 0)
        .unwrap_or(reading_bytes.len())];
    let (reading, _, _) = encoding_rs::SHIFT_JIS.decode(reading_bytes);
    let levels: [u8; 10] = record[0x3ec..0x3f6].try_into().unwrap();
    ensure!(
        levels.iter().all(|&x| x <= 12),
        "Invalid chart levels for music {id}"
    );
    Ok(Song {
        id,
        title,
        search_terms: vec![reading.into_owned()],
        charts: super::super::available_charts(levels),
    })
}
