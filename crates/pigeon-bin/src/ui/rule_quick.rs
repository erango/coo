//! Rules made from a message: the row menu's "Always for <sender>" opens a
//! short confirm ("Always archive mail from Rescana?"), and the nudge
//! offers one after you clear a sender's mail a few times without opening
//! it. Archiving something after reading it is normal; archiving it
//! unopened says what you think of the sender.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use dioxus::prelude::*;
use pigeon_core::rules::{Rule, Runs, Then, When};
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ListItem};

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

/// What a quick rule does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Archive,
    Read,
    Trash,
}

impl Kind {
    fn verb(self) -> &'static str {
        match self {
            Kind::Archive => "archive",
            Kind::Read => "mark read",
            Kind::Trash => "move to Trash",
        }
    }

    fn then(self) -> Then {
        match self {
            Kind::Archive => Then { archive: true, ..Default::default() },
            Kind::Read => Then { read: true, ..Default::default() },
            Kind::Trash => Then { trash: true, ..Default::default() },
        }
    }
}

/// The confirm's subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Quick {
    pub account_id: String,
    pub email: String,
    /// The sender's name, or their address.
    pub who: String,
    pub kind: Kind,
}

/// The nudge's subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Nudge {
    pub quick: Quick,
    /// How many unopened ones were cleared.
    pub count: usize,
    pub what: &'static str,
}

/// Messages opened in the reader (account|id).
static OPENED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Per sender (lowercased): when you cleared their unopened mail.
static CLEARED: Mutex<Option<HashMap<String, Vec<i64>>>> = Mutex::new(None);
/// Senders offered a rule this session.
static OFFERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The nudge comes after this many unopened clears in [`WINDOW_MS`].
const THRESHOLD: usize = 3;
const WINDOW_MS: i64 = 7 * 24 * 3_600_000;

/// The reader showed these messages.
pub fn opened(account_id: &str, ids: impl IntoIterator<Item = String>) {
    let mut o = OPENED.lock().unwrap_or_else(|p| p.into_inner());
    let set = o.get_or_insert_with(HashSet::new);
    for id in ids {
        set.insert(format!("{account_id}|{id}"));
    }
}

/// Whether clearing `item` with `action` says something about its sender:
/// it's unread and was never opened here.
fn unopened(item: &ListItem) -> bool {
    let unread = match item {
        ListItem::Thread(t) => t.unread_count > 0,
        ListItem::Message(m) => !m.header.is_read,
    };
    let key = format!("{}|{}", item.account_id(), item.header().id);
    unread && !OPENED.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|o| o.contains(&key))
}

/// Count of a sender's unopened clears in the window, after adding `now`.
/// Pure, for the tests.
fn bump(times: &mut Vec<i64>, now: i64) -> usize {
    times.retain(|t| now - t < WINDOW_MS);
    times.push(now);
    times.len()
}

/// The user archived, deleted or marked read `items`: maybe offer a rule.
pub fn acted(mut s: AppState, items: &[ListItem], action: &MailAction) {
    let (kind, what) = match action {
        MailAction::Archive => (Kind::Archive, "archived"),
        MailAction::Trash => (Kind::Trash, "deleted"),
        MailAction::MarkRead => (Kind::Read, "marked read"),
        _ => return,
    };
    let p = crate::prefs::get();
    if !p.rule_suggestions {
        return;
    }
    let now = pigeon_core::now_ms();
    for item in items.iter().filter(|i| unopened(i)) {
        let Some(from) = item.header().from.clone() else { continue };
        let email = from.email.to_lowercase();
        if p.rule_nudge_dismissed.contains(&email) || has_rule(&s, &email) {
            continue;
        }
        let n = {
            let mut c = CLEARED.lock().unwrap_or_else(|p| p.into_inner());
            bump(c.get_or_insert_with(HashMap::new).entry(email.clone()).or_default(), now)
        };
        if n < THRESHOLD {
            continue;
        }
        let fresh =
            OFFERED.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(email.clone());
        if fresh {
            let who = from.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| from.email.clone());
            let quick = Quick { account_id: item.account_id().to_string(), email, who, kind };
            s.rule_nudge.set(Some(Nudge { quick, count: n, what }));
            return;
        }
    }
}

/// A rule already names this sender (or their domain).
fn has_rule(s: &AppState, email: &str) -> bool {
    s.rules.peek().iter().any(|r| r.when.from.iter().any(|f| pigeon_core::rules::address_matches(f, email, "")))
}

/// The rule a quick confirm makes.
fn rule_for(q: &Quick, domain: bool) -> Rule {
    let who = if domain {
        crate::vip::domain_of(&q.email).map(|d| format!("@{d}")).unwrap_or_else(|| q.email.clone())
    } else {
        q.email.clone()
    };
    let when = When { from: vec![who], ..Default::default() };
    let mut r = Rule::new("", Some(q.account_id.clone()), when, q.kind.then(), pigeon_core::now_ms());
    r.name = pigeon_engine::rules::default_name(&r);
    if !domain {
        r.name = q.who.clone();
    }
    r
}

/// The full editor, filled in with the sender (their domain when it's a
/// company's).
pub fn open_editor(s: AppState, q: &Quick) {
    let domain = crate::vip::domain_of(&q.email).is_some_and(|d| !crate::vip::is_shared_domain(&d));
    let mut r = rule_for(q, domain);
    r.then = Then::default();
    super::rule_editor::open(s, r, true);
}

static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);

#[component]
pub fn QuickRule() -> Element {
    let s = use_context::<AppState>();
    let Some(q) = s.quick_rule.read().clone() else { return rsx! {} };
    rsx! { QuickSheet { key: "{q.email}{q.kind:?}", q } }
}

#[component]
fn QuickSheet(q: Quick) -> Element {
    let mut s = use_context::<AppState>();
    let domain_name = crate::vip::domain_of(&q.email).filter(|d| !crate::vip::is_shared_domain(d));
    let mut domain = use_signal(|| false);
    let mut also = use_signal(|| true);
    let q2 = q.clone();
    let rule = use_memo(move || rule_for(&q2, *domain.read()));
    let key = use_memo(move || {
        let k = ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        send_cmd(Cmd::RuleMatches { key: k, rule: rule() });
        k
    });
    let inbox = s.rule_matches.read().as_ref().filter(|m| m.0 == key()).map(|m| m.2).unwrap_or(0);
    let r = rule();
    let runs = super::rule_editor::runs(&s, &r);
    let target =
        if *domain.read() { format!("everyone at {}", domain_name.clone().unwrap_or_default()) } else { q.who.clone() };
    let title = format!("Always {} mail from {target}?", q.kind.verb());
    let moves = r.then.moves_mail();
    let where_icon: &'static str = if matches!(runs, Runs::Server(_)) { "cloud-check" } else { "pigeon" };
    let mut close = move || s.quick_rule.set(None);
    let make = move |_| {
        let r = rule();
        let apply = *also.peek() && moves && inbox > 0;
        send_cmd(Cmd::SaveRule { rule: r.clone(), apply_now: apply });
        if !apply {
            s.local_toast(format!("Rule made: {}", r.when.summary()), false);
        }
        s.quick_rule.set(None);
    };
    let more = move |_| {
        let r = rule();
        s.quick_rule.set(None);
        super::rule_editor::open(s, r, true);
    };
    rsx! {
        div { class: "update-scrim", onclick: move |_| close(),
            div { class: "dialog quick-rule", role: "dialog", "aria-label": "{title}",
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| if e.key() == Key::Escape { close() },
                div { class: "dialog__title", dir: "auto", "{title}" }
                if let Some(d) = domain_name.clone() {
                    div { class: "segmented segmented--sm", role: "radiogroup", "aria-label": "Which senders",
                        button { r#type: "button", role: "radio", "aria-checked": "{!*domain.read()}", "aria-selected": "{!*domain.read()}",
                            onclick: move |_| domain.set(false), "This address" }
                        button { r#type: "button", role: "radio", "aria-checked": "{*domain.read()}", "aria-selected": "{*domain.read()}",
                            onclick: move |_| domain.set(true), "Everyone at {d}" }
                    }
                }
                if moves && inbox > 0 {
                    label { class: "rule-check",
                        input { r#type: "checkbox", checked: *also.read(), onchange: move |e| also.set(e.checked()) }
                        "Also do this now to the {inbox} already in Inbox"
                    }
                }
                div { class: "rule-where", "data-server": matches!(runs, Runs::Server(_)).then_some("true"),
                    div { class: "rule-where__title",
                        Icon { name: where_icon, class: "icon--sm" }
                        {if matches!(runs, Runs::Server(_)) { format!("Saved as a {}: works even when Coo is closed.", runs.label()) } else { "Runs in Coo.".to_string() }}
                    }
                }
                div { class: "dialog__actions",
                    button { class: "link-btn quick-rule__more", r#type: "button", onclick: more, "More options…" }
                    span { class: "rule-sheet__spacer" }
                    button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| close(), "Cancel" }
                    button { class: "btn btn--primary btn--sm", r#type: "button", autofocus: true, onclick: make, "Make rule" }
                }
            }
        }
    }
}

/// "Archived. That's the 3rd from Rescana this week." with an offer.
#[component]
pub fn NudgeBar() -> Element {
    let mut s = use_context::<AppState>();
    // Gone on its own after a while (unless a newer one replaced it).
    // Hooks before the early return: their order must not change.
    use_effect(move || {
        let mut nudge = s.rule_nudge;
        let Some(shown) = nudge.read().clone() else { return };
        dioxus::core::spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            if nudge.peek().as_ref() == Some(&shown) {
                nudge.set(None);
            }
        });
    });
    let Some(n) = s.rule_nudge.read().clone() else { return rsx! {} };
    let q = n.quick.clone();
    let email = q.email.clone();
    let ordinal = match n.count {
        3 => "3rd".to_string(),
        k => format!("{k}th"),
    };
    let offer = format!("Always {} them", q.kind.verb());
    rsx! {
        div { class: "rule-nudge", role: "status",
            span { class: "rule-nudge__text", dir: "auto",
                "{n.what[..1].to_uppercase()}{&n.what[1..]} without opening. That’s the {ordinal} from {q.who} this week."
            }
            button { class: "btn btn--primary btn--sm", r#type: "button",
                onclick: move |_| { s.rule_nudge.set(None); s.quick_rule.set(Some(q.clone())); },
                "{offer}"
            }
            button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Don’t suggest this again", title: "Don’t suggest this again",
                onclick: move |_| {
                    let e = email.clone();
                    crate::prefs::update(move |p| if !p.rule_nudge_dismissed.contains(&e) { p.rule_nudge_dismissed.push(e) });
                    s.rule_nudge.set(None);
                },
                Icon { name: "x", class: "icon--sm" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clears_in_the_last_week_count() {
        let mut t = vec![0, WINDOW_MS / 2];
        assert_eq!(bump(&mut t, WINDOW_MS + 1), 2, "the first one aged out");
        assert_eq!(bump(&mut t, WINDOW_MS + 2), 3);
    }
}
