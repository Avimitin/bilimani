//! Developer preview of the real static files/server, without loading the game.
//! Run `cargo run --example overlay_preview`, then open the printed URL.
use chart_requester::{
    overlay::{History, Server, snapshot},
    platforms::Connection,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let static_dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web"));
    let server = Server::start(
        32133,
        &static_dir,
        &snapshot(
            None,
            &Connection {
                connected: false,
                text: "预览服务：未连接游戏".into(),
            },
            0,
            &History::default(),
        ),
    )
    .await?;
    println!("Overlay: http://{}/queue (Ctrl+C to stop)", server.address);
    std::future::pending::<()>().await;
    Ok(())
}
