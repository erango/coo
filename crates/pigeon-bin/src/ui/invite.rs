//! Meeting invites in mail: the reading-pane card (with a mini day view of
//! your other events) and the inline invite on list rows. RSVPs go through
//! the calendar API against the synced event the invite resolves to.

use dioxus::prelude::*;
use pigeon_core::model::{EventRow, Invite, InviteMethod, Response};
use pigeon_engine::service::{Cmd, InviteView};

use super::Icon;
use crate::format;
use crate::send_cmd;
use crate::state::AppState;

const HOUR_MS: i64 = 3_600_000;
/// Mini day: 40 px per hour, about 3¾ hours visible.
const MINI_HOUR_PX: f64 = 40.0;
const MINI_HOURS: i64 = 4;

pub fn meeting_service(url: &str) -> &'static str {
    if url.contains("zoom.us") {
        "Zoom"
    } else if url.contains("meet.google") {
        "Google Meet"
    } else if url.contains("teams.microsoft") || url.contains("teams.live") {
        "Teams"
    } else {
        "Video call"
    }
}

/// Start of the mini-day window: the hour before the invite starts.
pub fn window_start(start_ms: i64) -> i64 {
    let local = format::local(start_ms);
    let into_hour = local
        .map(|t| i64::from(chrono::Timelike::minute(&t)) * 60_000 + i64::from(chrono::Timelike::second(&t)) * 1000)
        .unwrap_or(0);
    start_ms - into_hour - HOUR_MS
}

/// Longest overlap (minutes) between the invite and any event you haven't declined.
pub fn overlap_minutes(start: i64, end: i64, busy: &[EventRow]) -> i64 {
    busy.iter()
        .filter(|b| b.event.my_response != Response::Declined)
        .map(|b| (end.min(b.event.end_at) - start.max(b.event.start_at)).max(0))
        .max()
        .unwrap_or(0)
        / 60_000
}

/// (top, height) in px for an interval inside the mini-day window, clipped.
pub fn mini_box(win: i64, start: i64, end: i64) -> Option<(f64, f64)> {
    let win_end = win + MINI_HOURS * HOUR_MS;
    let (s, e) = (start.max(win), end.min(win_end));
    if e <= s {
        return None;
    }
    let px = |ms: i64| (ms - win) as f64 / HOUR_MS as f64 * MINI_HOUR_PX;
    Some((px(s), (px(e) - px(s)).max(12.0)))
}

fn when_text(inv: &Invite) -> (String, String) {
    let day = format::local(inv.start_at).map(|d| d.format("%a, %b %-d").to_string()).unwrap_or_default();
    if inv.all_day {
        return (day, "all day".into());
    }
    (day, format!("{}–{}", format::hhmm(inv.start_at), format::hhmm(inv.end_at)))
}

/// The response to show: what we just sent, else what the calendar says.
fn my_response(s: &AppState, ev: &EventRow) -> Response {
    let key = format!("{}|{}", ev.account_id, ev.event.id);
    s.rsvp_sent.read().get(&key).copied().unwrap_or(ev.event.my_response)
}

fn rsvp(s: AppState, ev: &EventRow, response: Response) {
    let mut sent = s.rsvp_sent;
    sent.write().insert(format!("{}|{}", ev.account_id, ev.event.id), response);
    send_cmd(Cmd::Rsvp {
        account_id: ev.account_id.clone(),
        calendar_id: ev.event.calendar_id.clone(),
        event_id: ev.event.id.clone(),
        response,
    });
}

#[component]
fn RsvpButtons(ev: EventRow, small: bool) -> Element {
    let s = use_context::<AppState>();
    let (a, m, d) = (ev.clone(), ev.clone(), ev.clone());
    rsx! {
        button { class: if small { "btn btn--soft" } else { "btn btn--primary" }, r#type: "button",
            onclick: move |e| { e.stop_propagation(); rsvp(s, &a, Response::Accepted) },
            if !small { Icon { name: "check", class: "icon--sm" } }
            "Accept"
        }
        button { class: "btn btn--secondary", r#type: "button",
            onclick: move |e| { e.stop_propagation(); rsvp(s, &m, Response::Tentative) }, "Maybe" }
        button { class: "btn btn--secondary", r#type: "button",
            onclick: move |e| { e.stop_propagation(); rsvp(s, &d, Response::Declined) }, "Decline" }
    }
}

#[component]
pub fn InviteCard(account_id: String, invite: Invite, view: InviteView) -> Element {
    let s = use_context::<AppState>();
    let mut changing = use_signal(|| false);
    let ev = view.event.clone();
    let hue = s.hue(ev.as_ref().map(|e| e.account_id.as_str()).unwrap_or(&account_id));
    let cancelled = invite.method == InviteMethod::Cancel || ev.as_ref().is_some_and(|e| e.event.cancelled);
    let (day, time) = when_text(&invite);
    let url = invite.online_url.clone().or_else(|| ev.as_ref().and_then(|e| e.event.online_url.clone()));
    let link_text: String =
        url.as_deref().unwrap_or_default().trim_start_matches("https://").chars().take(42).collect();
    let title = if invite.title.is_empty() {
        ev.as_ref().map(|e| e.event.title.clone()).unwrap_or_default()
    } else {
        invite.title.clone()
    };

    // "Priya (organizer), Jonah, Lena +2"
    let organizer = invite.organizer.clone().or_else(|| ev.as_ref().and_then(|e| e.event.organizer.clone()));
    let mine: Vec<String> = s.accounts.read().iter().map(|a| a.account.email.clone()).collect();
    let mut people: Vec<String> = organizer.iter().map(|o| format!("{} (organizer)", o.display())).collect();
    if let Some(e) = &ev {
        let others: Vec<String> = e
            .event
            .attendees
            .iter()
            .filter(|a| organizer.as_ref().is_none_or(|o| !o.email.eq_ignore_ascii_case(&a.address.email)))
            .filter(|a| !mine.iter().any(|m| m.eq_ignore_ascii_case(&a.address.email)))
            .map(|a| a.address.display().split_whitespace().next().unwrap_or_default().to_string())
            .collect();
        let shown = others.len().min(3);
        people.extend(others[..shown].iter().cloned());
        if others.len() > shown {
            people.push(format!("+{}", others.len() - shown));
        }
    }

    let response = ev.as_ref().map(|e| my_response(&s, e));
    let can_rsvp =
        !cancelled && invite.method == InviteMethod::Request && ev.is_some() && response != Some(Response::Organizer);
    let answered = matches!(response, Some(Response::Accepted | Response::Tentative | Response::Declined));

    let win = window_start(invite.start_at);
    let overlap = if cancelled { 0 } else { overlap_minutes(invite.start_at, invite.end_at, &view.busy) };
    let proposed = mini_box(win, invite.start_at, invite.end_at);
    let busy: Vec<(EventRow, (f64, f64))> = view
        .busy
        .iter()
        .filter_map(|b| mini_box(win, b.event.start_at, b.event.end_at).map(|bx| (b.clone(), bx)))
        .collect();

    rsx! {
        div { class: "invite-card", "data-hue": "{hue}", "data-cancelled": cancelled.then_some("true"),
            div { class: "invite-card__main",
                div { class: "invite-card__title",
                    Icon { name: "cal" }
                    span { dir: "auto", style: if cancelled { "text-decoration:line-through" } else { "" }, "{title}" }
                    if cancelled { span { class: "tag", "Canceled" } }
                }
                div { class: "invite-card__facts",
                    span { b { "{day}" } " · {time}" }
                    if let Some(u) = url.clone() {
                        span { style: "display:flex;align-items:center;gap:6px",
                            Icon { name: "video", class: "icon--sm" }
                            "{meeting_service(&u)} · "
                            a { class: "t-mono", href: "#",
                                onclick: move |e| { e.prevent_default(); crate::meeting::join(&u); },
                                {link_text.clone()}
                            }
                        }
                    }
                    if let Some(loc) = invite.location.clone().filter(|l| url.as_deref() != Some(l.as_str())) {
                        span { dir: "auto", "{loc}" }
                    }
                    if !people.is_empty() { span { dir: "auto", {people.join(", ")} } }
                }
                if can_rsvp {
                    if answered && !*changing.read() {
                        div { class: "invite-card__actions",
                            span { class: "invite-card__answered",
                                Icon { name: "check", class: "icon--sm" }
                                match response {
                                    Some(Response::Accepted) => "Going",
                                    Some(Response::Tentative) => "Maybe",
                                    _ => "Not going",
                                }
                            }
                            button { class: "btn btn--ghost", r#type: "button", onclick: move |_| changing.set(true), "Change" }
                        }
                    } else if let Some(e) = ev.clone() {
                        div { class: "invite-card__actions", onclick: move |_| changing.set(false),
                            RsvpButtons { ev: e, small: false }
                        }
                    }
                } else if !cancelled && invite.method == InviteMethod::Request && ev.is_none() {
                    div { class: "t-caption", "Not in your calendar yet. It shows up after the next sync, then you can reply here." }
                }
            }
            if !invite.all_day {
                div { class: "mini-day", "aria-label": "Your day around this meeting",
                    for h in 0..MINI_HOURS {
                        div { class: "mini-day__hour", key: "{h}", style: "top:{h as f64 * MINI_HOUR_PX}px",
                            span { "{format::hhmm(win + h * HOUR_MS)}" }
                        }
                    }
                    for (b, (top, height)) in busy.iter().cloned() {
                        div { class: "mini-day__event", key: "{b.account_id}{b.event.id}", "data-hue": "{s.hue(&b.account_id)}",
                            style: "top:{top}px;height:{height}px;width:64px", dir: "auto",
                            "{b.event.title}"
                        }
                    }
                    if let Some((top, height)) = proposed {
                        div { class: "mini-day__event mini-day__event--proposed",
                            style: if busy.is_empty() { format!("top:{top}px;height:{height}px;right:0") } else { format!("top:{top}px;height:{height}px;left:104px;right:0") },
                            dir: "auto", "{title}"
                        }
                    }
                    if overlap > 0 {
                        div { class: "mini-day__conflict", Icon { name: "alert", class: "icon--xs" } "Overlaps {overlap} min" }
                    }
                }
            }
        }
    }
}

/// Inline invite on a list row (only when the body is already loaded).
#[component]
pub fn RowInvite(invite: Invite, event: Option<EventRow>) -> Element {
    let s = use_context::<AppState>();
    let (day, time) = when_text(&invite);
    let url = invite.online_url.clone().or_else(|| event.as_ref().and_then(|e| e.event.online_url.clone()));
    let service = url.as_deref().map(|u| format!(" · {}", meeting_service(u))).unwrap_or_default();
    let response = event.as_ref().map(|e| my_response(&s, e));
    let pending = matches!(response, Some(Response::None));
    rsx! {
        div { class: "row__invite",
            span { class: "row__invite-when", "{day} · {time}{service}" }
            if pending && invite.method == InviteMethod::Request {
                if let Some(e) = event { RsvpButtons { ev: e, small: true } }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::Event;

    fn row(start: i64, end: i64, r: Response) -> EventRow {
        EventRow {
            account_id: "a".into(),
            event: Event { start_at: start, end_at: end, my_response: r, ..Default::default() },
        }
    }

    #[test]
    fn overlap_ignores_declined() {
        let (s, e) = (10 * HOUR_MS, 11 * HOUR_MS);
        let busy = vec![row(9 * HOUR_MS, s + 15 * 60_000, Response::Accepted), row(s, e, Response::Declined)];
        assert_eq!(overlap_minutes(s, e, &busy), 15);
        assert_eq!(overlap_minutes(s, e, &[]), 0);
    }

    #[test]
    fn mini_boxes_clip_to_window() {
        let win = 0;
        assert_eq!(mini_box(win, HOUR_MS, 2 * HOUR_MS), Some((40.0, 40.0)));
        assert_eq!(mini_box(win, -HOUR_MS, HOUR_MS / 2), Some((0.0, 20.0)));
        assert_eq!(mini_box(win, 5 * HOUR_MS, 6 * HOUR_MS), None);
    }

    #[test]
    fn services() {
        assert_eq!(meeting_service("https://meet.google.com/x"), "Google Meet");
        assert_eq!(meeting_service("https://us02web.zoom.us/j/1"), "Zoom");
        assert_eq!(meeting_service("https://teams.microsoft.com/l/meetup-join/x"), "Teams");
    }
}
