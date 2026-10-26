//! OAuth 2.0 authorization code + PKCE for desktop (public) clients, with a
//! loopback redirect served by a one-shot local HTTP listener.

use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use crate::http::client;
use crate::model::ProviderKind;
use crate::tokens::TokenStore;
use crate::{Error, Result};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh this long before the access token actually expires.
const EXPIRY_SLACK: Duration = Duration::from_secs(120);

const MS_SCOPES: &[&str] = &[
    "offline_access",
    "openid",
    "profile",
    "email",
    "User.Read",
    "Mail.ReadWrite",
    "Mail.Send",
    "Calendars.ReadWrite",
];

const GOOGLE_SCOPES: &[&str] = &[
    "openid",
    "email",
    "profile",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/calendar",
    // Only the files the user picks in the Google Drive picker.
    crate::drive::DRIVE_FILE_SCOPE,
];

/// Zoom's OAuth app: PKCE public client (no secret). Zoom only accepts a
/// loopback redirect for PKCE apps, at this exact registered address.
pub const ZOOM_REDIRECT_PORT: u16 = 47821;
pub const ZOOM_REDIRECT_PATH: &str = "/zoom/callback";
/// Token-store key for the connected Zoom account (there's at most one).
pub const ZOOM_TOKEN_KEY: &str = "zoom";

/// What an OAuth app signs in to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Mail(ProviderKind),
    Zoom,
}

impl AppKind {
    fn label(self) -> &'static str {
        match self {
            AppKind::Mail(p) => p.as_str(),
            AppKind::Zoom => "zoom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OAuthApp {
    pub kind: AppKind,
    pub client_id: String,
    /// Google "Desktop app" clients ship a non-confidential secret that the
    /// token endpoint still requires. Microsoft public clients have none.
    pub client_secret: Option<String>,
    auth_url: &'static str,
    token_url: &'static str,
    scopes: &'static [&'static str],
    extra_auth_params: &'static [(&'static str, &'static str)],
    redirect_host: &'static str,
    /// 0 = any free port (providers that match loopback on any port).
    redirect_port: u16,
    redirect_path: &'static str,
    /// Pre-fills the account (`login_hint`), for adding scopes to an
    /// account that's already signed in.
    login_hint: Option<String>,
}

impl OAuthApp {
    pub fn microsoft(client_id: impl Into<String>) -> Self {
        OAuthApp {
            kind: AppKind::Mail(ProviderKind::Microsoft),
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scopes: MS_SCOPES,
            extra_auth_params: &[("prompt", "select_account")],
            // Entra matches `http://localhost` registrations on any port.
            redirect_host: "localhost",
            redirect_port: 0,
            redirect_path: "",
            login_hint: None,
        }
    }

    pub fn google(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        OAuthApp {
            kind: AppKind::Mail(ProviderKind::Google),
            client_id: client_id.into(),
            client_secret: Some(client_secret.into()),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: GOOGLE_SCOPES,
            // `consent` guarantees a refresh token on every sign-in.
            extra_auth_params: &[("access_type", "offline"), ("prompt", "consent select_account")],
            redirect_host: "127.0.0.1",
            redirect_port: 0,
            redirect_path: "",
            login_hint: None,
        }
    }

    /// Google incremental consent for an account that's already signed in:
    /// the same app and scopes (now including Drive), pinned to `email`,
    /// keeping what was granted before.
    pub fn google_add_scopes(self, email: &str) -> Self {
        OAuthApp {
            extra_auth_params: &[("access_type", "offline"), ("prompt", "consent"), ("include_granted_scopes", "true")],
            login_hint: Some(email.to_string()),
            ..self
        }
    }

    /// Pre-fill the sign-in page with the address typed in "Add account"
    /// (`login_hint`; Google and Microsoft both take it). Blank is no hint.
    pub fn with_login_hint(self, email: Option<&str>) -> Self {
        let hint = email.map(str::trim).filter(|e| !e.is_empty()).map(String::from);
        OAuthApp { login_hint: hint.or(self.login_hint.clone()), ..self }
    }

    /// Zoom (meetings). Scopes come from the Marketplace app's configuration.
    pub fn zoom(client_id: impl Into<String>) -> Self {
        OAuthApp {
            kind: AppKind::Zoom,
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://zoom.us/oauth/authorize",
            token_url: "https://zoom.us/oauth/token",
            scopes: &[],
            extra_auth_params: &[],
            redirect_host: "127.0.0.1",
            redirect_port: ZOOM_REDIRECT_PORT,
            redirect_path: ZOOM_REDIRECT_PATH,
            login_hint: None,
        }
    }

    /// The Zoom app with the client id from the environment or baked in.
    pub fn zoom_from_env() -> Result<Self> {
        zoom_client_id().map(Self::zoom).ok_or_else(|| Error::Auth("PIGEON_ZOOM_CLIENT_ID is not set".into()))
    }

    /// The mail provider this app signs in to (None for Zoom).
    pub fn provider(&self) -> Option<ProviderKind> {
        match self.kind {
            AppKind::Mail(p) => Some(p),
            AppKind::Zoom => None,
        }
    }

    pub fn redirect_uri(&self, port: u16) -> String {
        format!("http://{}:{port}{}", self.redirect_host, self.redirect_path)
    }

    /// The authorization URL the browser opens.
    pub fn authorize_url(&self, redirect_uri: &str, state: &str, challenge: &str) -> Result<Url> {
        let mut params = vec![
            ("client_id", self.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("state", state),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
        ];
        let scope = self.scope();
        if !scope.is_empty() {
            params.push(("scope", &scope));
        }
        params.extend(self.extra_auth_params.iter().copied());
        if let Some(hint) = &self.login_hint {
            params.push(("login_hint", hint));
        }
        Url::parse_with_params(self.auth_url, &params).map_err(|e| Error::Other(e.to_string()))
    }

    /// Client credentials from the runtime environment, falling back to
    /// values baked in at compile time (release builds).
    pub fn from_env(provider: ProviderKind) -> Result<Self> {
        fn var(name: &str, baked: Option<&'static str>) -> Result<String> {
            std::env::var(name)
                .ok()
                .or(baked.map(String::from))
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Error::Auth(format!("{name} is not set")))
        }
        Ok(match provider {
            ProviderKind::Microsoft => Self::microsoft(var("PIGEON_MS_CLIENT_ID", option_env!("PIGEON_MS_CLIENT_ID"))?),
            ProviderKind::Google => Self::google(
                var("PIGEON_GOOGLE_CLIENT_ID", option_env!("PIGEON_GOOGLE_CLIENT_ID"))?,
                var("PIGEON_GOOGLE_CLIENT_SECRET", option_env!("PIGEON_GOOGLE_CLIENT_SECRET"))?,
            ),
            ProviderKind::ICloud => {
                return Err(Error::Auth("iCloud uses an app-specific password, not OAuth".into()));
            }
        })
    }

    fn scope(&self) -> String {
        self.scopes.join(" ")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// The scopes granted (space-separated), when the provider says.
    #[serde(default)]
    pub scope: Option<String>,
}

impl TokenResponse {
    fn expires_at(&self) -> Instant {
        let ttl = Duration::from_secs(self.expires_in.unwrap_or(3600));
        Instant::now() + ttl.saturating_sub(EXPIRY_SLACK)
    }
}

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("system randomness");
    URL_SAFE_NO_PAD.encode(buf)
}

/// RFC 7636 S256 code challenge.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Run the interactive browser sign-in and exchange the code for tokens.
/// `on_url` receives the authorization URL (e.g. to print it in case the
/// browser does not open).
pub async fn authorize(app: &OAuthApp, on_url: impl FnOnce(&str)) -> Result<TokenResponse> {
    let listener = match TcpListener::bind(("127.0.0.1", app.redirect_port)).await {
        Ok(l) => l,
        Err(e) if app.redirect_port != 0 && e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err(Error::Auth(format!(
                "Another app is using port {}, so Coo can't receive the sign-in. Close it and try again.",
                app.redirect_port
            )));
        }
        Err(e) => return Err(e.into()),
    };
    let port = listener.local_addr()?.port();
    let redirect_uri = app.redirect_uri(port);
    let state = random_token(16);
    let verifier = random_token(48);
    let challenge = pkce_challenge(&verifier);
    let scope = app.scope();
    let url = app.authorize_url(&redirect_uri, &state, &challenge)?;

    on_url(url.as_str());
    if let Err(e) = open::that_detached(url.as_str()) {
        tracing::warn!(error = %e, "could not open browser");
    }

    let code =
        tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, &state)).await.map_err(|_| match app.kind {
            AppKind::Zoom => Error::Auth(
                "The Zoom sign-in didn't finish. While Coo's Zoom app is in development, only accounts in its \
                 owner's Zoom organization can connect."
                    .into(),
            ),
            AppKind::Mail(_) => Error::Auth("timed out waiting for the browser sign-in".into()),
        })??;

    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", app.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    if let Some(secret) = &app.client_secret {
        form.push(("client_secret", secret));
    }
    if app.kind == AppKind::Mail(ProviderKind::Microsoft) {
        form.push(("scope", &scope));
    }
    token_request(app, &form).await
}

/// Exchange a refresh token for a new access token (and possibly a rotated
/// refresh token).
pub async fn refresh(app: &OAuthApp, refresh_token: &str) -> Result<TokenResponse> {
    let scope = app.scope();
    let mut form =
        vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", app.client_id.as_str())];
    if let Some(secret) = &app.client_secret {
        form.push(("client_secret", secret));
    }
    if app.kind == AppKind::Mail(ProviderKind::Microsoft) {
        form.push(("scope", &scope));
    }
    token_request(app, &form).await
}

async fn token_request(app: &OAuthApp, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = client().post(app.token_url).form(form).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        if body.contains("invalid_grant") {
            return Err(Error::SignInRequired(app.kind.label().into()));
        }
        return Err(Error::Auth(format!("token endpoint returned {status}: {body}")));
    }
    Ok(serde_json::from_str(&body)?)
}

async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        while len < buf.len() {
            let n = sock.read(&mut buf[len..]).await?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf[..len]);
        let outcome = parse_callback(&request, state);
        let page = match &outcome {
            Callback::Code(_) => "Signed in. You can close this tab and return to Coo.",
            Callback::Failed(_) => "Sign-in failed. Return to Coo for details.",
            Callback::Ignore => {
                let _ =
                    sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>Coo</title>\
             <body style=\"font:16px -apple-system,system-ui;display:grid;place-items:center;height:90vh\">{page}</body>"
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        return match outcome {
            Callback::Code(code) => Ok(code),
            Callback::Failed(msg) => Err(Error::Auth(msg)),
            Callback::Ignore => unreachable!(),
        };
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Failed(String),
    /// Not the redirect (favicon, probes).
    Ignore,
}

fn parse_callback(request: &str, expected_state: &str) -> Callback {
    let Some(target) = request.lines().next().and_then(|l| l.split_whitespace().nth(1)) else {
        return Callback::Ignore;
    };
    let Ok(url) = Url::parse(&format!("http://loopback{target}")) else {
        return Callback::Ignore;
    };
    let get = |k: &str| url.query_pairs().find(|(name, _)| name == k).map(|(_, v)| v.into_owned());
    if let Some(err) = get("error") {
        let desc = get("error_description").unwrap_or_default();
        return Callback::Failed(format!("{err}: {desc}").trim_end_matches(": ").to_string());
    }
    match (get("code"), get("state")) {
        (Some(code), Some(state)) if state == expected_state => Callback::Code(code),
        (Some(_), _) => Callback::Failed("state mismatch".into()),
        _ => Callback::Ignore,
    }
}

/// Hands out valid access tokens for one account, refreshing on demand and
/// persisting rotated refresh tokens.
pub struct TokenSource {
    app: OAuthApp,
    account_id: String,
    store: TokenStore,
    cached: tokio::sync::Mutex<Option<(String, Instant)>>,
}

impl TokenSource {
    pub fn new(app: OAuthApp, account_id: impl Into<String>, store: TokenStore) -> Self {
        TokenSource { app, account_id: account_id.into(), store, cached: Default::default() }
    }

    /// Seed with a fresh access token (right after sign-in).
    pub fn with_access(self, token: &TokenResponse) -> Self {
        *self.cached.try_lock().expect("uncontended") = Some((token.access_token.clone(), token.expires_at()));
        self
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub async fn access_token(&self) -> Result<String> {
        Ok(self.access_token_with_expiry().await?.0)
    }

    /// The access token and when it expires (the broker tells peers).
    pub async fn access_token_with_expiry(&self) -> Result<(String, Instant)> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expires_at)) = cached.as_ref()
            && *expires_at > Instant::now()
        {
            return Ok((token.clone(), *expires_at));
        }
        // The helper asks the app, which holds the refresh token.
        if let Some(client) = self.store.broker().cloned() {
            let account = self.account_id.clone();
            let req = crate::broker::Request::AccessToken { account: account.clone() };
            let answer = tokio::task::spawn_blocking(move || client.call(&req))
                .await
                .map_err(|e| Error::Other(e.to_string()))?;
            match answer {
                Ok(r) if r.ok && r.token.is_some() => {
                    // A minute of margin so a token never expires mid-request.
                    let ttl = Duration::from_secs(r.expires_in.unwrap_or(300).saturating_sub(60).max(30));
                    let token = r.token.expect("checked");
                    let expires_at = Instant::now() + ttl;
                    *cached = Some((token.clone(), expires_at));
                    return Ok((token, expires_at));
                }
                Ok(r) => return Err(r.into_error(&account)),
                Err(_) if self.store.fallback().is_some() => {}
                Err(_) => return Err(Error::TokenStore(crate::broker::APP_NOT_RUNNING.into())),
            }
        }
        let store = self.store.fallback().unwrap_or(&self.store);
        let refresh_token =
            store.get(&self.account_id)?.ok_or_else(|| Error::SignInRequired(self.account_id.clone()))?;
        let resp = refresh(&self.app, &refresh_token).await.map_err(|e| match e {
            Error::SignInRequired(_) => Error::SignInRequired(self.account_id.clone()),
            e => e,
        })?;
        if let Some(rotated) = resp.refresh_token.as_deref()
            && rotated != refresh_token
        {
            store.set(&self.account_id, rotated)?;
        }
        *cached = Some((resp.access_token.clone(), resp.expires_at()));
        let expires_at = resp.expires_at();
        Ok((resp.access_token, expires_at))
    }

    /// Drop the cached access token (e.g. after a 401).
    pub async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }
}

/// Microsoft sign-in failed because the tenant requires an admin to approve
/// the app: returns the AADSTS code (or "consent_required").
pub fn admin_consent_code(error: &str) -> Option<&'static str> {
    const CODES: [&str; 4] = ["AADSTS65001", "AADSTS90094", "AADSTS90095", "AADSTS900941"];
    let e = error.to_ascii_lowercase();
    if let Some(c) = CODES.iter().find(|c| e.contains(&c.to_ascii_lowercase())) {
        return Some(c);
    }
    (e.contains("consent_required") || e.contains("admin approval") || e.contains("need admin approval"))
        .then_some("consent_required")
}

/// The Zoom (PKCE public) client id in effect (runtime env, else baked in).
pub fn zoom_client_id() -> Option<String> {
    std::env::var("PIGEON_ZOOM_CLIENT_ID")
        .ok()
        .or(option_env!("PIGEON_ZOOM_CLIENT_ID").map(String::from))
        .filter(|v| !v.is_empty())
}

/// Best-effort revoke of a Zoom token (public clients may not be allowed to;
/// the local copy is deleted either way).
pub async fn zoom_revoke(client_id: &str, token: &str) -> Result<()> {
    let resp = client()
        .post("https://zoom.us/oauth/revoke")
        .form(&[("token", token), ("client_id", client_id)])
        .send()
        .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(Error::Auth(format!("Zoom revoke returned {}", resp.status())))
    }
}

/// The Google API key for the Drive picker (runtime env, else baked in).
/// Without it the "From Google Drive" option is unavailable.
pub fn google_api_key() -> Option<String> {
    std::env::var("PIGEON_GOOGLE_API_KEY")
        .ok()
        .or(option_env!("PIGEON_GOOGLE_API_KEY").map(String::from))
        .filter(|v| !v.is_empty())
}

/// The Microsoft client id in effect (runtime env, else baked in).
pub fn microsoft_client_id() -> Option<String> {
    std::env::var("PIGEON_MS_CLIENT_ID")
        .ok()
        .or(option_env!("PIGEON_MS_CLIENT_ID").map(String::from))
        .filter(|v| !v.is_empty())
}

/// Where a tenant admin approves the app for everyone in the organization.
pub fn admin_consent_url(client_id: &str) -> String {
    format!("https://login.microsoftonline.com/organizations/adminconsent?client_id={client_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_are_urlsafe_and_unique() {
        let (a, b) = (random_token(32), random_token(32));
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn callback_parsing() {
        let req = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert_eq!(parse_callback(&req("/?code=abc%2F1&state=s1"), "s1"), Callback::Code("abc/1".into()));
        assert_eq!(parse_callback(&req("/?code=abc&state=evil"), "s1"), Callback::Failed("state mismatch".into()));
        assert_eq!(
            parse_callback(&req("/?error=access_denied&error_description=User+said+no"), "s1"),
            Callback::Failed("access_denied: User said no".into())
        );
        assert_eq!(parse_callback(&req("/favicon.ico"), "s1"), Callback::Ignore);
        assert_eq!(parse_callback("", "s1"), Callback::Ignore);
    }

    #[test]
    fn zoom_authorize_url_is_pkce_with_the_registered_redirect() {
        let app = OAuthApp::zoom("cid");
        let redirect = app.redirect_uri(ZOOM_REDIRECT_PORT);
        assert_eq!(redirect, "http://127.0.0.1:47821/zoom/callback");
        let url = app.authorize_url(&redirect, "st", "ch").unwrap();
        assert_eq!(url.host_str(), Some("zoom.us"));
        assert_eq!(url.path(), "/oauth/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:47821/zoom/callback");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["response_type"], "code");
        assert!(!q.contains_key("scope"), "Zoom scopes come from the app config");
        assert!(app.client_secret.is_none() && app.provider().is_none());
        // Mail providers keep a random port and the bare root path.
        assert_eq!(OAuthApp::google("a", "b").redirect_uri(5000), "http://127.0.0.1:5000");
    }

    #[tokio::test]
    async fn zoom_listens_on_the_registered_port_and_says_when_its_taken() {
        // Hold the port, then try to sign in: a clear error, no browser.
        let Ok(_held) = TcpListener::bind(("127.0.0.1", ZOOM_REDIRECT_PORT)).await else {
            return; // something else already has it; nothing to prove here
        };
        let mut opened = false;
        let err = authorize(&OAuthApp::zoom("cid"), |_| opened = true).await.unwrap_err();
        assert!(err.to_string().contains("port 47821"), "{err}");
        assert!(!opened, "must fail before opening the browser");
    }

    #[test]
    fn typed_address_becomes_the_login_hint() {
        let q = |app: OAuthApp| -> std::collections::HashMap<String, String> {
            app.authorize_url("http://localhost:5000", "st", "ch").unwrap().query_pairs().into_owned().collect()
        };
        let ms = q(OAuthApp::microsoft("cid").with_login_hint(Some(" dana@acme.co.il ")));
        assert_eq!(ms["login_hint"], "dana@acme.co.il");
        assert_eq!(ms["prompt"], "select_account");
        let g = q(OAuthApp::google("cid", "secret").with_login_hint(Some("me@gmail.com")));
        assert_eq!(g["login_hint"], "me@gmail.com");
        assert!(!q(OAuthApp::google("cid", "secret").with_login_hint(Some("  "))).contains_key("login_hint"));
        assert!(!q(OAuthApp::microsoft("cid").with_login_hint(None)).contains_key("login_hint"));
    }

    #[test]
    fn google_scopes_include_drive_file_and_incremental_consent_pins_the_account() {
        let app = OAuthApp::google("cid", "secret");
        let url = app.authorize_url("http://127.0.0.1:5000", "st", "ch").unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert!(q["scope"].split(' ').any(|s| s == crate::drive::DRIVE_FILE_SCOPE), "{}", q["scope"]);
        assert!(!q.contains_key("login_hint") && !q.contains_key("include_granted_scopes"));
        assert_eq!(q["prompt"], "consent select_account");

        let more = OAuthApp::google("cid", "secret").google_add_scopes("me@x.io");
        let url = more.authorize_url("http://127.0.0.1:5000", "st", "ch").unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["login_hint"], "me@x.io");
        assert_eq!(q["include_granted_scopes"], "true");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["access_type"], "offline");
        assert!(q["scope"].contains("gmail.modify") && q["scope"].contains("drive.file"));
        assert_eq!(more.client_secret.as_deref(), Some("secret"));
    }

    #[test]
    fn zoom_token_response_parses() {
        let t: TokenResponse = serde_json::from_str(
            r#"{"access_token":"at","token_type":"bearer","refresh_token":"rt","expires_in":3599,"scope":"meeting:write:meeting user:read:user"}"#,
        )
        .unwrap();
        assert_eq!((t.access_token.as_str(), t.refresh_token.as_deref(), t.expires_in), ("at", Some("rt"), Some(3599)));
        assert_eq!(t.scope.as_deref(), Some("meeting:write:meeting user:read:user"));
    }

    #[test]
    fn detects_admin_consent_errors() {
        assert_eq!(
            admin_consent_code(
                "access_denied: AADSTS65001: The user or administrator has not consented to use the application"
            ),
            Some("AADSTS65001")
        );
        assert_eq!(admin_consent_code("consent_required: AADSTS90094: Need admin approval"), Some("AADSTS90094"));
        assert_eq!(admin_consent_code("interaction_required: consent_required"), Some("consent_required"));
        assert_eq!(admin_consent_code("access_denied: User said no"), None);
        assert!(admin_consent_url("abc").ends_with("adminconsent?client_id=abc"));
    }
}
