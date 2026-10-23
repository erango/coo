//! Just enough iCalendar (RFC 5545) to read a meeting invite: the first
//! VEVENT's UID, summary, times, location, organizer and meeting link, plus
//! the calendar METHOD.

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};

use crate::model::{Address, Attendee, Invite, InviteMethod, Response};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

/// Undo RFC 5545 line folding (CRLF/LF followed by a space or tab).
fn unfold(text: &str) -> String {
    text.replace("\r\n ", "").replace("\r\n\t", "").replace("\n ", "").replace("\n\t", "")
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

struct Prop<'a> {
    name: String,
    params: Vec<(String, String)>,
    value: &'a str,
}

impl Prop<'_> {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

/// `NAME;P1=a;P2="b:c":value` → parts; the value starts at the first colon
/// outside quotes.
fn prop(line: &str) -> Option<Prop<'_>> {
    let mut quoted = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ':' && !quoted
    })?;
    let (head, value) = (&line[..colon.0], &line[colon.0 + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value })
}

/// (epoch ms, all_day) for a DTSTART/DTEND value.
fn time(p: &Prop) -> Option<(i64, bool)> {
    let v = p.value.trim();
    if p.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        return Some((d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis(), true));
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((t.and_utc().timestamp_millis(), false));
    }
    let t = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let zoned = p
        .param("TZID")
        .and_then(|tz| tz.trim_start_matches('/').parse::<chrono_tz::Tz>().ok())
        .and_then(|tz| tz.from_local_datetime(&t).earliest().map(|d| d.timestamp_millis()));
    // Unknown zone names (often Windows ones from Outlook) or floating
    // times: read them as local time, which is right for most invites.
    let ms = zoned.or_else(|| Local.from_local_datetime(&t).earliest().map(|d| d.timestamp_millis()))?;
    Some((ms, false))
}

/// `PT1H30M` / `P1D` → ms.
fn duration(v: &str) -> Option<i64> {
    let v = v.trim().trim_start_matches('+').strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (0i64, String::new(), false);
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', _) => 7 * DAY_MS,
                    ('D', _) => DAY_MS,
                    ('H', true) => HOUR_MS,
                    ('M', true) => 60_000,
                    ('S', true) => 1_000,
                    _ => return None,
                };
            }
        }
    }
    Some(total)
}

/// First Zoom / Meet / Teams link in some text.
pub fn meeting_link(text: &str) -> Option<String> {
    const HOSTS: [&str; 4] =
        ["zoom.us/j/", "meet.google.com/", "teams.microsoft.com/l/meetup-join", "teams.live.com/meet"];
    let mut rest = text;
    while let Some(i) = rest.find("https://") {
        let url: String = rest[i..]
            .chars()
            .take_while(|c| !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | ')' | '\\'))
            .collect();
        if HOSTS.iter().any(|h| url.contains(h)) {
            return Some(url);
        }
        rest = &rest[i + 8..];
    }
    None
}

/// Parse an invite from iCalendar text. None if there's no usable VEVENT.
pub fn parse_invite(text: &str) -> Option<Invite> {
    let text = unfold(text);
    let mut method = InviteMethod::Request;
    let mut inv = Invite::default();
    let (mut in_event, mut done, mut depth) = (false, false, 0usize);
    let (mut start, mut end, mut dur) = (None, None, None);
    let mut description = String::new();
    for line in text.lines() {
        let Some(p) = prop(line.trim_end_matches('\r')) else { continue };
        let value = p.value;
        match p.name.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VEVENT") && !done => in_event = true,
            // Skip nested components (VALARM) inside the event.
            "BEGIN" if in_event => depth += 1,
            "END" if in_event && depth > 0 => depth -= 1,
            "END" if value.eq_ignore_ascii_case("VEVENT") && in_event => {
                in_event = false;
                done = true;
            }
            "METHOD" => {
                method = match value.trim().to_ascii_uppercase().as_str() {
                    "CANCEL" => InviteMethod::Cancel,
                    "REPLY" => InviteMethod::Reply,
                    _ => InviteMethod::Request,
                }
            }
            _ if !in_event || depth > 0 => {}
            "UID" => inv.uid = Some(value.trim().to_string()),
            "SUMMARY" => inv.title = unescape(value),
            "LOCATION" => inv.location = Some(unescape(value)).filter(|l| !l.trim().is_empty()),
            "DTSTART" => start = time(&p),
            "DTEND" => end = time(&p),
            "DURATION" => dur = duration(value),
            "STATUS" if value.trim().eq_ignore_ascii_case("CANCELLED") => method = InviteMethod::Cancel,
            "ORGANIZER" => {
                let email = value.trim();
                let email = email.get(..7).filter(|s| s.eq_ignore_ascii_case("mailto:")).map_or(email, |_| &email[7..]);
                if email.contains('@') {
                    inv.organizer = Some(Address::new(p.param("CN"), email));
                }
            }
            "X-GOOGLE-CONFERENCE" | "X-MICROSOFT-SKYPETEAMSMEETINGURL" => {
                inv.online_url = Some(value.trim().to_string())
            }
            "DESCRIPTION" | "X-ALT-DESC" => {
                description.push_str(&unescape(value));
                description.push('\n');
            }
            _ => {}
        }
    }
    let (start_at, all_day) = start?;
    inv.start_at = start_at;
    inv.all_day = all_day;
    inv.end_at =
        end.map(|(e, _)| e).or(dur.map(|d| start_at + d)).unwrap_or(start_at + if all_day { DAY_MS } else { HOUR_MS });
    if inv.online_url.is_none() {
        inv.online_url = inv.location.as_deref().and_then(meeting_link).or_else(|| meeting_link(&description));
    }
    inv.method = method;
    Some(inv)
}

/// A VEVENT as stored on a CalDAV server (one instance or a series master).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VEvent {
    pub uid: String,
    /// Set on overridden / expanded instances of a recurring series.
    pub recurrence_id: Option<String>,
    pub summary: String,
    pub start_at: i64,
    pub end_at: i64,
    pub all_day: bool,
    pub tzid: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub organizer: Option<Address>,
    pub attendees: Vec<Attendee>,
    pub cancelled: bool,
    /// Unexpanded series master (has RRULE): the server didn't expand it.
    pub has_rrule: bool,
    pub updated_at: i64,
    pub online_url: Option<String>,
}

fn mailto(v: &str) -> &str {
    let v = v.trim();
    v.get(..7).filter(|s| s.eq_ignore_ascii_case("mailto:")).map_or(v, |_| &v[7..])
}

fn partstat(v: Option<&str>) -> Response {
    match v.unwrap_or_default().to_ascii_uppercase().as_str() {
        "ACCEPTED" => Response::Accepted,
        "TENTATIVE" => Response::Tentative,
        "DECLINED" => Response::Declined,
        _ => Response::None,
    }
}

fn utc_stamp(v: &str) -> Option<i64> {
    let t = NaiveDateTime::parse_from_str(v.trim().trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    Some(t.and_utc().timestamp_millis())
}

/// Every VEVENT in an iCalendar object (CalDAV `calendar-data`).
pub fn parse_events(text: &str) -> Vec<VEvent> {
    let text = unfold(text);
    let mut out = Vec::new();
    let mut cur: Option<VEvent> = None;
    let mut depth = 0usize;
    let (mut start, mut end, mut dur) = (None, None, None);
    let mut description = String::new();
    for line in text.lines() {
        let Some(p) = prop(line.trim_end_matches('\r')) else { continue };
        let value = p.value;
        match p.name.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VEVENT") && cur.is_none() => {
                cur = Some(VEvent::default());
                (start, end, dur, depth) = (None, None, None, 0);
                description.clear();
            }
            "BEGIN" if cur.is_some() => depth += 1,
            "END" if cur.is_some() && depth > 0 => depth -= 1,
            "END" if value.eq_ignore_ascii_case("VEVENT") => {
                let Some(mut e) = cur.take() else { continue };
                let Some((s, all_day)) = start else { continue };
                e.start_at = s;
                e.all_day = all_day;
                e.end_at = end.or(dur.map(|d| s + d)).unwrap_or(s + if all_day { DAY_MS } else { HOUR_MS });
                if !description.trim().is_empty() {
                    e.description = Some(description.trim_end().to_string());
                }
                if e.online_url.is_none() {
                    e.online_url = e.location.as_deref().and_then(meeting_link).or_else(|| meeting_link(&description));
                }
                if !e.uid.is_empty() {
                    out.push(e);
                }
            }
            _ if depth > 0 => {}
            _ => {
                let Some(e) = cur.as_mut() else { continue };
                match p.name.as_str() {
                    "UID" => e.uid = value.trim().to_string(),
                    "RECURRENCE-ID" => e.recurrence_id = Some(value.trim().to_string()),
                    "SUMMARY" => e.summary = unescape(value),
                    "LOCATION" => e.location = Some(unescape(value)).filter(|l| !l.trim().is_empty()),
                    "DESCRIPTION" => description.push_str(&unescape(value)),
                    "DTSTART" => {
                        start = time(&p);
                        e.tzid = p.param("TZID").map(String::from);
                    }
                    "DTEND" => end = time(&p).map(|(t, _)| t),
                    "DURATION" => dur = duration(value),
                    "RRULE" => e.has_rrule = true,
                    "STATUS" => e.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED"),
                    "LAST-MODIFIED" => e.updated_at = utc_stamp(value).unwrap_or(e.updated_at),
                    "DTSTAMP" if e.updated_at == 0 => e.updated_at = utc_stamp(value).unwrap_or(0),
                    "ORGANIZER" => {
                        let email = mailto(value);
                        if email.contains('@') {
                            e.organizer = Some(Address::new(p.param("CN"), email));
                        }
                    }
                    "ATTENDEE" => {
                        let email = mailto(value);
                        if email.contains('@')
                            && !p
                                .param("CUTYPE")
                                .is_some_and(|c| c.eq_ignore_ascii_case("RESOURCE") || c.eq_ignore_ascii_case("ROOM"))
                        {
                            e.attendees.push(Attendee {
                                address: Address::new(p.param("CN"), email),
                                response: partstat(p.param("PARTSTAT")),
                                optional: p.param("ROLE").is_some_and(|r| r.eq_ignore_ascii_case("OPT-PARTICIPANT")),
                            });
                        }
                    }
                    "X-GOOGLE-CONFERENCE" | "X-MICROSOFT-SKYPETEAMSMEETINGURL" | "URL" => {
                        if meeting_link(value).is_some() || p.name != "URL" {
                            e.online_url = Some(value.trim().to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

/// Fold to 75-octet lines (RFC 5545 3.1), never splitting a character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut width = 0;
    for c in line.chars() {
        let w = c.len_utf8();
        if width + w > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += w;
    }
    out.push_str("\r\n");
    out
}

fn utc(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y%m%dT%H%M%SZ").to_string()).unwrap_or_default()
}

fn date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y%m%d").to_string()).unwrap_or_default()
}

/// What `build_calendar` writes.
#[derive(Debug, Clone, Default)]
pub struct NewVEvent<'a> {
    pub uid: &'a str,
    pub title: &'a str,
    pub start_at: i64,
    /// Exclusive (for all-day: the midnight after the last day).
    pub end_at: i64,
    pub all_day: bool,
    pub location: Option<&'a str>,
    pub description: Option<&'a str>,
    pub organizer: Option<&'a Address>,
    pub attendees: &'a [Address],
    pub now: i64,
}

/// A complete VCALENDAR with one VEVENT (timed events in UTC).
pub fn build_calendar(e: &NewVEvent) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Coo//Coo Mail//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", e.uid),
        format!("DTSTAMP:{}", utc(e.now)),
        format!("LAST-MODIFIED:{}", utc(e.now)),
        format!("SUMMARY:{}", escape(e.title)),
    ];
    if e.all_day {
        lines.push(format!("DTSTART;VALUE=DATE:{}", date(e.start_at)));
        lines.push(format!("DTEND;VALUE=DATE:{}", date(e.end_at)));
    } else {
        lines.push(format!("DTSTART:{}", utc(e.start_at)));
        lines.push(format!("DTEND:{}", utc(e.end_at)));
    }
    if let Some(l) = e.location.filter(|l| !l.is_empty()) {
        lines.push(format!("LOCATION:{}", escape(l)));
    }
    if let Some(d) = e.description.filter(|d| !d.is_empty()) {
        lines.push(format!("DESCRIPTION:{}", escape(d)));
    }
    let cn = |a: &Address| a.name.as_deref().map(|n| format!(";CN=\"{}\"", n.replace('"', ""))).unwrap_or_default();
    if let Some(o) = e.organizer.filter(|_| !e.attendees.is_empty()) {
        lines.push(format!("ORGANIZER{}:mailto:{}", cn(o), o.email));
    }
    for a in e.attendees {
        lines.push(format!(
            "ATTENDEE{};ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{}",
            cn(a),
            a.email
        ));
    }
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.iter().map(|l| fold(l)).collect()
}

/// Set `email`'s PARTSTAT on every ATTENDEE line for them (all instances).
/// None if they aren't an attendee.
pub fn set_partstat(text: &str, email: &str, response: Response) -> Option<String> {
    let value = match response {
        Response::Accepted => "ACCEPTED",
        Response::Tentative => "TENTATIVE",
        Response::Declined => "DECLINED",
        _ => "NEEDS-ACTION",
    };
    let text = unfold(text);
    let mut found = false;
    let mut out = String::with_capacity(text.len() + 64);
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let is_me = prop(line).is_some_and(|p| p.name == "ATTENDEE" && mailto(p.value).eq_ignore_ascii_case(email));
        if !is_me {
            out.push_str(&fold(line));
            continue;
        }
        found = true;
        // Rebuild params without PARTSTAT/RSVP, then add ours.
        let p = prop(line)?;
        let mut head = String::from("ATTENDEE");
        for (k, v) in &p.params {
            if k.eq_ignore_ascii_case("PARTSTAT") || k.eq_ignore_ascii_case("RSVP") {
                continue;
            }
            if v.contains([':', ';', ',']) || v.contains(' ') {
                head.push_str(&format!(";{k}=\"{v}\""));
            } else {
                head.push_str(&format!(";{k}={v}"));
            }
        }
        head.push_str(&format!(";PARTSTAT={value}:{}", p.value.trim()));
        out.push_str(&fold(&head));
    }
    found.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nMETHOD:REQUEST\r\n\
        BEGIN:VEVENT\r\nDTSTART:20261029T130000Z\r\nDTEND:20261029T140000Z\r\n\
        ORGANIZER;CN=Priya Raman:mailto:priya@halyard.co\r\nUID:abc123@google.com\r\n\
        SUMMARY:Roadmap review\\, Q4\r\nLOCATION:Room 4B\r\n\
        X-GOOGLE-CONFERENCE:https://meet.google.com/qrv-hmtz-kpa\r\n\
        DESCRIPTION:Join with Google Meet: https://meet.google.com/qrv-hmtz-kpa\\nMore phone numbers: http\r\n s://tel.meet/x\r\n\
        BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:This is an event reminder\r\nEND:VALARM\r\n\
        END:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_google_invite() {
        let inv = parse_invite(GOOGLE).unwrap();
        assert_eq!(inv.method, InviteMethod::Request);
        assert_eq!(inv.uid.as_deref(), Some("abc123@google.com"));
        assert_eq!(inv.title, "Roadmap review, Q4");
        assert_eq!(inv.end_at - inv.start_at, HOUR_MS);
        assert_eq!(inv.start_at, 1_793_278_800_000);
        assert!(!inv.all_day);
        assert_eq!(inv.organizer, Some(Address::new(Some("Priya Raman"), "priya@halyard.co")));
        assert_eq!(inv.online_url.as_deref(), Some("https://meet.google.com/qrv-hmtz-kpa"));
        assert_eq!(inv.location.as_deref(), Some("Room 4B"));
    }

    #[test]
    fn parses_outlook_style_cancel_with_tzid_and_teams_link() {
        let ics = "BEGIN:VCALENDAR\nMETHOD:CANCEL\nBEGIN:VEVENT\n\
            UID:040000008200E00074C5B7101A82E008\n\
            DTSTART;TZID=Europe/Berlin:20261029T140000\nDURATION:PT30M\n\
            SUMMARY;LANGUAGE=en-US:Hiring sync\nSTATUS:CANCELLED\n\
            DESCRIPTION:Join: https://teams.microsoft.com/l/meetup-join/19%3ameeting_x/0?context=y\n\
            END:VEVENT\nEND:VCALENDAR";
        let inv = parse_invite(ics).unwrap();
        assert_eq!(inv.method, InviteMethod::Cancel);
        // 14:00 CET (UTC+1 on Oct 29, after DST ends) = 13:00 UTC.
        assert_eq!(inv.start_at, 1_793_278_800_000);
        assert_eq!(inv.end_at - inv.start_at, 30 * 60_000);
        assert!(inv.online_url.unwrap().starts_with("https://teams.microsoft.com/l/meetup-join/"));
    }

    #[test]
    fn all_day_and_missing_end() {
        let inv = parse_invite("BEGIN:VEVENT\nDTSTART;VALUE=DATE:20261102\nSUMMARY:Offsite\nEND:VEVENT").unwrap();
        assert!(inv.all_day);
        assert_eq!(inv.end_at - inv.start_at, DAY_MS);
        assert!(parse_invite("BEGIN:VEVENT\nSUMMARY:No time\nEND:VEVENT").is_none());
        assert!(parse_invite("garbage").is_none());
    }

    #[test]
    fn durations_and_links() {
        assert_eq!(duration("PT1H30M"), Some(90 * 60_000));
        assert_eq!(duration("P1DT2H"), Some(DAY_MS + 2 * HOUR_MS));
        assert_eq!(duration("bogus"), None);
        assert_eq!(
            meeting_link("call https://zoom.us/j/123?pwd=x now").as_deref(),
            Some("https://zoom.us/j/123?pwd=x")
        );
        assert_eq!(
            meeting_link("https://example.com/a https://meet.google.com/x-y"),
            Some("https://meet.google.com/x-y".into())
        );
        assert_eq!(meeting_link("nothing"), None);
    }

    const CALDAV: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Asia/Jerusalem\r\nEND:VTIMEZONE\r\n\
        BEGIN:VEVENT\r\nUID:abc-123\r\nDTSTAMP:20261020T100000Z\r\nLAST-MODIFIED:20261021T100000Z\r\n\
        DTSTART;TZID=Asia/Jerusalem:20261029T150000\r\nDTEND;TZID=Asia/Jerusalem:20261029T160000\r\n\
        SUMMARY:\u{5e4}\u{5d2}\u{5d9}\u{5e9}\u{5d4}\\, weekly\r\nLOCATION:https://zoom.us/j/123\r\n\
        ORGANIZER;CN=Dana:mailto:dana@x.co.il\r\n\
        ATTENDEE;CN=Me;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:me@icloud.com\r\n\
        ATTENDEE;CN=\"Ofer, O.\";PARTSTAT=ACCEPTED;ROLE=OPT-PARTICIPANT:mailto:ofer@y.com\r\n\
        ATTENDEE;CUTYPE=ROOM;PARTSTAT=ACCEPTED:mailto:room@x.co.il\r\n\
        BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:abc-123\r\nRECURRENCE-ID:20261105T130000Z\r\nDTSTART:20261105T140000Z\r\n\
        DURATION:PT30M\r\nSUMMARY:moved\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_caldav_events_with_attendees_and_instances() {
        let ev = parse_events(CALDAV);
        assert_eq!(ev.len(), 2);
        let a = &ev[0];
        assert_eq!(a.uid, "abc-123");
        assert_eq!(a.summary, "\u{5e4}\u{5d2}\u{5d9}\u{5e9}\u{5d4}, weekly");
        assert_eq!(a.end_at - a.start_at, HOUR_MS);
        assert_eq!(
            a.start_at,
            chrono::DateTime::parse_from_rfc3339("2026-10-29T15:00:00+02:00").unwrap().timestamp_millis()
        );
        assert_eq!(a.tzid.as_deref(), Some("Asia/Jerusalem"));
        assert_eq!(a.online_url.as_deref(), Some("https://zoom.us/j/123"));
        assert_eq!(a.attendees.len(), 2, "rooms are not people");
        assert_eq!(a.attendees[1].address.name.as_deref(), Some("Ofer, O."));
        assert!(a.attendees[1].optional && a.attendees[1].response == Response::Accepted);
        assert_eq!(a.description, None, "VALARM text is not the event's");
        assert_eq!(a.updated_at, utc_stamp("20261021T100000Z").unwrap());
        let b = &ev[1];
        assert_eq!(b.recurrence_id.as_deref(), Some("20261105T130000Z"));
        assert!(b.cancelled);
        assert_eq!(b.end_at - b.start_at, 30 * 60_000);
    }

    #[test]
    fn build_and_rsvp_roundtrip() {
        let me = Address::new(Some("Me"), "me@icloud.com");
        let guests = [Address::new(Some("Dana Cohen"), "dana@x.co.il")];
        let ics = build_calendar(&NewVEvent {
            uid: "u1",
            title: "Planning; Q4, \u{5e9}\u{5dc}\u{5d5}\u{5dd}",
            start_at: 1_793_000_000_000,
            end_at: 1_793_003_600_000,
            location: Some("Room 4"),
            organizer: Some(&me),
            attendees: &guests,
            now: 1_792_000_000_000,
            ..Default::default()
        });
        assert!(ics.lines().all(|l| l.len() <= 76), "folded");
        let back = parse_events(&ics);
        assert_eq!(back[0].summary, "Planning; Q4, \u{5e9}\u{5dc}\u{5d5}\u{5dd}");
        assert_eq!((back[0].start_at, back[0].end_at), (1_793_000_000_000, 1_793_003_600_000));
        assert_eq!(back[0].organizer.as_ref().unwrap().email, "me@icloud.com");

        let answered = set_partstat(CALDAV, "ME@icloud.com", Response::Tentative).unwrap();
        let ev = parse_events(&answered);
        let mine = ev[0].attendees.iter().find(|a| a.address.email == "me@icloud.com").unwrap();
        assert_eq!(mine.response, Response::Tentative);
        assert!(!answered.contains("RSVP=TRUE:mailto:me@icloud.com"));
        assert_eq!(ev[0].attendees[1].address.name.as_deref(), Some("Ofer, O."), "other params kept");
        assert!(set_partstat(CALDAV, "nobody@x.com", Response::Accepted).is_none());

        let all_day = build_calendar(&NewVEvent {
            uid: "u2",
            title: "Off",
            start_at: 1_792_713_600_000,
            end_at: 1_792_800_000_000,
            all_day: true,
            ..Default::default()
        });
        assert!(all_day.contains("DTSTART;VALUE=DATE:20261023") && all_day.contains("DTEND;VALUE=DATE:20261024"));
    }
}
