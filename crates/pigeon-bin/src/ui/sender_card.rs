//! Sender / recipient ID card: hover (or focus) a name to see who it really
//! is: the full address with its domain highlighted, where replies would
//! go, which of your addresses it was delivered to, SPF/DKIM/DMARC, whether
//! you've had mail from them before, and anything that looks like phishing
//! (pigeon_core::identity). Wrap a name in `Person`; `SenderCard` renders the
//! one open card.

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dioxus::prelude::*;
use pigeon_core::identity::{self, Warning};
use pigeon_core::model::Address;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::format;
use crate::send_cmd;
use crate::state::AppState;

const OPEN_DELAY: Duration = Duration::from_millis(380);
const CLOSE_DELAY: Duration = Duration::from_millis(220);

/// Bumped on every open/close request; a timer only acts if it's still the
/// latest, so moving between name and card never flickers.
static GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq)]
pub struct CardState {
    pub account_id: String,
    pub message_id: Option<String>,
    pub addr: Address,
    /// The message's sender (vs. one of its recipients).
    pub sender: bool,
    /// Anchor's bottom-left, in viewport px.
    pub x: f64,
    pub y: f64,
    /// The window the anchor is in (message windows have their own card);
    /// None = the main window.
    pub window: Option<dioxus::desktop::tao::window::WindowId>,
}

fn open_after(s: AppState, el: Option<Rc<MountedData>>, mut card: CardState, delay: Duration) {
    let generation = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    spawn(async move {
        tokio::time::sleep(delay).await;
        if GEN.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Some(el) = el
            && let Ok(r) = el.get_client_rect().await
        {
            card.x = r.min_x();
            card.y = r.max_y();
        }
        let key = (card.account_id.clone(), card.addr.email.to_lowercase());
        if !s.sender_seen.read().contains_key(&key) {
            send_cmd(Cmd::SenderSeen { account_id: card.account_id.clone(), email: card.addr.email.clone() });
        }
        let mut c = s.sender_card;
        c.set(Some(card));
    });
}

pub fn close_after(s: AppState, delay: Duration) {
    let generation = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    spawn(async move {
        tokio::time::sleep(delay).await;
        if GEN.load(Ordering::SeqCst) == generation {
            let mut c = s.sender_card;
            c.set(None);
        }
    });
}

/// Keep the card open (pointer moved onto it).
fn keep_open() {
    GEN.fetch_add(1, Ordering::SeqCst);
}

/// A name that shows the ID card on hover or keyboard focus.
#[component]
pub fn Person(
    account_id: String,
    message_id: Option<String>,
    addr: Address,
    #[props(default)] sender: bool,
    #[props(default)] class: String,
    /// Let clicks reach the parent (a list row selects on click).
    #[props(default)]
    click_through: bool,
    children: Element,
) -> Element {
    let s = use_context::<AppState>();
    let mut el = use_signal(|| None::<Rc<MountedData>>);
    let window = Some(dioxus::desktop::window().id());
    let card = CardState { account_id, message_id, addr, sender, x: 0.0, y: 0.0, window };
    let (c1, c2) = (card.clone(), card);
    rsx! {
        // List rows are navigated with J/K, so their names aren't tab stops.
        span { class: "person {class}", tabindex: if click_through { "-1" } else { "0" }, role: "button", "aria-haspopup": "dialog",
            onmounted: move |e| el.set(Some(e.data())),
            onmouseenter: move |e| {
                let mut c = c1.clone();
                let p = e.client_coordinates();
                (c.x, c.y) = (p.x, p.y + 12.0);
                open_after(s, el.read().clone(), c, OPEN_DELAY);
            },
            onmouseleave: move |_| close_after(s, CLOSE_DELAY),
            onfocus: move |_| open_after(s, el.read().clone(), c2.clone(), Duration::ZERO),
            onblur: move |_| close_after(s, CLOSE_DELAY),
            onkeydown: move |e| if e.key() == Key::Escape { close_after(s, Duration::ZERO) },
            // Don't open the message row / expand the message underneath.
            onclick: move |e| {
                if !click_through {
                    e.stop_propagation();
                }
            },
            {children}
        }
    }
}

fn warning_text(w: &Warning) -> String {
    match w {
        Warning::AuthFailed => {
            "This message failed sender verification (SPF/DKIM/DMARC). It may not be from who it says.".into()
        }
        Warning::NameShowsOtherAddress(a) => format!("The name shows “{a}”, but the real address is different."),
        Warning::BrandMismatch(b) => {
            format!("The name mentions {}, but the address isn't one of theirs.", title_case(b))
        }
        Warning::ReplyToDiffers(a) => format!("Replies would go to {a}, not to the sender."),
        Warning::InternationalDomain(d) => {
            format!("The domain uses international characters: {d}. Check it isn't a lookalike.")
        }
        Warning::FirstTime => "First message from this address.".into(),
    }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The open card (rendered once, at the app root).
#[component]
pub fn SenderCard() -> Element {
    let s = use_context::<AppState>();
    let Some(card) = s.sender_card.read().clone() else { return rsx! {} };
    if !card.window.is_none_or(|w| w == dioxus::desktop::window().id()) {
        return rsx! {};
    }
    let addr = card.addr.clone();
    let email = addr.email.clone();
    let name = addr.name.clone().filter(|n| !n.trim().is_empty() && !n.eq_ignore_ascii_case(&email));
    let (local, domain) =
        email.rsplit_once('@').map(|(l, d)| (l.to_string(), d.to_string())).unwrap_or((email.clone(), String::new()));
    let (shown_domain, puny) = identity::display_domain(&domain.to_ascii_lowercase());
    let hue = s.hue(&card.account_id);
    let account_email = s
        .accounts
        .read()
        .iter()
        .find(|a| a.account.id == card.account_id)
        .map(|a| a.account.email.clone())
        .unwrap_or_default();
    let origin = card
        .message_id
        .as_ref()
        .and_then(|m| s.bodies.read().get(&(card.account_id.clone(), m.clone())).and_then(|b| b.origin.clone()));
    let seen = s.sender_seen.read().get(&(card.account_id.clone(), email.to_lowercase())).copied();
    // Warnings are about the sender; a recipient card only shows the facts.
    let warnings = if card.sender { identity::warnings(&addr, origin.as_ref(), seen) } else { Vec::new() };
    let verified = card.sender && identity::is_verified(&addr, origin.as_ref(), &warnings);
    let verified_tip = format!(
        "Verified sender\nThis message really came from {shown_domain}: SPF, DKIM and DMARC all passed.\n\
         It confirms the domain, not who's behind it. Check it's the domain you expect."
    );
    let serious = warnings.iter().any(|w| !matches!(w, Warning::FirstTime));
    let delivered = origin
        .as_ref()
        .and_then(|o| o.delivered_to.clone())
        .filter(|d| card.sender && !d.eq_ignore_ascii_case(&account_email));
    let auth = origin.as_ref().and_then(|o| o.auth.clone()).filter(|_| card.sender);
    let reply_to = origin.as_ref().map(|o| o.reply_to.clone()).unwrap_or_default();
    let reply_to = reply_to.into_iter().find(|r| card.sender && !r.email.eq_ignore_ascii_case(&email));
    let left = card.x.max(8.0);
    let top = card.y + 6.0;
    let (copy_email, search_email) = (email.clone(), email.clone());

    rsx! {
        div { class: "popover sender-card", role: "dialog", "aria-label": "Sender details",
            "data-alert": serious.then_some("true"),
            style: "left:min({left}px, calc(100vw - 372px));top:min({top}px, calc(100vh - 340px))",
            onmouseenter: move |_| keep_open(),
            onmouseleave: move |_| close_after(s, CLOSE_DELAY),
            onfocusin: move |_| keep_open(),
            onfocusout: move |_| close_after(s, CLOSE_DELAY),
            div { class: "sender-card__head",
                super::avatar::Avatar { email: email.clone(), label: name.clone().unwrap_or_else(|| email.clone()), hue, class: "avatar--40" }
                div { class: "sender-card__who",
                    div { class: "sender-card__name-row",
                        div { class: "sender-card__name", dir: "auto", "{name.clone().unwrap_or_else(|| email.clone())}" }
                        if verified {
                            span { class: "verified-badge", title: "{verified_tip}", "aria-label": "Verified sender",
                                Icon { name: "check", class: "icon--xs" }
                            }
                        }
                    }
                    div { class: "sender-card__addr", title: "{email}",
                        span { class: "sender-card__local", "{local}@" }
                        b { class: "sender-card__domain", "data-puny": puny.then_some("true"), "{shown_domain}" }
                    }
                    if puny { div { class: "sender-card__raw", "{domain}" } }
                }
                span { class: "sender-card__role", if card.sender { "From" } else { "Recipient" } }
            }
            if !warnings.is_empty() {
                div { class: "sender-card__warnings",
                    for (i, w) in warnings.iter().enumerate() {
                        {
                            let info = matches!(w, Warning::FirstTime);
                            let icon: &'static str = if info { "users" } else { "alert" };
                            rsx! {
                                div { key: "{i}", class: "sender-card__warning", "data-level": if info { "info" } else { "alert" },
                                    Icon { name: icon, class: "icon--sm" }
                                    span { "{warning_text(w)}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "sender-card__facts",
                if let Some(r) = reply_to {
                    div { class: "sender-card__fact", "data-alert": "true",
                        span { class: "sender-card__label", "Reply-To" }
                        span { class: "sender-card__value", dir: "auto", "{format::with_address(&r)}" }
                    }
                }
                if !account_email.is_empty() {
                    div { class: "sender-card__fact",
                        span { class: "sender-card__label", if card.sender { "Received by" } else { "In account" } }
                        span { class: "sender-card__value", span { class: "dot dot--sm", "data-hue": "{hue}" } "{account_email}" }
                    }
                }
                if let Some(d) = delivered {
                    div { class: "sender-card__fact",
                        span { class: "sender-card__label", "Delivered to" }
                        span { class: "sender-card__value", "{d}" }
                    }
                }
                if let Some(a) = auth {
                    div { class: "sender-card__fact",
                        span { class: "sender-card__label", "Verified" }
                        span { class: "sender-card__value sender-card__auth",
                            for (k, v) in [("SPF", a.spf.clone()), ("DKIM", a.dkim.clone()), ("DMARC", a.dmarc.clone())] {
                                if let Some(v) = v {
                                    span { key: "{k}", class: "auth-chip", "data-result": "{v}", title: "{k}: {v}", "{k}" }
                                }
                            }
                        }
                    }
                }
                if card.sender && let Some(n) = seen.filter(|n| *n > 0) {
                    div { class: "sender-card__fact",
                        span { class: "sender-card__label", "History" }
                        span { class: "sender-card__value", if n == 1 { "1 message in this account" } else { "{n} messages in this account" } }
                    }
                }
            }
            div { class: "sender-card__actions",
                button { class: "btn btn--ghost btn--sm", r#type: "button",
                    onclick: move |_| {
                        let js = format!("navigator.clipboard.writeText({})", serde_json::to_string(&copy_email).unwrap_or_default());
                        document::eval(&js);
                        s.local_toast("Address copied", false);
                    },
                    Icon { name: "copy", class: "icon--sm" } "Copy address"
                }
                button { class: "btn btn--ghost btn--sm", r#type: "button",
                    onclick: move |_| {
                        let mut c = s.sender_card;
                        c.set(None);
                        super::search::open(s, &format!("from:{search_email}"), pigeon_engine::service::SearchScope::All);
                    },
                    Icon { name: "search", class: "icon--sm" } "Mail from this sender"
                }
            }
        }
    }
}
