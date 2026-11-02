//! `cargo run -p anilili-core --example ja4`: the TLS/HTTP2 fingerprint each of our clients
//! presents, as tls.peet.ws sees it. Compare with WebView2's (`fingerprint_compare` in the app).
use anilili_core::net::Http;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (name, http) in [("chrome", Http::new()?), ("edge", Http::edge()?)] {
        let body = http.client().get("https://tls.peet.ws/api/all").send().await?.text().await?;
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let field = |p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("?").to_owned();
        println!(
            "{name}: ja4={}\n  ja4_r={}\n  akamai={}",
            field("/tls/ja4"),
            field("/tls/ja4_r"),
            field("/http2/akamai_fingerprint_hash")
        );
    }
    Ok(())
}
