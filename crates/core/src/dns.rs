//! DNS over HTTPS (Android's `AppDns.kt` and `AutoDns.kt`; keep the providers in step): who
//! answers "what address is this host at" when the network's own answer can't be trusted.
//!
//! The block this is for is the cheapest one an ISP can apply: leave a source's servers reachable
//! and refuse to hand out their address. From inside the app that looks like every source being
//! down at once. A DoH lookup moves that one question onto HTTPS, and the source loads again.
//!
//! Every client the app builds ([`crate::net::Http`]) resolves through [`resolver`]:
//! - **Automatic** (the default): the network's resolver first; when it has no answer, or answers
//!   with a sinkhole (0.0.0.0, a loopback), a secure resolver is asked, and if that rescues the
//!   name, every later lookup this session goes to the secure resolver first (Android engages per
//!   network; a desktop rarely changes network mid-session).
//! - **Off**: the network's resolver only.
//! - **A provider**: that provider for every lookup, the network's as a fallback.
//!
//! Each provider is reached at hard-coded bootstrap addresses, since otherwise the first thing a
//! DoH lookup needs is a lookup from the resolver being bypassed. mpv resolves names itself, so a
//! stream from a host only DoH could find goes through the app's proxy ([`needs_proxy`]).

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use wreq::dns::{Addrs, Name, Resolve, Resolving};

/// A DoH provider, as Android's `DohProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    /// Android's stored value.
    pub id: &'static str,
    pub url: &'static str,
    pub host: &'static str,
    pub bootstrap: &'static [Ipv4Addr],
}

/// Android's providers, verified there against their own hostnames. Cloudflare's family resolver
/// (1.1.1.3) is left out on purpose: its adult list refuses the catalogue itself.
pub const PROVIDERS: &[Provider] = &[
    Provider { id: "google", url: "https://dns.google/dns-query", host: "dns.google", bootstrap: &[Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)] },
    Provider { id: "cloudflare", url: "https://cloudflare-dns.com/dns-query", host: "cloudflare-dns.com", bootstrap: &[Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)] },
    Provider {
        id: "cloudflare_security",
        url: "https://security.cloudflare-dns.com/dns-query",
        host: "security.cloudflare-dns.com",
        bootstrap: &[Ipv4Addr::new(1, 1, 1, 2), Ipv4Addr::new(1, 0, 0, 2)],
    },
    Provider { id: "adguard", url: "https://dns.adguard-dns.com/dns-query", host: "dns.adguard-dns.com", bootstrap: &[Ipv4Addr::new(94, 140, 14, 14), Ipv4Addr::new(94, 140, 15, 15)] },
    Provider { id: "quad9", url: "https://dns.quad9.net/dns-query", host: "dns.quad9.net", bootstrap: &[Ipv4Addr::new(9, 9, 9, 9), Ipv4Addr::new(149, 112, 112, 112)] },
    Provider { id: "dns_sb", url: "https://doh.dns.sb/dns-query", host: "doh.dns.sb", bootstrap: &[Ipv4Addr::new(185, 222, 222, 222), Ipv4Addr::new(45, 11, 45, 11)] },
    Provider {
        id: "canadian_shield",
        url: "https://private.canadianshield.cira.ca/dns-query",
        host: "private.canadianshield.cira.ca",
        bootstrap: &[Ipv4Addr::new(149, 112, 121, 10), Ipv4Addr::new(149, 112, 122, 10)],
    },
];

/// The secure resolvers Automatic tries, in order.
const AUTO_PROVIDERS: &[&str] = &["cloudflare", "google", "quad9"];
/// Answers are kept this long at most (and no longer than their TTL).
const MAX_TTL: Duration = Duration::from_secs(300);
/// A name nothing could resolve is not asked again for this long.
const DEAD_FOR: Duration = Duration::from_secs(60);
const DOH_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Off,
    Provider(&'static Provider),
}

impl Mode {
    /// Android's stored values: `auto`, `none`, or a provider's id; anything else is Automatic.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "none" => Mode::Off,
            id => PROVIDERS.iter().find(|p| p.id == id).map_or(Mode::Auto, Mode::Provider),
        }
    }
}

pub struct Resolver {
    mode: RwLock<Mode>,
    /// Automatic found the network blocking a name: secure first for the rest of the session.
    engaged: AtomicBool,
    cache: Mutex<HashMap<String, (Vec<IpAddr>, Instant)>>,
    dead: Mutex<HashMap<String, Instant>>,
    /// Names the network's resolver wouldn't give (or sinkholed) but a secure one did: what mpv,
    /// which resolves on its own, must reach through the proxy.
    blocked: Mutex<HashSet<String>>,
    clients: Mutex<HashMap<&'static str, wreq::Client>>,
}

/// The one resolver every client shares.
pub fn resolver() -> &'static Arc<Resolver> {
    static RESOLVER: OnceLock<Arc<Resolver>> = OnceLock::new();
    RESOLVER.get_or_init(|| {
        Arc::new(Resolver {
            mode: RwLock::new(Mode::Auto),
            engaged: AtomicBool::new(false),
            cache: Mutex::default(),
            dead: Mutex::default(),
            blocked: Mutex::default(),
            clients: Mutex::default(),
        })
    })
}

/// Settings → Servers → DNS over HTTPS changed: takes effect from the next lookup.
pub fn set_mode(mode: Mode) {
    let resolver = resolver();
    let changed = *resolver.mode.read().unwrap() != mode;
    *resolver.mode.write().unwrap() = mode;
    if changed {
        resolver.cache.lock().unwrap().clear();
        resolver.dead.lock().unwrap().clear();
        resolver.engaged.store(false, Ordering::Relaxed);
    }
}

/// Whether Automatic has switched to a secure resolver this session.
pub fn engaged() -> bool {
    resolver().engaged.load(Ordering::Relaxed)
}

/// Whether mpv must reach `host` through the proxy: the network's own resolver can't give it (it
/// failed, or sinkholed), and DNS handling is on. Asks the network's resolver when it hasn't yet.
pub async fn needs_proxy(host: &str) -> bool {
    let resolver = resolver();
    if *resolver.mode.read().unwrap() == Mode::Off || is_private(host) {
        return false;
    }
    if resolver.blocked.lock().unwrap().contains(host) {
        return true;
    }
    let blocked = match system(host).await {
        Some(addrs) => addrs.iter().all(is_sinkhole),
        None => true,
    };
    if blocked {
        resolver.blocked.lock().unwrap().insert(host.to_owned());
    }
    blocked
}

/// The resolver as wreq takes it.
#[derive(Clone)]
pub struct Shared(pub Arc<Resolver>);

impl Resolve for Shared {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.0.clone();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let ips = resolver.lookup(&host).await.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            let addrs: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

impl Resolver {
    async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        let mode = *self.mode.read().unwrap();
        if mode == Mode::Off || is_private(host) {
            return system(host).await.ok_or_else(|| format!("{host}: no address"));
        }
        if let Some((ips, until)) = self.cache.lock().unwrap().get(host)
            && Instant::now() < *until
        {
            return Ok(ips.clone());
        }
        if self.dead.lock().unwrap().get(host).is_some_and(|until| Instant::now() < *until) {
            return Err(format!("{host}: unresolvable (remembered)"));
        }

        let secure_first = match mode {
            Mode::Provider(_) => true,
            _ => self.engaged.load(Ordering::Relaxed),
        };
        if secure_first {
            if let Some((ips, ttl)) = self.secure(host, mode).await {
                self.remember(host, &ips, ttl);
                return Ok(ips);
            }
            return system(host).await.ok_or_else(|| format!("{host}: no address"));
        }

        // Automatic, not engaged: the network's answer unless it is missing or a sinkhole.
        let local = system(host).await;
        if let Some(ips) = &local
            && !ips.iter().all(is_sinkhole)
        {
            return Ok(ips.clone());
        }
        if let Some((ips, ttl)) = self.secure(host, mode).await
            && !ips.iter().all(is_sinkhole)
        {
            let reason = if local.is_some() { "sinkholed" } else { "no answer" };
            tracing::info!(%host, reason, "the network's DNS blocks a host; secure DNS from now on");
            self.engaged.store(true, Ordering::Relaxed);
            self.blocked.lock().unwrap().insert(host.to_owned());
            self.remember(host, &ips, ttl);
            return Ok(ips);
        }
        if let Some(ips) = local {
            return Ok(ips);
        }
        self.dead.lock().unwrap().insert(host.to_owned(), Instant::now() + DEAD_FOR);
        Err(format!("{host}: no address"))
    }

    fn remember(&self, host: &str, ips: &[IpAddr], ttl: Duration) {
        self.cache.lock().unwrap().insert(host.to_owned(), (ips.to_vec(), Instant::now() + ttl.min(MAX_TTL)));
    }

    /// The chosen provider, or Automatic's in turn: the first answer with addresses.
    async fn secure(&self, host: &str, mode: Mode) -> Option<(Vec<IpAddr>, Duration)> {
        let providers: Vec<&'static Provider> = match mode {
            Mode::Provider(p) => vec![p],
            _ => AUTO_PROVIDERS.iter().filter_map(|id| PROVIDERS.iter().find(|p| p.id == *id)).collect(),
        };
        for provider in providers {
            match self.ask(provider, host).await {
                Ok(Some(answer)) => return Some(answer),
                Ok(None) => return None, // The provider says the name doesn't exist: believe it.
                Err(e) => tracing::debug!(provider = provider.id, %host, error = %e, "DoH lookup failed"),
            }
        }
        None
    }

    /// One provider's A and AAAA answers for `host`; `None` when it has none.
    async fn ask(&self, provider: &'static Provider, host: &str) -> Result<Option<(Vec<IpAddr>, Duration)>, String> {
        let client = self.client(provider)?;
        let (v4, v6) = tokio::join!(query(&client, provider, host, TYPE_A), query(&client, provider, host, TYPE_AAAA));
        let (mut ips, mut ttl) = (Vec::new(), MAX_TTL);
        let mut answered = false;
        // A failed half (an IPv6 query some providers refuse) still leaves the other's answer.
        for (found, found_ttl) in [v4, v6].into_iter().flatten() {
            answered = true;
            if !found.is_empty() {
                ttl = ttl.min(found_ttl);
            }
            ips.extend(found);
        }
        if !answered {
            return Err("no answer from the provider".into());
        }
        Ok((!ips.is_empty()).then_some((ips, ttl)))
    }

    /// A client for one provider that reaches it at its bootstrap addresses, never through DNS.
    fn client(&self, provider: &'static Provider) -> Result<wreq::Client, String> {
        let mut clients = self.clients.lock().unwrap();
        if let Some(client) = clients.get(provider.id) {
            return Ok(client.clone());
        }
        let addrs: Vec<SocketAddr> = provider.bootstrap.iter().map(|ip| SocketAddr::new(IpAddr::V4(*ip), 443)).collect();
        let client = wreq::Client::builder()
            .resolve_to_addrs(provider.host, addrs)
            .connect_timeout(Duration::from_secs(4))
            .timeout(DOH_TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        clients.insert(provider.id, client.clone());
        Ok(client)
    }
}

/// The network's resolver (the system's), as every client used before this module.
async fn system(host: &str) -> Option<Vec<IpAddr>> {
    let found: Vec<IpAddr> = tokio::net::lookup_host((host, 0)).await.ok()?.map(|a| a.ip()).collect();
    (!found.is_empty()).then_some(found)
}

/// Names only the local network can answer.
fn is_private(host: &str) -> bool {
    host == "localhost" || host.ends_with(".local") || host.ends_with(".lan") || host.ends_with(".internal") || !host.contains('.')
}

/// An address a blocking resolver hands out instead of the real one.
fn is_sinkhole(ip: &IpAddr) -> bool {
    ip.is_unspecified() || ip.is_loopback()
}

const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;

/// One RFC 8484 query: the DNS message POSTed as `application/dns-message`.
async fn query(client: &wreq::Client, provider: &Provider, host: &str, kind: u16) -> Result<(Vec<IpAddr>, Duration), String> {
    let response = client
        .post(provider.url)
        .header("Content-Type", "application/dns-message")
        .header("Accept", "application/dns-message")
        .body(encode_query(host, kind)?)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    decode_answer(&body, kind)
}

/// A DNS query message: id 0 (as RFC 8484 asks, for caching), recursion desired, one question.
fn encode_query(host: &str, kind: u16) -> Result<Vec<u8>, String> {
    let mut message = vec![0, 0, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("{host}: not a host name"));
        }
        message.push(label.len() as u8);
        message.extend_from_slice(label.as_bytes());
    }
    message.push(0);
    message.extend_from_slice(&kind.to_be_bytes());
    message.extend_from_slice(&1u16.to_be_bytes()); // class IN
    Ok(message)
}

/// The addresses of type `kind` in an answer message, and the smallest TTL among them.
fn decode_answer(message: &[u8], kind: u16) -> Result<(Vec<IpAddr>, Duration), String> {
    let short = || "a short DNS answer".to_owned();
    if message.len() < 12 {
        return Err(short());
    }
    let rcode = message[3] & 0x0f;
    // NXDOMAIN: the name doesn't exist, which is an answer, not a failure.
    if rcode == 3 {
        return Ok((vec![], MAX_TTL));
    }
    if rcode != 0 {
        return Err(format!("DNS error {rcode}"));
    }
    let count = |at: usize| u16::from_be_bytes([message[at], message[at + 1]]) as usize;
    let (questions, answers) = (count(4), count(6));
    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(message, at).ok_or_else(short)? + 4;
    }
    let (mut ips, mut ttl) = (Vec::new(), MAX_TTL);
    for _ in 0..answers {
        at = skip_name(message, at).ok_or_else(short)?;
        let header = message.get(at..at + 10).ok_or_else(short)?;
        let record = u16::from_be_bytes([header[0], header[1]]);
        let record_ttl = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        let length = u16::from_be_bytes([header[8], header[9]]) as usize;
        let data = message.get(at + 10..at + 10 + length).ok_or_else(short)?;
        match (record, kind) {
            (TYPE_A, TYPE_A) if length == 4 => ips.push(IpAddr::from(<[u8; 4]>::try_from(data).unwrap())),
            (TYPE_AAAA, TYPE_AAAA) if length == 16 => ips.push(IpAddr::from(<[u8; 16]>::try_from(data).unwrap())),
            _ => {} // CNAMEs on the way: the resolver already followed them.
        }
        if record == kind {
            ttl = ttl.min(Duration::from_secs(u64::from(record_ttl)));
        }
        at += 10 + length;
    }
    Ok((ips, ttl))
}

/// The offset just past a (possibly compressed) name.
fn skip_name(message: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let length = *message.get(at)? as usize;
        if length == 0 {
            return Some(at + 1);
        }
        if length & 0xc0 == 0xc0 {
            return Some(at + 2);
        }
        at += 1 + length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_query() {
        let q = encode_query("www.miruro.bz", TYPE_A).unwrap();
        assert_eq!(&q[..12], &[0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..], b"\x03www\x06miruro\x02bz\x00\x00\x01\x00\x01");
        assert!(encode_query("a..b", TYPE_A).is_err());
    }

    #[test]
    fn reads_an_answer_through_a_cname() {
        // Question www.example.com A; answers: a CNAME (to a name by pointer), then an A record.
        let mut m = vec![0, 0, 0x81, 0x80, 0, 1, 0, 2, 0, 0, 0, 0];
        m.extend_from_slice(b"\x03www\x07example\x03com\x00\x00\x01\x00\x01");
        m.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xc0, 16]);
        m.extend_from_slice(&[0xc0, 16, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 93, 184, 216, 34]);
        let (ips, ttl) = decode_answer(&m, TYPE_A).unwrap();
        assert_eq!(ips, [IpAddr::from([93, 184, 216, 34])]);
        assert_eq!(ttl, Duration::from_secs(30));
    }

    #[test]
    fn nxdomain_is_an_empty_answer_and_servfail_an_error() {
        let mut m = vec![0, 0, 0x81, 0x83, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(decode_answer(&m, TYPE_A).unwrap().0, Vec::<IpAddr>::new());
        m[3] = 0x82;
        assert!(decode_answer(&m, TYPE_A).is_err());
    }

    #[test]
    fn stored_values_and_sinkholes() {
        assert_eq!(Mode::from_stored("none"), Mode::Off);
        assert_eq!(Mode::from_stored("auto"), Mode::Auto);
        assert_eq!(Mode::from_stored("something new"), Mode::Auto);
        assert!(matches!(Mode::from_stored("quad9"), Mode::Provider(p) if p.host == "dns.quad9.net"));
        assert!(is_sinkhole(&IpAddr::from([0, 0, 0, 0])));
        assert!(is_sinkhole(&IpAddr::from([127, 0, 0, 1])));
        assert!(!is_sinkhole(&IpAddr::from([104, 21, 1, 1])));
        assert!(is_private("localhost") && is_private("router") && !is_private("miruro.bz"));
    }

    /// Live: every provider answers for a stable name. `cargo test -p anilili-core -- --ignored doh`
    #[tokio::test]
    #[ignore]
    async fn doh_every_provider_answers() {
        let resolver = resolver();
        for provider in PROVIDERS {
            let answer = resolver.ask(provider, "example.com").await;
            assert!(matches!(answer, Ok(Some((ref ips, _))) if !ips.is_empty()), "{}: {answer:?}", provider.id);
        }
    }
}
