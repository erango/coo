//! Move to folder (`v`) and Gmail "Label…" (`l`): a filterable popover over
//! the selected message's account folders, recent targets first.

use dioxus::prelude::*;
use pigeon_core::model::{FolderInfo, FolderKind, FolderRole, ProviderKind};
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use super::{Icon, act, selected_item};
use crate::state::{AppState, MovePicker};

const RECENT_MAX: usize = 8;

/// Open the picker for the selected item. `label` = Gmail add-label mode.
pub fn open(s: AppState, label: bool) {
    // A multi-selection moves together; folders belong to one account.
    let many = super::multi::items(&s);
    if many.iter().any(|i| i.account_id() != many[0].account_id()) {
        return s.local_toast("Select messages from one account to move them together", true);
    }
    let Some(item) = many.first().cloned().or_else(|| selected_item(&s)) else { return };
    open_for(s, item, label);
}

/// Open the picker for `item` (the list's right-click menu), whatever is
/// open. An item inside a multi-selection moves the whole selection.
pub fn open_for(s: AppState, item: ListItem, label: bool) {
    let provider = s.accounts.read().iter().find(|a| a.account.id == item.account_id()).map(|a| a.account.provider);
    if label && provider != Some(ProviderKind::Google) {
        return open_for(s, item, false);
    }
    let mut p = s.move_picker;
    p.set(Some(MovePicker { item, label }));
    crate::bridge::focus("move-input");
}

/// Folders that make sense as a target. Never the folder being viewed,
/// never virtual ones (drafts, starred, important), and for label mode only
/// user labels.
pub fn candidates<'a>(folders: &'a [FolderInfo], current: Option<&str>, label: bool) -> Vec<&'a FolderInfo> {
    folders
        .iter()
        .filter(|f| Some(f.folder.id.as_str()) != current)
        .filter(|f| !f.folder.id.starts_with("CATEGORY_"))
        .filter(|f| !matches!(f.folder.role, Some(FolderRole::Drafts | FolderRole::Starred | FolderRole::Important)))
        .filter(|f| !label || (f.folder.role.is_none() && f.folder.kind == FolderKind::Label))
        .collect()
}

/// Subsequence match score of `q` in `name` (higher is better), or None.
/// Word-start and prefix hits score more, so "q4" ranks "Q4 Planning" first.
pub fn fuzzy(q: &str, name: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = name.to_lowercase().chars().collect();
    let mut score = 0;
    let mut pos = 0;
    let mut prev: Option<usize> = None;
    for qc in q.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = (pos..hay.len()).find(|&i| hay[i] == qc)?;
        let word_start = found == 0 || !hay[found - 1].is_alphanumeric();
        score += 1 + if word_start { 4 } else { 0 } + if prev == Some(found.wrapping_sub(1)) { 3 } else { 0 };
        prev = Some(found);
        pos = found + 1;
    }
    if hay.len() >= q.len() && name.to_lowercase().starts_with(&q.to_lowercase()) {
        score += 10;
    }
    Some(score - (hay.len() as i32) / 8)
}

/// Order candidates: empty query → recent targets (most recent first), then
/// role folders, then the rest by name; otherwise by fuzzy score.
pub fn rank(query: &str, names: &[(String, String)], recent: &[String]) -> Vec<usize> {
    let q = query.trim();
    let recent_pos = |id: &str| recent.iter().position(|r| r == id);
    let mut scored: Vec<(usize, i32)> = names
        .iter()
        .enumerate()
        .filter_map(|(i, (id, name))| {
            let base = fuzzy(q, name)?;
            let bonus = recent_pos(id).map_or(0, |p| if q.is_empty() { 1000 - p as i32 } else { 2 });
            Some((i, base + bonus))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| names[a.0].1.to_lowercase().cmp(&names[b.0].1.to_lowercase())));
    scored.into_iter().map(|(i, _)| i).collect()
}

fn remember(account_id: &str, folder_id: &str) {
    let key = format!("{account_id}|{folder_id}");
    crate::prefs::update(|p| {
        p.recent_move_targets.retain(|r| r != &key);
        p.recent_move_targets.insert(0, key);
        p.recent_move_targets.truncate(RECENT_MAX);
    });
}

/// The concrete folder the item is listed from, so it isn't offered as a
/// target.
fn current_folder(s: &AppState, item: &ListItem) -> Option<String> {
    match &*s.mailbox.read() {
        Mailbox::Folder { folder_id, .. } => Some(folder_id.clone()),
        Mailbox::Unified(role) => s
            .accounts
            .read()
            .iter()
            .find(|a| a.account.id == item.account_id())
            .and_then(|a| a.folders.iter().find(|f| f.folder.role == Some(*role)))
            .map(|f| f.folder.id.clone()),
    }
}

#[component]
pub fn MovePickerView() -> Element {
    let s = use_context::<AppState>();
    let mut picker = s.move_picker;
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);
    let Some(p) = picker.read().clone() else { return rsx! {} };
    let account_id = p.item.account_id().to_string();
    let folders =
        s.accounts.read().iter().find(|a| a.account.id == account_id).map(|a| a.folders.clone()).unwrap_or_default();
    let current = current_folder(&s, &p.item);
    let cands = candidates(&folders, current.as_deref(), p.label);
    let names: Vec<(String, String)> = cands.iter().map(|f| (f.folder.id.clone(), f.folder.name.clone())).collect();
    let recent: Vec<String> = crate::prefs::get()
        .recent_move_targets
        .iter()
        .filter_map(|r| r.strip_prefix(&format!("{account_id}|")).map(String::from))
        .collect();
    let q = query.read().clone();
    let order = rank(&q, &names, &recent);
    let n = order.len();
    let at = (*cursor.read()).min(n.saturating_sub(1));
    let chosen = order.get(at).map(|&i| names[i].0.clone());

    let pick = move |folder_id: String| {
        let item = p.item.clone();
        remember(item.account_id(), &folder_id);
        // Copy the signal so the closure stays `Fn` (handlers clone it).
        let mut pk = picker;
        pk.set(None);
        let action = if p.label { MailAction::AddLabel(folder_id) } else { MailAction::MoveTo(folder_id) };
        // The selection moves together, unless the picker is for a row outside it.
        let many = super::multi::items(&s);
        if many.iter().any(|i| AppState::key(i) == AppState::key(&item)) {
            super::multi::act_many(s, &many, action)
        } else {
            act(s, &item, action)
        }
    };
    let title = if p.label { "Label as…" } else { "Move to…" };

    rsx! {
        div { class: "move-scrim", onclick: move |_| picker.set(None),
            div { class: "menu move-picker", role: "dialog", "aria-label": "{title}", onclick: move |e| e.stop_propagation(),
                div { class: "menu__label", "{title}" }
                input { id: "move-input", class: "input move-picker__input", dir: "auto", value: "{q}", autocomplete: "off",
                    placeholder: if p.label { "Find a label" } else { "Find a folder" },
                    oninput: move |e| { query.set(e.value()); cursor.set(0); },
                    onkeydown: {
                        let pick = pick.clone();
                        move |e: KeyboardEvent| match e.key() {
                            Key::ArrowDown => { e.prevent_default(); cursor.set((at + 1).min(n.saturating_sub(1))); }
                            Key::ArrowUp => { e.prevent_default(); cursor.set(at.saturating_sub(1)); }
                            Key::Enter => { e.prevent_default(); if let Some(id) = &chosen { pick(id.clone()); } }
                            _ => {}
                        }
                    },
                }
                div { class: "move-picker__list",
                    if order.is_empty() { div { class: "menu__label", "No matching folders" } }
                    for (k, &i) in order.iter().enumerate().take(40) {
                        {
                            let f = cands[i];
                            let id = f.folder.id.clone();
                            let is_recent = q.trim().is_empty() && recent.contains(&id);
                            let icon = match f.folder.role {
                                Some(FolderRole::Inbox) => "inbox",
                                Some(FolderRole::Archive) => "archive",
                                Some(FolderRole::Trash) => "trash",
                                Some(FolderRole::Spam) => "alert",
                                Some(FolderRole::Sent) => "send",
                                _ if f.folder.kind == FolderKind::Label => "tag",
                                _ => "folder",
                            };
                            let pick = pick.clone();
                            rsx! {
                                div { class: "menu-item", key: "{id}", "data-active": "{k == at}",
                                    onmouseenter: move |_| cursor.set(k),
                                    onclick: move |_| pick(id.clone()),
                                    Icon { name: icon }
                                    span { dir: "auto", "{f.folder.name}" }
                                    if is_recent { span { class: "menu-item__hint", "recent" } }
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
    use pigeon_core::model::Folder;

    fn f(id: &str, name: &str, role: Option<FolderRole>, kind: FolderKind) -> FolderInfo {
        FolderInfo {
            folder: Folder { id: id.into(), name: name.into(), parent_id: None, role, kind },
            total: 0,
            unread: 0,
        }
    }

    #[test]
    fn candidates_skip_current_virtual_and_categories() {
        let folders = vec![
            f("INBOX", "Inbox", Some(FolderRole::Inbox), FolderKind::Label),
            f("STARRED", "Starred", Some(FolderRole::Starred), FolderKind::Label),
            f("DRAFT", "Drafts", Some(FolderRole::Drafts), FolderKind::Label),
            f("CATEGORY_SOCIAL", "Social", None, FolderKind::Label),
            f("L1", "Travel", None, FolderKind::Label),
            f("TRASH", "Trash", Some(FolderRole::Trash), FolderKind::Label),
        ];
        let ids = |v: Vec<&FolderInfo>| v.into_iter().map(|x| x.folder.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(candidates(&folders, Some("INBOX"), false)), ["L1", "TRASH"]);
        assert_eq!(ids(candidates(&folders, Some("INBOX"), true)), ["L1"]);
    }

    #[test]
    fn fuzzy_prefers_prefix_and_word_starts() {
        assert!(fuzzy("q4", "Q4 Planning").unwrap() > fuzzy("q4", "Archive Q4").unwrap());
        assert!(fuzzy("plan", "Q4 Planning").is_some());
        assert!(fuzzy("xyz", "Q4 Planning").is_none());
        assert!(fuzzy("טיול", "טיולים 2026").is_some());
    }

    #[test]
    fn rank_puts_recent_first_without_query() {
        let names: Vec<(String, String)> = [("a", "Archive"), ("b", "Bills"), ("c", "Clients")]
            .iter()
            .map(|(i, n)| (i.to_string(), n.to_string()))
            .collect();
        assert_eq!(rank("", &names, &["c".into(), "b".into()]), [2, 1, 0]);
        assert_eq!(rank("cli", &names, &[]), [2]);
        assert_eq!(rank("", &names, &[]), [0, 1, 2]);
    }
}
