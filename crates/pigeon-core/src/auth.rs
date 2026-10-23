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
];

#[derive(Debug, Clone)]
pub struct OAuthApp {
    pub provider: ProviderKind,
    pub client_id: String,
    /// Google "Desktop app" clients ship a non-confidential secret that the
    /// token endpoint still requires. Microsoft public clients have none.
    pub client_secret: Option<String>,
    auth_url: &'static str,
    token_url: &'static str,
    scopes: &'static [&'static str],
    extra_auth_params: &'static [(&'static str, &'static str)],
    redirect_host: &'static str,
}

impl OAuthApp {
    pub fn microsoft(client_id: impl Into<String>) -> Self {
        OAuthApp {
            provider: ProviderKind::Microsoft,
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scopes: MS_SCOPES,
            extra_auth_params: &[("prompt", "select_account")],
            // Entra matches `http://localhost` registrations on any port.
            redirect_host: "localhost",
        }
    }

    pub fn google(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        OAuthApp {
            provider: ProviderKind::Google,
            client_id: client_id.into(),
            client_secret: Some(client_secret.into()),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: GOOGLE_SCOPES,
            // `consent` guarantees a refresh token on every sign-in.
            extra_auth_params: &[("access_type", "offline"), ("prompt", "consent select_account")],
            redirect_host: "127.0.0.1",
        }
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
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://{}:{port}", app.redirect_host);
    let state = random_token(16);
    let verifier = random_token(48);

    let mut params = vec![
        ("client_id", app.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("state", state.as_str()),
        ("code_challenge_method", "S256"),
    ];
    let challenge = pkce_challenge(&verifier);
    let scope = app.scope();
    params.push(("code_challenge", &challenge));
    params.push(("scope", &scope));
    params.extend(app.extra_auth_params.iter().copied());
    let url = Url::parse_with_params(app.auth_url, &params).map_err(|e| Error::Other(e.to_string()))?;

    on_url(url.as_str());
    if let Err(e) = open::that_detached(url.as_str()) {
        tracing::warn!(error = %e, "could not open browser");
    }

    let code = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| Error::Auth("timed out waiting for the browser sign-in".into()))??;

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
    if app.provider == ProviderKind::Microsoft {
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
    if app.provider == ProviderKind::Microsoft {
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
            return Err(Error::SignInRequired(app.provider.as_str().into()));
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
        let mut cached = self.cached.lock().await;
        if let Some((token, expires_at)) = cached.as_ref()
            && *expires_at > Instant::now()
        {
            return Ok(token.clone());
        }
        let refresh_token =
            self.store.get(&self.account_id)?.ok_or_else(|| Error::SignInRequired(self.account_id.clone()))?;
        let resp = refresh(&self.app, &refresh_token).await.map_err(|e| match e {
            Error::SignInRequired(_) => Error::SignInRequired(self.account_id.clone()),
            e => e,
        })?;
        if let Some(rotated) = resp.refresh_token.as_deref()
            && rotated != refresh_token
        {
            self.store.set(&self.account_id, rotated)?;
        }
        *cached = Some((resp.access_token.clone(), resp.expires_at()));
        Ok(resp.access_token)
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
