//! Public, bounded activity retained independently of expiring text-file notices.
use crate::platforms::Chat;
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Serialize)]
pub(super) struct Entry {
    id: u64,
    at: u64,
    kind: &'static str,
    name: String,
    text: String,
}

pub struct History {
    pub(super) entries: VecDeque<Entry>,
    pub(super) limit: usize,
    next_id: u64,
}
impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            limit: limit.clamp(1, 100),
            next_id: 1,
        }
    }
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.clamp(1, 100);
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn chat(&mut self, chat: &Chat, at: u64) {
        self.push(at, "chat", &chat.name, &chat.text);
    }
    pub fn notice(&mut self, at: u64, text: &str) {
        self.push(at, "event", "", text);
    }
    fn push(&mut self, at: u64, kind: &'static str, name: &str, text: &str) {
        fn clean(value: &str, limit: usize) -> String {
            value
                .chars()
                .filter(|c| !c.is_control())
                .take(limit)
                .collect()
        }
        self.entries.push_back(Entry {
            id: self.next_id,
            at,
            kind,
            name: clean(name, 80),
            text: clean(text, 1024),
        });
        self.next_id += 1;
        self.set_limit(self.limit);
    }
}
impl Default for History {
    fn default() -> Self {
        Self::new(crate::config::Overlay::default().history_limit)
    }
}
