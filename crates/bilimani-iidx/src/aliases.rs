//! Pure CSV validation; callers replace the live table only after full success.
use crate::{catalog::normalize, game::Song};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum AliasError {
    #[error("无法读取别名 CSV：{0}")]
    Io(#[from] std::io::Error),
    #[error("别名 CSV 超过 1 MiB")]
    TooLarge,
    #[error("别名 CSV 格式错误：{0}")]
    Csv(#[from] csv::Error),
    #[error("CSV 表头必须为 alias,target")]
    Header,
    #[error("CSV 第 {line} 行：别名和目标不能为空，且不能超过 1024 字节")]
    Empty { line: u64 },
    #[error("CSV 第 {line} 行：重复别名 {alias}")]
    Duplicate { line: u64, alias: String },
    #[error("CSV 第 {line} 行：找不到歌曲 {target}")]
    Missing { line: u64, target: String },
    #[error("CSV 第 {line} 行：曲名 {target} 不唯一，请使用歌曲 ID")]
    Ambiguous { line: u64, target: String },
}
pub fn parse(bytes: &[u8], songs: &[Song]) -> Result<BTreeMap<String, u32>, AliasError> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(bytes);
    if reader.headers()?.iter().ne(["alias", "target"]) {
        return Err(AliasError::Header);
    }
    let ids: std::collections::HashSet<_> = songs.iter().map(|song| song.id).collect();
    let mut titles: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for song in songs {
        titles
            .entry(normalize(&song.title))
            .or_default()
            .push(song.id);
    }
    let mut aliases = BTreeMap::new();
    for record in reader.records() {
        let record = record?;
        let line = record.position().map_or(0, csv::Position::line);
        let alias = normalize(&record[0]);
        let target = &record[1];
        if alias.is_empty() || target.is_empty() || alias.len() > 1024 || target.len() > 1024 {
            return Err(AliasError::Empty { line });
        }
        let by_id = target.parse::<u32>().ok().filter(|id| ids.contains(id));
        let id = match by_id {
            Some(id) => id,
            None => {
                let target_name = normalize(target);
                let matches = titles
                    .get(&target_name)
                    .ok_or_else(|| AliasError::Missing {
                        line,
                        target: target.into(),
                    })?;
                if matches.len() != 1 {
                    return Err(AliasError::Ambiguous {
                        line,
                        target: target.into(),
                    });
                }
                matches[0]
            }
        };
        if aliases.insert(alias.clone(), id).is_some() {
            return Err(AliasError::Duplicate { line, alias });
        }
    }
    Ok(aliases)
}
pub fn read(path: &std::path::Path) -> Result<Vec<u8>, AliasError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(AliasError::TooLarge);
    }
    Ok(bytes)
}
