//! Developer preview of the real static files/server, without loading the game.
//! Run `cargo run --example overlay_preview`, then open the printed URL.
use bilimani::{
    overlay::{History, Server, snapshot},
    platforms::Connection,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let static_dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/card"));
    let server = Server::start(
        "127.0.0.1:32133".parse()?,
        &static_dir,
        &snapshot(
            None,
            &Connection {
                connected: false,
                text: "预览服务：未连接游戏".into(),
            },
            None,
            0,
            &History::default(),
        ),
    )
    .await?;
    println!("Overlay: http://{}/queue (Ctrl+C to stop)", server.address);
    std::future::pending::<()>().await;
    Ok(())
}
