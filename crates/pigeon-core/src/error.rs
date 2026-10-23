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
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            Error::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
