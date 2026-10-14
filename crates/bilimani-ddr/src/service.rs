//! Worker-owned state. All transitions receive time/data explicitly; no IO here.
use crate::{
    catalog::Catalog,
    ddr,
    game::{Chart, GameUpdate, Mode, NowPlaying, Phase, Selection, SelectionError, Snapshot},
};
use bilimani_api::{ApiError, Command, SelectionInput};
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    Waiting,
    Executing,
    Selected,
    Failed(SelectionError),
}
#[derive(Clone, Debug)]
pub struct Operation {
    pub id: u64,
    pub song_id: u32,
    mode: Mode,
    chart: Option<Chart>,
    until: u64,
    pub state: OperationState,
    queue_id: Option<u64>,
}
impl Operation {
    pub fn value(&self) -> Value {
        let (status, error) = match self.state {
            OperationState::Waiting => ("waiting", None),
            OperationState::Executing => ("executing", None),
            OperationState::Selected => ("selected", None),
            OperationState::Failed(error) => (
                "failed",
                Some(json!({"code": error, "message": error.to_string()})),
            ),
        };
        json!({"id": self.id, "song_id": self.song_id, "status": status, "error": error})
    }
    fn fail(&mut self, error: SelectionError) {
        self.state = OperationState::Failed(error);
    }
    fn terminal(&self) -> bool {
        matches!(
            self.state,
            OperationState::Selected | OperationState::Failed(_)
        )
    }
}
pub enum GameAction {
    Submit(Selection),
    Cancel,
}
pub struct Service {
    pub user: ddr::user::UserSnapshot,
    pub catalog: Option<Catalog>,
    pub snapshot: Snapshot,
    pub now_playing: NowPlaying,
    pub plays: u64,
    active: Option<Operation>,
    history: VecDeque<Operation>,
    next_id: u64,
    pub queue: bilimani_queue::Queue,
    pub menu_open: bool,
    pub close_menu: bool,
    now: u64,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            user: Default::default(),
            catalog: None,
            snapshot: Default::default(),
            now_playing: Default::default(),
            plays: 0,
            active: None,
            history: VecDeque::new(),
            next_id: 1,
            queue: Default::default(),
            menu_open: false,
            close_menu: false,
            now: 0,
        }
    }
}
impl Service {
    pub fn command(&mut self, command: Command, now: u64) -> Result<Value, ApiError> {
        self.now = now;
        match command {
            Command::Enqueue(input) => {
                if input.requester.len() > 512 || input.requester.chars().any(char::is_control) {
                    return Err(ApiError::InvalidRequest("点歌者名称无效".into()));
                }
                let selection = SelectionInput {
                    song_id: input.song_id,
                    mode: input.mode,
                    chart: input.chart,
                };
                let (title, mode, chart) = self.validate(&selection)?;
                let entry = self
                    .queue
                    .add(bilimani_queue::SongRequest {
                        song_id: input.song_id,
                        title,
                        mode: mode.0.into(),
                        chart: chart.map(|c| c.id.into()),
                        requester: input.requester,
                    })
                    .map_err(queue_error)?;
                Ok(json!(entry))
            }
            Command::Remove(id) => {
                self.queue.remove(id).map_err(queue_error)?;
                Ok(json!(self.queue.view(now)))
            }
            Command::Priority(id) => {
                if self.active.is_some() {
                    return Err(ApiError::Busy);
                }
                let entry = self.queue.get(id).ok_or(ApiError::NotFound)?;
                if self.snapshot.phase != Phase::Select
                    || self.snapshot.mode.map(|mode| mode.0) != Some(entry.request.mode.as_str())
                {
                    return Err(ApiError::ModeUnavailable);
                }
                self.queue.prioritize(id).map_err(queue_error)?;
                Ok(json!(self.queue.view(now)))
            }
            // Reload is intercepted by the worker's IO boundary.
            Command::ReloadAliases => Err(ApiError::Unavailable),
            Command::Search { query, limit } => {
                let songs = self
                    .catalog
                    .as_mut()
                    .ok_or(ApiError::NotReady)?
                    .search(&query, limit);
                Ok(json!({"songs": songs}))
            }
            Command::Select(input) => self.select(input, now),
            Command::Selection(id) => self
                .active
                .iter()
                .chain(self.history.iter())
                .find(|op| op.id == id)
                .map(Operation::value)
                .ok_or(ApiError::NotFound),
        }
    }
    fn select(&mut self, input: SelectionInput, now: u64) -> Result<Value, ApiError> {
        if self.active.is_some() {
            return Err(ApiError::Busy);
        }
        let view = self.queue.view(now);
        if view.current.is_some() || !view.items.is_empty() {
            return Err(ApiError::Busy);
        }
        self.start(input, None, now)
    }
    fn validate(&self, input: &SelectionInput) -> Result<(String, Mode, Option<Chart>), ApiError> {
        let catalog = self.catalog.as_ref().ok_or(ApiError::NotReady)?;
        let song = catalog
            .songs
            .iter()
            .find(|s| s.id == input.song_id)
            .ok_or(ApiError::NotFound)?;
        let chart = input
            .chart
            .as_deref()
            .map(|id| {
                ddr::parse_chart(id)
                    .ok_or_else(|| ApiError::InvalidRequest("未知的 DDR 谱面".into()))
            })
            .transpose()?;
        let mode = input
            .mode
            .as_deref()
            .map(|mode| match mode {
                "SP" => Ok(ddr::SP),
                "DP" => Ok(ddr::DP),
                _ => Err(ApiError::InvalidRequest("未知的 DDR 模式".into())),
            })
            .transpose()?
            .or(chart.map(|c| c.mode))
            .or(self.snapshot.mode)
            .ok_or(ApiError::ModeUnavailable)?;
        if chart.is_some_and(|c| c.mode != mode) || !song.supports(mode, chart) {
            return Err(ApiError::InvalidRequest(
                "歌曲不存在所请求的模式或谱面".into(),
            ));
        }
        Ok((song.title.clone(), mode, chart))
    }
    fn start(
        &mut self,
        input: SelectionInput,
        queue_id: Option<u64>,
        now: u64,
    ) -> Result<Value, ApiError> {
        let (_, mode, chart) = self.validate(&input)?;
        if self.snapshot.mode.is_some_and(|current| current != mode) {
            return Err(ApiError::ModeUnavailable);
        }
        let op = Operation {
            queue_id,
            id: self.next_id,
            song_id: input.song_id,
            mode,
            chart,
            until: now + 30_000,
            state: OperationState::Waiting,
        };
        self.next_id += 1;
        let value = op.value();
        self.active = Some(op);
        Ok(value)
    }
    pub fn update(&mut self, update: GameUpdate, now: u64) -> Option<GameAction> {
        self.user = update.user;
        self.now = now;
        self.snapshot = update.snapshot;
        self.now_playing = update.now_playing;
        self.plays = update.plays;
        self.queue.observe(update.plays, now);
        if self.active.is_none()
            && self.snapshot.phase == Phase::Select
            && let Some(mode) = self.snapshot.mode
            && let Some(dispatch) = self.queue.next(mode.0, self.menu_open)
        {
            let entry = dispatch.entry;
            if let Err(error) = self.start(
                SelectionInput {
                    song_id: entry.request.song_id,
                    mode: Some(entry.request.mode),
                    chart: entry.request.chart,
                },
                Some(entry.id),
                now,
            ) {
                self.queue.finish(
                    entry.id,
                    bilimani_queue::Completion::Failed(error.to_string()),
                    now,
                );
            }
        }
        let op = self.active.as_mut()?;
        if let Some(ack) = update.selection_result.filter(|ack| ack.token == op.id) {
            match ack.result {
                Some(Ok(())) => op.state = OperationState::Selected,
                Some(Err(error)) => op.fail(error),
                None => op.state = OperationState::Waiting,
            }
        }
        let mut action = None;
        if !op.terminal() && now >= op.until {
            op.fail(SelectionError::Expired);
            action = Some(GameAction::Cancel);
        } else if op.state == OperationState::Waiting && self.snapshot.phase == Phase::Select {
            if self.snapshot.mode != Some(op.mode) {
                op.fail(SelectionError::ModeChanged);
            } else {
                op.state = OperationState::Executing;
                action = Some(GameAction::Submit(Selection {
                    token: op.id,
                    song_id: op.song_id,
                    mode: op.mode,
                    chart: op.chart,
                    epoch: self.snapshot.epoch,
                }));
            }
        }
        if op.terminal() {
            let op = self.active.take().expect("active operation");
            if let Some(id) = op.queue_id {
                let completion = match op.state {
                    OperationState::Selected => bilimani_queue::Completion::Selected,
                    OperationState::Failed(error) => {
                        bilimani_queue::Completion::Failed(error.to_string())
                    }
                    _ => unreachable!("terminal operation"),
                };
                self.close_menu |= self.queue.finish(id, completion, now);
            }
            self.history.push_back(op);
            while self.history.len() > 32 {
                self.history.pop_front();
            }
        }
        action
    }
    pub fn menu_view(&self, path: &std::path::Path, notice: &str) -> bilimani_menu::View {
        bilimani_menu::View {
            queue: self.queue.view(self.now),
            can_select: self.snapshot.phase == Phase::Select && self.active.is_none(),
            mode: self.snapshot.mode.map(|m| m.0.into()),
            aliases_path: path.display().to_string(),
            aliases_count: self.catalog.as_ref().map_or(0, Catalog::aliases_count),
            notice: notice.into(),
        }
    }
    pub fn snapshots(&self) -> Vec<(String, Value)> {
        let game = json!(self.now_playing);
        let queue = json!(self.queue.view(self.now));
        let state = json!({"ready": self.catalog.is_some(), "phase": match self.snapshot.phase { Phase::Other => "other", Phase::Select => "select", Phase::Playing => "playing" },
            "mode": self.snapshot.mode, "epoch": self.snapshot.epoch, "plays": self.plays, "now_playing": game, "queue": queue});
        vec![
            (
                "/api/v1/meta".into(),
                json!({"api_version": 1, "game": "ddr", "major_version": "WORLD", "build": "2026022400", "ready": self.catalog.is_some()}),
            ),
            ("/api/v1/state".into(), state),
            ("/api/v1/queue".into(), queue),
            ("/api/v1/ddr/now-playing".into(), game.clone()),
            ("/api/v1/ddr/user".into(), json!(self.user)),
        ]
    }
}

fn queue_error(error: bilimani_queue::QueueError) -> ApiError {
    match error {
        bilimani_queue::QueueError::Full => ApiError::QueueFull,
        bilimani_queue::QueueError::Busy => ApiError::Busy,
        bilimani_queue::QueueError::NotFound => ApiError::NotFound,
    }
}
