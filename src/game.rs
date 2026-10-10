//! Host-independent contract between the request engine and a game adapter.
//! IDs and labels are defined by the adapter; core code must not interpret them.
use anyhow::Result;
use serde::Serialize;
use std::{fmt, path::Path};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Mode(pub &'static str);
impl fmt::Debug for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Chart {
    pub mode: Mode,
    pub id: &'static str,
}
impl Chart {
    pub fn label(self) -> &'static str {
        self.id
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AvailableChart {
    pub chart: Chart,
    /// Adapter-formatted level; games may use integers, decimals or named ranks.
    pub level: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Song {
    pub id: u32,
    pub title: String,
    pub search_terms: Vec<String>,
    pub charts: Vec<AvailableChart>,
}

/// Owned display data sampled by the adapter, independent of viewer requests.
#[derive(Clone, Debug, Default, Serialize)]
pub struct NowPlaying {
    pub phase: SongPhase,
    pub song: Option<SongInfo>,
    /// One entry per participating player; side is one-based.
    pub players: Vec<PlayerChart>,
    /// Physical lanes, left to right; empty when the adapter cannot sample them.
    pub lane_order: Vec<LaneOrder>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaneOrder {
    pub side: u8,
    pub random: RandomMode,
    pub mirror: bool,
    pub status: LaneOrderStatus,
    /// Original chart key at each physical key, one-based. Scratch is excluded.
    pub keys: Option<[u8; 7]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RandomMode {
    Off,
    Random,
    RRandom,
    SRandom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneOrderStatus {
    Ready,
    Pending,
    Dynamic,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SongPhase {
    #[default]
    Idle,
    Selecting,
    Playing,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlayerChart {
    pub side: u8,
    pub chart: Chart,
}

#[derive(Clone, Debug, Serialize)]
pub struct SongInfo {
    pub id: u32,
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub game_version: Option<u16>,
    pub charts: Vec<ChartInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChartInfo {
    pub chart: Chart,
    pub difficulty: &'static str,
    pub level: String,
    pub style: ChartStyle,
    pub bpm: Option<BpmRange>,
    pub note_count: Option<u32>,
    pub radar: Option<Radar>,
    pub density: Option<NoteDensity>,
    /// Original chart lanes, before RANDOM/MIRROR. Native weighted note counts.
    pub lane_counts: Option<LaneCounts>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaneCounts {
    /// SP always uses chart side 1, even when played on the physical 2P side.
    pub sides: Vec<LaneSideCounts>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaneSideCounts {
    pub side: u8,
    /// Original keys 1..7; CN/HCN count twice, like the native detail graph.
    pub keys: [u32; 7],
    pub scratch: u32,
}

/// Native chart histogram. Scratch is a subset of notes; charge notes count
/// twice in their onset bin, matching IIDX's detail graph (not held duration).
#[derive(Clone, Debug, Serialize)]
pub struct NoteDensity {
    pub bin_ms: u32,
    pub duration_ms: u32,
    pub notes: Vec<u32>,
    pub scratch: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct BpmRange {
    pub min: u32,
    pub max: u32,
}

/// Display values (100.0 is the game's 100% reference ring).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Radar {
    pub notes: f64,
    pub peak: f64,
    pub scratch: f64,
    pub soflan: f64,
    pub charge: f64,
    pub chord: f64,
}
impl Song {
    pub fn supports(&self, mode: Mode, chart: Option<Chart>) -> bool {
        self.charts
            .iter()
            .any(|c| c.chart.mode == mode && chart.is_none_or(|wanted| wanted == c.chart))
    }
}

/// Presentation tokens, independent of any game's difficulty names.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartStyle {
    #[default]
    Neutral,
    Green,
    Blue,
    Amber,
    Red,
    Purple,
}

pub trait GameRules: Send + Sync {
    fn parse_chart(&self, text: &str) -> Option<Chart>;
    fn request_hint(&self) -> &'static str;
    fn chart_style(&self, _chart: Chart) -> ChartStyle {
        ChartStyle::Neutral
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Other,
    Select,
    Playing,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub mode: Option<Mode>,
    pub epoch: u64,
    /// Adapter-owned capability, including mode/player/scene eligibility.
    pub can_skip: bool,
}

/// Only the adapter payload crosses the boundary, never user/platform metadata.
#[derive(Clone, Debug)]
pub struct Selection {
    pub token: u64,
    pub song_id: u32,
    pub mode: Mode,
    pub chart: Option<Chart>,
    pub epoch: u64,
}
#[derive(Clone, Debug)]
pub struct SelectionResult {
    pub token: u64,
    /// None means the scene changed before execution; keep the request for retry.
    pub result: Option<Result<(), String>>,
}
#[derive(Clone, Debug)]
pub struct SkipRequest {
    pub token: u64,
    pub epoch: u64,
    /// Human-readable adapter context for diagnostics, not interpreted by core.
    pub description: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    Down,
    Up,
    Left,
    Right,
    Confirm,
    Back,
}
#[derive(Default)]
pub struct GameUpdate {
    /// Confirmed logged-in card, absent for guests/ambiguous player context.
    pub player_card: Option<crate::profiles::CardId>,
    pub snapshot: Snapshot,
    pub now_playing: NowPlaying,
    /// Monotonic counter: do not miss a short gameplay transition between polls.
    pub plays: u64,
    pub selection_result: Option<SelectionResult>,
    pub skip: Option<SkipRequest>,
    /// Controller gesture with adapter-validated scene/player context.
    pub toggle_menu: bool,
    pub navigation: Vec<Navigation>,
    pub input_status: String,
}

pub trait GameAdapter: Send + Sync {
    fn cancel_selection(&self) {}
    fn set_menu_open(&self, _open: bool) {}
    fn configure_controls(&self, _controls: &crate::config::Controls) {}
    fn rules(&self) -> &'static dyn GameRules;
    fn startup_messages(&self) -> Vec<(&'static str, String)>;
    fn catalog(&self, override_path: &Path) -> Result<Option<Vec<Song>>>;
    fn take_search_index(&self) -> Option<Vec<(u32, String)>>;
    fn poll(&self) -> GameUpdate;
    fn submit(&self, selection: Selection);
    fn set_skip_target(&self, token: Option<u64>);
    fn diagnostics(&self) -> String;
    fn disabled(&self) -> bool;
    fn stop(&self);
}
