//! After the first reply (and the first forward) sent from Coo: a small
//! celebration that offers archiving conversations once they're answered
//! (Settings › Writing › After replying / After forwarding).

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, ReplyKind};
use pigeon_engine::service::Cmd;

use crate::send_cmd;
use crate::state::AppState;

/// The conversation just replied to or forwarded, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveOffer {
    pub forward: bool,
    /// Coo's very first message of this kind (otherwise the first since the
    /// setting arrived).
    pub first: bool,
    pub account_id: String,
    pub thread_id: String,
}

/// `draft` was sent: archive its conversation when that's the setting, or
/// make the one-time offer.
pub fn after_sent(s: &mut AppState, draft: &Draft) {
    let Some(r) = &draft.reply else { return };
    let forward = match r.kind {
        ReplyKind::Reply | ReplyKind::ReplyAll => false,
        ReplyKind::Forward => true,
        ReplyKind::Reuse => return,
    };
    let p = crate::prefs::get();
    let (on, asked) = if forward {
        (p.archive_after_forward, p.asked_archive_after_forward)
    } else {
        (p.archive_after_reply, p.asked_archive_after_reply)
    };
    if on {
        archive(*s, &r.account_id, &r.thread_id, forward);
    } else if !asked {
        crate::prefs::update(|p| {
            if forward {
                p.asked_archive_after_forward = true;
            } else {
                p.asked_archive_after_reply = true;
            }
        });
        let first = p.milestones.sent <= 1;
        s.archive_offer.set(Some(ArchiveOffer {
            forward,
            first,
            account_id: r.account_id.clone(),
            thread_id: r.thread_id.clone(),
        }));
    }
}

/// Archive the conversation (its Inbox messages); when it's the one open,
/// move on the way archiving it by hand would.
fn archive(s: AppState, account_id: &str, thread_id: &str, forward: bool) {
    let key = format!("{account_id}|{thread_id}");
    if s.selected.peek().as_deref() == Some(key.as_str())
        && let Some(item) = s.visible_items().into_iter().find(|i| AppState::key(i) == key)
    {
        super::step_off(s, &item);
    }
    let label = if forward { "Archived after forwarding" } else { "Archived after replying" };
    send_cmd(Cmd::ArchiveThread { account_id: account_id.into(), thread_id: thread_id.into(), label: label.into() });
}

/// Where the confetti sits around the pigeon: left, top (px), color, turn.
const CONFETTI: [(u32, u32, &str, i32); 10] = [
    (14, 18, "accent", -30),
    (40, 4, "warn", 20),
    (112, 10, "blue", 50),
    (146, 26, "accent", -15),
    (160, 70, "warn", 70),
    (6, 74, "blue", 10),
    (28, 100, "warn", -60),
    (134, 104, "accent", 35),
    (80, 0, "accent", 0),
    (96, 112, "blue", -40),
];

#[component]
pub fn ArchiveOfferDialog() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.archive_offer;
    let Some(offer) = open.read().clone() else { return rsx! {} };
    let (title, yes) = match (offer.forward, offer.first) {
        (false, true) => ("First reply sent!", "Archive after replying"),
        (false, false) => ("Reply sent!", "Archive after replying"),
        (true, true) => ("First forward sent!", "Archive after forwarding"),
        (true, false) => ("Forward sent!", "Archive after forwarding"),
    };
    let question = if offer.forward {
        "From now on, should Coo archive a conversation once you forward it?"
    } else {
        "From now on, should Coo archive a conversation once you reply to it?"
    };
    let badge = if offer.forward { "forward" } else { "reply" };
    let o2 = offer.clone();
    rsx! {
        div { class: "agent-setup-scrim archive-offer-scrim", onclick: move |_| open.set(None),
            div { class: "dialog archive-offer", role: "dialog", "aria-label": "{title}", "aria-modal": "true",
                onclick: move |e| e.stop_propagation(),
                div { class: "archive-offer__art", "aria-hidden": "true",
                    for (i, (x, y, color, turn)) in CONFETTI.into_iter().enumerate() {
                        span { key: "{i}", class: "archive-offer__bit", "data-color": color,
                            style: "left:{x}px;top:{y}px;--turn:{turn}deg;--i:{i}",
                        }
                    }
                    div { class: "archive-offer__bird",
                        super::bird::Bird { pose: super::bird::Pose::Awake }
                        span { class: "milestone__badge", super::Icon { name: badge, class: "icon--xs" } }
                    }
                }
                div { class: "archive-offer__text",
                    div { class: "archive-offer__title", "{title}" }
                    div { class: "archive-offer__ask", "{question}" }
                }
                div { class: "archive-offer__actions",
                    button { class: "btn btn--primary", r#type: "button", autofocus: true,
                        onclick: move |_| {
                            let forward = o2.forward;
                            crate::prefs::update(|p| {
                                if forward {
                                    p.archive_after_forward = true;
                                } else {
                                    p.archive_after_reply = true;
                                }
                            });
                            open.set(None);
                            archive(s, &o2.account_id, &o2.thread_id, forward);
                        },
                        "{yes}"
                    }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| open.set(None), "Keep in Inbox" }
                }
                div { class: "archive-offer__foot", "Change it anytime in Settings › Writing." }
            }
        }
    }
}
