//! The one HTTP client everything shares.
//!
//! It is `wreq` emulating Chrome: BoringSSL with Chrome's exact ClientHello and HTTP/2 SETTINGS
//! order, so TLS-fingerprinting CDNs and Cloudflare see a browser rather than a library. The user
//! agent therefore has to claim the same Chrome; mixing a Chrome fingerprint with another UA is
//! itself a tell.

use std::time::Duration;

use wreq::IntoEmulation;
use wreq_util::Emulation;

use crate::{Error, Result};

/// Matches the [`Emulation`] profile below.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";

/// The ML-DSA signature schemes (0x0904-0x0906) Chromium 154 lists first in its ClientHello.
const WEBVIEW_MLDSA_SIGALGS: &str = "mldsa44:mldsa65:mldsa87";

#[derive(Clone)]
pub struct Http {
    client: wreq::Client,
}

impl Http {
    pub fn new() -> Result<Self> {
        Self::with_emulation(Emulation::Chrome149.into_emulation())
    }

    /// Microsoft Edge's TLS/HTTP2 signature: what WebView2 looks like on the wire, for replaying
    /// requests a webview earned (Cloudflare clearance).
    pub fn edge() -> Result<Self> {
        let mut emulation = Emulation::Edge148.into_emulation();
        // WebView2 (Chromium 154) offers the ML-DSA signature schemes ahead of the classical ones.
        // The profile predates that; our BoringSSL knows them (patches/boringssl-mldsa-sigalgs.patch).
        if let Some(tls) = emulation.tls_options.as_mut() {
            let classical = tls.sigalgs_list.as_deref().unwrap_or_default();
            tls.sigalgs_list = Some(format!("{WEBVIEW_MLDSA_SIGALGS}:{classical}").into());
        }
        Self::with_emulation(emulation)
    }

    /// No browser emulation: BoringSSL's own ClientHello, like curl or ffmpeg. For the CDNs that
    /// refuse a browser fingerprint arriving without a browser behind it (UniqueStream's
    /// `mediacache.cc` answers both Chrome and Edge profiles 403, and this 200; 2026-10-04).
    pub fn plain() -> Result<Self> {
        Self::build(wreq::Client::builder())
    }

    fn with_emulation(emulation: wreq::Emulation) -> Result<Self> {
        Self::build(wreq::Client::builder().emulation(emulation))
    }

    fn build(builder: wreq::ClientBuilder) -> Result<Self> {
        let client = builder
            // wreq follows no redirects unless told to (unlike browsers and reqwest); CDNs and
            // mirrors redirect constantly.
            .redirect(wreq::redirect::Policy::limited(10))
            // The network's DNS, or a secure one where it blocks a source (Settings → Servers).
            .dns_resolver(crate::dns::Shared(crate::dns::resolver().clone()))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &wreq::Client {
        &self.client
    }

    /// GET returning the body, failing on any non-2xx with the body's first bytes as the detail.
    pub async fn get_bytes(
        &self,
        service: &'static str,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<(bytes::Bytes, Option<String>)> {
        let mut request = self.client.get(url).timeout(timeout);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().await?;
        let status = response.status();
        let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_owned);
        if status.as_u16() == 429 {
            let retry_after =
                response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(60);
            return Err(Error::RateLimited { service, retry_after });
        }
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::Http { service, status: status.as_u16(), detail: problem_detail(&body) });
        }
        Ok((body, content_type))
    }
}

/// The `detail` of an RFC 7807 problem document, else the body's start.
pub(crate) fn problem_detail(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_owned))
        .unwrap_or_else(|| {
            let trimmed: String = text.chars().take(160).collect();
            if trimmed.trim().is_empty() { "no body".into() } else { trimmed }
        })
}
