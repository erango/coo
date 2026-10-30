//! The milestone card: a small, one-time note with a pigeon doodle, above
//! the toasts (milestones.rs decides when). It hides itself after a few
//! seconds unless the pointer rests on it.

use dioxus::prelude::*;

use super::Icon;
use crate::milestones::{Milestone, MilestoneState};

/// The card on screen (with a counter, so a new one starts fresh).
static CARD: GlobalSignal<Option<(u64, &'static Milestone)>> = Signal::global(|| None);

/// How long the card stays when left alone.
const SHOW_MS: u64 = 8_000;

/// Update the milestone counts (`f` gets them and the time), then show the
/// card that's due, if any. Prefs are written only when something changed.
pub fn record(f: impl FnOnce(&mut MilestoneState, i64)) {
    let now = pigeon_core::now_ms();
    let before = crate::prefs::get().milestones;
    let mut m = before.clone();
    f(&mut m, now);
    let due = m.take_due(now);
    if m != before {
        crate::prefs::update(move |p| p.milestones = m);
    }
    if let Some(card) = due {
        show(card);
    }
}

/// The card on screen's counter, if one is.
fn shown_key() -> Option<u64> {
    (*CARD.peek()).map(|(n, _)| n)
}

fn show(card: &'static Milestone) {
    let n = shown_key().unwrap_or(0) + 1;
    *CARD.write() = Some((n, card));
}

/// Debug: show a milestone's card (screenshots), a few seconds after
/// launch so it's on screen once the window has painted.
pub fn show_id(id: &str) {
    if let Some(card) = crate::milestones::find(id) {
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            show(card);
        });
    }
}

/// Messages archived from Coo: a thread counts its messages.
pub fn archived(items: &[pigeon_engine::service::ListItem]) {
    let n: u64 = items
        .iter()
        .map(|i| match i {
            pigeon_engine::service::ListItem::Thread(t) => u64::from(t.message_count.max(1)),
            pigeon_engine::service::ListItem::Message(_) => 1,
        })
        .sum();
    record(|m, now| m.archived(n, now));
}

/// Today in local time.
pub fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

#[component]
pub fn MilestoneCard() -> Element {
    let card = *CARD.read();
    let mut hovered = use_signal(|| false);
    let mut leaving = use_signal(|| false);
    let key = card.map(|(n, _)| n);
    use_effect(use_reactive!(|key| {
        let Some(key) = key else { return };
        leaving.set(false);
        spawn(async move {
            let mut left = SHOW_MS;
            while left > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                if shown_key() != Some(key) {
                    return;
                }
                if !*hovered.peek() {
                    left = left.saturating_sub(250);
                }
            }
            dismiss(leaving, key).await;
        });
    }));
    let Some((key, m)) = card else { return rsx! {} };
    rsx! {
        div { class: "milestone", role: "status", key: "{key}",
            "data-leaving": leaving.read().then_some("true"),
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            div { class: "milestone__doodle", "aria-hidden": "true",
                svg { view_box: "4 4 60 56", dangerous_inner_html: super::list::AWAKE_BIRD }
                span { class: "milestone__badge", Icon { name: m.badge, class: "icon--xs" } }
            }
            span { class: "milestone__line", "{m.line}" }
            button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Close", title: "Close",
                onclick: move |_| { spawn(dismiss(leaving, key)); },
                Icon { name: "x", class: "icon--xs" }
            }
        }
    }
}

/// Slide the card away, then remove it (unless a newer one took its place).
async fn dismiss(mut leaving: Signal<bool>, key: u64) {
    leaving.set(true);
    tokio::time::sleep(std::time::Duration::from_millis(240)).await;
    if shown_key() == Some(key) {
        *CARD.write() = None;
    }
}
