//! TLS connections, login, and a small pool of authenticated sessions.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use pigeon_core::{Error, Result};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use rustls_platform_verifier::ConfigVerifierExt;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::ServerConfig;

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

fn tls() -> Result<TlsConnector> {
    static CFG: OnceLock<std::result::Result<Arc<ClientConfig>, String>> = OnceLock::new();
    let cfg = CFG.get_or_init(|| ClientConfig::with_platform_verifier().map(Arc::new).map_err(|e| e.to_string()));
    cfg.clone().map(TlsConnector::from).map_err(|e| Error::Other(format!("TLS setup: {e}")))
}

pub async fn tls_connect(host: &str, port: u16) -> Result<TlsStream<TcpStream>> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| Error::Other(format!("can't reach {host}:{port} (timed out)")))??;
    upgrade(tcp, host).await
}

pub async fn upgrade(tcp: TcpStream, host: &str) -> Result<TlsStream<TcpStream>> {
    let name = ServerName::try_from(host.to_string()).map_err(|e| Error::Other(format!("bad host {host}: {e}")))?;
    tokio::time::timeout(CONNECT_TIMEOUT, tls()?.connect(name, tcp))
        .await
        .map_err(|_| Error::Other(format!("TLS handshake with {host} timed out")))?
        .map_err(Error::Io)
}

/// Is this IMAP error the server rejecting our credentials?
fn is_auth_failure(e: &async_imap::error::Error) -> bool {
    let s = e.to_string().to_ascii_uppercase();
    matches!(e, async_imap::error::Error::No(_)) || s.contains("AUTHENTICATIONFAILED") || s.contains("AUTHENTICATE")
}

/// Open and log in. iCloud usually wants the name part of the address,
/// sometimes the full address; try both before giving up.
pub async fn login(cfg: &ServerConfig, email: &str, password: &str) -> Result<Session> {
    let mut last_auth_error = None;
    for user in cfg.usernames(email) {
        let tls = tls_connect(cfg.imap_host, cfg.imap_port).await?;
        let mut client = async_imap::Client::new(tls);
        client
            .read_response()
            .await
            .map_err(Error::Io)?
            .ok_or_else(|| Error::Other(format!("{} closed the connection", cfg.imap_host)))?;
        match client.login(&user, password).await {
            Ok(session) => return Ok(session),
            Err((e, _)) if is_auth_failure(&e) => last_auth_error = Some(e.to_string()),
            Err((e, _)) => return Err(Error::Other(format!("IMAP login: {e}"))),
        }
    }
    Err(Error::Auth(format!(
        "{} rejected the password. Use an app-specific password (not your Apple Account password). ({})",
        cfg.label,
        last_auth_error.unwrap_or_default()
    )))
}

/// Up to `max` concurrent sessions, reused across calls.
pub struct Pool {
    cfg: ServerConfig,
    email: String,
    password: String,
    idle: Mutex<Vec<Session>>,
    slots: Arc<Semaphore>,
}

/// A borrowed session; returned to the pool on drop unless `discard`ed.
pub struct Lease<'a> {
    pool: &'a Pool,
    session: Option<Session>,
    _permit: OwnedSemaphorePermit,
}

impl Pool {
    pub fn new(cfg: ServerConfig, email: String, password: String, max: usize) -> Self {
        Pool { cfg, email, password, idle: Mutex::new(Vec::new()), slots: Arc::new(Semaphore::new(max.max(1))) }
    }

    pub async fn get(&self) -> Result<Lease<'_>> {
        let permit = self.slots.clone().acquire_owned().await.map_err(|e| Error::Other(e.to_string()))?;
        let cached = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop();
        let session = match cached {
            Some(mut s) => match s.noop().await {
                Ok(()) => s,
                Err(_) => login(&self.cfg, &self.email, &self.password).await?,
            },
            None => login(&self.cfg, &self.email, &self.password).await?,
        };
        Ok(Lease { pool: self, session: Some(session), _permit: permit })
    }
}

impl Lease<'_> {
    pub fn session(&mut self) -> &mut Session {
        self.session.as_mut().expect("session present until drop")
    }

    /// Drop the connection instead of reusing it (after an error).
    pub fn discard(mut self) {
        self.session = None;
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Some(s) = self.session.take() {
            self.pool.idle.lock().unwrap_or_else(|p| p.into_inner()).push(s);
        }
    }
}

/// Map an IMAP library error to ours.
pub fn imap_err(e: async_imap::error::Error) -> Error {
    match e {
        async_imap::error::Error::Io(io) => Error::Io(io),
        other => Error::Other(format!("IMAP: {other}")),
    }
}
