//! In-app meeting alerts (meeting_alerts.rs decides when): the banner at
//! the top of Coo a couple of minutes before a meeting, and the strip in a
//! composer when it starts while you're writing.

use dioxus::prelude::*;

use super::Icon;
use super::compose::ComposeSlot;
use crate::meeting_alerts::{self as alerts, Shown};
use crate::state::AppState;

fn events(s: &AppState) -> Vec<pigeon_core::model::EventRow> {
    s.next_up_shown()
}

#[component]
pub fn MeetingBanner() -> Element {
    let s = use_context::<AppState>();
    let _tick = s.meeting_tick.read();
    let now = pigeon_core::now_ms();
    let p = crate::prefs::get();
    let Some(m) = alerts::with(|a| alerts::banner(&events(&s), now, &p, a)) else { return rsx! {} };
    // A message being written here shows the composer strip instead.
    if p.meeting_composer_strip && super::compose::docked_draft(&s).is_some() {
        return rsx! {};
    }
    let hotkey = alerts::hotkey_label(&p);
    let when = m.when_words(now);
    let headline = match when.as_str() {
        "now" => format!("{} is starting", m.title),
        w if w.starts_with("in ") => format!("{} starts {w}", m.title),
        w => format!("{} {w}", m.title),
    };
    let detail = [
        format!("{}–{}", crate::format::hhmm(m.start_at), crate::format::hhmm(m.end_at)),
        m.service.clone(),
        m.people_line(),
    ]
    .into_iter()
    .filter(|x| !x.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let (k1, k2, k3) = (m.key.clone(), m.key.clone(), m.key.clone());
    let url = m.url.clone();
    let started = now >= m.start_at;
    rsx! {
        div { class: "meeting-banner", role: "status", "data-live": started.then_some("true"),
            Icon { name: "clock", class: "icon--sm" }
            div { class: "meeting-banner__text",
                span { class: "meeting-banner__title", dir: "auto", "{headline}" }
                span { class: "meeting-banner__detail", dir: "auto", "{detail}" }
            }
            button { class: "btn btn--ghost btn--sm", r#type: "button", title: "No more alerts about this meeting",
                onclick: move |_| { alerts::mark_done(&k1); s.meeting_changed(); },
                "Not going" }
            if !started {
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| {
                        let until = pigeon_core::now_ms() + 60_000;
                        alerts::with(|a| { a.banner_until.insert(k2.clone(), until); });
                        s.meeting_changed();
                    },
                    "In 1 min" }
            }
            if let Some(url) = url {
                button { class: "btn btn--primary btn--sm", r#type: "button",
                    title: if hotkey.is_empty() { "Join (⇧J)".to_string() } else { format!("Join (⇧J here, {hotkey} from any app)") },
                    onclick: move |_| { alerts::mark_done(&k3); crate::meeting::join(&url); s.meeting_changed(); },
                    Icon { name: "video", class: "icon--sm" }
                    "{m.join_label()}"
                    if !hotkey.is_empty() { kbd { class: "kbd", "{hotkey}" } }
                }
            } else {
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| { alerts::mark_done(&k3); s.meeting_changed(); }, "OK" }
            }
        }
    }
}

/// The meeting the strip shows, polled (compose windows have their own
/// state, so this reads the shared agenda the alert loop keeps).
fn strip_now() -> Option<Shown> {
    let p = crate::prefs::get();
    let events = AGENDA.lock().ok().map(|a| a.clone()).unwrap_or_default();
    alerts::with(|a| alerts::strip(&events, pigeon_core::now_ms(), &p, a))
}

/// The agenda as the main window last saw it, for compose windows.
pub static AGENDA: std::sync::Mutex<Vec<pigeon_core::model::EventRow>> = std::sync::Mutex::new(Vec::new());

#[component]
pub fn ComposeStrip() -> Element {
    let slot = use_context::<ComposeSlot>();
    let mut shown = use_signal(strip_now);
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let now = strip_now();
            if *shown.peek() != now {
                shown.set(now);
            }
        }
    });
    let Some(m) = shown.read().clone() else { return rsx! {} };
    let when = match m.when_words(pigeon_core::now_ms()).as_str() {
        "now" => "is starting".to_string(),
        w if w.starts_with("in ") => format!("starts {w}"),
        w => w.to_string(),
    };
    let (k1, k2) = (m.key.clone(), m.key.clone());
    let url = m.url.clone();
    rsx! {
        div { class: "meeting-strip", role: "status",
            Icon { name: "video", class: "icon--sm" }
            span { class: "meeting-strip__text", dir: "auto", b { "{m.title}" } " {when}" if !m.service.is_empty() { " · {m.service}" } }
            button { class: "btn btn--ghost btn--sm", r#type: "button",
                onclick: move |_| { alerts::with(|a| { a.strip_hidden.insert(k1.clone()); }); shown.set(None); },
                "Keep writing" }
            if let Some(url) = url {
                button { class: "btn btn--primary btn--sm", r#type: "button",
                    onclick: move |_| {
                        let key = k2.clone();
                        let url = url.clone();
                        shown.set(None);
                        spawn(async move {
                            super::compose::save_draft_now(slot).await;
                            alerts::mark_done(&key);
                            crate::meeting::join(&url);
                        });
                    },
                    "Save draft & join"
                }
            }
        }
    }
}
