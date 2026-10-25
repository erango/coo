//! The Google Drive picker, run in the user's browser.
//!
//! Coo serves one page from a one-shot loopback server
//! (`http://127.0.0.1:<random port>/<random nonce>/`). The page loads
//! Google's Picker with the account's access token, the API key and the app
//! id (written into the page itself, never into a URL), and posts the
//! picked files back to `/<nonce>/picked` (or `/<nonce>/cancel`). The page
//! names the account (nickname and address) and stretches the Picker over
//! the window.

use std::time::Duration;

use pigeon_core::auth::random_token;
use pigeon_core::drive::DriveFile;
use pigeon_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PAGE: &str = include_str!("picker.html");
const CONFIG_SLOT: &str = "/*COO_CONFIG*/null";
const PICK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_REQUEST: usize = 1024 * 1024;

/// What the picker page needs.
pub struct PickerConfig {
    pub access_token: String,
    pub api_key: String,
    /// The Cloud project number (see `pigeon_core::drive::app_id`).
    pub app_id: String,
    /// The account the token is for: named in the page header and handed to
    /// the Picker as the account to use.
    pub email: String,
    pub nickname: Option<String>,
}

impl std::fmt::Debug for PickerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerConfig").field("app_id", &self.app_id).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickOutcome {
    Picked(Vec<DriveFile>),
    Cancelled,
}

/// Open the picker in the browser and wait (up to 10 minutes) for the
/// user's pick. `on_url` gets the page address (to show if the browser
/// doesn't open).
pub async fn pick(cfg: &PickerConfig, on_url: impl FnOnce(&str)) -> Result<PickOutcome> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let nonce = random_token(18);
    let url = format!("http://127.0.0.1:{port}/{nonce}/");
    let page = render_page(cfg);
    on_url(&url);
    if let Err(e) = open::that_detached(&url) {
        tracing::warn!(error = %e, "could not open the browser for the Drive picker");
    }
    tokio::time::timeout(PICK_TIMEOUT, serve(&listener, &nonce, port, &page))
        .await
        .map_err(|_| Error::Other("Google Drive: no files were picked (timed out)".into()))?
}

async fn serve(listener: &TcpListener, nonce: &str, port: u16, page: &str) -> Result<PickOutcome> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let Ok(Some(req)) = read_request(&mut sock).await else {
            continue;
        };
        let origin_ok = req.origin.as_deref().is_none_or(|o| o == format!("http://127.0.0.1:{port}"));
        match route(&req.method, &req.path, nonce) {
            Route::Page => {
                respond(&mut sock, "200 OK", "text/html; charset=utf-8", page.as_bytes()).await;
            }
            Route::Picked if origin_ok => match parse_picked(&req.body) {
                Ok(files) => {
                    respond(&mut sock, "200 OK", "text/plain", b"ok").await;
                    return Ok(PickOutcome::Picked(files));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the Drive picker sent something unreadable");
                    respond(&mut sock, "400 Bad Request", "text/plain", b"bad request").await;
                }
            },
            Route::Cancel if origin_ok => {
                respond(&mut sock, "200 OK", "text/plain", b"ok").await;
                return Ok(PickOutcome::Cancelled);
            }
            _ => respond(&mut sock, "404 Not Found", "text/plain", b"not found").await,
        }
    }
}

async fn respond(sock: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         Referrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body).await;
    let _ = sock.shutdown().await;
}

#[derive(Debug, PartialEq, Eq)]
struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    body: Vec<u8>,
}

async fn read_request(sock: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut chunk))
            .await
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
        if n == 0 {
            return Ok(parse_request(&buf));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST {
            return Ok(None);
        }
        if let Some(req) = parse_request(&buf) {
            return Ok(Some(req));
        }
    }
}

/// A complete request (head + `Content-Length` body), or None while more
/// bytes are needed.
fn parse_request(buf: &[u8]) -> Option<Request> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&buf[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let mut length = 0usize;
    let mut origin = None;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = v.trim().parse().ok()?,
            "origin" => origin = Some(v.trim().to_string()),
            _ => {}
        }
    }
    let body = buf.get(end + 4..end + 4 + length)?.to_vec();
    Some(Request { method, path, origin, body })
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Page,
    Picked,
    Cancel,
    NotFound,
}

fn route(method: &str, path: &str, nonce: &str) -> Route {
    let path = path.split('?').next().unwrap_or_default();
    let Some(rest) = path.strip_prefix('/').and_then(|p| p.strip_prefix(nonce)) else { return Route::NotFound };
    match (method, rest) {
        ("GET", "/" | "") => Route::Page,
        ("POST", "/picked") => Route::Picked,
        ("POST", "/cancel") => Route::Cancel,
        _ => Route::NotFound,
    }
}

fn parse_picked(body: &[u8]) -> Result<Vec<DriveFile>> {
    #[derive(Deserialize)]
    struct Picked {
        #[serde(default)]
        docs: Vec<DriveFile>,
    }
    let p: Picked = serde_json::from_slice(body)?;
    Ok(p.docs.into_iter().filter(|d| !d.id.is_empty()).collect())
}

/// The picker page with its settings written into the script. `<` is
/// escaped so nothing in a value can close the script tag.
fn render_page(cfg: &PickerConfig) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Config<'a> {
        token: &'a str,
        key: &'a str,
        app_id: &'a str,
        email: &'a str,
        nickname: Option<&'a str>,
    }
    let config = Config {
        token: &cfg.access_token,
        key: &cfg.api_key,
        app_id: &cfg.app_id,
        email: &cfg.email,
        nickname: cfg.nickname.as_deref(),
    };
    let json = serde_json::to_string(&config).expect("plain strings serialize").replace('<', "\\u003c");
    PAGE.replacen(CONFIG_SLOT, &json, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PickerConfig {
        PickerConfig {
            access_token: "ya29.tok</script>".into(),
            api_key: "AIzaKEY".into(),
            app_id: "1234".into(),
            email: "me@x.com".into(),
            nickname: Some("Work <b>".into()),
        }
    }

    #[test]
    fn page_carries_the_settings_safely() {
        let page = render_page(&cfg());
        assert!(!page.contains(CONFIG_SLOT));
        assert!(page.contains(
            r#"const COO = {"token":"ya29.tok\u003c/script>","key":"AIzaKEY","appId":"1234","email":"me@x.com","nickname":"Work \u003cb>"};"#
        ));
        assert!(!page.contains("ya29.tok</script>"), "a value can't end the script");
        assert!(page.contains("setAppId(COO.appId)") && page.contains("setOrigin(window.location.origin)"));
        assert!(page.contains("MULTISELECT_ENABLED") && page.contains("https://apis.google.com/js/api.js"));
        assert!(page.contains("setAuthUser(COO.email)") && page.contains(r#"id="account""#), "names the account");
        assert!(page.contains(".picker-dialog-content iframe"), "the picker fills the window");
        let debug = format!("{:?}", cfg());
        assert!(!debug.contains("ya29") && !debug.contains("me@x.com"), "debug output hides the token and address");
    }

    #[test]
    fn a_page_without_a_nickname_says_null() {
        let page = render_page(&PickerConfig { nickname: None, ..cfg() });
        assert!(page.contains(r#""email":"me@x.com","nickname":null}"#));
    }

    #[test]
    fn only_the_nonce_paths_answer() {
        assert_eq!(route("GET", "/n0nce/", "n0nce"), Route::Page);
        assert_eq!(route("GET", "/n0nce?x=1", "n0nce"), Route::Page);
        assert_eq!(route("POST", "/n0nce/picked", "n0nce"), Route::Picked);
        assert_eq!(route("POST", "/n0nce/cancel", "n0nce"), Route::Cancel);
        assert_eq!(route("GET", "/n0nce/picked", "n0nce"), Route::NotFound);
        assert_eq!(route("GET", "/", "n0nce"), Route::NotFound);
        assert_eq!(route("GET", "/other/", "n0nce"), Route::NotFound);
        assert_eq!(route("GET", "/favicon.ico", "n0nce"), Route::NotFound);
    }

    #[test]
    fn requests_parse_once_complete() {
        let body = r#"{"docs":[{"id":"f1","name":"Budget","mimeType":"application/vnd.google-apps.spreadsheet","url":"https://docs.google.com/s"},{"id":"","name":"x","mimeType":"y"}]}"#;
        let raw = format!(
            "POST /n/picked HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://127.0.0.1:5000\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(parse_request(&raw.as_bytes()[..raw.len() - 5]), None, "waits for the whole body");
        let req = parse_request(raw.as_bytes()).unwrap();
        assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", "/n/picked"));
        assert_eq!(req.origin.as_deref(), Some("http://127.0.0.1:5000"));
        let files = parse_picked(&req.body).unwrap();
        assert_eq!(files.len(), 1, "entries without an id are dropped");
        assert_eq!(files[0].id, "f1");
        assert!(files[0].is_native());
        assert!(parse_picked(b"not json").is_err());
        let get = parse_request(b"GET /n/ HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert!(get.body.is_empty() && get.origin.is_none());
    }

    #[tokio::test]
    async fn serves_the_page_then_returns_the_pick() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { serve(&listener, "abc", port, "<p>page</p>").await });
        let ask = |raw: String| async move {
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(raw.as_bytes()).await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        let page = ask("GET /abc/ HTTP/1.1\r\nHost: x\r\n\r\n".into()).await;
        assert!(page.starts_with("HTTP/1.1 200") && page.ends_with("<p>page</p>") && page.contains("no-store"));
        let lost = ask("GET /wrong/ HTTP/1.1\r\n\r\n".into()).await;
        assert!(lost.starts_with("HTTP/1.1 404"));
        // Another site can't post a pick, even knowing the path.
        let evil = r#"{"docs":[{"id":"x","name":"x","mimeType":"x"}]}"#;
        let refused = ask(format!(
            "POST /abc/picked HTTP/1.1\r\nOrigin: https://evil.example\r\nContent-Length: {}\r\n\r\n{evil}",
            evil.len()
        ))
        .await;
        assert!(refused.starts_with("HTTP/1.1 404"));
        let body = r#"{"docs":[{"id":"f9","name":"a.pdf","mimeType":"application/pdf","sizeBytes":"12"}]}"#;
        let ok = ask(format!(
            "POST /abc/picked HTTP/1.1\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ))
        .await;
        assert!(ok.starts_with("HTTP/1.1 200"));
        match server.await.unwrap().unwrap() {
            PickOutcome::Picked(files) => assert_eq!((files[0].id.as_str(), files[0].size_bytes), ("f9", Some(12))),
            other => panic!("{other:?}"),
        }
    }
}
