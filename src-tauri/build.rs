fn main() {
    open_subtitles_key();
    tauri_build::build()
}

/// The OpenSubtitles consumer key, built in as Android's is (`openSubtitlesApiKey` in its private
/// Gradle properties, or `OPEN_SUBTITLES_API_KEY`), and never written into this repository. Without
/// one the app builds with OpenSubtitles switched off.
fn open_subtitles_key() {
    const ENV: &str = "OPEN_SUBTITLES_API_KEY";
    println!("cargo:rerun-if-env-changed={ENV}");
    println!("cargo:rerun-if-env-changed=GRADLE_USER_HOME");
    let from_env = std::env::var(ENV).ok().filter(|k| !k.trim().is_empty());
    let gradle = std::env::var_os("GRADLE_USER_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|h| std::path::Path::new(&h).join(".gradle")))
        .map(|dir| dir.join("gradle.properties"));
    let from_gradle = || {
        let path = gradle.as_ref()?;
        let text = std::fs::read_to_string(path).ok()?;
        // Only for a file that exists: a missing one would rebuild every time.
        println!("cargo:rerun-if-changed={}", path.display());
        text.lines().find_map(|line| line.trim().strip_prefix("openSubtitlesApiKey=").map(|v| v.trim().to_owned()))
    };
    let key = from_env.or_else(from_gradle).unwrap_or_default();
    println!("cargo:rustc-env=ANILILI_OPEN_SUBTITLES_KEY={}", key.trim());
}
