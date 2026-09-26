use chart_requester::{catalog::Catalog, games::iidx::v33::catalog::parse_database};
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: cargo run --example catalog_check -- <music_data.bin> [query]")
    })?;
    let songs = parse_database(&std::fs::read(path)?)?;
    println!("{} canonical songs parsed", songs.len());
    let mut c = Catalog::new(songs, &Default::default())?;
    if let Some(q) = std::env::args().nth(2) {
        for s in c.search(&q, 5) {
            println!("{} {} {:?}", s.id, s.title, s.charts);
        }
    }
    Ok(())
}
