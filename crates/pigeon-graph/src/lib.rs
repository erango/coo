//! Microsoft Graph (Exchange Online / M365) backend: mail via per-folder
//! `messages/delta`, calendars via `calendarView`.

mod actions;
mod body;
mod calendar;
mod calendar_write;
mod drafts;
mod mail;
mod send;
mod types;

use std::sync::Arc;

use pigeon_core::auth::TokenSource;
use pigeon_core::http::{client, get_json};
use pigeon_core::{Error, Result};
use serde::de::DeserializeOwned;

pub(crate) const BASE: &str = "https://graph.microsoft.com/v1.0";

/// Immutable ids survive moves between folders; page size for delta; event
/// times in UTC.
pub(crate) const PREFER: &str = r#"IdType="ImmutableId", odata.maxpagesize=100, outlook.timezone="UTC""#;

pub struct Graph {
    pub(crate) tokens: Arc<TokenSource>,
    /// Initial mail sync reaches back this many days.
    pub mail_window_days: i64,
}

impl Graph {
    pub fn new(tokens: Arc<TokenSource>) -> Self {
        Graph { tokens, mail_window_days: 90 }
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let token = self.tokens.access_token().await?;
        let result = get_json(|| client().get(url).bearer_auth(&token).header("Prefer", PREFER)).await;
        if matches!(result, Err(ref e) if e.status() == Some(401)) {
            self.tokens.invalidate().await;
        }
        result
    }

    /// Follow `@odata.nextLink` to the end.
    pub(crate) async fn get_all<T: DeserializeOwned>(&self, url: &str) -> Result<Vec<T>> {
        let mut out = Vec::new();
        let mut next = Some(url.to_string());
        while let Some(url) = next {
            let page: types::Page<T> = self.get(&url).await?;
            out.extend(page.value);
            next = page.next_link;
        }
        Ok(out)
    }
}

/// Signed-in user's (email, display name), from a raw access token.
pub async fn whoami(access_token: &str) -> Result<(String, String)> {
    let me: types::Me = get_json(|| {
        client().get(format!("{BASE}/me?$select=displayName,mail,userPrincipalName")).bearer_auth(access_token)
    })
    .await?;
    let email = me
        .mail
        .filter(|m| !m.is_empty())
        .or(me.user_principal_name)
        .ok_or_else(|| Error::Other("Graph /me returned no address".into()))?;
    Ok((email, me.display_name.unwrap_or_default()))
}
