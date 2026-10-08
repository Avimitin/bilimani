//! Exercise the same core using a deliberately non-IIDX vocabulary and layout.
use anyhow::Result;
use bilimani::{
    catalog::Catalog,
    config::EngineConfig,
    engine::Engine,
    game::{
        AvailableChart, Chart, ChartStyle, GameAdapter, GameRules, GameUpdate, Mode, Phase,
        Selection, SelectionResult, SkipRequest, Snapshot, Song,
    },
    overlay,
    platforms::{Chat, ChatSource, Connection, Event},
};
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, watch};

const KEYS: Mode = Mode("KEYS");
const DRUMS: Mode = Mode("DRUMS");
const EXTRA: Chart = Chart {
    mode: KEYS,
    id: "EXPERT+",
};
const DRUM_EXTRA: Chart = Chart {
    mode: DRUMS,
    id: "D-EXPERT",
};
struct Rules;
static RULES: Rules = Rules;
impl GameRules for Rules {
    fn parse_chart(&self, text: &str) -> Option<Chart> {
        [EXTRA, DRUM_EXTRA]
            .into_iter()
            .find(|c| c.id.eq_ignore_ascii_case(text))
    }
    fn request_hint(&self) -> &'static str {
        "点歌 <曲名> [EXPERT+ 等难度]"
    }
    fn chart_style(&self, _: Chart) -> ChartStyle {
        ChartStyle::Purple
    }
}
fn songs() -> Vec<Song> {
    (1..=2)
        .map(|id| Song {
            id,
            title: format!("Test Song {id}"),
            search_terms: vec![format!("keyword{id}")],
            // More than ten charts, no SP/DP and no level-12 limit in core.
            charts: [
                "EXPERT+", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10",
            ]
            .into_iter()
            .map(|id| AvailableChart {
                chart: Chart { mode: KEYS, id },
                level: "20".into(),
            })
            .chain([AvailableChart {
                chart: DRUM_EXTRA,
                level: "9.95".into(),
            }])
            .collect(),
        })
        .collect()
}
#[derive(Default)]
struct State {
    snapshot: Snapshot,
    plays: u64,
    result: Option<SelectionResult>,
    skip: Option<SkipRequest>,
    target: Option<u64>,
    selections: Vec<Selection>,
    stopped: bool,
}
struct FakeGame(Arc<Mutex<State>>);
impl GameAdapter for FakeGame {
    fn rules(&self) -> &'static dyn GameRules {
        &RULES
    }
    fn startup_messages(&self) -> Vec<(&'static str, String)> {
        vec![]
    }
    fn catalog(&self, _: &Path) -> Result<Option<Vec<Song>>> {
        Ok(Some(songs()))
    }
    fn take_search_index(&self) -> Option<Vec<(u32, String)>> {
        None
    }
    fn poll(&self) -> GameUpdate {
        let mut s = self.0.lock().unwrap();
        GameUpdate {
            player_card: None,
            snapshot: s.snapshot,
            now_playing: Default::default(),
            plays: s.plays,
            selection_result: s.result.take(),
            skip: s.skip.take(),
            toggle_menu: false,
            navigation: vec![],
            input_status: "test input ready".into(),
        }
    }
    fn submit(&self, request: Selection) {
        self.0.lock().unwrap().selections.push(request);
    }
    fn set_skip_target(&self, token: Option<u64>) {
        self.0.lock().unwrap().target = token;
    }
    fn diagnostics(&self) -> String {
        "fake adapter".into()
    }
    fn disabled(&self) -> bool {
        self.0.lock().unwrap().stopped
    }
    fn stop(&self) {
        self.0.lock().unwrap().stopped = true;
    }
}
struct FakeSource;
impl ChatSource for FakeSource {
    fn run(
        self: Box<Self>,
        tx: mpsc::Sender<Event>,
        mut stop: watch::Receiver<bool>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            tx.send(Event::Status(Connection {
                connected: true,
                text: "Connected to test source".into(),
            }))
            .await
            .unwrap();
            for text in ["点歌 keyword1 expert+", "点歌 keyword2"] {
                tx.send(Event::Chat(Chat {
                    user: "0".into(), // Valid opaque ID for this source.
                    name: "Tester".into(),
                    text: text.into(),
                }))
                .await
                .unwrap();
            }
            while !*stop.borrow() {
                if stop.changed().await.is_err() {
                    break;
                }
            }
        })
    }
}

#[tokio::test]
async fn non_iidx_adapter_and_non_bilibili_source_drive_the_same_engine() {
    let shared = Arc::new(Mutex::new(State {
        snapshot: Snapshot {
            phase: Phase::Select,
            mode: Some(KEYS),
            epoch: 7,
            can_skip: true,
        },
        ..State::default()
    }));
    let game: Box<dyn GameAdapter> = Box::new(FakeGame(shared.clone()));
    let catalog = Catalog::new(
        game.catalog(Path::new("")).unwrap().unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(catalog.songs[0].charts.len(), 12);
    assert_eq!(catalog.songs[0].charts[11].level, "9.95");
    let mut engine = Engine::new(EngineConfig::default(), catalog, game.rules());
    engine.observe(game.poll().snapshot, 0);
    let (tx, mut rx) = mpsc::channel(8);
    let (stop, stop_rx) = watch::channel(false);
    let source: Box<dyn ChatSource> = Box::new(FakeSource);
    let task = tokio::spawn(source.run(tx, stop_rx));
    let Event::Status(connection) = rx.recv().await.unwrap() else {
        panic!("missing connection");
    };
    assert!(
        overlay::snapshot(None, &connection, None, 0, &overlay::History::default())["connected"]
            .as_bool()
            .unwrap()
    );
    for _ in 0..2 {
        let Event::Chat(chat) = rx.recv().await.unwrap() else {
            panic!("missing chat");
        };
        assert_eq!(engine.chat(chat, 0), "enqueued");
    }
    assert_eq!(engine.queue[0].chart, Some(EXTRA));
    assert_eq!(engine.queue[1].chart, None);
    let jump = engine.next_jump(1).unwrap();
    game.submit(jump.selection());
    assert_eq!(engine.queue.len(), 2); // Submission alone does not dequeue.
    let token = jump.request.token;
    {
        let mut s = shared.lock().unwrap();
        assert_eq!(s.selections[0].song_id, 1);
        assert_eq!(s.selections[0].chart, Some(EXTRA));
        s.result = Some(SelectionResult {
            token,
            result: None,
        });
    }
    let ack = game.poll().selection_result.unwrap();
    engine.jump_result(ack.token, ack.result, 1);
    assert_eq!(engine.queue.len(), 2);
    game.submit(engine.next_jump(2).unwrap().selection());
    shared.lock().unwrap().result = Some(SelectionResult {
        token,
        result: Some(Ok(())),
    });
    let ack = game.poll().selection_result.unwrap();
    engine.jump_result(ack.token, ack.result, 2);
    assert_eq!(engine.queue.len(), 1);
    game.set_skip_target(Some(token));
    assert_eq!(shared.lock().unwrap().target, Some(token));
    let view = overlay::snapshot(
        Some(&engine),
        &connection,
        None,
        2,
        &overlay::History::default(),
    );
    assert_eq!(view["current"]["mode"], "KEYS");
    assert_eq!(view["current"]["chart"], "EXPERT+");
    assert_eq!(view["current"]["chart_style"], "purple");
    assert!(engine.render(2).0.contains("Test Song 1 [EXPERT+]"));

    // Skip is an adapter event; core has no Start button or player-side rule.
    shared.lock().unwrap().skip = Some(SkipRequest {
        token,
        epoch: 7,
        description: "test gesture".into(),
    });
    let update = game.poll();
    engine.observe(update.snapshot, 3);
    let skip = update.skip.unwrap();
    assert!(engine.skip_current(skip.token, skip.epoch, 3));
    assert_eq!(engine.next_jump(3).unwrap().request.song.id, 2);
    stop.send(true).unwrap();
    task.await.unwrap();
    game.stop();
    assert!(game.disabled());
}

#[test]
fn vocabulary_and_skip_eligibility_come_from_the_adapter() {
    let mut e = Engine::new(
        EngineConfig::default(),
        Catalog::new(songs(), &Default::default()).unwrap(),
        &RULES,
    );
    e.observe(
        Snapshot {
            phase: Phase::Select,
            mode: Some(KEYS),
            epoch: 1,
            can_skip: false,
        },
        0,
    );
    let chat = |text: &str| Chat {
        user: "user".into(),
        name: "Viewer".into(),
        text: text.into(),
    };
    assert_eq!(e.chat(chat("点歌"), 0), "rejected_missing_song");
    assert!(e.messages.back().unwrap().1.contains("EXPERT+"));
    assert_eq!(
        e.chat(chat("点歌 keyword1 D-EXPERT"), 0),
        "rejected_opposite_mode"
    );
    assert_eq!(e.chat(chat("点歌 keyword1 EXPERT+"), 0), "enqueued");
    let jump = e.next_jump(0).unwrap();
    e.jump_result(jump.request.token, Some(Ok(())), 0);
    assert!(!e.skip_current(jump.request.token, 1, 0));
    e.snapshot.can_skip = true;
    assert!(e.skip_current(jump.request.token, 1, 0));
}

#[test]
fn registry_selects_only_verified_profiles() {
    use bilimani::games::{Profile, iidx::v33::SUPPORTED_SHA256, resolve};
    assert_eq!(resolve(SUPPORTED_SHA256), Some(Profile::Iidx33));
    for unknown in [
        "",
        "IIDX33",
        "2025090100",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    ] {
        assert_eq!(resolve(unknown), None);
    }
}
