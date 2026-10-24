//! Calendar writes (insert/patch/delete with `sendUpdates=all`), RSVP, and
//! free/busy.

use chrono::DateTime;
use pigeon_core::http::{client, send, send_once};
use pigeon_core::model::{Conferencing, Event, EventDraft, FreeBusy, Response};
use pigeon_core::provider::CalendarWriter;
use pigeon_core::{Error, Result};
use reqwest::{Method, Response as HttpResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::calendar::{GEvent, events_url, to_event};
use crate::{CALENDAR, Google};

fn rfc3339(ms: i64) -> Result<String> {
    DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339())
        .ok_or_else(|| Error::Other(format!("bad timestamp {ms}")))
}

fn date(ms: i64) -> Result<String> {
    DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .ok_or_else(|| Error::Other(format!("bad timestamp {ms}")))
}

/// Calendar API event resource for a draft.
fn event_body(d: &EventDraft) -> Result<Value> {
    let when = |ms: i64| -> Result<Value> {
        Ok(if d.all_day {
            json!({ "date": date(ms)? })
        } else {
            let mut v = json!({ "dateTime": rfc3339(ms)? });
            if let Some(tz) = &d.timezone {
                v["timeZone"] = json!(tz);
            }
            v
        })
    };
    let attendees: Vec<Value> = d
        .attendees
        .iter()
        .map(|a| {
            let mut v = json!({ "email": a.address.email, "optional": a.optional });
            if let Some(n) = &a.address.name {
                v["displayName"] = json!(n);
            }
            v
        })
        .collect();
    let mut body = json!({
        "summary": d.title,
        "start": when(d.start_at)?,
        "end": when(d.end_at)?,
        "location": d.location.clone().unwrap_or_default(),
        "attendees": attendees,
    });
    // Ask Google to create a Meet link for this event (needs
    // conferenceDataVersion=1 on the request).
    if d.conferencing == Conferencing::GoogleMeet {
        body["conferenceData"] = json!({
            "createRequest": {
                "requestId": pigeon_core::auth::random_token(16),
                "conferenceSolutionKey": { "type": "hangoutsMeet" },
            }
        });
    }
    // No description means "leave it as is": we don't store descriptions
    // locally, so a patch must never blank the server's copy.
    if let Some(text) = &d.description {
        body["description"] = json!(text);
    }
    Ok(body)
}

fn rsvp_status(r: Response) -> Result<&'static str> {
    Ok(match r {
        Response::Accepted => "accepted",
        Response::Tentative => "tentative",
        Response::Declined => "declined",
        _ => return Err(Error::Other("RSVP must be accepted, tentative or declined".into())),
    })
}

/// Set our own attendee's status (and note) in an event's attendee list.
fn set_self_response(event: &Value, status: &str, comment: Option<&str>) -> Result<Value> {
    let mut attendees = event["attendees"].as_array().cloned().unwrap_or_default();
    let me = attendees
        .iter_mut()
        .find(|a| a["self"].as_bool() == Some(true))
        .ok_or_else(|| Error::Other("you are not an attendee of this event".into()))?;
    me["responseStatus"] = json!(status);
    if let Some(c) = comment {
        me["comment"] = json!(c);
    }
    Ok(json!({ "attendees": attendees }))
}

#[derive(Deserialize)]
struct FreeBusyResponse {
    #[serde(default)]
    calendars: serde_json::Map<String, Value>,
}

fn to_free_busy(resp: FreeBusyResponse, emails: &[String]) -> FreeBusy {
    let parse = |v: &Value| v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis());
    emails
        .iter()
        .map(|email| {
            let busy = resp
                .calendars
                .get(email)
                .and_then(|c| c["busy"].as_array())
                .map(|list| list.iter().filter_map(|b| Some((parse(&b["start"])?, parse(&b["end"])?))).collect())
                .unwrap_or_default();
            (email.clone(), busy)
        })
        .collect()
}

impl Google {
    async fn write(&self, method: Method, url: Url, body: Option<&Value>, once: bool) -> Result<HttpResponse> {
        let token = self.tokens.access_token().await?;
        let make = || {
            let req = client().request(method.clone(), url.clone()).bearer_auth(&token);
            match body {
                Some(b) => req.json(b),
                None => req,
            }
        };
        if once { send_once(make()).await } else { send(make).await }
    }

    fn event_url(calendar_id: &str, event_id: Option<&str>) -> Url {
        let mut url = events_url(calendar_id);
        if let Some(id) = event_id {
            url.path_segments_mut().expect("base url").push(id);
        }
        url.query_pairs_mut().append_pair("sendUpdates", "all");
        url
    }

    /// `event_url`, plus `conferenceDataVersion=1` when a Meet link is requested
    /// (without it Google silently ignores conferenceData).
    fn write_url(draft: &EventDraft, event_id: Option<&str>) -> Url {
        let mut url = Self::event_url(&draft.calendar_id, event_id);
        if draft.conferencing == Conferencing::GoogleMeet {
            url.query_pairs_mut().append_pair("conferenceDataVersion", "1");
        }
        url
    }

    async fn event_from(&self, resp: HttpResponse, calendar_id: &str) -> Result<Event> {
        let raw: GEvent = resp.json().await?;
        to_event(raw, calendar_id).ok_or_else(|| Error::Other("server returned an event without times".into()))
    }
}

impl CalendarWriter for Google {
    async fn create_event(&self, draft: &EventDraft) -> Result<Event> {
        let body = event_body(draft)?;
        // Not idempotent: never retried (a retry could double-book).
        let resp = self.write(Method::POST, Self::write_url(draft, None), Some(&body), true).await?;
        self.event_from(resp, &draft.calendar_id).await
    }

    async fn update_event(&self, event_id: &str, draft: &EventDraft) -> Result<Event> {
        let body = event_body(draft)?;
        let resp = self.write(Method::PATCH, Self::write_url(draft, Some(event_id)), Some(&body), false).await?;
        self.event_from(resp, &draft.calendar_id).await
    }

    async fn delete_event(&self, calendar_id: &str, event_id: &str) -> Result<()> {
        match self.write(Method::DELETE, Self::event_url(calendar_id, Some(event_id)), None, false).await {
            Err(e) if matches!(e.status(), Some(404 | 410)) => Ok(()),
            r => r.map(drop),
        }
    }

    async fn respond(
        &self,
        calendar_id: &str,
        event_id: &str,
        response: Response,
        comment: Option<&str>,
    ) -> Result<()> {
        let status = rsvp_status(response)?;
        let mut get_url = events_url(calendar_id);
        get_url.path_segments_mut().expect("base url").push(event_id);
        let event: Value = self.get(get_url.as_str(), &[]).await?;
        let body = set_self_response(&event, status, comment)?;
        self.write(Method::PATCH, Self::event_url(calendar_id, Some(event_id)), Some(&body), false).await.map(drop)
    }

    async fn free_busy(&self, emails: &[String], from: i64, to: i64) -> Result<FreeBusy> {
        let body = json!({
            "timeMin": rfc3339(from)?,
            "timeMax": rfc3339(to)?,
            "items": emails.iter().map(|e| json!({ "id": e })).collect::<Vec<_>>(),
        });
        let url = Url::parse(&format!("{CALENDAR}/freeBusy")).expect("static url");
        let resp: FreeBusyResponse = self.write(Method::POST, url, Some(&body), false).await?.json().await?;
        Ok(to_free_busy(resp, emails))
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Address, Attendee};

    #[test]
    fn meet_request_only_when_asked() {
        use super::*;
        let mut d = EventDraft {
            calendar_id: "primary".into(),
            title: "Sync".into(),
            start_at: 0,
            end_at: 3_600_000,
            ..Default::default()
        };
        assert!(event_body(&d).unwrap().get("conferenceData").is_none());
        assert!(!Google::write_url(&d, None).as_str().contains("conferenceDataVersion"));
        d.conferencing = Conferencing::GoogleMeet;
        let b = event_body(&d).unwrap();
        assert_eq!(b["conferenceData"]["createRequest"]["conferenceSolutionKey"]["type"], "hangoutsMeet");
        assert!(b["conferenceData"]["createRequest"]["requestId"].as_str().is_some_and(|r| !r.is_empty()));
        assert!(Google::write_url(&d, None).as_str().contains("conferenceDataVersion=1"));
    }

    use super::*;

    fn draft() -> EventDraft {
        EventDraft {
            calendar_id: "primary".into(),
            title: "Sync".into(),
            start_at: 0,
            end_at: 30 * 60_000,
            timezone: Some("Asia/Jerusalem".into()),
            attendees: vec![Attendee {
                address: Address::new(Some("Ann"), "ann@x.com"),
                response: Response::None,
                optional: true,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn bodies_for_timed_and_all_day() {
        let b = event_body(&draft()).unwrap();
        assert_eq!(b["start"], json!({"dateTime": "1970-01-01T00:00:00+00:00", "timeZone": "Asia/Jerusalem"}));
        assert_eq!(b["attendees"][0], json!({"email": "ann@x.com", "displayName": "Ann", "optional": true}));
        let mut d = draft();
        d.all_day = true;
        d.end_at = 86_400_000;
        let b = event_body(&d).unwrap();
        assert_eq!(
            (b["start"].clone(), b["end"].clone()),
            (json!({"date": "1970-01-01"}), json!({"date": "1970-01-02"}))
        );
    }

    #[test]
    fn description_only_sent_when_given() {
        let mut d = draft();
        d.description = None;
        assert!(event_body(&d).unwrap().get("description").is_none(), "unchanged description must not be sent");
        d.description = Some("Agenda".into());
        assert_eq!(event_body(&d).unwrap()["description"], "Agenda");
    }

    #[test]
    fn urls_are_encoded_and_notify_guests() {
        assert_eq!(
            Google::event_url("a#b@group.calendar.google.com", Some("ev1")).as_str(),
            "https://www.googleapis.com/calendar/v3/calendars/a%23b@group.calendar.google.com/events/ev1?sendUpdates=all"
        );
    }

    #[test]
    fn rsvp_patches_only_self() {
        let event = json!({"attendees": [
            {"email": "boss@x.com", "organizer": true, "responseStatus": "accepted"},
            {"email": "me@gmail.com", "self": true, "responseStatus": "needsAction"}
        ]});
        let body = set_self_response(&event, "declined", Some("conflict")).unwrap();
        assert_eq!(body["attendees"][0]["responseStatus"], "accepted");
        assert_eq!(body["attendees"][1]["responseStatus"], "declined");
        assert_eq!(body["attendees"][1]["comment"], "conflict");
        assert!(set_self_response(&json!({"attendees": []}), "declined", None).is_err());
        assert!(rsvp_status(Response::Organizer).is_err());
    }

    #[test]
    fn free_busy_keeps_request_order() {
        let resp: FreeBusyResponse = serde_json::from_value(json!({"calendars": {
            "b@x.com": {"busy": [{"start": "1970-01-01T00:00:00Z", "end": "1970-01-01T01:00:00Z"}]},
            "a@x.com": {"errors": [{"reason": "notFound"}]}
        }}))
        .unwrap();
        let fb = to_free_busy(resp, &["a@x.com".into(), "b@x.com".into()]);
        assert_eq!(fb, vec![("a@x.com".into(), vec![]), ("b@x.com".into(), vec![(0, 3_600_000)])]);
    }
}
