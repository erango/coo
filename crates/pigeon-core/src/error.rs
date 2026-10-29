use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{status} from {url}: {body}")]
    Api { status: u16, url: String, body: String },
    /// The provider no longer accepts our sync cursor; the scope must resync from scratch.
    #[error("sync cursor expired")]
    CursorExpired,
    #[error("auth: {0}")]
    Auth(String),
    /// No usable refresh token: the user has to sign in again.
    #[error("sign-in required for {0}")]
    SignInRequired(String),
    #[error("token store: {0}")]
    TokenStore(String),
    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Which kind of failure, in words that survive Sentry's scrubbing
    /// (the message itself often reads as sensitive and arrives filtered).
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Http(e) if e.is_timeout() => "http-timeout",
            Error::Http(e) if e.is_connect() => "http-connect",
            Error::Http(e) if e.is_decode() => "http-decode",
            Error::Http(_) => "http",
            Error::Api { .. } => "api",
            Error::CursorExpired => "cursor-expired",
            Error::Auth(_) => "auth",
            Error::SignInRequired(_) => "sign-in-required",
            Error::TokenStore(_) => "token-store",
            Error::Db(_) => "db",
            Error::Io(_) => "io",
            Error::Json(_) => "json",
            Error::Other(_) => "other",
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            Error::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
}

impl Error {
    /// The network is unreachable (offline, DNS, connect/timeout), as
    /// opposed to the server answering with an error.
    pub fn is_network(&self) -> bool {
        use std::io::ErrorKind::*;
        match self {
            Error::Http(e) => e.is_connect() || e.is_timeout() || (e.is_request() && e.status().is_none()),
            Error::Io(e) => matches!(
                e.kind(),
                ConnectionRefused
                    | ConnectionReset
                    | ConnectionAborted
                    | NotConnected
                    | TimedOut
                    | HostUnreachable
                    | NetworkUnreachable
                    | NetworkDown
            ),
            Error::Other(m) => network_text(m),
            _ => false,
        }
    }
}

fn network_text(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    [
        "dns error",
        "failed to lookup address",
        "network is unreachable",
        "no route to host",
        "connection refused",
        "timed out",
    ]
    .iter()
    .any(|k| m.contains(k))
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_classification() {
        assert!(Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "x")).is_network());
        assert!(Error::Other("imap: dns error: failed to lookup address information".into()).is_network());
        assert!(!Error::Api { status: 500, url: String::new(), body: String::new() }.is_network());
        assert!(!Error::SignInRequired("a".into()).is_network());
    }
}
