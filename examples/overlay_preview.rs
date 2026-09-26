//! Developer preview of the real embedded assets/server, without loading the game.
//! Run `cargo run --example overlay_preview`, then open the printed URL.
use chart_requester::overlay::{Server, snapshot};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let server = Server::start(32133, &snapshot(None, "预览服务：未连接游戏", 0)).await?;
    println!("Preview: http://{}/ (Ctrl+C to stop)", server.address);
    std::future::pending::<()>().await;
    Ok(())
}
