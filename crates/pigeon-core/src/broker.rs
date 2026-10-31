//! Sign-in broker: the Coo app shares short-lived access tokens with its own
//! command-line helper (`coo`, run by AI agents as an MCP server) over a Unix
//! socket, so only the app ever reads the Keychain.
//!
//! Protocol: one JSON object per line, one request per connection, then the
//! server answers with one JSON line and closes.
//!
//! Security model:
//! - The socket lives in the per-user data dir with mode 0600, so only this
//!   user can connect.
//! - The server checks the peer's uid, and when the app itself is signed by
//!   the Developer ID team, the peer's code signature too (via its audit
//!   token, which can't be spoofed by pid reuse): only Coo's own signed
//!   binaries may ask.
//! - OAuth refresh tokens never leave the app; peers get access tokens that
//!   expire within the hour. iCloud's app-specific password is the one secret
//!   that has to be shared, because IMAP/SMTP/CalDAV log in with it directly.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::paths::APP_ID;

pub const SOCKET_NAME: &str = "agent.sock";
/// What agents and people see when the app isn't there to share sign-ins.
pub const APP_NOT_RUNNING: &str =
    "Coo needs to be running to share your sign-ins with the agent. Open Coo and try again.";

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const LAUNCH_WAIT: Duration = Duration::from_secs(8);
const MAX_LINE: usize = 16 * 1024;

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SOCKET_NAME)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping,
    /// A valid OAuth access token for the account (Microsoft, Google).
    AccessToken {
        account: String,
    },
    /// The stored password (iCloud only).
    Secret {
        account: String,
    },
    /// Whether the app holds a sign-in for the account.
    Has {
        account: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    SignInRequired,
    UnknownAccount,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Seconds the token stays valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn token(token: String, expires_in: Duration) -> Self {
        Response { ok: true, token: Some(token), expires_in: Some(expires_in.as_secs()), ..Default::default() }
    }
    pub fn has(has: bool) -> Self {
        Response { ok: true, has: Some(has), ..Default::default() }
    }
    pub fn pong() -> Self {
        Response { ok: true, version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() }
    }
    pub fn fail(failure: Failure, error: impl Into<String>) -> Self {
        Response { ok: false, failure: Some(failure), error: Some(error.into()), ..Default::default() }
    }

    /// Map a failed answer to the engine's error for `account`.
    pub fn into_error(self, account: &str) -> Error {
        let msg = self.error.unwrap_or_else(|| "the Coo app refused the request".into());
        match self.failure {
            Some(Failure::SignInRequired) => Error::SignInRequired(account.to_string()),
            Some(Failure::UnknownAccount) => Error::Other(format!("{account} isn't signed in to Coo")),
            Some(Failure::Denied) => Error::TokenStore(msg),
            _ => Error::Other(msg),
        }
    }
}

/// The app's answer to an error while serving a request.
pub fn failure_for(e: &Error) -> Response {
    match e {
        Error::SignInRequired(a) => {
            Response::fail(Failure::SignInRequired, format!("{a} needs to sign in again in Coo"))
        }
        e => Response::fail(Failure::Failed, e.to_string()),
    }
}

/// What an agent hears when its helper isn't accepted. Almost always the
/// helper started before Coo updated: the running copy no longer matches
/// the signed one on disk, and only a fresh start fixes that.
pub const REFUSED: &str = "Coo didn't accept this connection, usually because Coo updated after the agent started it. \
     Tell the user to restart the agent's connection to Coo: in Claude Code, run /mcp and reconnect \"coo\"; \
     in Claude Desktop, quit and reopen Claude. If it still fails, set it up again from Coo's menu: \
     Help › Connect an AI Agent.";

/// The broker couldn't be reached at all (as opposed to answering "no").
#[derive(Debug)]
pub struct Unavailable(pub std::io::Error);

/// Client side, used by the `coo` helper.
#[derive(Debug, Clone)]
pub struct Client {
    pub socket: PathBuf,
    /// Launch the app (hidden) when it isn't running.
    pub launch: bool,
}

impl Client {
    pub fn new(data_dir: &Path, launch: bool) -> Self {
        Client { socket: socket_path(data_dir), launch }
    }

    /// One round trip, launching Coo once if nobody is listening.
    pub fn call(&self, req: &Request) -> std::result::Result<Response, Unavailable> {
        match call(&self.socket, req, IO_TIMEOUT) {
            Err(e) if self.launch && not_listening(&e) => {
                tracing::info!("Coo isn't running; launching it hidden to share sign-ins");
                launch_app();
                let deadline = Instant::now() + LAUNCH_WAIT;
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    match call(&self.socket, req, IO_TIMEOUT) {
                        Err(e) if not_listening(&e) && Instant::now() < deadline => continue,
                        r => return r.map_err(Unavailable),
                    }
                }
            }
            r => r.map_err(Unavailable),
        }
    }
}

fn not_listening(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(e.kind(), NotFound | ConnectionRefused)
}

/// Open the installed Coo in the background. Only for the standard data
/// folder: an installed app can't serve a custom `PIGEON_DATA_DIR`.
fn launch_app() {
    if std::env::var_os("PIGEON_DATA_DIR").is_some() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/open").args(["-g", "-j", "-b", APP_ID]).status();
    }
}

/// One blocking request/response.
pub fn call(socket: &Path, req: &Request, timeout: Duration) -> std::io::Result<Response> {
    #[cfg(unix)]
    {
        let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut line = serde_json::to_vec(req)?;
        line.push(b'\n');
        stream.write_all(&line)?;
        let mut reader = BufReader::new(stream).take(MAX_LINE as u64);
        let mut buf = String::new();
        reader.read_line(&mut buf)?;
        if buf.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "the Coo app closed the connection",
            ));
        }
        Ok(serde_json::from_str(&buf)?)
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, req, timeout);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no sign-in broker on this platform"))
    }
}

/// Server side: what the app answers. Implemented by the engine.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: Request) -> futures::future::BoxFuture<'_, Response>;
    /// A program that isn't (or no longer checks out as) Coo's own asked.
    fn refused(&self) {}
}

/// Listen until the process exits. Removes a stale socket first.
#[cfg(unix)]
pub async fn serve(socket: PathBuf, handler: std::sync::Arc<dyn Handler>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
        return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another Coo is already sharing sign-ins"));
    }
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    tracing::info!("sign-in broker listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_one(stream, &*handler).await {
                tracing::debug!(error = %e, "broker connection ended");
            }
        });
    }
}

#[cfg(unix)]
async fn serve_one(stream: tokio::net::UnixStream, handler: &dyn Handler) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    let allowed = peer::check(&stream);
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(read.take(MAX_LINE as u64));
    tokio::time::timeout(IO_TIMEOUT, reader.read_line(&mut line))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "request timed out"))??;
    let resp = match (allowed, serde_json::from_str::<Request>(&line)) {
        (Err(why), _) => {
            tracing::warn!(reason = %why, "refused a sign-in request from an untrusted program");
            handler.refused();
            Response::fail(Failure::Denied, REFUSED)
        }
        (Ok(()), Err(e)) => Response::fail(Failure::Failed, format!("bad request: {e}")),
        (Ok(()), Ok(req)) => handler.handle(req).await,
    };
    let mut out = serde_json::to_vec(&resp)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.shutdown().await
}

/// Who may connect.
#[cfg(unix)]
mod peer {
    /// Requirement for Coo's own signed programs: the Developer ID team plus
    /// one of the identifiers we sign with (`coo` is codesign's default for
    /// the helper in builds before it got an explicit identifier).
    #[cfg(target_os = "macos")]
    const TEAM_REQUIREMENT: &str = r#"anchor apple generic and certificate leaf[subject.OU] = "GWZUYS277A""#;
    #[cfg(target_os = "macos")]
    const PEER_REQUIREMENT: &str = r#"anchor apple generic and certificate leaf[subject.OU] = "GWZUYS277A" and (identifier "il.co.frost-media.pigeon" or identifier "il.co.frost-media.pigeon.coo" or identifier "coo")"#;

    pub fn check(stream: &tokio::net::UnixStream) -> Result<(), String> {
        let cred = stream.peer_cred().map_err(|e| format!("no peer credentials: {e}"))?;
        // SAFETY: geteuid has no preconditions.
        if cred.uid() != unsafe { libc::geteuid() } {
            return Err("peer is another user".into());
        }
        #[cfg(target_os = "macos")]
        if self_is_signed() {
            use std::os::fd::AsRawFd;
            return signed_peer(stream.as_raw_fd());
        }
        Ok(())
    }

    /// Release builds are Developer ID signed; dev builds (ad-hoc or none)
    /// accept any same-user peer, because nothing about them is signed.
    #[cfg(target_os = "macos")]
    fn self_is_signed() -> bool {
        use std::str::FromStr;

        use security_framework::os::macos::code_signing::{Flags, SecCode, SecRequirement};
        static SIGNED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *SIGNED.get_or_init(|| {
            let Ok(req) = SecRequirement::from_str(TEAM_REQUIREMENT) else { return false };
            SecCode::for_self(Flags::NONE).and_then(|c| c.check_validity(Flags::NONE, &req)).is_ok()
        })
    }

    #[cfg(target_os = "macos")]
    fn signed_peer(fd: std::os::fd::RawFd) -> Result<(), String> {
        use std::str::FromStr;

        use core_foundation::base::TCFType;
        use core_foundation::data::CFData;
        use security_framework::os::macos::code_signing::{Flags, GuestAttributes, SecCode, SecRequirement};

        // <sys/un.h>: SOL_LOCAL / LOCAL_PEERTOKEN, an audit_token_t (8 x u32).
        const SOL_LOCAL: libc::c_int = 0;
        const LOCAL_PEERTOKEN: libc::c_int = 0x006;
        let mut token = [0u8; 32];
        let mut len = token.len() as libc::socklen_t;
        // SAFETY: valid fd, buffer and length for this option.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                SOL_LOCAL,
                LOCAL_PEERTOKEN,
                token.as_mut_ptr().cast(),
                &mut len as *mut libc::socklen_t,
            )
        };
        if rc != 0 || len as usize != token.len() {
            return Err(format!("no audit token: {}", std::io::Error::last_os_error()));
        }
        let data = CFData::from_buffer(&token);
        let mut attrs = GuestAttributes::new();
        attrs.set_audit_token(data.as_concrete_TypeRef());
        let code = SecCode::copy_guest_with_attribues(None, &attrs, Flags::NONE)
            .map_err(|e| format!("can't identify the peer: {e}"))?;
        let req = SecRequirement::from_str(PEER_REQUIREMENT).map_err(|e| e.to_string())?;
        code.check_validity(Flags::NONE, &req).map_err(|e| format!("peer isn't Coo: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Echo;
    impl Handler for Echo {
        fn handle(&self, req: Request) -> futures::future::BoxFuture<'_, Response> {
            Box::pin(async move {
                match req {
                    Request::Ping => Response::pong(),
                    Request::AccessToken { account } if account == "google:a@x.com" => {
                        Response::token("at-1".into(), Duration::from_secs(3000))
                    }
                    Request::AccessToken { account } => Response::fail(Failure::UnknownAccount, account),
                    Request::Has { .. } => Response::has(true),
                    Request::Secret { .. } => Response::fail(Failure::Denied, "OAuth accounts share tokens only"),
                }
            })
        }
    }

    #[test]
    fn wire_format_is_tagged_json_lines() {
        let r = Request::AccessToken { account: "google:a@x.com".into() };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"op":"access_token","account":"google:a@x.com"}"#);
        let back: Response = serde_json::from_str(r#"{"ok":false,"failure":"sign_in_required","error":"x"}"#).unwrap();
        assert!(matches!(back.into_error("google:a@x.com"), Error::SignInRequired(a) if a == "google:a@x.com"));
        assert_eq!(serde_json::to_string(&Response::has(false)).unwrap(), r#"{"ok":true,"has":false}"#);
    }

    #[tokio::test]
    async fn round_trip_and_unavailable() {
        let dir = std::env::temp_dir().join(format!("coo-broker-{}", crate::auth::random_token(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = socket_path(&dir);

        // Nobody listening: reported as unavailable, not as a refusal.
        let client = Client::new(&dir, false);
        assert!(client.call(&Request::Ping).is_err());

        tokio::spawn(serve(sock.clone(), Arc::new(Echo)));
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);

        let c = client.clone();
        let resp = tokio::task::spawn_blocking(move || {
            (
                c.call(&Request::Ping).unwrap(),
                c.call(&Request::AccessToken { account: "google:a@x.com".into() }).unwrap(),
                c.call(&Request::AccessToken { account: "nope".into() }).unwrap(),
            )
        })
        .await
        .unwrap();
        assert!(resp.0.ok && resp.0.version.is_some());
        assert_eq!(resp.1.token.as_deref(), Some("at-1"));
        assert_eq!(resp.1.expires_in, Some(3000));
        assert_eq!(resp.2.failure, Some(Failure::UnknownAccount));

        // A second server must not steal the socket from a live one.
        assert!(serve(sock.clone(), Arc::new(Echo)).await.is_err());

        // Garbage in: a clean error line, not a hang.
        let raw = tokio::task::spawn_blocking(move || {
            let mut s = std::os::unix::net::UnixStream::connect(&sock).unwrap();
            s.write_all(b"not json\n").unwrap();
            let mut out = String::new();
            BufReader::new(s).read_line(&mut out).unwrap();
            out
        })
        .await
        .unwrap();
        assert!(raw.contains("bad request"), "{raw}");
        std::fs::remove_dir_all(dir).ok();
    }
}
