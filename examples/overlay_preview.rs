//! Developer preview of the real embedded assets/server, without loading the game.
//! Run `cargo run --example overlay_preview`, then open the printed URL.
use chart_requester::{
    overlay::{Server, snapshot},
    platforms::Connection,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let server = Server::start(
        32133,
        &snapshot(
            None,
            &Connection {
                connected: false,
                text: "预览服务：未连接游戏".into(),
            },
            0,
        ),
    )
    .await?;
    println!("Overlay: http://{}/queue (Ctrl+C to stop)", server.address);
    std::future::pending::<()>().await;
    Ok(())
}
