use std::fmt;

/// Every failure the core reports. [`Error::failure`] maps it onto the catalogue
/// ([`crate::catalogue`]), which is what a viewer sees.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("{service} answered HTTP {status}: {detail}")]
    Http { service: &'static str, status: u16, detail: String },
    #[error("{service} is rate limiting us; retry in {retry_after}s")]
    RateLimited { service: &'static str, retry_after: u64 },
    #[error("unexpected response from {service}: {detail}")]
    Decode { service: &'static str, detail: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// The catalogue entry a viewer sees for this failure, and the lower layer's own code if any
    /// (`h503`). Callers with more context (which page failed) may pick a more specific entry.
    pub fn failure(&self) -> Failure {
        use crate::catalogue as c;
        let (entry, suffix) = match self {
            Error::Network(detail) => {
                let lower = detail.to_lowercase();
                if lower.contains("timed out") || lower.contains("timeout") {
                    (c::NET_TIMEOUT, None)
                } else if lower.contains("certificate") {
                    (c::NET_CERTIFICATE, None)
                } else {
                    (c::NET_OFFLINE, None)
                }
            }
            Error::Http { status: 429, .. } | Error::RateLimited { .. } => (c::NET_RATE_LIMITED, Some("h429".to_owned())),
            Error::Http { status, detail, .. } if matches!(status, 403 | 503) && detail.contains("Just a moment") => {
                (c::NET_CHALLENGE, Some(format!("h{status}")))
            }
            Error::Http { status, .. } => (c::NET_SERVER, Some(format!("h{status}"))),
            Error::Decode { .. } => (c::API_UNREADABLE, None),
            Error::NotFound(_) => (c::API_NOT_FOUND, None),
            Error::Storage(_) => (c::DB_FAILED, None),
            Error::Other(_) => (c::APP_GENERIC, None),
        };
        Failure { entry, suffix, detail: self.to_string() }
    }

    pub fn decode(service: &'static str, detail: impl fmt::Display) -> Self {
        Error::Decode { service, detail: detail.to_string() }
    }
}

/// A failure as reported: the catalogue entry, the lower layer's code, and the technical detail
/// (shown behind **Details**, never as the message).
#[derive(Debug, Clone)]
pub struct Failure {
    pub entry: &'static crate::catalogue::Entry,
    pub suffix: Option<String>,
    pub detail: String,
}

impl Failure {
    pub fn new(entry: &'static crate::catalogue::Entry, detail: impl Into<String>) -> Self {
        Self { entry, suffix: None, detail: detail.into() }
    }

    /// The full code, `DNET-03/h503`.
    pub fn code(&self) -> String {
        match &self.suffix {
            Some(suffix) => format!("{}/{suffix}", self.entry.code()),
            None => self.entry.code(),
        }
    }

    /// Records the failure in the log under its code, the name the diagnostics file uses.
    pub fn log(&self) {
        tracing::warn!(code = %self.code(), detail = %self.detail, "{}", self.entry.message);
    }
}

impl From<wreq::Error> for Error {
    fn from(e: wreq::Error) -> Self {
        Error::Network(e.to_string())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Other(format!("json: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
