//! CalDAV backend (RFC 4791, sync-collection RFC 6578). iCloud is the first
//! preset; nothing here is iCloud-specific beyond the discovery URL.
//!
//! Ids: calendars are collection URLs; events are resource URLs, plus
//! `#RECURRENCE-ID` for expanded instances of a recurring series.

mod xml;

use pigeon_core::http::{client, send, send_once};
use pigeon_core::ics::{self, NewVEvent, VEvent};
use pigeon_core::model::*;
use pigeon_core::provider::{CalendarProvider, CalendarWriter, EventPage};
use pigeon_core::{Error, Result, auth, now_ms};
use reqwest::{Method, RequestBuilder};
use tokio::sync::OnceCell;
use url::Url;

pub use xml::{El, Resp, multistatus};

pub const ICLOUD_URL: &str = "https://caldav.icloud.com/";
const CURSOR: &str = "sync:";

pub struct CalDav {
    base: Url,
    email: String,
    password: String,
    home: OnceCell<Url>,
}

fn method(m: &str) -> Method {
    Method::from_bytes(m.as_bytes()).expect("valid method")
}

fn stamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y%m%dT%H%M%SZ").to_string()).unwrap_or_default()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `#RRGGBBAA` (Apple) → `#RRGGBB`.
pub fn color(c: &str) -> Option<String> {
    let c = c.trim();
    (c.starts_with('#') && c.len() >= 7).then(|| c[..7].to_string())
}

/// Events stored at `href` (one resource) as our model.
pub fn to_events(href: &str, calendar_id: &str, ics_text: &str, me: &str) -> Vec<Event> {
    ics::parse_events(ics_text)
        .into_iter()
        .map(|v: VEvent| {
            let me_attending = v.attendees.iter().find(|a| a.address.email.eq_ignore_ascii_case(me));
            let organizer_is_me = v.organizer.as_ref().is_some_and(|o| o.email.eq_ignore_ascii_case(me));
            Event {
                id: match &v.recurrence_id {
                    Some(r) => format!("{href}#{r}"),
                    None => href.to_string(),
                },
                calendar_id: calendar_id.to_string(),
                series_id: v.recurrence_id.as_ref().map(|_| href.to_string()),
                title: v.summary,
                start_at: v.start_at,
                end_at: v.end_at,
                all_day: v.all_day,
                timezone: v.tzid,
                location: v.location,
                my_response: if organizer_is_me {
                    Response::Organizer
                } else {
                    me_attending.map(|a| a.response).unwrap_or_default()
                },
                attendees: v.attendees,
                organizer: v.organizer,
                online_url: v.online_url,
                cancelled: v.cancelled,
                updated_at: v.updated_at,
                ical_uid: Some(v.uid),
            }
        })
        .collect()
}

/// The resource URL of an event id (drops `#RECURRENCE-ID`).
pub fn resource_of(event_id: &str) -> (&str, Option<&str>) {
    match event_id.split_once('#') {
        Some((href, rec)) => (href, Some(rec)),
        None => (event_id, None),
    }
}

fn status_error(status: u16, what: &str) -> Error {
    match status {
        401 => Error::Auth(format!("{what}: the server rejected the password (use an app-specific password)")),
        _ => Error::Api { status, url: what.to_string(), body: String::new() },
    }
}

impl CalDav {
    pub fn new(base: &str, email: &str, password: &str) -> Result<Self> {
        Ok(CalDav {
            base: Url::parse(base).map_err(|e| Error::Other(e.to_string()))?,
            email: email.to_string(),
            password: password.to_string(),
            home: OnceCell::new(),
        })
    }

    fn req(&self, m: &str, url: &Url, depth: Option<&str>, body: Option<&str>) -> RequestBuilder {
        let mut r = client().request(method(m), url.clone()).basic_auth(&self.email, Some(&self.password));
        if let Some(d) = depth {
            r = r.header("Depth", d);
        }
        if let Some(b) = body {
            r = r.header("Content-Type", "application/xml; charset=utf-8").body(b.to_string());
        }
        r
    }

    /// A read-only DAV request (retried on throttling).
    async fn dav(&self, m: &str, url: &Url, depth: &str, body: &str) -> Result<(Vec<Resp>, Option<String>)> {
        let resp = send(|| self.req(m, url, Some(depth), Some(body))).await.map_err(|e| match e.status() {
            Some(s @ 401) => status_error(s, url.as_str()),
            _ => e,
        })?;
        multistatus(&resp.text().await?)
    }

    fn join(&self, base: &Url, href: &str) -> Result<Url> {
        base.join(href).map_err(|e| Error::Other(format!("bad href {href}: {e}")))
    }

    /// current-user-principal → calendar-home-set (RFC 6764 / 5397).
    pub async fn home(&self) -> Result<&Url> {
        self.home
            .get_or_try_init(|| async {
                let body = r#"<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
                let (r, _) = self.dav("PROPFIND", &self.base, "0", body).await?;
                let principal = r
                    .iter()
                    .find_map(|x| x.prop.find("current-user-principal").and_then(|p| p.text_of("href")))
                    .ok_or_else(|| Error::Other("CalDAV: no current-user-principal".into()))?;
                let principal = self.join(&self.base, &principal)?;
                let body = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
                let (r, _) = self.dav("PROPFIND", &principal, "0", body).await?;
                let home = r
                    .iter()
                    .find_map(|x| x.prop.find("calendar-home-set").and_then(|p| p.text_of("href")))
                    .ok_or_else(|| Error::Other("CalDAV: no calendar-home-set".into()))?;
                self.join(&principal, &home)
            })
            .await
    }

    async fn get(&self, url: &Url) -> Result<(String, Option<String>)> {
        let resp = send(|| self.req("GET", url, None, None)).await?;
        let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        Ok((resp.text().await?, etag))
    }

    async fn put(&self, url: &Url, ics_text: String, if_match: Option<&str>, create: bool) -> Result<()> {
        let mut r =
            self.req("PUT", url, None, None).header("Content-Type", "text/calendar; charset=utf-8").body(ics_text);
        if create {
            r = r.header("If-None-Match", "*");
        }
        if let Some(e) = if_match {
            r = r.header("If-Match", e);
        }
        send_once(r).await.map(drop).map_err(|e| match e.status() {
            Some(412) => Error::Other("the event changed on the server; sync and try again".into()),
            Some(s @ 401) => status_error(s, url.as_str()),
            _ => e,
        })
    }

    fn expand(&self, from: i64, to: i64) -> String {
        format!(
            r#"<d:prop><d:getetag/><c:calendar-data><c:expand start="{}" end="{}"/></c:calendar-data></d:prop>"#,
            stamp(from),
            stamp(to)
        )
    }

    fn events_in(&self, resps: &[Resp], cal: &Calendar, base: &Url, from: i64, to: i64) -> Vec<CalendarChange> {
        let mut out = Vec::new();
        for r in resps.iter().filter(|r| r.status == 200) {
            let Some(data) = r.prop.text_of("calendar-data") else { continue };
            let Ok(href) = self.join(base, &r.href) else { continue };
            out.push(CalendarChange::Delete { id: href.to_string() });
            for e in to_events(href.as_str(), &cal.id, &data, &self.email) {
                if e.start_at < to && e.end_at > from {
                    out.push(CalendarChange::Upsert(e));
                }
            }
        }
        out
    }

    async fn sync_token(&self, url: &Url) -> Result<Option<String>> {
        let body = r#"<d:propfind xmlns:d="DAV:"><d:prop><d:sync-token/></d:prop></d:propfind>"#;
        let (r, _) = self.dav("PROPFIND", url, "0", body).await?;
        Ok(r.iter().find_map(|x| x.prop.text_of("sync-token")))
    }
}

impl CalendarProvider for CalDav {
    fn calendar_concurrency(&self) -> usize {
        2
    }

    async fn calendars(&self) -> Result<Vec<Calendar>> {
        let home = self.home().await?.clone();
        let body = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
<d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><d:current-user-privilege-set/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;
        let (resps, _) = self.dav("PROPFIND", &home, "1", body).await?;
        let mut out = Vec::new();
        for r in resps.iter().filter(|r| r.status == 200) {
            let is_calendar = r.prop.find("resourcetype").is_some_and(|t| t.has("calendar"));
            // Tasks-only (VTODO) collections are skipped.
            let events = r.prop.find("supported-calendar-component-set").is_none_or(|set| {
                set.children.is_empty() || set.all("comp").any(|c| c.name_attr.as_deref() == Some("VEVENT"))
            });
            if !is_calendar || !events {
                continue;
            }
            let can_edit = r
                .prop
                .find("current-user-privilege-set")
                .is_none_or(|p| p.has("write") || p.has("write-content") || p.has("all"));
            let url = self.join(&home, &r.href)?;
            out.push(Calendar {
                id: url.to_string(),
                name: r.prop.text_of("displayname").unwrap_or_else(|| "Calendar".into()),
                color: r.prop.text_of("calendar-color").and_then(|c| color(&c)),
                is_primary: false,
                can_edit,
            });
        }
        let primary = out
            .iter()
            .position(|c| {
                c.can_edit && (c.name.eq_ignore_ascii_case("calendar") || c.name.eq_ignore_ascii_case("home"))
            })
            .or_else(|| out.iter().position(|c| c.can_edit));
        if let Some(i) = primary {
            out[i].is_primary = true;
        }
        Ok(out)
    }

    async fn event_page(&self, cal: &Calendar, from: i64, to: i64, cursor: Option<&str>) -> Result<EventPage> {
        let url = Url::parse(&cal.id).map_err(|e| Error::Other(e.to_string()))?;
        match cursor.and_then(|c| c.strip_prefix(CURSOR)) {
            None => {
                let token = self.sync_token(&url).await.unwrap_or(None);
                let body = format!(
                    r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">{}<c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
                    self.expand(from, to),
                    stamp(from),
                    stamp(to)
                );
                let (resps, _) = self.dav("REPORT", &url, "1", &body).await?;
                let changes = self.events_in(&resps, cal, &url, from, to);
                Ok(EventPage { changes, next: None, sync_cursor: token.map(|t| format!("{CURSOR}{t}")) })
            }
            Some(token) => {
                let body = format!(
                    r#"<d:sync-collection xmlns:d="DAV:"><d:sync-token>{}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>"#,
                    xml_escape(token)
                );
                let (resps, new_token) = match self.dav("REPORT", &url, "0", &body).await {
                    Err(e) if matches!(e.status(), Some(403 | 409 | 412)) => return Err(Error::CursorExpired),
                    r => r?,
                };
                let mut changes = Vec::new();
                let mut changed = Vec::new();
                for r in &resps {
                    let href = self.join(&url, &r.href)?;
                    if href == url {
                        continue;
                    }
                    if r.status == 404 {
                        changes.push(CalendarChange::Delete { id: href.to_string() });
                    } else {
                        changed.push(r.href.clone());
                    }
                }
                for chunk in changed.chunks(50) {
                    let hrefs: String = chunk.iter().map(|h| format!("<d:href>{}</d:href>", xml_escape(h))).collect();
                    let body = format!(
                        r#"<c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">{}{hrefs}</c:calendar-multiget>"#,
                        self.expand(from, to)
                    );
                    let (got, _) = self.dav("REPORT", &url, "1", &body).await?;
                    changes.extend(self.events_in(&got, cal, &url, from, to));
                }
                let next = new_token.unwrap_or_else(|| token.to_string());
                Ok(EventPage { changes, next: None, sync_cursor: Some(format!("{CURSOR}{next}")) })
            }
        }
    }
}

impl CalDav {
    fn me(&self) -> Address {
        Address::new(None, &self.email)
    }

    async fn write_event(
        &self,
        url: &Url,
        uid: &str,
        d: &EventDraft,
        organizer: Option<&Address>,
        if_match: Option<&str>,
        create: bool,
    ) -> Result<Event> {
        let attendees: Vec<Address> = d.attendees.iter().map(|a| a.address.clone()).collect();
        let me = self.me();
        let text = ics::build_calendar(&NewVEvent {
            uid,
            title: &d.title,
            start_at: d.start_at,
            end_at: d.end_at,
            all_day: d.all_day,
            location: d.location.as_deref(),
            description: d.description.as_deref(),
            organizer: Some(organizer.unwrap_or(&me)),
            attendees: &attendees,
            now: now_ms(),
        });
        self.put(url, text.clone(), if_match, create).await?;
        to_events(url.as_str(), &d.calendar_id, &text, &self.email)
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("wrote an event that doesn't parse back".into()))
    }
}

impl CalendarWriter for CalDav {
    async fn create_event(&self, draft: &EventDraft) -> Result<Event> {
        let mut collection = Url::parse(&draft.calendar_id).map_err(|e| Error::Other(e.to_string()))?;
        if !collection.path().ends_with('/') {
            collection.set_path(&format!("{}/", collection.path()));
        }
        let uid = format!("{}@pigeon", auth::random_token(16));
        let url = collection.join(&format!("{uid}.ics")).map_err(|e| Error::Other(e.to_string()))?;
        self.write_event(&url, &uid, draft, None, None, true).await
    }

    async fn update_event(&self, event_id: &str, draft: &EventDraft) -> Result<Event> {
        let (href, instance) = resource_of(event_id);
        if instance.is_some() {
            return Err(Error::Other("editing one occurrence of a repeating iCloud event isn't supported yet".into()));
        }
        let url = Url::parse(href).map_err(|e| Error::Other(e.to_string()))?;
        let (existing, etag) = self.get(&url).await?;
        let parsed = ics::parse_events(&existing);
        let first = parsed.first().ok_or_else(|| Error::Other("the event on the server is unreadable".into()))?;
        if parsed.iter().any(|v| v.has_rrule) {
            return Err(Error::Other("editing repeating iCloud events isn't supported yet".into()));
        }
        let mut d = draft.clone();
        if d.description.is_none() {
            // Unchanged in the editor: keep the server's copy.
            d.description = first.description.clone();
        }
        self.write_event(&url, &first.uid.clone(), &d, first.organizer.as_ref(), etag.as_deref(), false).await
    }

    async fn delete_event(&self, _calendar_id: &str, event_id: &str) -> Result<()> {
        let (href, instance) = resource_of(event_id);
        if instance.is_some() {
            return Err(Error::Other("deleting one occurrence of a repeating iCloud event isn't supported yet".into()));
        }
        let url = Url::parse(href).map_err(|e| Error::Other(e.to_string()))?;
        match send_once(self.req("DELETE", &url, None, None)).await {
            Ok(_) => Ok(()),
            Err(e) if matches!(e.status(), Some(404 | 410)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn respond(
        &self,
        _calendar_id: &str,
        event_id: &str,
        response: Response,
        _comment: Option<&str>,
    ) -> Result<()> {
        let (href, _) = resource_of(event_id);
        let url = Url::parse(href).map_err(|e| Error::Other(e.to_string()))?;
        let (existing, etag) = self.get(&url).await?;
        let updated = ics::set_partstat(&existing, &self.email, response)
            .ok_or_else(|| Error::Other("you're not on this event's guest list".into()))?;
        self.put(&url, updated, etag.as_deref(), false).await
    }

    async fn free_busy(&self, _emails: &[String], _from: i64, _to: i64) -> Result<FreeBusy> {
        Err(Error::Other("free/busy lookups aren't available for iCloud calendars yet".into()))
    }
}

/// Check credentials by running discovery.
pub async fn verify(base: &str, email: &str, password: &str) -> Result<()> {
    CalDav::new(base, email, password)?.home().await.map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u1\r\nRECURRENCE-ID:20261102T080000Z\r\n\
        DTSTART:20261102T080000Z\r\nDTEND:20261102T083000Z\r\nSUMMARY:Standup\r\n\
        ORGANIZER:mailto:boss@x.com\r\nATTENDEE;PARTSTAT=TENTATIVE:mailto:ME@icloud.com\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:u1\r\nRECURRENCE-ID:20261103T080000Z\r\nDTSTART:20261103T080000Z\r\n\
        DTEND:20261103T083000Z\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn expanded_instances_share_a_series() {
        let ev = to_events("https://p1-caldav.icloud.com/1/calendars/home/u1.ics", "cal", ICS, "me@icloud.com");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].id, "https://p1-caldav.icloud.com/1/calendars/home/u1.ics#20261102T080000Z");
        assert_eq!(ev[0].series_id.as_deref(), Some("https://p1-caldav.icloud.com/1/calendars/home/u1.ics"));
        assert_eq!(ev[0].my_response, Response::Tentative);
        assert_eq!(ev[0].ical_uid.as_deref(), Some("u1"));
        assert_eq!(resource_of(&ev[1].id).0, "https://p1-caldav.icloud.com/1/calendars/home/u1.ics");
        assert_eq!(color("#FF2968FF").as_deref(), Some("#FF2968"));
        assert_eq!(color("red"), None);
    }
}
