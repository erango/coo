//! Meetings you can't miss: alerts that get harder to ignore as a meeting
//! gets close. Each step is a setting (Settings › Notifications):
//!
//! - from `meeting_countdown_minutes` before: the title and minutes left in
//!   the menu bar, and a warmer Next up card in the sidebar;
//! - `event_reminder_minutes` before: the macOS notification (notify.rs);
//! - `meeting_banner_minutes` before: a banner across the top of Coo;
//! - at the start: a panel over every app (meeting_panel.rs) and a strip in
//!   any open composer; the panel comes back once if put away.
//!
//! Join, Dismiss or Not going on any of them ends the rest for that meeting.
//! Which meetings climb how far is `reach`. Decisions are pure functions of
//! the agenda, the time, the prefs and `Alerts` (what the user did), so
//! they're tested without a window.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use pigeon_core::meeting::meeting_link;
use pigeon_core::model::{EventRow, Response};

use crate::prefs::Prefs;

/// Join from any app, unless changed in Settings.
pub const DEFAULT_HOTKEY: &str = "shift+cmd+KeyJ";

/// The join shortcuts on offer: (pref value, keycap label).
pub const HOTKEYS: [(&str, &str); 4] =
    [("shift+cmd+KeyJ", "⇧⌘J"), ("ctrl+alt+KeyJ", "⌃⌥J"), ("ctrl+shift+KeyJ", "⌃⇧J"), ("", "Off")];

/// How long after its start a meeting still alerts.
const LIVE_WINDOW: i64 = 5 * 60_000;
/// A meeting that starts within this of another one's end is back to back.
const BACK_TO_BACK: i64 = 60_000;
const MIN: i64 = 60_000;

/// How far a meeting climbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reach {
    /// Next up only: declined, all-day, cancelled, a quiet calendar or a
    /// muted account.
    Quiet,
    /// Up to the notification (and the menu bar countdown).
    Notify,
    /// Every step that's turned on.
    Full,
}

pub fn reach(e: &EventRow, p: &Prefs) -> Reach {
    let ev = &e.event;
    if ev.all_day || ev.cancelled || ev.my_response == Response::Declined {
        return Reach::Quiet;
    }
    if p.notify_accounts_muted.contains(&e.account_id)
        || p.meeting_quiet_calendars.contains(&format!("{}|{}", e.account_id, ev.calendar_id))
    {
        return Reach::Quiet;
    }
    let me = e.account_id.split_once(':').map(|(_, m)| m).unwrap_or(&e.account_id);
    let not_me = |email: &str| !email.eq_ignore_ascii_case(me);
    let others = ev.attendees.iter().any(|a| not_me(&a.address.email))
        || ev.organizer.as_ref().is_some_and(|o| not_me(&o.email));
    if !others {
        return if meeting_link(ev).is_some() || p.meeting_alert_solo { Reach::Full } else { Reach::Notify };
    }
    match ev.my_response {
        Response::Accepted | Response::Organizer => Reach::Full,
        _ if p.meeting_alert_tentative => Reach::Full,
        _ => Reach::Notify,
    }
}

pub fn key(e: &EventRow) -> String {
    crate::notify::reminder_key(e)
}

/// What the user did about each meeting (by `key`).
#[derive(Debug, Default)]
pub struct Alerts {
    /// Joined, dismissed or "Not going": nothing more about it.
    pub done: HashSet<String>,
    /// Banner put off ("In 1 min") until then.
    pub banner_until: HashMap<String, i64>,
    /// "Keep writing" in a composer.
    pub strip_hidden: HashSet<String>,
    /// How many times the panel has shown.
    pub panel_shown: HashMap<String, u8>,
    /// The meeting the panel shows now.
    pub panel_open: Option<String>,
    /// The agenda as last seen: key → join link, for joins from anywhere.
    pub links: Vec<(String, i64, i64, String)>,
}

static ALERTS: Mutex<Option<Alerts>> = Mutex::new(None);

/// Run `f` on the shared state.
pub fn with<R>(f: impl FnOnce(&mut Alerts) -> R) -> R {
    let mut g = ALERTS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Alerts::default))
}

pub fn mark_done(key: &str) {
    with(|a| {
        a.done.insert(key.to_string());
    });
}

/// A meeting link was opened (from anywhere in Coo): that meeting is joined.
pub fn joined_url(url: &str) {
    with(|a| {
        let keys: Vec<String> = a.links.iter().filter(|(_, _, _, u)| u == url).map(|(k, ..)| k.clone()).collect();
        a.done.extend(keys);
    });
}

/// The link the join shortcut opens: the panel's meeting, else the one
/// that's on now or about to start (soonest first).
pub fn link_to_join(now: i64) -> Option<String> {
    with(|a| {
        if let Some(open) = &a.panel_open
            && let Some((.., u)) = a.links.iter().find(|(k, ..)| k == open)
        {
            return Some(u.clone());
        }
        a.links
            .iter()
            .filter(|(_, start, end, _)| *start - 10 * MIN <= now && now < *end)
            .min_by_key(|(_, start, ..)| (now - start).abs())
            .map(|(.., u)| u.clone())
    })
}

/// One meeting as an alert shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub key: String,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub url: Option<String>,
    /// "Zoom", "Google Meet", … ("" without a link).
    pub service: String,
    pub people: Vec<String>,
    /// For a back-to-back start: the meeting that ends now.
    pub after: Option<String>,
}

impl Shown {
    fn of(e: &EventRow, events: &[EventRow]) -> Self {
        let ev = &e.event;
        let link = meeting_link(ev);
        let me = e.account_id.split_once(':').map(|(_, m)| m).unwrap_or("");
        let mut people: Vec<String> = Vec::new();
        for a in ev.organizer.iter().chain(ev.attendees.iter().map(|a| &a.address)) {
            if a.email.eq_ignore_ascii_case(me) {
                continue;
            }
            let name = a.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| a.email.clone());
            if !people.contains(&name) {
                people.push(name);
            }
        }
        let after = events
            .iter()
            .filter(|o| o.event.id != ev.id && !o.event.all_day && !o.event.cancelled)
            .find(|o| o.event.start_at < ev.start_at && (o.event.end_at - ev.start_at).abs() <= BACK_TO_BACK)
            .map(|o| o.event.title.clone());
        Shown {
            key: key(e),
            title: ev.title.clone(),
            start_at: ev.start_at,
            end_at: ev.end_at,
            url: link.as_ref().map(|l| l.url.clone()),
            service: link.map(|l| l.service.label().to_string()).unwrap_or_default(),
            people,
            after,
        }
    }

    /// "Maya, Dan and 3 more".
    pub fn people_line(&self) -> String {
        match self.people.as_slice() {
            [] => String::new(),
            [a] => a.clone(),
            [a, b] => format!("{a} and {b}"),
            [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
        }
    }

    /// "Join Zoom" / "Join".
    pub fn join_label(&self) -> String {
        if self.service.is_empty() { "Join".into() } else { format!("Join {}", self.service) }
    }

    /// "in 2 min", "now", "started 3 min ago".
    pub fn when_words(&self, now: i64) -> String {
        let d = self.start_at - now;
        if d > 30_000 {
            format!("in {} min", (d / MIN).max(1))
        } else if d > -MIN {
            "now".into()
        } else {
            format!("started {} min ago", -d / MIN)
        }
    }
}

/// Initials for a face: "Maya Alon" → "MA".
pub fn initials(name: &str) -> String {
    let name = name.split('@').next().unwrap_or(name);
    let mut parts = name.split(|c: char| c.is_whitespace() || c == '.' || c == '_').filter(|p| !p.is_empty());
    let a = parts.next().and_then(|p| p.chars().next());
    let b = parts.next().and_then(|p| p.chars().next());
    a.into_iter().chain(b).flat_map(char::to_uppercase).collect()
}

/// Meetings that may still alert, soonest first.
fn live<'a>(events: &'a [EventRow], now: i64, p: &Prefs, a: &Alerts, want: Reach) -> Vec<&'a EventRow> {
    let mut v: Vec<&EventRow> = events
        .iter()
        .filter(|e| reach(e, p) >= want)
        .filter(|e| now < e.event.start_at + LIVE_WINDOW && now < e.event.end_at)
        .filter(|e| !a.done.contains(&key(e)))
        .collect();
    v.sort_by_key(|e| e.event.start_at);
    v
}

/// The banner across the top of Coo.
pub fn banner(events: &[EventRow], now: i64, p: &Prefs, a: &Alerts) -> Option<Shown> {
    if !p.meeting_banner {
        return None;
    }
    let lead = i64::from(p.meeting_banner_minutes.max(1)) * MIN;
    live(events, now, p, a, Reach::Full)
        .into_iter()
        .filter(|e| now >= e.event.start_at - lead)
        .find(|e| a.banner_until.get(&key(e)).is_none_or(|until| now >= *until))
        .map(|e| Shown::of(e, events))
}

/// The strip in an open composer: from the banner's lead time (it takes
/// the banner's place there), else from the start.
pub fn strip(events: &[EventRow], now: i64, p: &Prefs, a: &Alerts) -> Option<Shown> {
    if !p.meeting_composer_strip {
        return None;
    }
    // While you write, the strip stands in for the banner too.
    let lead = if p.meeting_banner { i64::from(p.meeting_banner_minutes.max(1)) * MIN } else { 30_000 };
    live(events, now, p, a, Reach::Full)
        .into_iter()
        .filter(|e| now >= e.event.start_at - lead)
        .find(|e| !a.strip_hidden.contains(&key(e)))
        .map(|e| Shown::of(e, events))
}

/// The panel over every app, when it's due to (re)appear.
pub fn panel_due(events: &[EventRow], now: i64, p: &Prefs, a: &Alerts) -> Option<Shown> {
    if !p.meeting_panel || a.panel_open.is_some() {
        return None;
    }
    let again = i64::from(p.meeting_panel_again_minutes.max(1)) * MIN;
    live(events, now, p, a, Reach::Full)
        .into_iter()
        .filter(|e| now >= e.event.start_at)
        .find(|e| match a.panel_shown.get(&key(e)).copied().unwrap_or(0) {
            0 => true,
            1 => p.meeting_panel_again && now >= e.event.start_at + again,
            _ => false,
        })
        .map(|e| Shown::of(e, events))
}

/// The menu bar title: "Design review · 8m", "· now", "· 2m late".
pub fn countdown(events: &[EventRow], now: i64, p: &Prefs, a: &Alerts) -> Option<String> {
    if !p.meeting_countdown {
        return None;
    }
    let lead = i64::from(p.meeting_countdown_minutes.max(1)) * MIN;
    let e = live(events, now, p, a, Reach::Notify).into_iter().find(|e| now >= e.event.start_at - lead)?;
    let d = e.event.start_at - now;
    let when = if d > 30_000 {
        format!("{}m", (d / MIN).max(1))
    } else if d > -MIN {
        "now".into()
    } else {
        format!("{}m late", -d / MIN)
    };
    Some(format!("{} · {when}", short_title(&e.event.title)))
}

/// Long titles cut at 22 characters.
pub fn short_title(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() <= 22 {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(21).collect::<String>().trim_end())
    }
}

/// How warm the Next up card is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heat {
    None,
    /// Within the countdown window.
    Warm,
    /// Within the banner window.
    Hot,
    /// Started, not joined or dismissed.
    Live,
}

impl Heat {
    pub fn attr(self) -> Option<&'static str> {
        match self {
            Heat::None => None,
            Heat::Warm => Some("warm"),
            Heat::Hot => Some("hot"),
            Heat::Live => Some("live"),
        }
    }
}

pub fn heat(e: &EventRow, now: i64, p: &Prefs, a: &Alerts) -> Heat {
    if !p.meeting_card_warm || reach(e, p) == Reach::Quiet || a.done.contains(&key(e)) {
        return Heat::None;
    }
    let ev = &e.event;
    if now >= ev.end_at || now >= ev.start_at + LIVE_WINDOW {
        return Heat::None;
    }
    let d = ev.start_at - now;
    if d <= 0 {
        Heat::Live
    } else if d <= i64::from(p.meeting_banner_minutes.max(1)) * MIN {
        Heat::Hot
    } else if d <= i64::from(p.meeting_countdown_minutes.max(1)) * MIN {
        Heat::Warm
    } else {
        Heat::None
    }
}

/// Remember the agenda's join links (for `joined_url` / `link_to_join`).
pub fn note_agenda(events: &[EventRow]) {
    let links: Vec<(String, i64, i64, String)> = events
        .iter()
        .filter_map(|e| meeting_link(&e.event).map(|l| (key(e), e.event.start_at, e.event.end_at, l.url)))
        .collect();
    with(|a| a.links = links);
}

/// The three loudness presets (Settings): Gentle, Standard, Unmissable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Gentle,
    Standard,
    Unmissable,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Gentle, Level::Standard, Level::Unmissable];

    pub fn label(self) -> &'static str {
        match self {
            Level::Gentle => "Gentle",
            Level::Standard => "Standard",
            Level::Unmissable => "Unmissable",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Level::Gentle => "Next up, the menu bar and a notification.",
            Level::Standard => "Also a banner in Coo and a strip while you write.",
            Level::Unmissable => "Also a panel over every app when it starts.",
        }
    }

    /// Set the steps this level stands for.
    pub fn apply(self, p: &mut Prefs) {
        let (banner, panel) = match self {
            Level::Gentle => (false, false),
            Level::Standard => (true, false),
            Level::Unmissable => (true, true),
        };
        p.notify_events = true;
        p.meeting_countdown = true;
        p.meeting_card_warm = true;
        p.meeting_banner = banner;
        p.meeting_composer_strip = banner;
        p.meeting_panel = panel;
        p.meeting_panel_again = panel;
    }

    /// The level the settings match, if any (else they're custom).
    pub fn of(p: &Prefs) -> Option<Level> {
        Level::ALL.into_iter().find(|l| {
            let mut q = p.clone();
            l.apply(&mut q);
            q == *p
        })
    }
}

/// The keycap label of the join shortcut ("" when off).
pub fn hotkey_label(p: &Prefs) -> &'static str {
    HOTKEYS.iter().find(|(v, _)| *v == p.meeting_join_hotkey && !v.is_empty()).map(|(_, l)| *l).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::{Address, Attendee, Event};

    const T: i64 = 1_800_000_000_000;

    fn ev(id: &str, start: i64, others: bool, response: Response, link: bool) -> EventRow {
        let me = Address::new(Some("Sam"), "sam@x.co");
        let attendees = if others {
            vec![
                Attendee { address: me.clone(), response, optional: false },
                Attendee {
                    address: Address::new(Some("Maya Alon"), "maya@y.co"),
                    response: Response::Accepted,
                    optional: false,
                },
            ]
        } else {
            vec![]
        };
        EventRow {
            account_id: "google:sam@x.co".into(),
            event: Event {
                id: id.into(),
                calendar_id: "primary".into(),
                title: format!("Meeting {id}"),
                start_at: start,
                end_at: start + 30 * MIN,
                attendees,
                organizer: if others { Some(Address::new(Some("Maya Alon"), "maya@y.co")) } else { None },
                my_response: response,
                online_url: link.then(|| "https://zoom.us/j/123".to_string()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn who_climbs_how_far() {
        let p = Prefs::default();
        assert_eq!(reach(&ev("a", T, true, Response::Accepted, false), &p), Reach::Full);
        assert_eq!(reach(&ev("b", T, true, Response::Tentative, true), &p), Reach::Notify);
        assert_eq!(reach(&ev("c", T, true, Response::None, true), &p), Reach::Notify);
        assert_eq!(reach(&ev("d", T, false, Response::None, false), &p), Reach::Notify, "just you");
        assert_eq!(reach(&ev("e", T, false, Response::None, true), &p), Reach::Full, "just you, with a link");
        assert_eq!(reach(&ev("f", T, true, Response::Declined, true), &p), Reach::Quiet);
        let quiet = Prefs { meeting_quiet_calendars: vec!["google:sam@x.co|primary".into()], ..Prefs::default() };
        assert_eq!(reach(&ev("a", T, true, Response::Accepted, false), &quiet), Reach::Quiet);
        let loose = Prefs { meeting_alert_tentative: true, meeting_alert_solo: true, ..Prefs::default() };
        assert_eq!(reach(&ev("b", T, true, Response::Tentative, true), &loose), Reach::Full);
        assert_eq!(reach(&ev("d", T, false, Response::None, false), &loose), Reach::Full);
    }

    #[test]
    fn the_ladder_over_time() {
        let p = Prefs::default();
        let a = Alerts::default();
        let events = vec![ev("a", T, true, Response::Accepted, true)];
        // 20 min out: nothing yet.
        assert_eq!(countdown(&events, T - 20 * MIN, &p, &a), None);
        assert_eq!(heat(&events[0], T - 20 * MIN, &p, &a), Heat::None);
        // 8 min out: countdown, warm card, no banner.
        assert_eq!(countdown(&events, T - 8 * MIN, &p, &a).as_deref(), Some("Meeting a · 8m"));
        assert_eq!(heat(&events[0], T - 8 * MIN, &p, &a), Heat::Warm);
        assert!(banner(&events, T - 8 * MIN, &p, &a).is_none());
        // 2 min out: banner, hot card, no panel yet.
        assert!(banner(&events, T - 2 * MIN, &p, &a).is_some());
        assert_eq!(heat(&events[0], T - 2 * MIN, &p, &a), Heat::Hot);
        assert!(panel_due(&events, T - 2 * MIN, &p, &a).is_none());
        // While writing, the strip takes the banner's place.
        assert!(strip(&events, T - 2 * MIN, &p, &a).is_some());
        assert!(strip(&events, T - 3 * MIN, &p, &a).is_none());
        // At the start: panel, strip, "now".
        assert!(panel_due(&events, T, &p, &a).is_some());
        assert!(strip(&events, T, &p, &a).is_some());
        assert_eq!(countdown(&events, T + 10_000, &p, &a).as_deref(), Some("Meeting a · now"));
        assert_eq!(countdown(&events, T + 2 * MIN, &p, &a).as_deref(), Some("Meeting a · 2m late"));
        assert_eq!(heat(&events[0], T + MIN, &p, &a), Heat::Live);
        // Over after the live window.
        assert!(banner(&events, T + 6 * MIN, &p, &a).is_none());
    }

    #[test]
    fn the_panel_comes_back_once() {
        let p = Prefs::default();
        let mut a = Alerts::default();
        let events = vec![ev("a", T, true, Response::Accepted, true)];
        let k = key(&events[0]);
        a.panel_open = Some(k.clone());
        assert!(panel_due(&events, T + 3 * MIN, &p, &a).is_none(), "already up");
        a.panel_open = None;
        a.panel_shown.insert(k.clone(), 1);
        assert!(panel_due(&events, T + MIN, &p, &a).is_none(), "too soon");
        assert!(panel_due(&events, T + 2 * MIN, &p, &a).is_some());
        a.panel_shown.insert(k.clone(), 2);
        assert!(panel_due(&events, T + 3 * MIN, &p, &a).is_none(), "only once more");
        let off = Prefs { meeting_panel_again: false, ..Prefs::default() };
        a.panel_shown.insert(k, 1);
        assert!(panel_due(&events, T + 3 * MIN, &off, &a).is_none());
    }

    #[test]
    fn join_or_dismiss_ends_it() {
        let p = Prefs::default();
        let mut a = Alerts::default();
        let events = vec![ev("a", T, true, Response::Accepted, true)];
        a.done.insert(key(&events[0]));
        assert!(banner(&events, T - MIN, &p, &a).is_none());
        assert!(panel_due(&events, T, &p, &a).is_none());
        assert!(strip(&events, T, &p, &a).is_none());
        assert!(countdown(&events, T - MIN, &p, &a).is_none());
        assert_eq!(heat(&events[0], T, &p, &a), Heat::None);
    }

    #[test]
    fn tentative_stops_at_the_notification() {
        let p = Prefs::default();
        let a = Alerts::default();
        let events = vec![ev("b", T, true, Response::Tentative, true)];
        assert!(countdown(&events, T - 5 * MIN, &p, &a).is_some());
        assert!(banner(&events, T - MIN, &p, &a).is_none());
        assert!(panel_due(&events, T, &p, &a).is_none());
    }

    #[test]
    fn in_one_minute_puts_the_banner_off() {
        let p = Prefs::default();
        let mut a = Alerts::default();
        let events = vec![ev("a", T, true, Response::Accepted, true)];
        a.banner_until.insert(key(&events[0]), T - MIN);
        assert!(banner(&events, T - 2 * MIN, &p, &a).is_none());
        assert!(banner(&events, T - MIN, &p, &a).is_some());
    }

    #[test]
    fn back_to_back_names_the_one_ending() {
        let first = ev("a", T - 30 * MIN, true, Response::Accepted, true);
        let next = ev("b", T, true, Response::Accepted, true);
        let events = vec![first, next];
        let shown = panel_due(&events, T, &Prefs::default(), &Alerts::default()).unwrap();
        assert_eq!(shown.title, "Meeting b");
        assert_eq!(shown.after.as_deref(), Some("Meeting a"));
    }

    #[test]
    fn levels_round_trip() {
        for l in Level::ALL {
            let mut p = Prefs::default();
            l.apply(&mut p);
            assert_eq!(Level::of(&p), Some(l));
        }
        assert_eq!(Level::of(&Prefs::default()), Some(Level::Unmissable), "the default");
        let custom = Prefs { meeting_banner: false, ..Prefs::default() };
        assert_eq!(Level::of(&custom), None);
    }

    #[test]
    fn words() {
        let s = Shown::of(&ev("a", T, true, Response::Accepted, true), &[]);
        assert_eq!(s.people_line(), "Maya Alon");
        assert_eq!(s.join_label(), "Join Zoom");
        assert_eq!(s.when_words(T - 90_000), "in 1 min");
        assert_eq!(s.when_words(T - 40_000), "in 1 min");
        assert_eq!(s.when_words(T), "now");
        assert_eq!(s.when_words(T + 3 * MIN), "started 3 min ago");
        assert_eq!(initials("Maya Alon"), "MA");
        assert_eq!(initials("dan.levi@y.co"), "DL");
        assert_eq!(initials("dan@y.co"), "D");
        assert_eq!(short_title("A very long meeting title that goes on"), "A very long meeting t…");
        assert_eq!(hotkey_label(&Prefs::default()), "⇧⌘J");
    }
}
