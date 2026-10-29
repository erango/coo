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
    OpenWindow,
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
    Sep,
}

impl Entry {
    /// (label, icon, the shortcut shown).
    fn look(self) -> (&'static str, &'static str, Option<Do>) {
        match self {
            Entry::Reply => ("Reply", "reply", Some(Do::Reply)),
            Entry::ReplyAll => ("Reply all", "replyall", Some(Do::ReplyAll)),
            Entry::Forward => ("Forward", "forward", Some(Do::Forward)),
            Entry::OpenWindow => ("Open in new window", "popout", None),
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
            Entry::Sep => ("", "", None),
        }
    }
}

/// The menu for one row (`bulk` = a multi-selection: no reply/forward, no
/// window). `flag`/`read` are what the toggles would do; `labels` adds
/// Gmail's "Label as…"; `spam` is what the accounts support
/// ([`super::spam::offered`]).
pub fn entries(bulk: bool, labels: bool, flag: &MailAction, read: &MailAction, spam: &[MailAction]) -> Vec<Entry> {
    let mut v = Vec::new();
    if !bulk {
        v.extend([Entry::Reply, Entry::ReplyAll, Entry::Forward, Entry::Sep, Entry::OpenWindow, Entry::Sep]);
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
    entries(m.bulk, labels, &flag, &read, &super::spam::for_items(s, &items))
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
        Entry::OpenWindow => return super::reader_window::open(s, item.clone()),
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
        let one = entries(false, false, &MailAction::Flag, &MailAction::MarkUnread, &[]);
        assert_eq!(
            one,
            [
                Entry::Reply,
                Entry::ReplyAll,
                Entry::Forward,
                Entry::Sep,
                Entry::OpenWindow,
                Entry::Sep,
                Entry::Archive,
                Entry::Delete,
                Entry::Move,
                Entry::Sep,
                Entry::Flag,
                Entry::MarkUnread,
            ]
        );
        let many = entries(true, true, &MailAction::Unflag, &MailAction::MarkRead, &[]);
        assert_eq!(
            many,
            [Entry::Archive, Entry::Delete, Entry::Move, Entry::Label, Entry::Sep, Entry::Unflag, Entry::MarkRead]
        );
    }

    #[test]
    fn spam_entries_follow_what_the_accounts_support() {
        let spam = [MailAction::Spam, MailAction::Phishing];
        let e = entries(true, false, &MailAction::Flag, &MailAction::MarkRead, &spam);
        assert_eq!(&e[..5], [Entry::Archive, Entry::Delete, Entry::Move, Entry::Spam, Entry::Phishing]);
        let e = entries(false, true, &MailAction::Flag, &MailAction::MarkRead, &[MailAction::NotSpam]);
        assert!(e.contains(&Entry::NotSpam) && !e.contains(&Entry::Spam) && !e.contains(&Entry::Phishing));
    }

    #[test]
    fn keyboard_steps_skip_separators_and_stop_at_the_ends() {
        let e = entries(false, false, &MailAction::Flag, &MailAction::MarkRead, &[]);
        assert_eq!(step(&e, None, true), Some(0));
        assert_eq!(step(&e, None, false), Some(e.len() - 1));
        assert_eq!(step(&e, Some(2), true), Some(4), "skips the separator after Forward");
        assert_eq!(step(&e, Some(4), false), Some(2));
        assert_eq!(step(&e, Some(0), false), Some(0));
        assert_eq!(step(&e, Some(e.len() - 1), true), Some(e.len() - 1));
    }

    #[test]
    fn height_counts_items_and_separators() {
        assert_eq!(height(&[Entry::Archive, Entry::Sep, Entry::Flag]), 12.0 + 64.0 + 11.0);
    }
}
