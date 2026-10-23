use chrono::DateTime;
use pigeon_core::model::{Attendee, Calendar, CalendarChange, Event, Response};
use pigeon_core::provider::{CalendarProvider, EventPage};
use pigeon_core::{Error, Result};

use crate::types::{self, GCalendar, GEvent, Page};
use crate::{BASE, Graph};

const EVENT_FIELDS: &str = "id,iCalUId,seriesMasterId,subject,start,end,isAllDay,isCancelled,originalStartTimeZone,\
    location,organizer,attendees,responseStatus,onlineMeeting,lastModifiedDateTime";

impl CalendarProvider for Graph {
    async fn calendars(&self) -> Result<Vec<Calendar>> {
        let raw: Vec<GCalendar> = self
            .get_all(&format!("{BASE}/me/calendars?$select=id,name,hexColor,isDefaultCalendar,canEdit&$top=100"))
            .await?;
        Ok(raw
            .into_iter()
            .map(|c| Calendar {
                id: c.id,
                name: c.name,
                color: c.hex_color.filter(|h| !h.is_empty()),
                is_primary: c.is_default_calendar,
                can_edit: c.can_edit,
            })
            .collect())
    }

    async fn event_page(&self, calendar: &Calendar, from: i64, to: i64, cursor: Option<&str>) -> Result<EventPage> {
        let iso = |ms: i64| {
            DateTime::from_timestamp_millis(ms)
                .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .ok_or_else(|| Error::Other(format!("bad timestamp {ms}")))
        };
        // v1.0 only offers event delta for the default calendar; the others
        // are refetched in full over the window.
        let delta = calendar.is_primary;
        let url = match cursor {
            Some(c) => c.to_string(),
            None if delta => {
                format!("{BASE}/me/calendarView/delta?startDateTime={}&endDateTime={}", iso(from)?, iso(to)?)
            }
            None => format!(
                "{BASE}/me/calendars/{}/calendarView?startDateTime={}&endDateTime={}&$select={EVENT_FIELDS}&$top=500",
                calendar.id,
                iso(from)?,
                iso(to)?
            ),
        };
        let page: Page<serde_json::Value> = self.get(&url).await.map_err(|e| match e {
            Error::Api { status: 410, .. } => Error::CursorExpired,
            Error::Api { ref body, .. } if body.contains("SyncStateNotFound") || body.contains("resyncRequired") => {
                Error::CursorExpired
            }
            e => e,
        })?;
        Ok(to_page(page, &calendar.id, delta))
    }
}

fn to_page(page: Page<serde_json::Value>, calendar_id: &str, delta: bool) -> EventPage {
    let changes = page
        .value
        .into_iter()
        .filter_map(|item| {
            if item.get("@removed").is_some() {
                return Some(CalendarChange::Delete { id: item["id"].as_str()?.to_string() });
            }
            match serde_json::from_value::<GEvent>(item) {
                Ok(e) => to_event(e, calendar_id).map(CalendarChange::Upsert),
                Err(err) => {
                    tracing::warn!(error = %err, "skipping unparseable event");
                    None
                }
            }
        })
        .collect();
    EventPage { changes, next: page.next_link, sync_cursor: if delta { page.delta_link } else { None } }
}

pub(crate) fn response(s: Option<&str>) -> Response {
    match s.unwrap_or_default() {
        "organizer" => Response::Organizer,
        "accepted" => Response::Accepted,
        "tentativelyAccepted" => Response::Tentative,
        "declined" => Response::Declined,
        _ => Response::None,
    }
}

pub(crate) fn to_event(e: GEvent, calendar_id: &str) -> Option<Event> {
    Some(Event {
        calendar_id: calendar_id.to_string(),
        series_id: e.series_master_id,
        title: e.subject.unwrap_or_default(),
        start_at: types::parse_graph_utc(&e.start.date_time)?,
        end_at: types::parse_graph_utc(&e.end.date_time)?,
        all_day: e.is_all_day,
        timezone: e.original_start_time_zone,
        location: e.location.and_then(|l| l.display_name).filter(|l| !l.is_empty()),
        organizer: e.organizer.and_then(|o| o.email_address.to_address()),
        attendees: e
            .attendees
            .into_iter()
            .filter(|a| a.kind.as_deref() != Some("resource"))
            .filter_map(|a| {
                Some(Attendee {
                    address: a.email_address.to_address()?,
                    response: response(a.status.as_ref().and_then(|s| s.response.as_deref())),
                    optional: a.kind.as_deref() == Some("optional"),
                })
            })
            .collect(),
        my_response: response(e.response_status.as_ref().and_then(|s| s.response.as_deref())),
        online_url: e.online_meeting.and_then(|m| m.join_url),
        cancelled: e.is_cancelled,
        updated_at: e.last_modified_date_time.as_deref().and_then(types::parse_rfc3339).unwrap_or(0),
        ical_uid: e.ical_uid,
        id: e.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_event() {
        let json = r#"{
          "id": "E1", "seriesMasterId": "S1", "subject": "Standup",
          "start": {"dateTime": "2026-10-23T09:00:00.0000000", "timeZone": "UTC"},
          "end": {"dateTime": "2026-10-23T09:15:00.0000000", "timeZone": "UTC"},
          "isAllDay": false, "isCancelled": false, "originalStartTimeZone": "Israel Standard Time",
          "location": {"displayName": ""},
          "organizer": {"emailAddress": {"name": "Boss", "address": "boss@x.com"}},
          "attendees": [
            {"emailAddress": {"name": "Ann", "address": "ann@x.com"}, "status": {"response": "tentativelyAccepted"}, "type": "optional"},
            {"emailAddress": {"name": "Room 1", "address": "room1@x.com"}, "status": {"response": "accepted"}, "type": "resource"}
          ],
          "responseStatus": {"response": "accepted"},
          "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup-join/abc"},
          "lastModifiedDateTime": "2026-10-20T10:00:00Z"
        }"#;
        let e = to_event(serde_json::from_str(json).unwrap(), "cal").unwrap();
        assert_eq!(e.end_at - e.start_at, 15 * 60 * 1000);
        assert_eq!(e.location, None);
        assert_eq!(e.attendees.len(), 1);
        assert_eq!(e.attendees[0].response, Response::Tentative);
        assert!(e.attendees[0].optional);
        assert_eq!(e.my_response, Response::Accepted);
        assert_eq!(e.series_id.as_deref(), Some("S1"));
        assert!(e.online_url.unwrap().contains("teams"));
    }

    #[test]
    fn delta_pages() {
        let page: Page<serde_json::Value> = serde_json::from_str(
            r#"{"@odata.deltaLink": "https://graph/delta?$deltatoken=D", "value": [
                 {"id": "gone", "@removed": {"reason": "deleted"}},
                 {"id": "E2", "subject": "X",
                  "start": {"dateTime": "2026-10-23T09:00:00.0000000", "timeZone": "UTC"},
                  "end": {"dateTime": "2026-10-23T10:00:00.0000000", "timeZone": "UTC"}},
                 {"id": "partial"}
               ]}"#,
        )
        .unwrap();
        let p = to_page(page, "cal", true);
        assert_eq!(p.changes.len(), 2, "unparseable items are skipped");
        assert_eq!(p.changes[0], CalendarChange::Delete { id: "gone".into() });
        assert_eq!((p.next, p.sync_cursor.as_deref()), (None, Some("https://graph/delta?$deltatoken=D")));

        let page: Page<serde_json::Value> =
            serde_json::from_str(r#"{"@odata.nextLink": "https://graph/next", "value": []}"#).unwrap();
        let p = to_page(page, "cal", false);
        assert_eq!((p.next.as_deref(), p.sync_cursor), (Some("https://graph/next"), None));
    }
}
