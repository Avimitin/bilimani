//! One worker owns this queue. Time, game state and execution results are inputs;
//! HTTP, graphics, platform protocols and native memory never enter this crate.
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize)]
pub struct SongRequest {
    pub song_id: u32,
    pub title: String,
    pub mode: String,
    pub chart: Option<String>,
    pub requester: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: u64,
    #[serde(flatten)]
    pub request: SongRequest,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ItemView {
    #[serde(flatten)]
    pub entry: Entry,
    pub executing: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct CurrentView {
    #[serde(flatten)]
    pub entry: Entry,
    pub remaining_seconds: u64,
    pub duration_seconds: u64,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Added,
    Removed,
    Prioritized,
    Selected,
    Failed,
    Played,
    Expired,
}
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub id: u64,
    pub kind: EventKind,
    pub entry: Entry,
}
#[derive(Clone, Debug, Serialize)]
pub struct View {
    pub revision: u64,
    pub capacity: usize,
    pub current: Option<CurrentView>,
    pub items: Vec<ItemView>,
    pub events: Vec<Event>,
}
#[derive(Clone, Debug)]
struct Current {
    entry: Entry,
    until: u64,
}
#[derive(Clone, Copy, Debug)]
struct Flight {
    id: u64,
    plays: u64,
    manual: bool,
}
#[derive(Clone, Debug)]
pub struct Dispatch {
    pub entry: Entry,
    pub manual: bool,
}
#[derive(Clone, Debug)]
pub enum Completion {
    Selected,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("点歌队列已满")]
    Full,
    #[error("队列项已不存在")]
    NotFound,
    #[error("选曲正在执行，请稍后再操作")]
    Busy,
}

pub struct Queue {
    capacity: usize,
    current_timeout_ms: u64,
    entries: VecDeque<Entry>,
    current: Option<Current>,
    flight: Option<Flight>,
    priority: Option<u64>,
    plays: u64,
    next_id: u64,
    revision: u64,
    events: VecDeque<Event>,
}
impl Default for Queue {
    fn default() -> Self {
        Self::new(20, 600_000)
    }
}
impl Queue {
    pub fn new(capacity: usize, current_timeout_ms: u64) -> Self {
        Self {
            capacity,
            current_timeout_ms,
            entries: VecDeque::new(),
            current: None,
            flight: None,
            priority: None,
            plays: 0,
            next_id: 1,
            revision: 0,
            events: VecDeque::new(),
        }
    }
    pub fn add(&mut self, request: SongRequest) -> Result<Entry, QueueError> {
        if self.entries.len() >= self.capacity {
            return Err(QueueError::Full);
        }
        let entry = Entry {
            id: self.next_id,
            request,
            error: None,
        };
        self.next_id += 1;
        self.entries.push_back(entry.clone());
        self.event(EventKind::Added, entry.clone());
        Ok(entry)
    }
    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
    pub fn busy(&self) -> bool {
        self.flight.is_some()
    }
    pub fn remove(&mut self, id: u64) -> Result<(), QueueError> {
        if self.flight.is_some_and(|flight| flight.id == id) {
            return Err(QueueError::Busy);
        }
        let entry = if self
            .current
            .as_ref()
            .is_some_and(|current| current.entry.id == id)
        {
            self.current.take().expect("current request").entry
        } else {
            let index = self
                .entries
                .iter()
                .position(|entry| entry.id == id)
                .ok_or(QueueError::NotFound)?;
            self.entries.remove(index).expect("existing entry")
        };
        if self.priority == Some(id) {
            self.priority = None;
        }
        self.event(EventKind::Removed, entry);
        Ok(())
    }
    pub fn prioritize(&mut self, id: u64) -> Result<(), QueueError> {
        if self.busy() {
            return Err(QueueError::Busy);
        }
        let entry = self.get(id).ok_or(QueueError::NotFound)?.clone();
        self.priority = Some(id);
        self.event(EventKind::Prioritized, entry);
        Ok(())
    }
    /// Opening the menu pauses automatic dispatch; a deliberate priority still runs.
    pub fn next(&mut self, mode: &str, paused: bool) -> Option<Dispatch> {
        if self.busy() {
            return None;
        }
        let manual = self.priority.is_some();
        let entry = match self.priority {
            Some(id) => self.get(id).filter(|entry| entry.request.mode == mode),
            None if !paused && self.current.is_none() => self
                .entries
                .iter()
                .find(|entry| entry.error.is_none() && entry.request.mode == mode),
            None => None,
        }?
        .clone();
        self.priority = None;
        self.flight = Some(Flight {
            id: entry.id,
            plays: self.plays,
            manual,
        });
        self.revision += 1;
        Some(Dispatch { entry, manual })
    }
    /// A failed selection keeps its row for deliberate retry or deletion. An old
    /// current request remains current until the replacement actually succeeds.
    pub fn finish(&mut self, id: u64, result: Completion, now: u64) -> bool {
        let Some(flight) = self.flight.filter(|flight| flight.id == id) else {
            return false;
        };
        self.flight = None;
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .expect("flight is retained in queue");
        match result {
            Completion::Selected => {
                let mut entry = self.entries.remove(index).expect("flight entry");
                entry.error = None;
                if self.plays == flight.plays {
                    self.current = Some(Current {
                        entry: entry.clone(),
                        until: now.saturating_add(self.current_timeout_ms),
                    });
                }
                self.event(EventKind::Selected, entry);
                flight.manual
            }
            Completion::Failed(message) => {
                let entry = &mut self.entries[index];
                entry.error = Some(message);
                let entry = entry.clone();
                self.event(EventKind::Failed, entry);
                false
            }
        }
    }
    pub fn observe(&mut self, plays: u64, now: u64) {
        let played = plays != self.plays;
        self.plays = plays;
        let expired = self
            .current
            .as_ref()
            .is_some_and(|current| now >= current.until);
        if (played || expired)
            && let Some(current) = self.current.take()
        {
            self.event(
                if played {
                    EventKind::Played
                } else {
                    EventKind::Expired
                },
                current.entry,
            );
        }
    }
    pub fn view(&self, now: u64) -> View {
        View {
            revision: self.revision,
            capacity: self.capacity,
            current: self.current.as_ref().map(|current| CurrentView {
                entry: current.entry.clone(),
                remaining_seconds: current.until.saturating_sub(now).div_ceil(1000),
                duration_seconds: self.current_timeout_ms.div_ceil(1000),
            }),
            items: self
                .entries
                .iter()
                .map(|entry| ItemView {
                    entry: entry.clone(),
                    executing: self.flight.is_some_and(|flight| flight.id == entry.id),
                })
                .collect(),
            events: self.events.iter().cloned().collect(),
        }
    }
    fn event(&mut self, kind: EventKind, entry: Entry) {
        self.revision += 1;
        self.events.push_back(Event {
            id: self.revision,
            kind,
            entry,
        });
        while self.events.len() > 64 {
            self.events.pop_front();
        }
    }
}
