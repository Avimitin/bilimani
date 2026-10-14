//! Development simulator using the production DDR HTTP, queue and search code.
use bilimani_ddr::{
    catalog::Catalog,
    ddr,
    game::*,
    service::{GameAction, Service},
};
use std::{
    io::BufRead,
    path::PathBuf,
    time::{Duration, Instant},
};
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "dist/web".into());
    let (server, mut commands) =
        bilimani_api::Server::start(([127, 0, 0, 1], 32134).into(), &root).await?;
    let charts: Vec<_> = ddr::CHARTS
        .iter()
        .enumerate()
        .map(|(i, c)| AvailableChart {
            chart: *c,
            level: (i + 5).to_string(),
        })
        .collect();
    let songs: Vec<_> = [
        "Streetlight Shuffle",
        "Neon Footwork",
        "Concrete Rhythm",
        "Midnight Cypher",
        "踊る街",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, title)| Song {
        id: i as u32 + 1,
        title: title.into(),
        charts: charts.clone(),
        search_terms: vec![],
    })
    .collect();
    let mut service = Service::default();
    service.catalog = Some(Catalog::new(songs.clone())?);
    let mut snapshot = Snapshot {
        can_open_menu: true,
        phase: Phase::Select,
        mode: Some(ddr::SP),
        epoch: 1,
    };
    let mut song = 0;
    let mut difficulty = 3;
    let mut plays = 0;
    let mut ack = None;
    let (tx, input) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let clock = Instant::now();
    let aliases = PathBuf::from("config/bilimani_ddr_aliases.csv");
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    println!(
        "DDR synthetic preview: http://{}/ddr/street/",
        server.address
    );
    println!("Commands: select, play, other, sp, dp, next, enqueue, quit");
    loop {
        tick.tick().await;
        let now = clock.elapsed().as_millis() as u64;
        for line in input.try_iter() {
            match line.trim() {
                "quit" => return Ok(()),
                "select" => {
                    snapshot.phase = Phase::Select;
                    snapshot.epoch += 1;
                }
                "play" => {
                    snapshot.phase = Phase::Playing;
                    plays += 1;
                    snapshot.epoch += 1;
                }
                "other" => {
                    snapshot.phase = Phase::Other;
                    snapshot.epoch += 1;
                }
                "sp" => {
                    snapshot.mode = Some(ddr::SP);
                    snapshot.epoch += 1;
                }
                "dp" => {
                    snapshot.mode = Some(ddr::DP);
                    snapshot.epoch += 1;
                }
                "next" => song = (song + 1) % songs.len(),
                "enqueue" => {
                    let _ = service.command(
                        bilimani_api::Command::Enqueue(bilimani_api::QueueInput {
                            song_id: songs[song].id,
                            mode: Some("SP".into()),
                            chart: Some("ESP".into()),
                            requester: "Preview dancer".into(),
                        }),
                        now,
                    );
                }
                command if command.starts_with("chart ") => {
                    if let Some(chart) = ddr::parse_chart(command[6..].trim()) {
                        snapshot.mode = Some(chart.mode);
                        difficulty = (0..=4)
                            .find(|d| ddr::native_chart(chart.mode, *d) == Some(chart))
                            .unwrap();
                        snapshot.epoch += 1;
                    } else {
                        eprintln!("Use chart bSP/BSP/DSP/ESP/CSP/BDP/DDP/EDP/CDP");
                    }
                }
                _ => eprintln!("Unknown command"),
            }
        }
        snapshot.can_open_menu = snapshot.phase == Phase::Select;
        let selected = &songs[song];
        let update = GameUpdate {
            snapshot,
            plays,
            selection_result: ack.take(),
            now_playing: NowPlaying {
                selected_chart: selected
                    .charts
                    .iter()
                    .find(|entry| {
                        Some(entry.chart)
                            == snapshot
                                .mode
                                .and_then(|mode| ddr::native_chart(mode, difficulty))
                    })
                    .cloned(),
                song: Some(SongInfo {
                    id: selected.id,
                    title: selected.title.clone(),
                    artist: "Synthetic preview / BILIMANI".into(),
                    charts: selected.charts.clone(),
                }),
            },
            user: ddr::user::UserSnapshot {
                players: vec![ddr::user::Player {
                    side: 1,
                    name: "PREVIEW".into(),
                }],
            },
            ..Default::default()
        };
        if let Some(action) = service.update(update, now) {
            match action {
                GameAction::Cancel => ack = None,
                GameAction::Submit(selection) => {
                    if let Some(chart) = selection.chart {
                        difficulty = (0..=4)
                            .find(|d| ddr::native_chart(chart.mode, *d) == Some(chart))
                            .unwrap();
                    }
                    song = songs
                        .iter()
                        .position(|s| s.id == selection.song_id)
                        .unwrap();
                    ack = Some(SelectionResult {
                        token: selection.token,
                        result: Some(Ok(())),
                    });
                }
            }
        }
        for _ in 0..64 {
            let Ok(message) = commands.try_recv() else {
                break;
            };
            let result =
                bilimani_ddr::application::execute(&mut service, message.command, now, &aliases);
            let _ = message.reply.send(result);
        }
        server.publish(service.snapshots())?;
    }
}
