//! Video-meeting links: find them on an event (provider field first, then
//! links pasted into the location), name the service, decide when "Join"
//! is offered, and map web links to the native apps' URL schemes.

use crate::model::Event;

/// "Join" appears this long before the start, and stays until the end.
pub const JOIN_LEAD_MS: i64 = 15 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Zoom,
    Meet,
    Teams,
    Webex,
    Other,
}

impl Service {
    pub fn label(self) -> &'static str {
        match self {
            Service::Zoom => "Zoom",
            Service::Meet => "Google Meet",
            Service::Teams => "Teams",
            Service::Webex => "Webex",
            Service::Other => "meeting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingLink {
    pub url: String,
    pub service: Service,
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}

pub fn service_of(url: &str) -> Service {
    let h = host(url).to_ascii_lowercase();
    let is = |d: &str| h == d || h.ends_with(&format!(".{d}"));
    if is("zoom.us") || is("zoomgov.com") {
        Service::Zoom
    } else if is("meet.google.com") {
        Service::Meet
    } else if is("teams.microsoft.com") || is("teams.live.com") {
        Service::Teams
    } else if is("webex.com") {
        Service::Webex
    } else {
        Service::Other
    }
}

/// The first known meeting URL in free text (locations often hold the link).
pub fn find_link(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | ','))
        .map(|t| t.trim_end_matches(['.', ';']))
        .filter(|t| t.starts_with("https://") || t.starts_with("http://"))
        .find(|t| service_of(t) != Service::Other)
        .map(String::from)
}

/// The event's meeting link: the provider's own field, else one pasted into
/// the location.
pub fn meeting_link(e: &Event) -> Option<MeetingLink> {
    let url = e.online_url.clone().or_else(|| e.location.as_deref().and_then(find_link))?;
    let service = service_of(&url);
    Some(MeetingLink { url, service })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinState {
    /// Too early to offer Join.
    Later,
    /// Within the lead time; starts in this many ms.
    Soon(i64),
    /// In progress.
    Live,
    Ended,
}

pub fn join_state(start_at: i64, end_at: i64, now: i64) -> JoinState {
    if now >= end_at {
        JoinState::Ended
    } else if now >= start_at {
        JoinState::Live
    } else if start_at - now <= JOIN_LEAD_MS {
        JoinState::Soon(start_at - now)
    } else {
        JoinState::Later
    }
}

/// The meeting to join right now among `events` (sorted by start): a live
/// one first (latest started, i.e. the one you're most likely late for),
/// else the next one inside the lead time. Cancelled and declined skipped.
pub fn joinable<'a>(events: impl IntoIterator<Item = &'a Event>, now: i64) -> Option<(&'a Event, MeetingLink)> {
    let mut live: Option<(&Event, MeetingLink)> = None;
    let mut soon: Option<(&Event, MeetingLink)> = None;
    for e in events {
        if e.cancelled || e.my_response == crate::model::Response::Declined {
            continue;
        }
        let Some(link) = meeting_link(e) else { continue };
        match join_state(e.start_at, e.end_at, now) {
            JoinState::Live if live.as_ref().is_none_or(|(l, _)| e.start_at > l.start_at) => live = Some((e, link)),
            JoinState::Soon(_) if soon.as_ref().is_none_or(|(s, _)| e.start_at < s.start_at) => soon = Some((e, link)),
            _ => {}
        }
    }
    live.or(soon)
}

/// Native-app URL for the link, so joining skips the browser hop:
/// Zoom `zoommtg://`, Teams `msteams:`. `None` means open the web link.
pub fn native_url(link: &MeetingLink) -> Option<String> {
    match link.service {
        Service::Zoom => {
            // https://<sub>.zoom.us/j/<id>?pwd=<pwd>
            let after = link.url.split_once("/j/")?.1;
            let (id, query) = after.split_once('?').unwrap_or((after, ""));
            let id: String = id.chars().take_while(|c| c.is_ascii_digit()).collect();
            if id.is_empty() {
                return None;
            }
            let pwd = query.split('&').find_map(|kv| kv.strip_prefix("pwd="));
            let host = host(&link.url);
            Some(match pwd {
                Some(p) => format!("zoommtg://{host}/join?action=join&confno={id}&pwd={p}"),
                None => format!("zoommtg://{host}/join?action=join&confno={id}"),
            })
        }
        Service::Teams => {
            let rest = link.url.split_once("://")?.1;
            Some(format!("msteams:{}", &rest[rest.find('/')?..]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(online: Option<&str>, location: Option<&str>, start: i64, end: i64) -> Event {
        Event {
            id: format!("{start}"),
            online_url: online.map(String::from),
            location: location.map(String::from),
            start_at: start,
            end_at: end,
            ..Default::default()
        }
    }

    #[test]
    fn services_and_links() {
        assert_eq!(service_of("https://us02web.zoom.us/j/123"), Service::Zoom);
        assert_eq!(service_of("https://meet.google.com/abc-defg-hij"), Service::Meet);
        assert_eq!(service_of("https://teams.microsoft.com/l/meetup-join/x"), Service::Teams);
        assert_eq!(service_of("https://notzoom.us.evil.com/j/1"), Service::Other);
        assert_eq!(
            find_link("Room 4B / https://us02web.zoom.us/j/8812345678?pwd=abc. Call me").as_deref(),
            Some("https://us02web.zoom.us/j/8812345678?pwd=abc")
        );
        assert_eq!(find_link("https://example.com/agenda"), None);
        let e = ev(None, Some("Zoom: <https://zoom.us/j/99>"), 0, 1);
        assert_eq!(meeting_link(&e).unwrap().service, Service::Zoom);
        let e = ev(Some("https://meet.google.com/x"), Some("https://zoom.us/j/1"), 0, 1);
        assert_eq!(meeting_link(&e).unwrap().service, Service::Meet, "provider field wins");
    }

    #[test]
    fn join_window_and_choice() {
        let m = 60_000;
        assert_eq!(join_state(30 * m, 60 * m, 0), JoinState::Later);
        assert_eq!(join_state(10 * m, 60 * m, 0), JoinState::Soon(10 * m));
        assert_eq!(join_state(0, 60 * m, 5 * m), JoinState::Live);
        assert_eq!(join_state(0, 60 * m, 60 * m), JoinState::Ended);

        let a = ev(Some("https://zoom.us/j/1"), None, 0, 60 * m); // live since 0
        let b = ev(Some("https://zoom.us/j/2"), None, 20 * m, 50 * m); // live since 20
        let c = ev(Some("https://zoom.us/j/3"), None, 40 * m, 70 * m); // soon
        let no_link = ev(None, Some("Room 4B"), 25 * m, 30 * m);
        let mut declined = ev(Some("https://zoom.us/j/4"), None, 26 * m, 60 * m);
        declined.my_response = crate::model::Response::Declined;
        let events = [a.clone(), b.clone(), c.clone(), no_link, declined];
        assert_eq!(joinable(&events, 30 * m).unwrap().0.id, b.id, "latest-started live meeting");
        assert_eq!(joinable(std::slice::from_ref(&c), 30 * m).unwrap().0.id, c.id);
        assert!(joinable(&[c], 0).is_none(), "too early");
    }

    #[test]
    fn native_schemes() {
        let z =
            MeetingLink { url: "https://us02web.zoom.us/j/8812345678?pwd=Ab1&from=x".into(), service: Service::Zoom };
        assert_eq!(
            native_url(&z).as_deref(),
            Some("zoommtg://us02web.zoom.us/join?action=join&confno=8812345678&pwd=Ab1")
        );
        let t = MeetingLink {
            url: "https://teams.microsoft.com/l/meetup-join/19%3a/0?context=x".into(),
            service: Service::Teams,
        };
        assert_eq!(native_url(&t).as_deref(), Some("msteams:/l/meetup-join/19%3a/0?context=x"));
        let m = MeetingLink { url: "https://meet.google.com/abc".into(), service: Service::Meet };
        assert_eq!(native_url(&m), None);
        let bad = MeetingLink { url: "https://zoom.us/my/room".into(), service: Service::Zoom };
        assert_eq!(native_url(&bad), None, "personal room links stay web");
    }
}
