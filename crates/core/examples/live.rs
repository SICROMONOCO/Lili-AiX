//! Live smoke test against the real services: `cargo run -p anilili-core --example live [anilist id]`.

use anilili_core::anilist::AniList;
use anilili_core::miruro::Miruro;
use anilili_core::net::Http;
use anilili_core::proxy::{Proxy, Session, transform_for};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id: i64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(154587);
    let http = Http::new()?;

    let anilist = AniList::new(http.clone());
    let detail = anilist.detail(id).await?;
    println!(
        "AniList: {} ({} eps, {} relations, {} recs)",
        detail.card.title,
        detail.card.episodes.unwrap_or(0),
        detail.relations.len(),
        detail.recommendations.len()
    );

    let miruro = Miruro::new(http.clone());
    let title = miruro.title(id).await?.ok_or("Miruro has no such title")?;
    let list = miruro.episodes(&title).await?;
    println!("Miruro: {} -> {} episodes (sub {:?}, dub {:?})", title.id, list.episodes.len(), list.sub_count, list.dub_count);

    let first = &list.episodes[0];
    let sources = miruro.sources(&title.id, first.number, first.skip).await?;
    for server in &sources.servers {
        println!(
            "  {:>6} {:<5} {:<22} native={} streams={} subs={} {}",
            server.label,
            server.track,
            server.name,
            server.is_native(),
            server.streams.len(),
            server.subtitles.len(),
            server.embed.as_deref().unwrap_or("")
        );
    }

    // Through the proxy: fetch a Kwik playlist and confirm the IVs were made explicit.
    if let Some(kwik) = sources.servers.iter().find(|s| s.provider == "animepahe" && s.is_native()) {
        let proxy = Proxy::start(http.clone()).await?;
        let stream = &kwik.streams[0];
        let local = proxy.register(
            &stream.url,
            Session { headers: kwik.headers.clone(), transform: transform_for(&kwik.provider, &stream.url), proxy_media: false },
        );
        let text = http.client().get(&local).send().await?.text().await?;
        let ivs = text.lines().filter(|l| l.contains(",IV=0x")).count();
        println!("proxy {local}\n  kwik playlist: {} lines, {ivs} explicit IVs", text.lines().count());
    }
    Ok(())
}
