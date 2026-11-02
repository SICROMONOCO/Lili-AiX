//! Live download test: `cargo run -p anilili-core --example download [anilist id] [episode] [provider]`.
//! Saves to target/downloads/ and prints the file size.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anilili_core::download::{DownloadRequest, download};
use anilili_core::miruro::Miruro;
use anilili_core::net::Http;
use anilili_core::proxy::transform_for;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let id: i64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(154587);
    let episode: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let provider = args.next().unwrap_or_else(|| "animepahe".into());

    let http = Http::new()?;
    let miruro = Miruro::new(http.clone());
    let title = miruro.title(id).await?.ok_or("unknown title")?;
    let sources = miruro.sources(&title.id, episode, None).await?;
    let server = sources.servers.iter().find(|s| s.provider == provider && s.is_native()).ok_or("provider has no native server")?;
    // Smallest rendition: this is a correctness test, not a quality one.
    let stream = server.streams.iter().min_by_key(|s| s.height.unwrap_or(u32::MAX)).unwrap();
    println!("{} {} {:?} {}", server.id, stream.format, stream.height, stream.url);

    let request = DownloadRequest {
        url: stream.url.clone(),
        format: stream.format.clone(),
        headers: server.headers.clone(),
        transform: transform_for(&server.provider, &stream.url),
        output: std::path::PathBuf::from("target/downloads").join(format!("{provider}-{id}-e{episode}")),
        subtitles: server.subtitles.clone(),
        max_height: None,
    };
    let started = std::time::Instant::now();
    let saved = download(&http, &request, Arc::new(AtomicBool::new(false)), |p| {
        if p.done % 20 == 0 || p.done == p.total {
            println!("  {}/{} segments, {:.1} MB", p.done, p.total, p.bytes as f64 / 1e6);
        }
    })
    .await?;
    let path = saved.path;
    let size = std::fs::metadata(&path)?.len();
    println!("saved {} ({:.1} MB, {:?}p) in {:.0}s", path.display(), size as f64 / 1e6, saved.height, started.elapsed().as_secs_f64());
    Ok(())
}
