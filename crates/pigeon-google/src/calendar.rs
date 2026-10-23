use chrono::{DateTime, NaiveDate};
use pigeon_core::model::{Address, Attendee, Calendar, CalendarChange, Event, Response};
use pigeon_core::provider::{CalendarProvider, EventPage};
use pigeon_core::{Error, Result};
use serde::Deserialize;
use url::Url;

use crate::{CALENDAR, Google};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEntry {
    id: String,
    summary: Option<String>,
    summary_override: Option<String>,
    background_color: Option<String>,
    #[serde(default)]
    primary: bool,
    access_role: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct When {
    date_time: Option<String>,
    date: Option<String>,
    time_zone: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Person {
    email: Option<String>,
    display_name: Option<String>,
    #[serde(rename = "self", default)]
    is_self: bool,
    response_status: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    resource: bool,
}

impl Person {
    fn address(&self) -> Option<Address> {
        Some(Address::new(self.display_name.as_deref(), self.email.as_deref()?))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryPoint {
    entry_point_type: Option<String>,
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConferenceData {
    #[serde(default)]
    entry_points: Vec<EntryPoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GEvent {
    id: String,
    recurring_event_id: Option<String>,
    summary: Option<String>,
    status: Option<String>,
    start: Option<When>,
    end: Option<When>,
    location: Option<String>,
    organizer: Option<Person>,
    #[serde(default)]
    attendees: Vec<Person>,
    hangout_link: Option<String>,
    conference_data: Option<ConferenceData>,
    updated: Option<String>,
    #[serde(rename = "iCalUID")]
    ical_uid: Option<String>,
}

impl CalendarProvider for Google {
    /// Calendars are independent sync tokens; a few at once is well under
    /// Google's per-user rate limit.
    fn calendar_concurrency(&self) -> usize {
        3
    }

    async fn calendars(&self) -> Result<Vec<Calendar>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![("maxResults", "250".to_string())];
            if let Some(t) = &token {
                query.push(("pageToken", t.clone()));
            }
            let page: Page<ListEntry> = self.get(&format!("{CALENDAR}/users/me/calendarList"), &query).await?;
            out.extend(page.items.into_iter().map(|c| Calendar {
                name: c.summary_override.or(c.summary).unwrap_or_else(|| c.id.clone()),
                color: c.background_color,
                is_primary: c.primary,
                can_edit: matches!(c.access_role.as_deref(), Some("owner" | "writer")),
                id: c.id,
            }));
            match page.next_page_token {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }

    async fn event_page(&self, calendar: &Calendar, from: i64, to: i64, cursor: Option<&str>) -> Result<EventPage> {
        let rfc3339 = |ms: i64| {
            DateTime::from_timestamp_millis(ms)
                .map(|d| d.to_rfc3339())
                .ok_or_else(|| Error::Other(format!("bad timestamp {ms}")))
        };
        let mut query = vec![("singleEvents", "true".to_string()), ("maxResults", "2500".to_string())];
        match Cursor::parse(cursor) {
            Cursor::Fresh => {
                query.push(("timeMin", rfc3339(from)?));
                query.push(("timeMax", rfc3339(to)?));
            }
            Cursor::WindowPage(t) => {
                query.push(("timeMin", rfc3339(from)?));
                query.push(("timeMax", rfc3339(to)?));
                query.push(("pageToken", t.to_string()));
            }
            // A sync token can't be combined with timeMin/timeMax.
            Cursor::Sync(t) => query.push(("syncToken", t.to_string())),
            Cursor::SyncPage(t) => query.push(("pageToken", t.to_string())),
        }
        let incremental = matches!(Cursor::parse(cursor), Cursor::Sync(_) | Cursor::SyncPage(_));
        let page: Page<GEvent> = match self.get(events_url(&calendar.id).as_str(), &query).await {
            Err(e) if e.status() == Some(410) => return Err(Error::CursorExpired),
            r => r?,
        };
        Ok(to_page(page, &calendar.id, incremental))
    }
}

/// Cursor encoding: `wpage:` paging a windowed snapshot, `sync:` an
/// incremental sync token, `spage:` paging incremental results.
#[derive(Debug, PartialEq, Eq)]
enum Cursor<'a> {
    Fresh,
    WindowPage(&'a str),
    Sync(&'a str),
    SyncPage(&'a str),
}

impl<'a> Cursor<'a> {
    fn parse(s: Option<&'a str>) -> Self {
        let Some(s) = s else { return Cursor::Fresh };
        match s.split_once(':') {
            Some(("wpage", t)) => Cursor::WindowPage(t),
            Some(("spage", t)) => Cursor::SyncPage(t),
            Some(("sync", t)) => Cursor::Sync(t),
            // Unknown: start over.
            _ => Cursor::Fresh,
        }
    }
}

fn to_page(page: Page<GEvent>, calendar_id: &str, incremental: bool) -> EventPage {
    let changes = page
        .items
        .into_iter()
        .filter_map(|e| {
            if e.status.as_deref() == Some("cancelled") {
                return Some(CalendarChange::Delete { id: e.id });
            }
            to_event(e, calendar_id).map(CalendarChange::Upsert)
        })
        .collect();
    let next = page.next_page_token.map(|t| if incremental { format!("spage:{t}") } else { format!("wpage:{t}") });
    EventPage { changes, next, sync_cursor: page.next_sync_token.map(|t| format!("sync:{t}")) }
}

/// Calendar ids contain `@` and `#`; build the path segment-encoded.
pub(crate) fn events_url(calendar_id: &str) -> Url {
    let mut url = Url::parse(CALENDAR).expect("static url");
    url.path_segments_mut().expect("base url").extend(["calendars", calendar_id, "events"]);
    url
}

fn when_ms(w: &When) -> Option<(i64, bool)> {
    if let Some(dt) = &w.date_time {
        return DateTime::parse_from_rfc3339(dt).ok().map(|d| (d.timestamp_millis(), false));
    }
    let date = NaiveDate::parse_from_str(w.date.as_deref()?, "%Y-%m-%d").ok()?;
    Some((date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis(), true))
}

fn response(s: Option<&str>) -> Response {
    match s.unwrap_or_default() {
        "accepted" => Response::Accepted,
        "tentative" => Response::Tentative,
        "declined" => Response::Declined,
        _ => Response::None,
    }
}

pub(crate) fn to_event(e: GEvent, calendar_id: &str) -> Option<Event> {
    let (start_at, all_day) = when_ms(e.start.as_ref()?)?;
    let (end_at, _) = when_ms(e.end.as_ref()?)?;
    let my_response = if e.organizer.as_ref().is_some_and(|o| o.is_self) {
        Response::Organizer
    } else {
        response(e.attendees.iter().find(|a| a.is_self).and_then(|a| a.response_status.as_deref()))
    };
    let online_url = e.hangout_link.clone().or_else(|| {
        e.conference_data?
            .entry_points
            .into_iter()
            .find(|p| p.entry_point_type.as_deref() == Some("video"))
            .and_then(|p| p.uri)
    });
    Some(Event {
        calendar_id: calendar_id.to_string(),
        series_id: e.recurring_event_id,
        title: e.summary.unwrap_or_default(),
        start_at,
        end_at,
        all_day,
        timezone: e.start.and_then(|s| s.time_zone),
        location: e.location.filter(|l| !l.is_empty()),
        organizer: e.organizer.as_ref().and_then(Person::address),
        attendees: e
            .attendees
            .iter()
            .filter(|a| !a.resource)
            .filter_map(|a| {
                Some(Attendee {
                    address: a.address()?,
                    response: response(a.response_status.as_deref()),
                    optional: a.optional,
                })
            })
            .collect(),
        my_response,
        online_url,
        cancelled: e.status.as_deref() == Some("cancelled"),
        updated_at: e
            .updated
            .as_deref()
            .and_then(|u| DateTime::parse_from_rfc3339(u).ok())
            .map_or(0, |d| d.timestamp_millis()),
        ical_uid: e.ical_uid,
        id: e.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_ids_are_path_encoded() {
        assert_eq!(
            events_url("en.il#holiday@group.v.calendar.google.com").as_str(),
            "https://www.googleapis.com/calendar/v3/calendars/en.il%23holiday@group.v.calendar.google.com/events"
        );
    }

    #[test]
    fn timed_event_with_meet() {
        let e: GEvent = serde_json::from_str(
            r#"{
              "id": "e1", "summary": "Sync", "status": "confirmed",
              "start": {"dateTime": "2026-10-23T12:00:00+03:00", "timeZone": "Asia/Jerusalem"},
              "end": {"dateTime": "2026-10-23T12:30:00+03:00"},
              "organizer": {"email": "boss@x.com"},
              "attendees": [
                {"email": "me@gmail.com", "self": true, "responseStatus": "tentative"},
                {"email": "room@resource.calendar.google.com", "resource": true, "responseStatus": "accepted"}
              ],
              "conferenceData": {"entryPoints": [
                {"entryPointType": "phone", "uri": "tel:+1"},
                {"entryPointType": "video", "uri": "https://meet.google.com/abc-defg-hij"}
              ]},
              "updated": "2026-10-20T10:00:00.000Z"
            }"#,
        )
        .unwrap();
        let ev = to_event(e, "primary").unwrap();
        assert!(!ev.all_day);
        assert_eq!(ev.end_at - ev.start_at, 30 * 60 * 1000);
        assert_eq!(ev.my_response, Response::Tentative);
        assert_eq!(ev.attendees.len(), 1);
        assert_eq!(ev.timezone.as_deref(), Some("Asia/Jerusalem"));
        assert_eq!(ev.online_url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
    }

    #[test]
    fn all_day_event_owned_by_me() {
        let e: GEvent = serde_json::from_str(
            r#"{"id": "e2", "start": {"date": "2026-10-23"}, "end": {"date": "2026-10-24"},
                "organizer": {"email": "me@gmail.com", "self": true}, "status": "cancelled"}"#,
        )
        .unwrap();
        let ev = to_event(e, "primary").unwrap();
        assert!(ev.all_day && ev.cancelled);
        assert_eq!(ev.end_at - ev.start_at, 86_400_000);
        assert_eq!(ev.my_response, Response::Organizer);
    }

    #[test]
    fn cursors_and_pages() {
        assert_eq!(Cursor::parse(None), Cursor::Fresh);
        assert_eq!(Cursor::parse(Some("sync:abc")), Cursor::Sync("abc"));
        assert_eq!(Cursor::parse(Some("spage:p:1")), Cursor::SyncPage("p:1"));
        assert_eq!(Cursor::parse(Some("garbage")), Cursor::Fresh);

        let page: Page<GEvent> = serde_json::from_str(
            r#"{"items": [
                  {"id": "x", "status": "cancelled"},
                  {"id": "y", "summary": "Y", "start": {"date": "2026-10-23"}, "end": {"date": "2026-10-24"}}
                ],
                "nextSyncToken": "T"}"#,
        )
        .unwrap();
        let p = to_page(page, "primary", true);
        assert_eq!(p.changes[0], CalendarChange::Delete { id: "x".into() });
        assert!(matches!(&p.changes[1], CalendarChange::Upsert(e) if e.id == "y"));
        assert_eq!((p.next, p.sync_cursor.as_deref()), (None, Some("sync:T")));

        let page: Page<GEvent> = serde_json::from_str(r#"{"items": [], "nextPageToken": "N"}"#).unwrap();
        assert_eq!(to_page(page, "primary", false).next.as_deref(), Some("wpage:N"));
    }
}
