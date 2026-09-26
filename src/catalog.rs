use anyhow::{Result, bail, ensure};
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    SP,
    DP,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chart {
    pub mode: Mode,
    pub difficulty: u8,
}
impl Chart {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.to_ascii_uppercase();
        let b = s.as_bytes();
        if b.len() != 3 {
            return None;
        }
        let mode = match &b[..2] {
            b"SP" => Mode::SP,
            b"DP" => Mode::DP,
            _ => return None,
        };
        let difficulty = b"BNHAL".iter().position(|&x| x == b[2])? as u8;
        Some(Self { mode, difficulty })
    }
    pub fn label(self) -> String {
        format!(
            "{:?}{}",
            self.mode, b"BNHAL"[self.difficulty as usize] as char
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Song {
    pub id: u32,
    pub title: String,
    pub reading: String,
    pub levels: [u8; 10],
}
impl Song {
    pub fn supports(&self, mode: Mode, chart: Option<Chart>) -> bool {
        let base = if mode == Mode::DP { 5 } else { 0 };
        match chart {
            Some(c) => c.mode == mode && self.levels[base + c.difficulty as usize] > 0,
            None => self.levels[base..base + 5].iter().any(|&x| x > 0),
        }
    }
}

pub fn normalize(s: &str) -> String {
    s.nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .trim()
        .to_owned()
}

pub struct Catalog {
    pub songs: Vec<Song>,
    aliases: HashMap<String, usize>,
    haystacks: Vec<(String, String)>,
    matcher: Matcher,
}
impl Catalog {
    pub fn new(mut songs: Vec<Song>, aliases: &BTreeMap<String, String>) -> Result<Self> {
        songs.sort_by_key(|s| s.id);
        ensure!(!songs.is_empty(), "Song database is empty");
        ensure!(
            songs.windows(2).all(|s| s[0].id != s[1].id),
            "Duplicate music IDs"
        );
        let mut resolved = HashMap::new();
        for (alias, target) in aliases {
            let matches: Vec<_> = songs
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    target.parse::<u32>().ok() == Some(s.id)
                        || normalize(&s.title) == normalize(target)
                })
                .collect();
            ensure!(
                matches.len() == 1,
                "Alias {alias:?} must identify exactly one song (use a music ID)"
            );
            ensure!(!normalize(alias).is_empty(), "Empty alias");
            ensure!(
                resolved.insert(normalize(alias), matches[0].0).is_none(),
                "Duplicate normalized alias"
            );
        }
        let haystacks = songs
            .iter()
            .map(|s| (normalize(&s.title), normalize(&s.reading)))
            .collect();
        Ok(Self {
            songs,
            aliases: resolved,
            haystacks,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
        })
    }
    pub fn search(&mut self, query: &str, limit: usize) -> Vec<Song> {
        let query = normalize(query);
        if let Some(&i) = self.aliases.get(&query) {
            return vec![self.songs[i].clone()];
        }
        let pattern = Pattern::new(
            &query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut buffer = Vec::new();
        let mut ranked: Vec<_> = self
            .haystacks
            .iter()
            .enumerate()
            .filter_map(|(i, (title, reading))| {
                let title_score =
                    pattern.score(Utf32Str::new(title, &mut buffer), &mut self.matcher);
                let reading_score =
                    pattern.score(Utf32Str::new(reading, &mut buffer), &mut self.matcher);
                title_score
                    .max(reading_score)
                    .map(|score| (i, normalize(&self.songs[i].title) == query, score))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(b.2.cmp(&a.2))
                .then(self.songs[a.0].id.cmp(&self.songs[b.0].id))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(i, _, _)| self.songs[i].clone())
            .collect()
    }
}

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
        reading: reading.into_owned(),
        levels,
    })
}
