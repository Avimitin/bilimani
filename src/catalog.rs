use crate::game::Song;
use anyhow::{Result, ensure};
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use std::collections::{BTreeMap, HashMap};
use unicode_normalization::UnicodeNormalization;

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
    haystacks: Vec<Vec<String>>,
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
            .map(|s| {
                std::iter::once(&s.title)
                    .chain(&s.search_terms)
                    .map(|s| normalize(s))
                    .collect()
            })
            .collect();
        Ok(Self {
            songs,
            aliases: resolved,
            haystacks,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
        })
    }
    /// Replace additional search terms published by the adapter.
    /// Keep canonical titles/readings available before the search UI initializes.
    pub fn set_search_index(&mut self, entries: &[(u32, String)]) -> usize {
        for (terms, song) in self.haystacks.iter_mut().zip(&self.songs) {
            *terms = std::iter::once(&song.title)
                .chain(&song.search_terms)
                .map(|s| normalize(s))
                .collect();
        }
        let mut added = 0;
        for (id, term) in entries {
            let Ok(i) = self.songs.binary_search_by_key(id, |s| s.id) else {
                continue;
            };
            let term = normalize(term);
            if !term.is_empty() && !self.haystacks[i].contains(&term) {
                self.haystacks[i].push(term);
                added += 1;
            }
        }
        added
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
            .filter_map(|(i, terms)| {
                terms
                    .iter()
                    .filter_map(|term| {
                        pattern.score(Utf32Str::new(term, &mut buffer), &mut self.matcher)
                    })
                    .max()
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
