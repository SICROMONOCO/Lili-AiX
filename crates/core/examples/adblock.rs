//! Loads the real filter lists from vendor/filters/ (scripts/fetch-filters.mjs), times the engine
//! build, and checks requests an embed page makes: ad hosts must be refused, and what players need
//! must not be. Exits non-zero on a wrong answer.
//!
//! cargo run -p anilili-core --example adblock [url source kind]...

use std::time::Instant;

use anilili_core::adblock::{Blocker, Kind};

const EMBED: &str = "https://megaplay.buzz/stream/s-2/12345/sub";

/// (url, kind, refused?)
const CASES: &[(&str, Kind, bool)] = &[
    ("https://www.googletagmanager.com/gtag/js?id=G-XXXX", Kind::Script, true),
    ("https://a.magsrv.com/ad-provider.js", Kind::Script, true),
    ("https://cdn.popcash.net/show.js", Kind::Script, true),
    ("https://acscdn.com/script/aclib.js", Kind::Script, true),
    ("https://static.cloudflareinsights.com/beacon.min.js", Kind::Script, true),
    ("https://www.google-analytics.com/g/collect?v=2", Kind::Ping, true),
    ("https://cdn.jsdelivr.net/npm/hls.js@1.5.15/dist/hls.min.js", Kind::Script, false),
    ("https://cdnjs.cloudflare.com/ajax/libs/video.js/8.10.0/video.min.js", Kind::Script, false),
    ("https://ssl.p.jwpcdn.com/player/v/8.36.4/jwplayer.js", Kind::Script, false),
    ("https://cdn.plyr.io/3.7.8/plyr.js", Kind::Script, false),
    ("https://vjs.zencdn.net/8.10.0/video.min.js", Kind::Script, false),
    ("https://code.jquery.com/jquery-3.7.1.min.js", Kind::Script, false),
    ("https://ajax.googleapis.com/ajax/libs/jquery/3.7.1/jquery.min.js", Kind::Script, false),
    ("https://fonts.googleapis.com/css2?family=Roboto", Kind::Stylesheet, false),
    ("https://challenges.cloudflare.com/turnstile/v0/api.js", Kind::Script, false),
    ("https://www.gstatic.com/cv/js/sender/v1/cast_sender.js", Kind::Script, false),
    ("https://s.megastatics.com/subtitle/abc/eng-2.vtt", Kind::Xhr, false),
    ("https://rrr.lab27core.site/pz/abc/master.m3u8", Kind::Xhr, false),
];

fn kind(name: &str) -> Kind {
    match name {
        "script" => Kind::Script,
        "image" => Kind::Image,
        "xhr" => Kind::Xhr,
        "subdocument" => Kind::Subdocument,
        "media" => Kind::Media,
        _ => Kind::Other,
    }
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/filters");
    let lists: Vec<String> = std::fs::read_dir(&dir)
        .expect("run scripts/fetch-filters.mjs first")
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
        .collect();
    let started = Instant::now();
    let blocker = Blocker::with_lists(lists.iter().map(String::as_str));
    println!("{} lists, engine built in {:.2?}", blocker.lists(), started.elapsed());

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() >= 3 {
        for chunk in args.chunks(3) {
            println!("{:?}  {}", blocker.check(&chunk[0], &chunk[1], kind(&chunk[2])), chunk[0]);
        }
        return;
    }
    let started = Instant::now();
    let mut wrong = 0;
    for (url, kind, refused) in CASES {
        let verdict = blocker.check(url, EMBED, *kind);
        let ok = verdict.is_some() == *refused;
        wrong += usize::from(!ok);
        println!("{}  {:<9} {url}  {verdict:?}", if ok { "ok " } else { "BAD" }, if *refused { "refuse" } else { "allow" });
    }
    println!("{} checks in {:.2?}", CASES.len(), started.elapsed());
    // Every request an embed makes goes through this: the steady state, after the first check.
    let started = Instant::now();
    for (url, kind, _) in CASES.iter().cycle().take(1000) {
        let _ = blocker.check(url, EMBED, *kind);
    }
    println!("then {:.1?} per check", started.elapsed() / 1000);
    std::process::exit(i32::from(wrong > 0));
}
