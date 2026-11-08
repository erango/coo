//! Right-click menu on a mail list row: what can be done with that message,
//! or, when the row is part of a multi-selection, with the selection (bulk
//! actions only). An in-app popover at the pointer, kept inside the window;
//! Esc, a click outside or picking an item closes it. As in Mail, the
//! clicked row is only ringed, not opened (opening would mark it read).

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_core::model::ProviderKind;
use pigeon_engine::MailAction;
use pigeon_engine::service::ListItem;

use super::{Icon, act, multi, reply};
use crate::shortcuts::{Do, hint};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
pub struct RowMenu {
    pub item: ListItem,
    /// The row is part of a multi-selection: the menu acts on all of it.
    pub bulk: bool,
    /// Pointer position, viewport px.
    pub x: f64,
    pub y: f64,
    /// Keyboard-highlighted entry (index into `entries`).
    pub active: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Reply,
    ReplyAll,
    Forward,
    Reuse,
    OpenWindow,
    Watch,
    Archive,
    Delete,
    Move,
    Label,
    Spam,
    NotSpam,
    Phishing,
    Flag,
    Unflag,
    MarkRead,
    MarkUnread,
    SaveEml,
    /// The sender: make a VIP, or no longer.
    Vip,
    Unvip,
    /// The sender's new mail: stop or resume its notifications.
    Mute,
    Unmute,
    /// Everyone at the sender's domain.
    MuteDomain,
    UnmuteDomain,
    Sep,
}

/// What the menu offers about the right-clicked message's sender (vip.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenderNotify {
    pub vip: bool,
    pub muted: bool,
    /// Their domain can be muted as a whole (not gmail.com and the like):
    /// whether it is now.
    pub domain_muted: Option<bool>,
}

impl Entry {
    /// (label, icon, the shortcut shown).
    fn look(self) -> (&'static str, &'static str, Option<Do>) {
        match self {
            Entry::Reply => ("Reply", "reply", Some(Do::Reply)),
            Entry::ReplyAll => ("Reply all", "replyall", Some(Do::ReplyAll)),
            Entry::Forward => ("Forward", "forward", Some(Do::Forward)),
            Entry::Reuse => ("Reuse message", "repeat", None),
            Entry::OpenWindow => ("Open in new window", "popout", None),
            Entry::Watch => ("Watch this conversation", "eye", None),
            Entry::Archive => ("Archive", "archive", Some(Do::Archive)),
            Entry::Delete => ("Delete", "trash", Some(Do::Delete)),
            Entry::Move => ("Move to…", "folder", Some(Do::Move)),
            Entry::Label => ("Label as…", "tag", Some(Do::Label)),
            Entry::Spam => ("Mark as spam", "alert", Some(Do::Spam)),
            Entry::NotSpam => ("Not spam", "inbox", Some(Do::Spam)),
            Entry::Phishing => ("Report phishing", "shield", None),
            Entry::Flag => ("Flag", "flag", Some(Do::Flag)),
            Entry::Unflag => ("Unflag", "flag", Some(Do::Flag)),
            Entry::MarkRead => ("Mark as read", "mailopen", Some(Do::MarkRead)),
            Entry::MarkUnread => ("Mark as unread", "mail", Some(Do::MarkUnread)),
            Entry::SaveEml => ("Save as .eml…", "download", None),
            Entry::Vip => ("Make sender a VIP", "star", None),
            Entry::Unvip => ("Remove sender from VIPs", "star", None),
            Entry::Mute => ("Mute notifications from sender", "bell-off", None),
            Entry::Unmute => ("Unmute sender", "bell", None),
            Entry::MuteDomain => ("Mute everyone at their domain", "bell-off", None),
            Entry::UnmuteDomain => ("Unmute everyone at their domain", "bell", None),
            Entry::Sep => ("", "", None),
        }
    }
}

/// The menu for one row (`bulk` = a multi-selection: no reply/forward, no
/// window). `flag`/`read` are what the toggles would do; `labels` adds
/// Gmail's "Label as…"; `spam` is what the accounts support
/// ([`super::spam::offered`]).
pub fn entries(
    bulk: bool,
    labels: bool,
    flag: &MailAction,
    read: &MailAction,
    spam: &[MailAction],
    sender: Option<SenderNotify>,
) -> Vec<Entry> {
    let mut v = Vec::new();
    if !bulk {
        v.extend([
            Entry::Reply,
            Entry::ReplyAll,
            Entry::Forward,
            Entry::Reuse,
            Entry::Sep,
            Entry::OpenWindow,
            Entry::Watch,
            Entry::Sep,
        ]);
        if let Some(n) = sender {
            v.push(if n.vip { Entry::Unvip } else { Entry::Vip });
            v.push(if n.muted { Entry::Unmute } else { Entry::Mute });
            if let Some(d) = n.domain_muted {
                v.push(if d { Entry::UnmuteDomain } else { Entry::MuteDomain });
            }
            v.push(Entry::Sep);
        }
    }
    v.extend([Entry::Archive, Entry::Delete, Entry::Move]);
    if labels {
        v.push(Entry::Label);
    }
    v.extend(spam.iter().filter_map(|a| match a {
        MailAction::Spam => Some(Entry::Spam),
        MailAction::NotSpam => Some(Entry::NotSpam),
        MailAction::Phishing => Some(Entry::Phishing),
        _ => None,
    }));
    v.push(Entry::Sep);
    v.push(if *flag == MailAction::Unflag { Entry::Unflag } else { Entry::Flag });
    v.push(if *read == MailAction::MarkRead { Entry::MarkRead } else { Entry::MarkUnread });
    v.extend([Entry::Sep, Entry::SaveEml]);
    v
}

/// Next keyboard-highlighted entry, skipping separators and stopping at
/// the ends. From nothing, down starts at the top and up at the bottom.
pub fn step(entries: &[Entry], active: Option<usize>, down: bool) -> Option<usize> {
    let ok = |i: &usize| entries[*i] != Entry::Sep;
    let next = match (active, down) {
        (None, true) => (0..entries.len()).find(ok),
        (None, false) => (0..entries.len()).rev().find(ok),
        (Some(a), true) => (a + 1..entries.len()).find(ok),
        (Some(a), false) => (0..a).rev().find(ok),
    };
    next.or(active)
}

/// The menu's height in px (design `.menu` padding, 32 px items, 11 px
/// separators), for keeping it inside the window.
pub fn height(entries: &[Entry]) -> f64 {
    let seps = entries.iter().filter(|e| **e == Entry::Sep).count() as f64;
    12.0 + 32.0 * (entries.len() as f64 - seps) + 11.0 * seps
}

const WIDTH: f64 = 236.0;

/// What the menu would act on: the selection, or the one row.
fn targets(s: &AppState, m: &RowMenu) -> Vec<ListItem> {
    if m.bulk { multi::items(s) } else { vec![m.item.clone()] }
}

fn unread(i: &ListItem) -> bool {
    match i {
        ListItem::Thread(t) => t.unread_count > 0,
        ListItem::Message(m) => !m.header.is_read,
    }
}

/// The entries for an open menu.
fn entries_for(s: &AppState, m: &RowMenu) -> Vec<Entry> {
    let items = targets(s, m);
    let google = |i: &ListItem| {
        s.accounts.read().iter().any(|a| a.account.id == i.account_id() && a.account.provider == ProviderKind::Google)
    };
    let labels =
        !items.is_empty() && items.iter().all(|i| i.account_id() == items[0].account_id()) && items.iter().all(google);
    let (flag, read) = if m.bulk {
        (multi::toggle_flag(&items), multi::toggle_read(&items))
    } else {
        let flag = if m.item.header().is_flagged { MailAction::Unflag } else { MailAction::Flag };
        (flag, if unread(&m.item) { MailAction::MarkRead } else { MailAction::MarkUnread })
    };
    let sender = if m.bulk { None } else { sender_notify(&m.item) };
    entries(m.bulk, labels, &flag, &read, &super::spam::for_items(s, &items), sender)
}

/// The row's sender, lowercased, when it has one.
fn sender_of(item: &ListItem) -> Option<String> {
    item.header().from.as_ref().map(|a| a.email.trim().to_lowercase()).filter(|e| e.contains('@'))
}

fn sender_notify(item: &ListItem) -> Option<SenderNotify> {
    use crate::vip::{self, Rule};
    let email = sender_of(item)?;
    let p = crate::prefs::get();
    let own = vip::rule_of(&p, &email);
    let domain = vip::domain_of(&email).filter(|d| !vip::is_shared_domain(d));
    Some(SenderNotify {
        vip: own == Some(Rule::Vip),
        muted: own == Some(Rule::Muted),
        domain_muted: domain.map(|d| vip::rule_of(&p, &format!("@{d}")) == Some(Rule::Muted)),
    })
}

/// Who the sender is in a toast: their name, else their address.
fn sender_name(item: &ListItem) -> String {
    item.header().from.as_ref().map(|a| a.display().to_string()).unwrap_or_else(|| "This sender".into())
}

/// Set the rule for `key` and say what changed.
fn set_sender_rule(s: AppState, key: String, rule: Option<crate::vip::Rule>, said: String) {
    crate::vip::update(move |p| crate::vip::set_rule(p, &key, rule));
    s.local_toast(said, false);
}

/// Right-click on a row.
pub fn open(s: AppState, item: &ListItem, x: f64, y: f64) {
    let key = AppState::key(item);
    let bulk = {
        let multi = s.multi.read();
        multi.len() >= 2 && multi.contains(&key)
    };
    let mut m = s.row_menu;
    m.set(Some(RowMenu { item: item.clone(), bulk, x, y, active: None }));
}

pub fn close(s: AppState) {
    let mut m = s.row_menu;
    m.set(None);
}

fn run(s: AppState, m: &RowMenu, entry: Entry) {
    close(s);
    let items = targets(&s, m);
    let item = &m.item;
    let action = match entry {
        Entry::Reply => return reply(item, ReplyKind::Reply),
        Entry::ReplyAll => return reply(item, ReplyKind::ReplyAll),
        Entry::Forward => return reply(item, ReplyKind::Forward),
        Entry::Reuse => return reply(item, ReplyKind::Reuse),
        Entry::OpenWindow => return super::reader_window::open(s, item.clone()),
        Entry::Watch => {
            let h = item.header();
            super::settings_watches::watch_thread(item.account_id(), &h.thread_id, &h.subject);
            return s.push_toast(pigeon_engine::service::Toast {
                message: "Watching this conversation: replies show in Needs you".into(),
                ..Default::default()
            });
        }
        Entry::Move | Entry::Label => {
            let label = entry == Entry::Label;
            return if m.bulk {
                super::move_picker::open(s, label)
            } else {
                super::move_picker::open_for(s, item.clone(), label)
            };
        }
        Entry::Sep => return,
        Entry::SaveEml => return super::save_eml(items),
        Entry::Vip | Entry::Unvip | Entry::Mute | Entry::Unmute => {
            use crate::vip::Rule;
            let Some(email) = sender_of(item) else { return };
            let who = sender_name(item);
            let (rule, said) = match entry {
                Entry::Vip => (Some(Rule::Vip), format!("{who} is a VIP now")),
                Entry::Mute => (Some(Rule::Muted), format!("{who} won't notify you anymore")),
                Entry::Unmute => (None, format!("{who} can notify you again")),
                _ => (None, format!("{who} isn't a VIP anymore")),
            };
            return set_sender_rule(s, email, rule, said);
        }
        Entry::MuteDomain | Entry::UnmuteDomain => {
            let Some(domain) = sender_of(item).and_then(|e| crate::vip::domain_of(&e)) else { return };
            let mute = entry == Entry::MuteDomain;
            let said = if mute {
                format!("Nobody at {domain} will notify you")
            } else {
                format!("Everyone at {domain} can notify you again")
            };
            return set_sender_rule(s, format!("@{domain}"), mute.then_some(crate::vip::Rule::Muted), said);
        }
        Entry::Archive => MailAction::Archive,
        Entry::Delete => MailAction::Trash,
        Entry::Flag => MailAction::Flag,
        Entry::Unflag => MailAction::Unflag,
        Entry::MarkRead => MailAction::MarkRead,
        Entry::MarkUnread => MailAction::MarkUnread,
        Entry::Spam => MailAction::Spam,
        Entry::NotSpam => MailAction::NotSpam,
        Entry::Phishing => MailAction::Phishing,
    };
    if m.bulk { multi::act_many(s, &items, action) } else { act(s, item, action) }
}

/// Keys while the menu is open: it takes them all, like a native menu.
pub fn on_key(s: AppState, key: &str) {
    let Some(m) = s.row_menu.read().clone() else { return };
    let list = entries_for(&s, &m);
    match key {
        "Escape" => close(s),
        "ArrowDown" | "j" | "ArrowUp" | "k" => {
            let active = step(&list, m.active, matches!(key, "ArrowDown" | "j"));
            let mut rm = s.row_menu;
            if let Some(open) = rm.write().as_mut() {
                open.active = active;
            }
        }
        "Enter" => {
            if let Some(&e) = m.active.and_then(|i| list.get(i)) {
                run(s, &m, e);
            }
        }
        _ => {}
    }
}

#[component]
pub fn RowMenuView() -> Element {
    let s = use_context::<AppState>();
    let Some(m) = s.row_menu.read().clone() else { return rsx! {} };
    let list = entries_for(&s, &m);
    let h = height(&list);
    let title = if m.bulk { format!("{} selected", multi::items(&s).len()) } else { String::new() };
    // Kept inside the window: CSS zoom doesn't rescale viewport units.
    let style = format!(
        "width:{WIDTH}px;left:max(8px, min({x}px, calc(100vw / var(--ui-zoom, 1) - {WIDTH}px - 8px)));\
         top:max(8px, min({y}px, calc(100vh / var(--ui-zoom, 1) - {h}px - 8px - {extra}px)))",
        x = m.x,
        y = m.y,
        extra = if m.bulk { 28.0 } else { 0.0 },
    );
    rsx! {
        div { class: "row-menu-scrim",
            onclick: move |_| close(s),
            oncontextmenu: move |e| { e.prevent_default(); close(s) },
            div { class: "menu row-menu", role: "menu", style: "{style}",
                onclick: move |e| e.stop_propagation(),
                oncontextmenu: move |e| { e.prevent_default(); e.stop_propagation() },
                if m.bulk { div { class: "menu__label", "{title}" } }
                for (i, entry) in list.iter().copied().enumerate() {
                    if entry == Entry::Sep {
                        div { class: "menu-sep", key: "{i}", role: "separator" }
                    } else {
                        {
                            let (label, icon, what) = entry.look();
                            let label = match entry {
                                Entry::MuteDomain | Entry::UnmuteDomain => {
                                    let d = sender_of(&m.item).and_then(|e| crate::vip::domain_of(&e)).unwrap_or_default();
                                    if entry == Entry::MuteDomain { format!("Mute everyone at {d}") } else { format!("Unmute everyone at {d}") }
                                }
                                _ => label.to_string(),
                            };
                            let hint = what.map(hint).unwrap_or_default();
                            let m = m.clone();
                            let danger = entry == Entry::Delete;
                            rsx! {
                                div { class: if danger { "menu-item menu-item--danger" } else { "menu-item" },
                                    key: "{i}", role: "menuitem",
                                    "data-active": "{m.active == Some(i)}",
                                    onmouseenter: move |_| {
                                        let mut rm = s.row_menu;
                                        if let Some(open) = rm.write().as_mut() { open.active = Some(i); }
                                    },
                                    onclick: move |_| run(s, &m, entry),
                                    Icon { name: icon }
                                    span { "{label}" }
                                    if !hint.is_empty() { span { class: "menu-item__hint", "{hint}" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_rows_get_reply_and_window_bulk_does_not() {
        let one = entries(false, false, &MailAction::Flag, &MailAction::MarkUnread, &[], None);
        assert_eq!(
            one,
            [
                Entry::Reply,
                Entry::ReplyAll,
                Entry::Forward,
                Entry::Reuse,
                Entry::Sep,
                Entry::OpenWindow,
                Entry::Watch,
                Entry::Sep,
                Entry::Archive,
                Entry::Delete,
                Entry::Move,
                Entry::Sep,
                Entry::Flag,
                Entry::MarkUnread,
                Entry::Sep,
                Entry::SaveEml,
            ]
        );
        let many = entries(true, true, &MailAction::Unflag, &MailAction::MarkRead, &[], None);
        assert_eq!(
            many,
            [
                Entry::Archive,
                Entry::Delete,
                Entry::Move,
                Entry::Label,
                Entry::Sep,
                Entry::Unflag,
                Entry::MarkRead,
                Entry::Sep,
                Entry::SaveEml
            ]
        );
    }

    #[test]
    fn spam_entries_follow_what_the_accounts_support() {
        let spam = [MailAction::Spam, MailAction::Phishing];
        let e = entries(true, false, &MailAction::Flag, &MailAction::MarkRead, &spam, None);
        assert_eq!(&e[..5], [Entry::Archive, Entry::Delete, Entry::Move, Entry::Spam, Entry::Phishing]);
        let e = entries(false, true, &MailAction::Flag, &MailAction::MarkRead, &[MailAction::NotSpam], None);
        assert!(e.contains(&Entry::NotSpam) && !e.contains(&Entry::Spam) && !e.contains(&Entry::Phishing));
    }

    #[test]
    fn keyboard_steps_skip_separators_and_stop_at_the_ends() {
        let e = entries(false, false, &MailAction::Flag, &MailAction::MarkRead, &[], None);
        assert_eq!(step(&e, None, true), Some(0));
        assert_eq!(step(&e, None, false), Some(e.len() - 1));
        assert_eq!(step(&e, Some(3), true), Some(5), "skips the separator after Reuse");
        assert_eq!(step(&e, Some(5), false), Some(3));
        assert_eq!(step(&e, Some(0), false), Some(0));
        assert_eq!(step(&e, Some(e.len() - 1), true), Some(e.len() - 1));
    }

    #[test]
    fn sender_lines_follow_their_rules() {
        let n = SenderNotify { vip: false, muted: true, domain_muted: Some(false) };
        let e = entries(false, false, &MailAction::Flag, &MailAction::MarkRead, &[], Some(n));
        let at = e.iter().position(|x| *x == Entry::Vip).unwrap();
        assert_eq!(e[at..at + 4], [Entry::Vip, Entry::Unmute, Entry::MuteDomain, Entry::Sep]);
        let shared = SenderNotify { vip: true, muted: false, domain_muted: None };
        let e = entries(false, false, &MailAction::Flag, &MailAction::MarkRead, &[], Some(shared));
        assert!(e.contains(&Entry::Unvip) && e.contains(&Entry::Mute));
        assert!(!e.contains(&Entry::MuteDomain) && !e.contains(&Entry::UnmuteDomain), "no gmail.com-wide mute");
        let bulk = entries(true, false, &MailAction::Flag, &MailAction::MarkRead, &[], Some(n));
        assert!(!bulk.contains(&Entry::Vip), "not for a selection");
    }

    #[test]
    fn height_counts_items_and_separators() {
        assert_eq!(height(&[Entry::Archive, Entry::Sep, Entry::Flag]), 12.0 + 64.0 + 11.0);
    }
}
