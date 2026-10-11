//! Development-only native API fixture, never included in the DLL or release.
mod preview;
use bilimani_iidx::{catalog::Catalog, game::Song, iidx, service::Service};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dist/web"));
    let (server, mut commands) =
        bilimani_api::Server::start(([127, 0, 0, 1], 32134).into(), &root).await?;
    let mut service = Service::default();
    service.catalog = Some(Catalog::new(
        (1..=9)
            .map(|id| Song {
                id,
                title: format!("Preview Song {id}"),
                search_terms: vec![],
                charts: iidx::available_charts([0, 5, 10, 12, 0, 0, 5, 9, 12, 0]),
            })
            .collect(),
    )?);
    let now = Instant::now();
    let mut simulator = preview::Simulator::default();
    let input = preview::input();
    let aliases = std::env::var_os("BILIMANI_PREVIEW_ALIASES")
        .map(PathBuf::from)
        .unwrap_or_else(|| "bilimani_iidx_aliases.csv".into());
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    println!(
        "API preview: http://{}/iidx/mecha/ (Ctrl+C to stop)",
        server.address
    );
    println!("{}", preview::PreviewError::Syntax);
    loop {
        tick.tick().await;
        let millis = now.elapsed().as_millis() as u64;
        for line in input.try_iter().take(32) {
            if let Err(error) = simulator.command(&line, &mut service, millis, &aliases) {
                eprintln!("{error}");
            }
        }
        if let Some(action) = service.update(simulator.update(millis), millis) {
            simulator.execute(action);
        }
        if std::mem::take(&mut service.close_menu) {
            service.menu_open = false;
        }
        while let Ok(message) = commands.try_recv() {
            let result = bilimani_iidx::application::execute(
                &mut service,
                message.command,
                millis,
                &aliases,
            );
            let _ = message.reply.send(result);
        }
        server.publish(service.snapshots())?;
    }
}
