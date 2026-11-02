//! `cargo run -p anilili-core --example probe <url> [referer]`: GET through the app's client;
//! prints the status and saves the body to target/probe-last.txt.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args().nth(1).expect("url");
    let http = anilili_core::net::Http::new()?;
    let mut request = http.client().get(&url).header("User-Agent", anilili_core::net::USER_AGENT);
    if let Some(referer) = std::env::args().nth(2) {
        request = request.header("Referer", referer);
    }
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    std::fs::write("target/probe-last.txt", &body)?;
    println!("{status} {} bytes\n{}", body.len(), body.chars().take(300).collect::<String>());
    Ok(())
}
