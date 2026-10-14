use crate::game::Song;
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
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
    haystacks: Vec<Vec<String>>,
    matcher: Matcher,
    aliases: std::collections::BTreeMap<String, u32>,
    alias_terms: Vec<Vec<String>>,
}
impl Catalog {
    pub fn new(mut songs: Vec<Song>) -> Result<Self, CatalogError> {
        songs.sort_by_key(|s| s.id);
        if songs.is_empty() {
            return Err(CatalogError::Empty);
        }
        if songs.windows(2).any(|s| s[0].id == s[1].id) {
            return Err(CatalogError::DuplicateId);
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
        let alias_terms = vec![Vec::new(); songs.len()];
        Ok(Self {
            songs,
            haystacks,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            aliases: Default::default(),
            alias_terms,
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
        if let Some(id) = self.aliases.get(&query) {
            return self
                .songs
                .iter()
                .filter(|song| song.id == *id)
                .take(limit)
                .cloned()
                .collect();
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
                    .chain(&self.alias_terms[i])
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
    pub fn reload_aliases(&mut self, bytes: &[u8]) -> Result<usize, crate::aliases::AliasError> {
        let aliases = crate::aliases::parse(bytes, &self.songs)?;
        let count = aliases.len();
        let mut alias_terms = vec![Vec::new(); self.songs.len()];
        for (term, id) in &aliases {
            let index = self
                .songs
                .binary_search_by_key(id, |song| song.id)
                .expect("validated alias target");
            alias_terms[index].push(term.clone());
        }
        self.aliases = aliases;
        self.alias_terms = alias_terms;
        Ok(count)
    }
    pub fn aliases_count(&self) -> usize {
        self.aliases.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Song database is empty")]
    Empty,
    #[error("Duplicate music IDs")]
    DuplicateId,
}
