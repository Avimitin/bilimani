//! Development simulator only: no native calls, hooks or live platform connection.
use bilimani_api::{Command, QueueInput};
use bilimani_iidx::{
    application::execute,
    game::*,
    iidx,
    service::{GameAction, Service},
};
use std::{io::BufRead, path::Path, sync::mpsc};

pub fn input() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error(
        "Commands: select | play | other | sp | dp | pause | resume | fail | queue | add <song-id> | remove <request-id> | priority <request-id> | reload"
    )]
    Syntax,
    #[error(transparent)]
    Api(#[from] bilimani_api::ApiError),
}
pub struct Simulator {
    phase: Phase,
    mode: Mode,
    epoch: u64,
    plays: u64,
    song_id: u32,
    started: u64,
    fail_next: bool,
    ack: Option<SelectionResult>,
}
impl Default for Simulator {
    fn default() -> Self {
        Self {
            phase: Phase::Select,
            mode: iidx::SP,
            epoch: 1,
            plays: 0,
            song_id: 1,
            started: 0,
            fail_next: false,
            ack: None,
        }
    }
}
impl Simulator {
    pub fn command(
        &mut self,
        line: &str,
        service: &mut Service,
        now: u64,
        aliases: &Path,
    ) -> Result<(), PreviewError> {
        let parts: Vec<_> = line.split_whitespace().collect();
        let command = match parts.as_slice() {
            ["select"] => {
                self.phase = Phase::Select;
                self.epoch += 1;
                None
            }
            ["play"] => {
                self.phase = Phase::Playing;
                self.plays += 1;
                self.started = now;
                None
            }
            ["other"] => {
                self.phase = Phase::Other;
                self.epoch += 1;
                None
            }
            ["sp"] => {
                self.mode = iidx::SP;
                self.epoch += 1;
                None
            }
            ["dp"] => {
                self.mode = iidx::DP;
                self.epoch += 1;
                None
            }
            ["pause"] => {
                service.menu_open = true;
                None
            }
            ["resume"] => {
                service.menu_open = false;
                None
            }
            ["fail"] => {
                self.fail_next = true;
                None
            }
            ["add", id] => Some(Command::Enqueue(QueueInput {
                song_id: id.parse().map_err(|_| PreviewError::Syntax)?,
                mode: Some(self.mode.0.into()),
                chart: None,
                requester: "Preview viewer".into(),
            })),
            ["remove", id] => Some(Command::Remove(
                id.parse().map_err(|_| PreviewError::Syntax)?,
            )),
            ["priority", id] => Some(Command::Priority(
                id.parse().map_err(|_| PreviewError::Syntax)?,
            )),
            ["reload"] => Some(Command::ReloadAliases),
            ["queue"] => {
                let view = service.queue.view(now);
                if let Some(current) = view.current {
                    println!(
                        "Current #{}: {}",
                        current.entry.id, current.entry.request.title
                    );
                }
                for item in view.items {
                    println!(
                        "Waiting #{}: {} {} {}",
                        item.entry.id,
                        item.entry.request.title,
                        item.entry.request.mode,
                        item.entry.error.as_deref().unwrap_or("")
                    );
                }
                None
            }
            [] => None,
            _ => return Err(PreviewError::Syntax),
        };
        if let Some(command) = command {
            let value = execute(service, command, now, aliases)?;
            if let Some(id) = value.get("id") {
                println!("Queued #{id}: {}", value["title"]);
            } else if let Some(count) = value.get("count") {
                println!("Reloaded {count} aliases");
            } else {
                println!("Queue updated; type queue to list request IDs");
            }
        }
        Ok(())
    }
    pub fn update(&mut self, now: u64) -> GameUpdate {
        GameUpdate {
            user: self.user(),
            snapshot: Snapshot {
                phase: self.phase,
                mode: Some(self.mode),
                epoch: self.epoch,
                can_open_menu: self.phase == Phase::Select,
            },
            plays: self.plays,
            now_playing: self.telemetry(now),
            selection_result: self.ack.take(),
            ..Default::default()
        }
    }
    pub fn execute(&mut self, action: GameAction) {
        match action {
            GameAction::Submit(selection) => {
                let result = if std::mem::take(&mut self.fail_next) {
                    Err(SelectionError::Unavailable)
                } else {
                    self.song_id = selection.song_id;
                    Ok(())
                };
                self.ack = Some(SelectionResult {
                    token: selection.token,
                    result: Some(result),
                });
            }
            GameAction::Cancel => self.ack = None,
        }
    }
    fn user(&self) -> iidx::user::UserSnapshot {
        use iidx::user::*;
        if self.phase == Phase::Other {
            return UserSnapshot::default();
        }
        UserSnapshot {
            active_side: Some(1),
            players: vec![UserProfile {
                side: 1,
                dj_name: "PREVIEW".into(),
                sp: StyleProfile {
                    dan: Some(DanRank::Kaiden),
                    dj_points: 24680,
                },
                dp: StyleProfile {
                    dan: Some(DanRank::Dan8),
                    dj_points: 1357,
                },
                weekly: Some(WeeklyActivity {
                    period: ActivityPeriod::GameWeek,
                    week_id: 100,
                    date: 20261005,
                    sp: Some(ActivityCounts {
                        songs: 128 + self.plays,
                        keys: 186432 + self.plays * 1500,
                    }),
                    dp: Some(ActivityCounts {
                        songs: 12,
                        keys: 14200,
                    }),
                }),
            }],
        }
    }
    fn telemetry(&self, now: u64) -> NowPlaying {
        if self.phase == Phase::Other {
            return NowPlaying::default();
        }
        let charts: Vec<_> = iidx::available_charts([0, 5, 10, 12, 0, 0, 5, 9, 12, 0])
            .into_iter()
            .map(|available| ChartInfo {
                chart: available.chart,
                difficulty: match available.chart.id.chars().last() {
                    Some('A') => "ANOTHER",
                    Some('H') => "HYPER",
                    _ => "NORMAL",
                },
                level: available.level,
                style: match available.chart.id.chars().last() {
                    Some('A') => ChartStyle::Red,
                    Some('H') => ChartStyle::Amber,
                    _ => ChartStyle::Blue,
                },
                bpm: Some(BpmRange { min: 150, max: 180 }),
                note_count: Some(1234),
                radar: Some(Radar {
                    notes: 100.0,
                    peak: 120.0,
                    scratch: 70.0,
                    soflan: 20.0,
                    charge: 60.0,
                    chord: 90.0,
                }),
                density: Some(NoteDensity {
                    bin_ms: 1000,
                    duration_ms: 120_000,
                    notes: (0..120).map(|i| 5 + i % 20).collect(),
                    scratch: (0..120).map(|i| i % 4).collect(),
                }),
                lane_counts: Some(LaneCounts {
                    sides: (1..=if available.chart.mode == iidx::DP {
                        2
                    } else {
                        1
                    })
                        .map(|side| LaneSideCounts {
                            side,
                            keys: [100, 150, 200, 160, 180, 210, 130],
                            scratch: 104,
                        })
                        .collect(),
                }),
            })
            .collect();
        let chart = iidx::parse_chart(if self.mode == iidx::SP { "SPA" } else { "DPA" })
            .expect("fixture chart");
        NowPlaying {
            phase: if self.phase == Phase::Playing {
                SongPhase::Playing
            } else {
                SongPhase::Selecting
            },
            song: Some(SongInfo {
                id: self.song_id,
                title: format!("Preview Song {}", self.song_id),
                artist: "Synthetic Artist".into(),
                genre: "DEVELOPMENT FIXTURE".into(),
                game_version: Some(33),
                charts,
            }),
            players: vec![PlayerChart { side: 1, chart }],
            lane_order: (1..=if self.mode == iidx::DP { 2 } else { 1 })
                .map(|side| LaneOrder {
                    side,
                    random: RandomMode::Off,
                    mirror: false,
                    status: LaneOrderStatus::Ready,
                    keys: Some([1, 2, 3, 4, 5, 6, 7]),
                })
                .collect(),
            playback: (self.phase == Phase::Playing).then_some(PlaybackProgress {
                position_ms: now.saturating_sub(self.started).min(120_000) as u32,
                duration_ms: 120_000,
            }),
        }
    }
}
