//! Shared HTTP client with retry/backoff for provider APIs.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::{Error, Result};

const MAX_ATTEMPTS: u32 = 5;
/// Per-minute quota errors (Gmail "Units per minute per user") only clear
/// when the minute window rolls over, so they get longer waits and more tries.
const MAX_QUOTA_ATTEMPTS: u32 = 8;
const QUOTA_MIN_WAIT: Duration = Duration::from_secs(15);
const MAX_BODY_IN_ERROR: usize = 2000;

pub fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(concat!("coo/", env!("CARGO_PKG_VERSION")))
            .gzip(true)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("http client")
    })
}

/// Send a request, retrying throttling and transient failures. Honors
/// `Retry-After`; otherwise backs off exponentially with jitter. `make` builds
/// a fresh request per attempt.
pub async fn send(mut make: impl FnMut() -> RequestBuilder) -> Result<Response> {
    let mut backoff = Duration::from_millis(500);
    let mut attempt = 0;
    let mut quota_hits = 0;
    loop {
        attempt += 1;
        let last = attempt >= MAX_ATTEMPTS + quota_hits.min(MAX_QUOTA_ATTEMPTS - MAX_ATTEMPTS);
        let started = std::time::Instant::now();
        let req = make();
        let target = req.try_clone().and_then(|r| r.build().ok()).map(|r| {
            let u = r.url();
            format!("{} {}{}", r.method(), u.host_str().unwrap_or_default(), u.path())
        });
        let sent = req.send().await;
        tracing::debug!(
            request = target.as_deref().unwrap_or("?"),
            status = sent.as_ref().map(|r| r.status().as_u16()).unwrap_or(0),
            ms = started.elapsed().as_millis() as u64,
            attempt,
            "http"
        );
        let wait = match sent {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status();
                let retry_after = retry_after(&resp);
                let url = resp.url().to_string();
                let mut body = resp.text().await.unwrap_or_default();
                if last || !is_retryable(status, &body) {
                    crate::text::truncate_utf8(&mut body, MAX_BODY_IN_ERROR);
                    return Err(Error::Api { status: status.as_u16(), url, body });
                }
                let quota = is_quota(status, &body);
                if quota {
                    quota_hits += 1;
                }
                let wait = match retry_after {
                    Some(w) => w,
                    None if quota => backoff.max(QUOTA_MIN_WAIT),
                    None => backoff,
                };
                tracing::info!(%status, attempt, wait_s = wait.as_secs(), "throttled or transient error; retrying");
                wait
            }
            Err(e) if !last && (e.is_timeout() || e.is_connect()) => {
                tracing::debug!(error = %e, attempt, "retrying");
                backoff
            }
            Err(e) => return Err(e.into()),
        };
        tokio::time::sleep(wait + jitter()).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

/// Throttling that waits on a per-minute (or per-day) quota window.
fn is_quota(status: StatusCode, body: &str) -> bool {
    matches!(status.as_u16(), 403 | 429)
        && (body.contains("ateLimitExceeded")
            || body.contains("RATE_LIMIT_EXCEEDED")
            || body.contains("Quota exceeded"))
}

/// Single attempt, for non-idempotent requests (sending mail): a retry after
/// a timeout could deliver twice.
pub async fn send_once(req: RequestBuilder) -> Result<Response> {
    let resp = req.send().await?;
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status().as_u16();
    let url = resp.url().to_string();
    let mut body = resp.text().await.unwrap_or_default();
    crate::text::truncate_utf8(&mut body, MAX_BODY_IN_ERROR);
    Err(Error::Api { status, url, body })
}

pub async fn get_json<T: DeserializeOwned>(make: impl FnMut() -> RequestBuilder) -> Result<T> {
    Ok(send(make).await?.json().await?)
}

fn is_retryable(status: StatusCode, body: &str) -> bool {
    match status.as_u16() {
        429 | 500 | 502 | 503 | 504 => true,
        // Gmail reports per-user throttling as 403.
        403 => body.contains("ateLimitExceeded"),
        _ => false,
    }
}

fn retry_after(resp: &Response) -> Option<Duration> {
    let secs: u64 = resp.headers().get("retry-after")?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs.min(120)))
}

fn jitter() -> Duration {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    Duration::from_millis(u64::from(nanos % 250))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS, ""));
        assert!(is_retryable(StatusCode::SERVICE_UNAVAILABLE, ""));
        assert!(is_retryable(StatusCode::FORBIDDEN, r#"{"reason":"userRateLimitExceeded"}"#));
        assert!(!is_retryable(StatusCode::FORBIDDEN, r#"{"reason":"insufficientPermissions"}"#));
        assert!(!is_retryable(StatusCode::NOT_FOUND, ""));
        let gmail = r#"{"error":{"code":403,"message":"Quota exceeded for quota metric 'Total Query Cost'","errors":[{"reason":"rateLimitExceeded"}]}}"#;
        assert!(is_retryable(StatusCode::FORBIDDEN, gmail) && is_quota(StatusCode::FORBIDDEN, gmail));
        assert!(!is_quota(StatusCode::SERVICE_UNAVAILABLE, ""));
    }
}
