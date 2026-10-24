//! Google backend: Gmail API (labels + `history.list` incremental sync) and
//! Calendar API v3, plus Drive (files the user picks to attach).

mod actions;
mod body;
mod calendar;
mod calendar_write;
mod drafts;
pub mod drive;
mod gmail;
pub mod picker;
mod send;

use std::sync::Arc;

use pigeon_core::Result;
use pigeon_core::auth::TokenSource;
use pigeon_core::http::{client, get_json};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde::de::DeserializeOwned;

pub(crate) const GMAIL: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
pub(crate) const CALENDAR: &str = "https://www.googleapis.com/calendar/v3";

pub struct Google {
    pub(crate) tokens: Arc<TokenSource>,
    /// Initial mail sync reaches back this many days.
    pub mail_window_days: i64,
}

impl Google {
    pub fn new(tokens: Arc<TokenSource>) -> Self {
        Google { tokens, mail_window_days: 90 }
    }

    /// GET `url` with query params, as JSON.
    pub(crate) async fn get<T: DeserializeOwned>(&self, url: &str, query: &[(&str, String)]) -> Result<T> {
        let token = self.tokens.access_token().await?;
        let result = get_json(|| self.req(url, &token).query(query)).await;
        if matches!(result, Err(ref e) if e.status() == Some(401)) {
            self.tokens.invalidate().await;
        }
        result
    }

    fn req(&self, url: &str, token: &str) -> RequestBuilder {
        client().get(url).bearer_auth(token)
    }
}

#[derive(Deserialize)]
struct UserInfo {
    email: String,
    #[serde(default)]
    name: Option<String>,
}

/// Signed-in user's (email, display name), from a raw access token.
pub async fn whoami(access_token: &str) -> Result<(String, String)> {
    let info: UserInfo =
        get_json(|| client().get("https://openidconnect.googleapis.com/v1/userinfo").bearer_auth(access_token)).await?;
    Ok((info.email, info.name.unwrap_or_default()))
}
