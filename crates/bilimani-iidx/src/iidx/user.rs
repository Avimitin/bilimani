//! Owned player display data. No card identifiers or platform identity.
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UserSnapshot {
    /// One-based cabinet side, matching now-playing players.
    pub active_side: Option<u8>,
    pub players: Vec<UserProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UserProfile {
    pub side: u8,
    pub dj_name: String,
    pub sp: StyleProfile,
    pub dp: StyleProfile,
    pub weekly: Option<WeeklyActivity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StyleProfile {
    /// -1 in native data means no grade; unknown IDs are not guessed.
    pub dan: Option<DanRank>,
    pub dj_points: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DanRank {
    Kyu7,
    Kyu6,
    Kyu5,
    Kyu4,
    Kyu3,
    Kyu2,
    Kyu1,
    Dan1,
    Dan2,
    Dan3,
    Dan4,
    Dan5,
    Dan6,
    Dan7,
    Dan8,
    Dan9,
    Dan10,
    Chuden,
    Kaiden,
}
impl DanRank {
    pub fn from_native(id: i32) -> Option<Self> {
        use DanRank::*;
        const RANKS: [DanRank; 19] = [
            Kyu7, Kyu6, Kyu5, Kyu4, Kyu3, Kyu2, Kyu1, Dan1, Dan2, Dan3, Dan4, Dan5, Dan6, Dan7,
            Dan8, Dan9, Dan10, Chuden, Kaiden,
        ];
        usize::try_from(id).ok().and_then(|i| RANKS.get(i)).copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WeeklyActivity {
    pub period: ActivityPeriod,
    pub week_id: u32,
    /// Opaque date value supplied by the game; no assumed timezone/epoch.
    pub date: u32,
    pub sp: Option<ActivityCounts>,
    pub dp: Option<ActivityCounts>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityPeriod {
    GameWeek,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ActivityCounts {
    pub songs: u64,
    /// Native keyboard_num, excluding scratches.
    pub keys: u64,
}
