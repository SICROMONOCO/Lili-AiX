//! Verifies a live signed manifest on the mirror with the desktop verifier:
//! `cargo run -p anilili-core --example verify_channel` (the Android channel) or
//! `cargo run -p anilili-core --example verify_channel desktop` (this app's channel).
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let http = anilili_core::net::Http::new()?;
    let desktop = std::env::args().nth(1).as_deref() == Some("desktop");
    let (file, d_tag) = if desktop { ("desktop.json", "anilili-desktop-update") } else { ("update.json", "anilili-update") };
    let url = format!("https://huggingface.co/pommieye/kitsune-7b/resolve/main/latest/{file}");
    let (body, _) = http.get_bytes("mirror", &url, &[], std::time::Duration::from_secs(20)).await?;
    anilili_core::update::verify_channel(&body, d_tag)?;
    let event: serde_json::Value = serde_json::from_slice(&body)?;
    let content = event.get("content").and_then(|c| c.as_str()).unwrap_or_default();
    println!("{d_tag}: verified against the project key ({} bytes)", body.len());
    println!("content: {}", content.chars().take(300).collect::<String>());
    Ok(())
}
