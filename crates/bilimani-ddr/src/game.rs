use anyhow::Result;
use serde::Serialize;
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Mode(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Chart {
    pub mode: Mode,
    pub id: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct AvailableChart {
    pub chart: Chart,
    pub level: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Song {
    pub id: u32,
    pub title: String,
    #[serde(skip_serializing)]
    pub search_terms: Vec<String>,
    pub charts: Vec<AvailableChart>,
}
impl Song {
    pub fn supports(&self, mode: Mode, chart: Option<Chart>) -> bool {
        self.charts
            .iter()
            .any(|c| c.chart.mode == mode && chart.is_none_or(|wanted| wanted == c.chart))
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct NowPlaying {
    pub song: Option<SongInfo>,
    pub selected_chart: Option<AvailableChart>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SongInfo {
    pub id: u32,
    pub title: String,
    pub artist: String,
    pub charts: Vec<AvailableChart>,
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
    pub can_open_menu: bool,
    pub phase: Phase,
    pub mode: Option<Mode>,
    pub epoch: u64,
}
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
    pub result: Option<Result<(), SelectionError>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SelectionError {
    #[error("DDR chart is invalid")]
    InvalidChart,
    #[error("Song is unavailable in the current selection")]
    Unavailable,
    #[error("DDR did not accept the selection")]
    Rejected,
    #[error("Selection timed out")]
    Expired,
    #[error("Game mode changed")]
    ModeChanged,
}
#[derive(Default)]
pub struct GameUpdate {
    pub user: crate::ddr::user::UserSnapshot,
    pub toggle_menu: bool,
    pub navigation: Vec<Navigation>,
    pub snapshot: Snapshot,
    pub now_playing: NowPlaying,
    pub plays: u64,
    pub selection_result: Option<SelectionResult>,
}
pub trait GameAdapter: Send + Sync {
    fn set_menu_open(&self, open: bool);
    fn cancel_selection(&self);
    fn startup_messages(&self) -> Vec<(&'static str, String)>;
    fn catalog(&self, override_path: &Path) -> Result<Option<Vec<Song>>>;
    fn take_search_index(&self) -> Option<Vec<(u32, String)>> {
        None
    }
    fn poll(&self) -> GameUpdate;
    fn submit(&self, selection: Selection);
    fn diagnostics(&self) -> String;
    fn disabled(&self) -> bool;
    fn stop(&self);
}
pub use bilimani_menu::Navigation;
