//! `cargo run -p anilili-core --example plugins [module] [anilist-id mal-id episode "Title|Other title"]`:
//! load the built provider modules (only `<module>.wasm` when given) and resolve an episode, sub and
//! dub, checking that each stream's first playlist really answers. Defaults to Frieren episode 1.
//!
//! With `PLAY=1`, each server's stream is also played for 3 seconds in the app's own libmpv
//! (`vendor/libmpv`), through the app's proxy when the app would use it, with the server's headers.
use anilili_core::model::Audio;
use anilili_core::plugin::{Plugins, TitleQuery};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let built = std::path::PathBuf::from("providers/target/wasm32-wasip2/release");
    let dir = match args.first().filter(|a| a.as_str() != "all") {
        Some(module) => {
            let only = std::path::PathBuf::from("target/plugins-only");
            let _ = std::fs::remove_dir_all(&only);
            std::fs::create_dir_all(&only)?;
            let file = format!("provider_{}.wasm", module.replace('-', "_"));
            std::fs::copy(built.join(&file), only.join(&file))?;
            only
        }
        None => built,
    };
    let anilist_id: i64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(154587);
    let mal_id: Option<i64> = args.get(2).and_then(|a| a.parse().ok()).or(Some(52991)).filter(|&m| m > 0);
    let episode: f64 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let titles: Vec<String> = args
        .get(4)
        .map(|t| t.split('|').map(str::to_owned).collect())
        .unwrap_or_else(|| vec!["Frieren: Beyond Journey's End".into(), "Sousou no Frieren".into()]);

    let http = anilili_core::net::Http::new()?;
    let plain = anilili_core::net::Http::plain()?;
    let play = std::env::var("PLAY").is_ok_and(|v| v == "1");
    let proxy = anilili_core::proxy::Proxy::start(http.clone()).await?;
    let started = std::time::Instant::now();
    let plugins = Plugins::load(http.clone(), &[dir], None, None, None).await?;
    println!(
        "loaded {:?} in {:.0} ms",
        plugins.list().iter().map(|p| format!("{} {}", p.id, p.version)).collect::<Vec<_>>(),
        started.elapsed().as_millis()
    );
    for audio in [Audio::Sub, Audio::Dub] {
        let started = std::time::Instant::now();
        let servers = plugins.servers(&TitleQuery { anilist_id, mal_id, titles: titles.clone(), episode, audio }).await;
        println!("{audio:?}: {} server(s) in {:.1}s", servers.len(), started.elapsed().as_secs_f64());
        for s in &servers {
            println!(
                "  {} [{}] streams={} subs={} url={}",
                s.id,
                s.track,
                s.streams.len(),
                s.subtitles.len(),
                s.streams.first().map(|x| x.url.as_str()).unwrap_or("-")
            );
            for sub in s.subtitles.iter().take(3) {
                println!("    sub {} ({}) {}", sub.label, sub.language, sub.url);
            }
            if let Some(url) = &s.thumbnails {
                match anilili_core::storyboard::frames(url, &s.headers).await {
                    Ok(frames) => println!("    thumbnails: {} frames, first {:?}", frames.len(), frames.first()),
                    Err(e) => println!("    thumbnails: {e}"),
                }
            }
            // Is the stream really there?
            if let Some(stream) = s.streams.first() {
                // The first bytes only: a progressive MP4 is the whole episode. The client is the one
                // the app's proxy would use for this stream.
                let client = match anilili_core::proxy::transform_for(&s.provider, &stream.url) {
                    anilili_core::proxy::Transform::MediaIdKey => &plain,
                    _ => &http,
                };
                let mut req =
                    client.client().get(&stream.url).header("User-Agent", anilili_core::net::USER_AGENT).header("Range", "bytes=0-255");
                for (k, v) in &s.headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                let r = req.send().await?;
                let status = r.status();
                let kind = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("-").to_owned();
                let body = r.bytes().await?;
                let head: String =
                    String::from_utf8_lossy(&body[..body.len().min(60)]).chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
                println!("    {} {status} {kind}: {head}", stream.format);
                if play {
                    println!("    play: {}", play_check(&proxy, s, &stream.url).await);
                }
            }
        }
    }
    Ok(())
}

/// Plays the stream in the app's own libmpv (no window, no audio) until 3 seconds have played:
/// proxied when the app proxies it, with the server's headers and the player's network options.
async fn play_check(proxy: &anilili_core::proxy::Proxy, server: &anilili_core::model::Server, url: &str) -> String {
    use anilili_core::proxy::{Session, Transform, transform_for};
    use anilili_mpv::{EndReason, Event, Format, Mpv, PropertyValue};
    let transform = transform_for(&server.provider, url);
    let url = if transform == Transform::None {
        url.to_owned()
    } else {
        proxy.register(url, Session { headers: server.headers.clone(), transform, proxy_media: false })
    };
    let library = std::path::Path::new("vendor/libmpv/win32-x64/libmpv-2.dll");
    let options = [
        ("vo", "null"),
        ("ao", "null"),
        ("idle", "yes"),
        ("terminal", "no"),
        ("ytdl", "no"),
        ("user-agent", anilili_core::net::USER_AGENT),
        ("network-timeout", "30"),
        ("demuxer-lavf-o", "seg_max_retry=8,reconnect=1,reconnect_streamed=1,reconnect_delay_max=6"),
    ];
    let mpv = match Mpv::new(library, &options) {
        Ok(m) => m,
        Err(e) => return format!("libmpv did not start: {e}"),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = mpv.observe("playback-time", Format::Double);
    let _ = mpv.request_log_messages("error");
    mpv.start_events(move |event| {
        let _ = tx.send(event);
    });
    for (name, value) in &server.headers {
        let _ = mpv.command(&["change-list", "http-header-fields", "append", &format!("{name}: {value}")]);
    }
    if let Err(e) = mpv.command(&["loadfile", &url]) {
        return format!("loadfile failed: {e}");
    }
    let started = std::time::Instant::now();
    let mut last_error = String::new();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(event) = rx.recv().await {
            match event {
                Event::PropertyChange { value: PropertyValue::Double(t), .. } if t >= 3.0 => return Ok(()),
                Event::Log { text, .. } => last_error = text.trim().to_owned(),
                Event::EndFile { reason: EndReason::Error, error, .. } => return Err(error.unwrap_or_default()),
                _ => {}
            }
        }
        Err("mpv stopped".into())
    })
    .await;
    let via = if transform == Transform::None { "" } else { " (proxied)" };
    match outcome {
        Ok(Ok(())) => format!("libmpv played 3 s after {:.1}s{via}", started.elapsed().as_secs_f64()),
        Ok(Err(e)) => format!("FAILED: {e} {last_error}"),
        Err(_) => format!("FAILED: timed out {last_error}"),
    }
}
