use crate::game::Song;
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DatabaseError {
    #[error("Not an IIDX music database")]
    Header,
    #[error("Only IIDX 33 database layout is supported")]
    Version,
    #[error("Invalid database dimensions")]
    Dimensions,
    #[error("Unexpected IIDX 33 record size")]
    Size,
    #[error("Invalid canonical song lookup for music {0}")]
    Lookup(u32),
    #[error("Empty title for music {0}")]
    EmptyTitle(u32),
    #[error("Invalid chart levels for music {0}")]
    Levels(u32),
}

/// IIDX 33 on-disk database: header, u32 ID->record table, 0x7f8-byte records.
/// The same record representation is used by the loaded database.
pub fn parse_database(data: &[u8]) -> Result<Vec<Song>, DatabaseError> {
    if data.len() < 16 || &data[..4] != b"IIDX" {
        return Err(DatabaseError::Header);
    }
    let get = |o| u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) as usize;
    if get(4) != 33 {
        return Err(DatabaseError::Version);
    }
    let count = get(8);
    let ids = get(12);
    if !(1..=10000).contains(&count) || !(1..=100000).contains(&ids) {
        return Err(DatabaseError::Dimensions);
    }
    let start = 16 + ids * 4;
    if data.len() != start + count * 0x7f8 {
        return Err(DatabaseError::Size);
    }
    (0..count)
        .map(|index| {
            let record = &data[start + index * 0x7f8..start + (index + 1) * 0x7f8];
            let id = u32::from_le_bytes(record[0x67c..0x680].try_into().unwrap());
            // Historical IDs can alias another record. The native getter rejects IDs
            // that do not equal the canonical ID stored inside that record.
            if id as usize >= ids || get(16 + id as usize * 4) != index {
                return Err(DatabaseError::Lookup(id));
            }
            parse_record(id, record)
        })
        .collect()
}
pub fn parse_record(id: u32, record: &[u8]) -> Result<Song, DatabaseError> {
    if record.len() != 0x7f8 {
        return Err(DatabaseError::Size);
    }
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
        return Err(DatabaseError::EmptyTitle(id));
    }
    let reading_bytes = &record[0x100..0x140];
    let reading_bytes = &reading_bytes[..reading_bytes
        .iter()
        .position(|&x| x == 0)
        .unwrap_or(reading_bytes.len())];
    let (reading, _, _) = encoding_rs::SHIFT_JIS.decode(reading_bytes);
    let levels: [u8; 10] = record[0x3ec..0x3f6].try_into().unwrap();
    if levels.iter().any(|&x| x > 12) {
        return Err(DatabaseError::Levels(id));
    }
    Ok(Song {
        id,
        title,
        search_terms: vec![reading.into_owned()],
        charts: crate::iidx::available_charts(levels),
    })
}
