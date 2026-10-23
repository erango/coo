//! Just enough iCalendar (RFC 5545) to read a meeting invite: the first
//! VEVENT's UID, summary, times, location, organizer and meeting link, plus
//! the calendar METHOD.

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};

use crate::model::{Address, Invite, InviteMethod};

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
}
