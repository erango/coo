//! Calendar writes, RSVP (`accept` / `tentativelyAccept` / `decline`), and
//! free/busy via `getSchedule`.

use chrono::DateTime;
use pigeon_core::http::{client, send, send_once};
use pigeon_core::model::{Event, EventDraft, FreeBusy, Response};
use pigeon_core::provider::CalendarWriter;
use pigeon_core::{Error, Result};
use reqwest::{Method, Response as HttpResponse};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::calendar::to_event;
use crate::types::{self, GEvent};
use crate::{BASE, Graph, PREFER};

/// Graph `dateTimeTimeZone` wall-clock string for a UTC instant.
fn wall_clock(ms: i64) -> Result<String> {
    DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
        .ok_or_else(|| Error::Other(format!("bad timestamp {ms}")))
}

fn event_body(d: &EventDraft) -> Result<Value> {
    // Timed events go out in UTC (Outlook shows them in each viewer's zone).
    // All-day events are midnight-to-midnight in the authoring zone.
    let tz = if d.all_day { d.timezone.clone().unwrap_or_else(|| "UTC".into()) } else { "UTC".into() };
    let attendees: Vec<Value> = d
        .attendees
        .iter()
        .map(|a| {
            json!({
                "emailAddress": { "address": a.address.email, "name": a.address.name.clone().unwrap_or_default() },
                "type": if a.optional { "optional" } else { "required" },
            })
        })
        .collect();
    Ok(json!({
        "subject": d.title,
        "body": { "contentType": "text", "content": d.description.clone().unwrap_or_default() },
        "start": { "dateTime": wall_clock(d.start_at)?, "timeZone": tz },
        "end": { "dateTime": wall_clock(d.end_at)?, "timeZone": tz },
        "isAllDay": d.all_day,
        "location": { "displayName": d.location.clone().unwrap_or_default() },
        "attendees": attendees,
    }))
}

fn rsvp_action(r: Response) -> Result<&'static str> {
    Ok(match r {
        Response::Accepted => "accept",
        Response::Tentative => "tentativelyAccept",
        Response::Declined => "decline",
        _ => return Err(Error::Other("RSVP must be accepted, tentative or declined".into())),
    })
}

#[derive(Deserialize)]
struct Schedules {
    #[serde(default)]
    value: Vec<Schedule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    schedule_id: String,
    #[serde(default)]
    schedule_items: Vec<ScheduleItem>,
}

#[derive(Deserialize)]
struct ScheduleItem {
    status: Option<String>,
    start: types::DateTimeTz,
    end: types::DateTimeTz,
}

fn to_free_busy(s: Schedules, emails: &[String]) -> FreeBusy {
    emails
        .iter()
        .map(|email| {
            let busy = s
                .value
                .iter()
                .find(|x| x.schedule_id.eq_ignore_ascii_case(email))
                .map(|x| {
                    x.schedule_items
                        .iter()
                        .filter(|i| !matches!(i.status.as_deref(), Some("free" | "workingElsewhere")))
                        .filter_map(|i| {
                            Some((
                                types::parse_graph_utc(&i.start.date_time)?,
                                types::parse_graph_utc(&i.end.date_time)?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            (email.clone(), busy)
        })
        .collect()
}

impl Graph {
    async fn write(&self, method: Method, url: &str, body: Option<&Value>, once: bool) -> Result<HttpResponse> {
        let token = self.tokens.access_token().await?;
        let make = || {
            let req = client().request(method.clone(), url).bearer_auth(&token).header("Prefer", PREFER);
            match body {
                Some(b) => req.json(b),
                None => req,
            }
        };
        if once { send_once(make()).await } else { send(make).await }
    }

    async fn event_from(resp: HttpResponse, calendar_id: &str) -> Result<Event> {
        let raw: GEvent = resp.json().await?;
        to_event(raw, calendar_id).ok_or_else(|| Error::Other("server returned an event without times".into()))
    }
}

impl CalendarWriter for Graph {
    async fn create_event(&self, draft: &EventDraft) -> Result<Event> {
        let url = format!("{BASE}/me/calendars/{}/events", draft.calendar_id);
        // Not idempotent: never retried (a retry could double-book).
        let resp = self.write(Method::POST, &url, Some(&event_body(draft)?), true).await?;
        Self::event_from(resp, &draft.calendar_id).await
    }

    async fn update_event(&self, event_id: &str, draft: &EventDraft) -> Result<Event> {
        let url = format!("{BASE}/me/events/{event_id}");
        let resp = self.write(Method::PATCH, &url, Some(&event_body(draft)?), false).await?;
        Self::event_from(resp, &draft.calendar_id).await
    }

    async fn delete_event(&self, _calendar_id: &str, event_id: &str) -> Result<()> {
        match self.write(Method::DELETE, &format!("{BASE}/me/events/{event_id}"), None, false).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop),
        }
    }

    async fn respond(
        &self,
        _calendar_id: &str,
        event_id: &str,
        response: Response,
        comment: Option<&str>,
    ) -> Result<()> {
        let url = format!("{BASE}/me/events/{event_id}/{}", rsvp_action(response)?);
        let body = json!({ "comment": comment.unwrap_or_default(), "sendResponse": true });
        self.write(Method::POST, &url, Some(&body), false).await.map(drop)
    }

    async fn free_busy(&self, emails: &[String], from: i64, to: i64) -> Result<FreeBusy> {
        let body = json!({
            "schedules": emails,
            "startTime": { "dateTime": wall_clock(from)?, "timeZone": "UTC" },
            "endTime": { "dateTime": wall_clock(to)?, "timeZone": "UTC" },
            "availabilityViewInterval": 30,
        });
        let resp = self.write(Method::POST, &format!("{BASE}/me/calendar/getSchedule"), Some(&body), false).await?;
        Ok(to_free_busy(resp.json().await?, emails))
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Address, Attendee};

    use super::*;

    #[test]
    fn event_bodies() {
        let mut d = EventDraft {
            calendar_id: "cal".into(),
            title: "Review".into(),
            start_at: 0,
            end_at: 3_600_000,
            timezone: Some("Israel Standard Time".into()),
            attendees: vec![Attendee {
                address: Address::new(None, "a@x.com"),
                response: Response::None,
                optional: false,
            }],
            ..Default::default()
        };
        let b = event_body(&d).unwrap();
        assert_eq!(b["start"], json!({"dateTime": "1970-01-01T00:00:00", "timeZone": "UTC"}));
        assert_eq!(b["attendees"][0]["type"], "required");
        d.all_day = true;
        d.end_at = 86_400_000;
        let b = event_body(&d).unwrap();
        assert_eq!(b["end"], json!({"dateTime": "1970-01-02T00:00:00", "timeZone": "Israel Standard Time"}));
        assert_eq!(b["isAllDay"], true);
    }

    #[test]
    fn rsvp_actions() {
        assert_eq!(rsvp_action(Response::Tentative).unwrap(), "tentativelyAccept");
        assert!(rsvp_action(Response::None).is_err());
    }

    #[test]
    fn schedules_to_busy() {
        let s: Schedules = serde_json::from_value(json!({"value": [
            {"scheduleId": "A@x.com", "scheduleItems": [
                {"status": "busy", "start": {"dateTime": "1970-01-01T00:00:00.0000000", "timeZone": "UTC"},
                 "end": {"dateTime": "1970-01-01T01:00:00.0000000", "timeZone": "UTC"}},
                {"status": "free", "start": {"dateTime": "1970-01-01T02:00:00.0000000", "timeZone": "UTC"},
                 "end": {"dateTime": "1970-01-01T03:00:00.0000000", "timeZone": "UTC"}}
            ]}
        ]}))
        .unwrap();
        let fb = to_free_busy(s, &["a@x.com".into(), "b@x.com".into()]);
        assert_eq!(fb, vec![("a@x.com".into(), vec![(0, 3_600_000)]), ("b@x.com".into(), vec![])]);
    }
}
