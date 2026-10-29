//! The buttons a message row shows under the pointer: up to five, in the
//! user's order, from anything in the row's right-click menu (Settings ›
//! Appearance › Hover actions; right-clicking the buttons goes there).

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_engine::MailAction;
use pigeon_engine::service::ListItem;

use crate::shortcuts::Do;
use crate::state::AppState;

/// How many buttons a row has room for.
pub const MAX: usize = 5;
/// What a row shows until the user changes it.
pub const DEFAULT: [&str; 4] = ["archive", "trash", "flag", "read"];

/// One action a row button can run: its saved id, its name, its icon, and
/// the shortcut whose key the tooltip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowAction {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub key: Option<Do>,
}

const fn a(id: &'static str, label: &'static str, icon: &'static str, key: Option<Do>) -> RowAction {
    RowAction { id, label, icon, key }
}

/// Everything a row button can be: the row's right-click menu.
pub const ALL: &[RowAction] = &[
    a("archive", "Archive", "archive", Some(Do::Archive)),
    a("trash", "Delete", "trash", Some(Do::Delete)),
    a("flag", "Flag / unflag", "flag", Some(Do::Flag)),
    a("read", "Mark read / unread", "mailopen", None),
    a("reply", "Reply", "reply", Some(Do::Reply)),
    a("replyall", "Reply all", "replyall", Some(Do::ReplyAll)),
    a("forward", "Forward", "forward", Some(Do::Forward)),
    a("reuse", "Reuse message", "repeat", None),
    a("move", "Move to…", "folder", Some(Do::Move)),
    a("label", "Label as… (Gmail)", "tag", Some(Do::Label)),
    a("spam", "Spam / not spam", "alert", Some(Do::Spam)),
    a("window", "Open in new window", "popout", None),
    a("eml", "Save as .eml…", "download", None),
];

pub fn find(id: &str) -> Option<&'static RowAction> {
    ALL.iter().find(|a| a.id == id)
}

/// The chosen ids (known ones, at most `MAX`, no repeats).
pub fn clean(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if find(id).is_some() && !out.contains(id) && out.len() < MAX {
            out.push(id.clone());
        }
    }
    out
}

/// The row buttons in use (components reading it re-render on a change).
pub static CHOSEN: GlobalSignal<Vec<String>> = Signal::global(|| clean(&crate::prefs::get().row_actions));

/// Save a new choice of row buttons.
pub fn set(ids: Vec<String>) {
    let ids = clean(&ids);
    *CHOSEN.write() = ids.clone();
    crate::prefs::update(move |p| p.row_actions = ids);
}

/// The button's name for this row (Flag or Unflag, Mark read or unread…).
pub fn label_for(action: &RowAction, item: &ListItem) -> String {
    let h = item.header();
    let unread = match item {
        ListItem::Thread(t) => t.unread_count > 0,
        ListItem::Message(m) => !m.header.is_read,
    };
    let (label, key) = match action.id {
        "flag" => (if h.is_flagged { "Unflag" } else { "Flag" }, Some(Do::Flag)),
        "read" if unread => ("Mark read", Some(Do::MarkRead)),
        "read" => ("Mark unread", Some(Do::MarkUnread)),
        _ => (action.label.trim_end_matches(" (Gmail)"), action.key),
    };
    match key {
        Some(k) => crate::shortcuts::tip(label, k),
        None => label.to_string(),
    }
}

/// Whether the button applies to this row (labels are Gmail's; spam needs
/// an account that takes reports).
pub fn applies(s: &AppState, action: &RowAction, item: &ListItem) -> bool {
    match action.id {
        "label" => s.accounts.read().iter().any(|a| {
            a.account.id == item.account_id() && a.account.provider == pigeon_core::model::ProviderKind::Google
        }),
        "spam" => spam_action(s, item).is_some(),
        _ => true,
    }
}

fn spam_action(s: &AppState, item: &ListItem) -> Option<MailAction> {
    super::spam::for_items(s, std::slice::from_ref(item)).into_iter().find(|a| *a != MailAction::Phishing)
}

/// Run `id` on a row.
pub fn run(s: AppState, item: &ListItem, id: &str) {
    let h = item.header();
    let unread = match item {
        ListItem::Thread(t) => t.unread_count > 0,
        ListItem::Message(m) => !m.header.is_read,
    };
    match id {
        "archive" => super::act(s, item, MailAction::Archive),
        "trash" => super::act(s, item, MailAction::Trash),
        "flag" => super::act(s, item, if h.is_flagged { MailAction::Unflag } else { MailAction::Flag }),
        "read" => super::act(s, item, if unread { MailAction::MarkRead } else { MailAction::MarkUnread }),
        "reply" => super::reply(item, ReplyKind::Reply),
        "replyall" => super::reply(item, ReplyKind::ReplyAll),
        "forward" => super::reply(item, ReplyKind::Forward),
        "reuse" => super::reply(item, ReplyKind::Reuse),
        "move" => super::move_picker::open_for(s, item.clone(), false),
        "label" => super::move_picker::open_for(s, item.clone(), true),
        "spam" => {
            if let Some(a) = spam_action(&s, item) {
                super::act(s, item, a);
            }
        }
        "window" => super::reader_window::open(s, item.clone()),
        "eml" => super::save_eml(vec![item.clone()]),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_are_known_unique_and_at_most_five() {
        let ids: Vec<String> = ["reply", "nope", "reply", "archive", "trash", "flag", "read", "eml"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(clean(&ids), ["reply", "archive", "trash", "flag", "read"]);
        assert!(DEFAULT.iter().all(|d| find(d).is_some()));
    }
}
