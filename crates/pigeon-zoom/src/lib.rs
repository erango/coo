//! Zoom meetings for calendar events: create, reschedule and delete the
//! meetings Coo makes, through the user's connected Zoom account (OAuth,
//! PKCE public client; see `pigeon_core::auth::OAuthApp::zoom`).

use std::sync::Arc;

use pigeon_core::auth::TokenSource;
use pigeon_core::http::{client, send, send_once};
use pigeon_core::{Error, Result};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

const API: &str = "https://api.zoom.us/v2";

/// A meeting as Coo uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub id: String,
    pub join_url: String,
    /// Numeric meeting id formatted for people ("812 3456 7890").
    pub display_id: String,
    pub passcode: Option<String>,
}

/// What a meeting is scheduled as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingTimes<'a> {
    pub topic: &'a str,
    pub start_at: i64,
    pub end_at: i64,
    /// IANA zone, when the event has one.
    pub timezone: Option<&'a str>,
}

/// Body for creating a scheduled meeting. Joining before the host and no
/// waiting room match what people expect from a calendar invite link.
pub fn create_body(t: &MeetingTimes<'_>) -> Value {
    let mut body = json!({
        "topic": topic(t.topic),
        "type": 2,
        "start_time": utc(t.start_at),
        "duration": duration_minutes(t.start_at, t.end_at),
        "settings": {
            "join_before_host": true,
            "waiting_room": false,
            "host_video": false,
            "participant_video": false,
            "mute_upon_entry": false,
        },
    });
    if let Some(tz) = t.timezone {
        body["timezone"] = json!(tz);
    }
    body
}

/// Body for rescheduling or renaming a meeting.
pub fn update_body(t: &MeetingTimes<'_>) -> Value {
    let mut body = json!({
        "topic": topic(t.topic),
        "start_time": utc(t.start_at),
        "duration": duration_minutes(t.start_at, t.end_at),
    });
    if let Some(tz) = t.timezone {
        body["timezone"] = json!(tz);
    }
    body
}

/// Zoom caps topics at 200 characters.
fn topic(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() { "Meeting".into() } else { t.chars().take(200).collect() }
}

/// `2026-10-23T14:00:00Z` (Zoom reads a trailing Z as UTC).
fn utc(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

fn duration_minutes(start: i64, end: i64) -> i64 {
    ((end - start) / 60_000).max(1)
}

#[derive(Deserialize)]
struct RawMeeting {
    id: Value,
    join_url: String,
    #[serde(default)]
    password: Option<String>,
}

/// Map Zoom's meeting response.
pub fn parse_meeting(v: &Value) -> Result<Meeting> {
    let raw: RawMeeting = serde_json::from_value(v.clone())?;
    let id = match raw.id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s,
        other => return Err(Error::Other(format!("unexpected Zoom meeting id {other}"))),
    };
    let display_id = group_digits(&id);
    Ok(Meeting { id, join_url: raw.join_url, display_id, passcode: raw.password.filter(|p| !p.is_empty()) })
}

/// "81234567890" → "812 3456 7890" (Zoom's own grouping for 10–11 digits).
fn group_digits(id: &str) -> String {
    if !id.chars().all(|c| c.is_ascii_digit()) || id.len() < 9 {
        return id.to_string();
    }
    let (a, rest) = id.split_at(3);
    let (b, c) = rest.split_at(rest.len() - 4);
    format!("{a} {b} {c}")
}

/// The block Coo adds to an event's description, and the location it uses.
pub fn invite_text(m: &Meeting) -> String {
    let mut s = format!("Join Zoom meeting: {}\nMeeting ID: {}", m.join_url, m.display_id);
    if let Some(p) = &m.passcode {
        s.push_str(&format!("\nPasscode: {p}"));
    }
    s
}

/// Put the join link in the event's location (appended if there is one) and
/// the invite block at the end of the description.
pub fn add_to_event(location: Option<&str>, description: Option<&str>, m: &Meeting) -> (String, String) {
    let location = match location.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if l.contains(&m.join_url) => l.to_string(),
        Some(l) => format!("{l} · {}", m.join_url),
        None => m.join_url.clone(),
    };
    let block = invite_text(m);
    let description = match description.map(str::trim_end).filter(|d| !d.is_empty()) {
        Some(d) => format!("{d}\n\n{block}"),
        None => block,
    };
    (location, description)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZoomUser {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
}

impl ZoomUser {
    pub fn name(&self) -> String {
        if let Some(n) = self.display_name.as_deref().filter(|n| !n.trim().is_empty()) {
            return n.to_string();
        }
        let full = format!("{} {}", self.first_name.as_deref().unwrap_or(""), self.last_name.as_deref().unwrap_or(""));
        let full = full.trim();
        if full.is_empty() { self.email.clone() } else { full.to_string() }
    }
}

/// Who an access token belongs to (right after sign-in).
pub async fn whoami(access_token: &str) -> Result<ZoomUser> {
    let resp = send(|| client().get(format!("{API}/users/me")).bearer_auth(access_token)).await.map_err(plain)?;
    Ok(resp.json().await?)
}

/// Turn Zoom API failures into something a person can act on.
fn plain(e: Error) -> Error {
    match e.status() {
        Some(401) => Error::Auth("Zoom needs you to connect it again (Settings › Accounts › Video calls).".into()),
        Some(403) => Error::Auth(
            "Zoom refused: while Coo's Zoom app is in development, only accounts in its owner's Zoom organization \
             can use it."
                .into(),
        ),
        Some(404) => Error::Other("That Zoom meeting no longer exists.".into()),
        _ => e,
    }
}

pub struct Zoom {
    tokens: Arc<TokenSource>,
}

impl Zoom {
    pub fn new(tokens: Arc<TokenSource>) -> Self {
        Zoom { tokens }
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>, once: bool) -> Result<reqwest::Response> {
        let token = self.tokens.access_token().await?;
        let url = format!("{API}{path}");
        let make = || {
            let req = client().request(method.clone(), &url).bearer_auth(&token);
            match &body {
                Some(b) => req.json(b),
                None => req,
            }
        };
        let r = if once { send_once(make()).await } else { send(make).await };
        if matches!(r.as_ref().err().and_then(Error::status), Some(401)) {
            self.tokens.invalidate().await;
        }
        r.map_err(plain)
    }

    /// Create a scheduled meeting. Never retried: a retry could make two.
    pub async fn create(&self, t: &MeetingTimes<'_>) -> Result<Meeting> {
        let resp = self.call(Method::POST, "/users/me/meetings", Some(create_body(t)), true).await?;
        parse_meeting(&resp.json().await?)
    }

    pub async fn update(&self, meeting_id: &str, t: &MeetingTimes<'_>) -> Result<()> {
        self.call(Method::PATCH, &format!("/meetings/{meeting_id}"), Some(update_body(t)), false).await.map(drop)
    }

    /// Delete a meeting; one that's already gone counts as deleted.
    pub async fn delete(&self, meeting_id: &str) -> Result<()> {
        match self.call(Method::DELETE, &format!("/meetings/{meeting_id}"), None, false).await {
            Err(e) if e.to_string().contains("no longer exists") => Ok(()),
            r => r.map(drop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times() -> MeetingTimes<'static> {
        // 2026-10-23 14:00 UTC, 90 minutes.
        MeetingTimes { topic: "Design review", start_at: 1_792_764_000_000, end_at: 1_792_769_400_000, timezone: None }
    }

    #[test]
    fn create_and_update_bodies() {
        let b = create_body(&times());
        assert_eq!(b["type"], 2);
        assert_eq!(b["topic"], "Design review");
        assert_eq!(b["start_time"], "2026-10-23T14:00:00Z");
        assert_eq!(b["duration"], 90);
        assert_eq!(b["settings"]["join_before_host"], true);
        assert_eq!(b["settings"]["waiting_room"], false);
        assert!(b.get("timezone").is_none());
        let mut t = times();
        t.timezone = Some("Asia/Jerusalem");
        t.topic = "  ";
        let u = update_body(&t);
        assert_eq!(u["timezone"], "Asia/Jerusalem");
        assert_eq!(u["topic"], "Meeting");
        assert!(u.get("type").is_none() && u.get("settings").is_none());
    }

    #[test]
    fn parses_meeting_responses() {
        let v = json!({"id": 81234567890u64, "join_url": "https://us02web.zoom.us/j/81234567890?pwd=abc", "password": "x1y2"});
        let m = parse_meeting(&v).unwrap();
        assert_eq!(m.id, "81234567890");
        assert_eq!(m.display_id, "812 3456 7890");
        assert_eq!(m.passcode.as_deref(), Some("x1y2"));
        let no_pass =
            parse_meeting(&json!({"id": "123", "join_url": "https://zoom.us/j/123", "password": ""})).unwrap();
        assert_eq!((no_pass.display_id.as_str(), no_pass.passcode), ("123", None));
    }

    #[test]
    fn invite_goes_into_location_and_description() {
        let m = Meeting {
            id: "81234567890".into(),
            join_url: "https://zoom.us/j/81234567890".into(),
            display_id: "812 3456 7890".into(),
            passcode: Some("p".into()),
        };
        let (loc, desc) = add_to_event(None, None, &m);
        assert_eq!(loc, "https://zoom.us/j/81234567890");
        assert_eq!(desc, "Join Zoom meeting: https://zoom.us/j/81234567890\nMeeting ID: 812 3456 7890\nPasscode: p");
        let (loc, desc) = add_to_event(Some("חדר ישיבות 3"), Some("סדר יום: תקציב\n"), &m);
        assert_eq!(loc, "חדר ישיבות 3 · https://zoom.us/j/81234567890");
        assert!(desc.starts_with("סדר יום: תקציב\n\nJoin Zoom meeting:"), "{desc}");
        // Already there (e.g. an edit): not added twice.
        let (loc, _) = add_to_event(Some("https://zoom.us/j/81234567890"), None, &m);
        assert_eq!(loc, "https://zoom.us/j/81234567890");
        // The join link is detected for one-click join.
        let e = pigeon_core::model::Event { location: Some(loc), ..Default::default() };
        assert_eq!(pigeon_core::meeting::meeting_link(&e).unwrap().service, pigeon_core::meeting::Service::Zoom);
    }

    #[test]
    fn user_names() {
        let u: ZoomUser =
            serde_json::from_value(json!({"email": "a@x.com", "first_name": "Ada", "last_name": "L"})).unwrap();
        assert_eq!(u.name(), "Ada L");
        let u: ZoomUser = serde_json::from_value(json!({"email": "a@x.com"})).unwrap();
        assert_eq!(u.name(), "a@x.com");
    }
}
