//! The connected Zoom account: sign-in, sign-out, and the token source the
//! meetings client (and the broker, for the `coo` helper) use.
//!
//! Zoom isn't a mail/calendar account, so it lives in the store's
//! `services` table (who's connected) plus one token-store entry
//! (`auth::ZOOM_TOKEN_KEY`) holding the refresh token.

use std::sync::Arc;

use pigeon_core::auth::{self, OAuthApp, TokenSource, ZOOM_TOKEN_KEY};
use pigeon_core::model::{ConnectedService, EventDraft};
use pigeon_core::{Error, Result, now_ms};

use crate::Pigeon;

pub(crate) const NOT_CONNECTED: &str = "Connect Zoom first, in Coo: Settings › Calendar › Video calls.";

impl Pigeon {
    /// The connected Zoom account, if any.
    pub fn zoom_service(&self) -> Result<Option<ConnectedService>> {
        self.store().service(ZOOM_TOKEN_KEY)
    }

    /// Browser sign-in to Zoom (PKCE, fixed loopback redirect).
    pub async fn zoom_connect(&self, on_url: impl FnOnce(&str)) -> Result<ConnectedService> {
        let app = OAuthApp::zoom_from_env()?;
        let token = auth::authorize(&app, on_url).await?;
        let refresh =
            token.refresh_token.as_deref().ok_or_else(|| Error::Auth("Zoom returned no refresh token".into()))?;
        let user = pigeon_zoom::whoami(&token.access_token).await?;
        let service = ConnectedService {
            id: ZOOM_TOKEN_KEY.into(),
            display_name: user.name(),
            email: user.email,
            connected_at: now_ms(),
        };
        self.tokens.set(ZOOM_TOKEN_KEY, refresh)?;
        let source = Arc::new(TokenSource::new(app, ZOOM_TOKEN_KEY, self.tokens.clone()).with_access(&token));
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).insert(ZOOM_TOKEN_KEY.into(), source);
        self.store().upsert_service(&service)?;
        Ok(service)
    }

    /// Forget the Zoom account. Revoking is best effort (Zoom may not let a
    /// public client revoke); the local sign-in is deleted either way.
    /// Meetings already created keep working.
    pub async fn zoom_disconnect(&self) -> Result<()> {
        if let (Some(client_id), Ok(Some(refresh))) = (auth::zoom_client_id(), self.tokens.get(ZOOM_TOKEN_KEY))
            && let Err(e) = auth::zoom_revoke(&client_id, &refresh).await
        {
            tracing::info!(error = %e, "Zoom revoke failed; deleting the local sign-in anyway");
        }
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).remove(ZOOM_TOKEN_KEY);
        self.tokens.delete(ZOOM_TOKEN_KEY)?;
        self.store().delete_service(ZOOM_TOKEN_KEY)
    }

    /// Access tokens for the connected Zoom account (the broker serves these
    /// to the helper under the `zoom` key).
    pub(crate) fn zoom_tokens(&self) -> Result<Arc<TokenSource>> {
        if self.zoom_service()?.is_none() {
            return Err(Error::Auth(NOT_CONNECTED.into()));
        }
        let mut sources = self.sources.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = sources.get(ZOOM_TOKEN_KEY) {
            return Ok(s.clone());
        }
        let source = Arc::new(TokenSource::new(OAuthApp::zoom_from_env()?, ZOOM_TOKEN_KEY, self.tokens.clone()));
        sources.insert(ZOOM_TOKEN_KEY.into(), source.clone());
        Ok(source)
    }

    pub(crate) fn zoom(&self) -> Result<pigeon_zoom::Zoom> {
        Ok(pigeon_zoom::Zoom::new(self.zoom_tokens()?))
    }
}

/// How a calendar event maps to a meeting's schedule.
pub(crate) fn meeting_times(d: &EventDraft) -> pigeon_zoom::MeetingTimes<'_> {
    pigeon_zoom::MeetingTimes {
        topic: &d.title,
        start_at: d.start_at,
        end_at: d.end_at,
        timezone: d.timezone.as_deref(),
    }
}

/// Run `create_event` after a meeting was made; if it fails, delete the
/// meeting so a failed save never leaves an orphan Zoom meeting behind.
pub(crate) async fn rollback_on_error<T, M>(
    meeting: Option<M>,
    result: Result<T>,
    delete: impl AsyncFnOnce(M) -> Result<()>,
) -> Result<(T, Option<M>)> {
    match result {
        Ok(v) => Ok((v, meeting)),
        Err(e) => {
            if let Some(m) = meeting
                && let Err(cleanup) = delete(m).await
            {
                tracing::warn!(error = %cleanup, "couldn't delete the Zoom meeting after the event failed");
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn failed_event_deletes_the_new_meeting() {
        let deleted = AtomicUsize::new(0);
        let del = async |_m: &str| {
            deleted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let r: Result<((), Option<&str>)> =
            rollback_on_error(Some("m1"), Err(Error::Other("calendar said no".into())), del).await;
        assert!(r.is_err());
        assert_eq!(deleted.load(Ordering::SeqCst), 1);

        let ok = rollback_on_error(Some("m2"), Ok(5), async |_m: &str| -> Result<()> { panic!("must not delete") })
            .await
            .unwrap();
        assert_eq!(ok, (5, Some("m2")));

        // No meeting: nothing to clean up, the error passes through.
        let none: Result<((), Option<&str>)> =
            rollback_on_error(None, Err(Error::Other("x".into())), async |_m: &str| -> Result<()> { panic!() }).await;
        assert!(none.is_err());
    }

    #[test]
    fn meeting_times_follow_the_draft() {
        let d = EventDraft {
            title: "Sync".into(),
            start_at: 1,
            end_at: 2,
            timezone: Some("UTC".into()),
            ..Default::default()
        };
        let t = meeting_times(&d);
        assert_eq!((t.topic, t.start_at, t.end_at, t.timezone), ("Sync", 1, 2, Some("UTC")));
    }
}
