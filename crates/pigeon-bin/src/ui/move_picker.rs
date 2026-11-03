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
    // Labels made or deleted elsewhere (Gmail on the web) show without
    // waiting for the next sync.
    crate::send_cmd(pigeon_engine::service::Cmd::RefreshFolders { account_id: item.account_id().to_string() });
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
        .filter(|f| !(f.folder.kind == FolderKind::Label && f.folder.role.is_none() && gmail_system(&f.folder.id)))
        .filter(|f| !label || (f.folder.role.is_none() && f.folder.kind == FolderKind::Label))
        .collect()
}

/// Gmail's own labels (the superstars such as `BLUE_STAR`, `CHAT`): their
/// ids are their upper-case names. Labels people make have `Label_…` ids.
fn gmail_system(id: &str) -> bool {
    !id.starts_with("Label_") && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
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

pub(super) fn remember(account_id: &str, folder_id: &str) {
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
    let role = match &*s.mailbox.read() {
        Mailbox::Folder { folder_id, .. } => return Some(folder_id.clone()),
        Mailbox::Unified(role) => *role,
        Mailbox::NeedsYou => pigeon_core::model::FolderRole::Inbox,
    };
    s.accounts
        .read()
        .iter()
        .find(|a| a.account.id == item.account_id())
        .and_then(|a| a.folders.iter().find(|f| f.folder.role == Some(role)))
        .map(|f| f.folder.id.clone())
}

/// Where each char of `q` matched in `name` (char positions), the same walk
/// as [`fuzzy`]: the picker marks them.
pub fn hits(q: &str, name: &str) -> Vec<usize> {
    let hay: Vec<char> = name.to_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut pos = 0;
    for qc in q.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let Some(found) = (pos..hay.len()).find(|&i| hay[i] == qc) else { return Vec::new() };
        out.push(found);
        pos = found + 1;
    }
    out
}

/// `name` split into (text, matched) runs.
fn runs(name: &str, hit: &[usize]) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for (i, ch) in name.chars().enumerate() {
        let on = hit.contains(&i);
        match out.last_mut() {
            Some((s, m)) if *m == on => s.push(ch),
            _ => out.push((ch.to_string(), on)),
        }
    }
    out
}

/// A label's parents, "Clients / " style (empty at the top level).
fn parent_path(folders: &[FolderInfo], f: &FolderInfo) -> String {
    let mut names = Vec::new();
    let mut at = f.folder.parent_id.clone();
    while let Some(id) = at {
        let Some(p) = folders.iter().find(|x| x.folder.id == id) else { break };
        names.push(p.folder.name.clone());
        at = p.folder.parent_id.clone();
        if names.len() > 8 {
            break;
        }
    }
    names.reverse();
    names.into_iter().map(|n| format!("{n} / ")).collect()
}

/// How many of the items carry each label: all, some or none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Has {
    All,
    Some,
    None,
}

pub fn has(items: &[ListItem], label: &str) -> Has {
    let n = items.iter().filter(|i| i.header().folder_ids.iter().any(|f| f == label)).count();
    match n {
        0 => Has::None,
        n if n == items.len() => Has::All,
        _ => Has::Some,
    }
}

/// What the ticks change: labels to add to every item, labels to take off.
/// `pending` holds the rows the user ticked (true) or cleared (false).
pub fn changes(items: &[ListItem], pending: &[(String, bool)]) -> (Vec<String>, Vec<String>) {
    let (mut add, mut remove) = (Vec::new(), Vec::new());
    for (id, on) in pending {
        match (on, has(items, id)) {
            (true, Has::All) | (false, Has::None) => {}
            (true, _) => add.push(id.clone()),
            (false, _) => remove.push(id.clone()),
        }
    }
    (add, remove)
}

#[component]
pub fn MovePickerView() -> Element {
    let s = use_context::<AppState>();
    let picker = s.move_picker;
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);
    // Label mode: rows ticked or cleared so far, in order.
    let pending = use_signal(Vec::<(String, bool)>::new);
    let Some(p) = picker.read().clone() else { return rsx! {} };
    let label_mode = p.label;
    let account_id = p.item.account_id().to_string();
    let folders =
        s.accounts.read().iter().find(|a| a.account.id == account_id).map(|a| a.folders.clone()).unwrap_or_default();
    // The selection moves together, unless the picker is for a row outside it.
    let items: Vec<ListItem> = {
        let many = super::multi::items(&s);
        if many.iter().any(|i| AppState::key(i) == AppState::key(&p.item)) { many } else { vec![p.item.clone()] }
    };
    let current = current_folder(&s, &p.item);
    let cands = candidates(&folders, current.as_deref(), p.label);
    let names: Vec<(String, String)> = cands.iter().map(|f| (f.folder.id.clone(), f.folder.name.clone())).collect();
    let recent: Vec<String> = crate::prefs::get()
        .recent_move_targets
        .iter()
        .filter_map(|r| r.strip_prefix(&format!("{account_id}|")).map(String::from))
        .collect();
    let q = query.read().clone();
    let qt = q.trim().to_string();

    // Sections: with no query, what's on it already (labels), recent, the
    // rest by name; with one, the matches.
    let mut sections: Vec<(String, Vec<usize>)> = Vec::new();
    if qt.is_empty() {
        let on: Vec<usize> = if label_mode {
            (0..cands.len()).filter(|&i| has(&items, &names[i].0) != Has::None).collect()
        } else {
            vec![]
        };
        let mut rec: Vec<usize> =
            (0..cands.len()).filter(|i| !on.contains(i) && recent.contains(&names[*i].0)).collect();
        rec.sort_by_key(|&i| recent.iter().position(|r| *r == names[i].0));
        // By full path, so nested labels sit under their parent.
        let mut rest: Vec<usize> = (0..cands.len()).filter(|i| !on.contains(i) && !rec.contains(i)).collect();
        rest.sort_by_cached_key(|&i| {
            format!("{}{}", parent_path(&folders, cands[i]), cands[i].folder.name).to_lowercase()
        });
        let what = if items.len() == 1 { "On this conversation" } else { "On the selection" };
        for (title, rows) in
            [(what, on), ("Recent", rec), (if label_mode { "All labels" } else { "All folders" }, rest)]
        {
            if !rows.is_empty() {
                sections.push((title.to_string(), rows));
            }
        }
    } else {
        let rows = rank(&qt, &names, &recent);
        if !rows.is_empty() {
            let title = if rows.len() == 1 { "1 match".to_string() } else { format!("{} matches", rows.len()) };
            sections.push((title, rows));
        }
    }
    let order: Vec<usize> = sections.iter().flat_map(|(_, r)| r.iter().copied()).collect();

    // Label mode: a name no label has yet can be created (and applied).
    let can_create = label_mode
        && !qt.is_empty()
        && !folders.iter().any(|f| f.folder.kind == FolderKind::Label && f.folder.name.eq_ignore_ascii_case(&qt));
    let n = order.len() + usize::from(can_create);
    let at = (*cursor.read()).min(n.saturating_sub(1));
    let chosen = order.get(at).map(|&i| names[i].0.clone());
    let on_create = can_create && at == order.len();

    let ticks = pending.read().clone();
    let (adds, removes) = changes(&items, &ticks);
    let is_on = {
        let (ticks, items) = (ticks.clone(), items.clone());
        move |id: &str| match ticks.iter().rev().find(|(t, _)| t == id) {
            Some((_, on)) => *on,
            None => has(&items, id) == Has::All,
        }
    };
    let name_of = |id: &String| names.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_default();
    let summary = {
        let mut parts = Vec::new();
        if !adds.is_empty() {
            parts.push(format!("Adds {}", adds.iter().map(name_of).collect::<Vec<_>>().join(", ")));
        }
        if !removes.is_empty() {
            parts.push(format!("Removes {}", removes.iter().map(name_of).collect::<Vec<_>>().join(", ")));
        }
        parts.join(" · ")
    };

    // Copies of the signals keep these closures `Fn` (handlers clone them).
    let close = move || {
        let (mut pk, mut pe, mut qu, mut cu) = (picker, pending, query, cursor);
        pk.set(None);
        pe.set(Vec::new());
        qu.set(String::new());
        cu.set(0);
    };
    let run = {
        let items = items.clone();
        move |action: MailAction| {
            if items.len() > 1 { super::multi::act_many(s, &items, action) } else { act(s, &items[0], action) }
        }
    };
    // Move: pick one and go. Label: apply what's ticked; with nothing ticked,
    // label the highlighted row (the quick `l`, type, Enter).
    let apply = {
        let (run, adds, removes, account_id) = (run.clone(), adds.clone(), removes.clone(), account_id.clone());
        move |fallback: Option<String>| {
            close();
            if !label_mode {
                if let Some(id) = fallback {
                    remember(&account_id, &id);
                    run(MailAction::MoveTo(id));
                }
                return;
            }
            if adds.is_empty() && removes.is_empty() {
                if let Some(id) = fallback {
                    remember(&account_id, &id);
                    run(MailAction::AddLabel(id));
                }
                return;
            }
            for id in &adds {
                remember(&account_id, id);
            }
            run(MailAction::Relabel { add: adds.clone(), remove: removes.clone() });
        }
    };
    let create = {
        let (apply, account_id, name, item) = (apply.clone(), account_id.clone(), qt.clone(), p.item.clone());
        move || {
            apply(None);
            create_and_apply(s, item.clone(), account_id.clone(), name.clone());
        }
    };
    let toggle = move |id: String| {
        let now = is_on(&id);
        let mut pe = pending;
        pe.with_mut(|v| {
            v.retain(|(t, _)| *t != id);
            v.push((id, !now));
        });
    };

    let title = if label_mode { "Label" } else { "Move to" };
    let context = if items.len() == 1 {
        let subj = items[0].header().subject.trim().to_string();
        if subj.is_empty() { "(no subject)".to_string() } else { subj }
    } else {
        format!("{} conversations", items.len())
    };
    let placeholder = if label_mode { "Find or create a label" } else { "Find a folder" };

    rsx! {
        div { class: "move-scrim", onclick: move |_| close(),
            div { class: "move-picker", role: "dialog", "aria-label": "{title}", onclick: move |e| e.stop_propagation(),
                div { class: "move-picker__head",
                    div { class: "move-picker__title",
                        h2 { "{title}" }
                        span { dir: "auto", "{context}" }
                    }
                    label { class: "move-picker__field",
                        Icon { name: "search" }
                        input { id: "move-input", class: "move-picker__input", dir: "auto", value: "{q}", autocomplete: "off", "autocorrect": "off", "writingsuggestions": "false", spellcheck: "false",
                            "aria-label": "{placeholder}", placeholder: "{placeholder}",
                            oninput: move |e| { query.set(e.value()); cursor.set(0); },
                            onkeydown: {
                                let (apply, create, chosen) = (apply.clone(), create.clone(), chosen.clone());
                                move |e: KeyboardEvent| match e.key() {
                                    Key::ArrowDown => { e.prevent_default(); cursor.set((at + 1).min(n.saturating_sub(1))); }
                                    Key::ArrowUp => { e.prevent_default(); cursor.set(at.saturating_sub(1)); }
                                    Key::Escape => { e.prevent_default(); close(); }
                                    Key::Enter => {
                                        e.prevent_default();
                                        if on_create {
                                            create();
                                        } else if label_mode && e.modifiers().meta() {
                                            if let Some(id) = chosen.clone() { toggle(id) }
                                        } else {
                                            apply(chosen.clone());
                                        }
                                    }
                                    _ => {}
                                }
                            },
                        }
                        kbd { class: "move-picker__kbd", "esc" }
                    }
                }
                div { class: "move-picker__list",
                    if n == 0 {
                        p { class: "move-picker__empty", if label_mode { "No labels yet. Type a name to create one." } else { "No matching folders" } }
                    }
                    for (title, rows) in sections.iter() {
                        div { class: "move-picker__section", key: "{title}", "{title}" }
                        for &i in rows.iter() {
                            {
                                let f = cands[i];
                                let id = f.folder.id.clone();
                                let k = order.iter().position(|&o| o == i).unwrap_or(0);
                                let hit = if qt.is_empty() { Vec::new() } else { hits(&qt, &f.folder.name) };
                                let parts = runs(&f.folder.name, &hit);
                                let parent = parent_path(&folders, f);
                                let state = has(&items, &id);
                                let ticked = ticks.iter().rev().find(|(t, _)| *t == id).map(|(_, on)| *on);
                                let check = match (ticked, state) {
                                    (Some(true), _) | (None, Has::All) => "on",
                                    (None, Has::Some) => "some",
                                    _ => "off",
                                };
                                let note = match (ticked, state) {
                                    (Some(true), Has::None | Has::Some) => "adding",
                                    (Some(false), Has::All | Has::Some) => "removing",
                                    (None, Has::Some) => "on some",
                                    _ if qt.is_empty() || !recent.contains(&id) => "",
                                    _ => "recent",
                                };
                                let icon = match f.folder.role {
                                    Some(FolderRole::Inbox) => "inbox",
                                    Some(FolderRole::Archive) => "archive",
                                    Some(FolderRole::Trash) => "trash",
                                    Some(FolderRole::Spam) => "alert",
                                    Some(FolderRole::Sent) => "send",
                                    _ if f.folder.kind == FolderKind::Label => "tag",
                                    _ => "folder",
                                };
                                let (apply, toggle) = (apply.clone(), toggle.clone());
                                rsx! {
                                    button { class: "move-picker__row", r#type: "button", key: "{id}", "data-active": "{k == at}",
                                        onmouseenter: move |_| cursor.set(k),
                                        onclick: move |_| if label_mode { toggle(id.clone()) } else { apply(Some(id.clone())) },
                                        if label_mode {
                                            span { class: "move-picker__check", "data-state": "{check}", "aria-hidden": "true",
                                                if check == "on" { Icon { name: "check" } }
                                                if check == "some" { Icon { name: "dash" } }
                                            }
                                        } else {
                                            Icon { name: icon }
                                        }
                                        span { class: "move-picker__name", dir: "auto",
                                            if !parent.is_empty() { span { class: "move-picker__parent", "{parent}" } }
                                            for (j, (text, m)) in parts.into_iter().enumerate() {
                                                span { key: "{j}", class: if m { "move-picker__hit" } else { "" }, "{text}" }
                                            }
                                        }
                                        if !note.is_empty() { span { class: "move-picker__note", "data-note": "{note}", "{note}" } }
                                    }
                                }
                            }
                        }
                    }
                    if can_create {
                        button { class: "move-picker__row move-picker__row--create", r#type: "button", "data-active": "{on_create}",
                            onmouseenter: move |_| cursor.set(order.len()),
                            onclick: {
                                let create = create.clone();
                                move |_| create()
                            },
                            span { class: "move-picker__check", "data-state": "new", "aria-hidden": "true", Icon { name: "plus" } }
                            span { class: "move-picker__name", dir: "auto", "Create label “" b { "{qt}" } "”" }
                        }
                    }
                }
                if label_mode {
                    div { class: "move-picker__foot",
                        span { class: "move-picker__summary", "data-empty": "{summary.is_empty()}",
                            if summary.is_empty() { "↵ labels the highlighted one · ⌘↵ ticks it" } else { "{summary}" }
                        }
                        button { class: "btn btn--primary btn--sm move-picker__apply", r#type: "button",
                            onclick: {
                                let apply = apply.clone();
                                move |_| apply(None)
                            },
                            "Apply"
                            kbd { "↵" }
                        }
                    }
                }
            }
        }
    }
}

/// Create a Gmail label, then apply it to `item` once it shows up in the
/// account's labels (the next publish after the server made it).
fn create_and_apply(s: AppState, item: ListItem, account_id: String, name: String) {
    crate::send_cmd(pigeon_engine::service::Cmd::CreateLabel { account_id: account_id.clone(), name: name.clone() });
    dioxus::core::spawn_forever(async move {
        // Nested names ("Parent/Child") show as their last part.
        let leaf = name.rsplit_once('/').map_or(name.as_str(), |(_, l)| l).to_string();
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let id = s.accounts.peek().iter().find(|a| a.account.id == account_id).and_then(|a| {
                a.folders
                    .iter()
                    .find(|f| f.folder.kind == FolderKind::Label && f.folder.role.is_none() && f.folder.name == leaf)
                    .map(|f| f.folder.id.clone())
            });
            if let Some(id) = id {
                remember(&account_id, &id);
                let many = super::multi::items(&s);
                let action = MailAction::AddLabel(id);
                if many.iter().any(|i| AppState::key(i) == AppState::key(&item)) {
                    super::multi::act_many(s, &many, action)
                } else {
                    act(s, &item, action)
                }
                return;
            }
        }
    });
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
            f("BLUE_STAR", "BLUE_STAR", None, FolderKind::Label),
            f("Label_1", "Travel", None, FolderKind::Label),
            f("TRASH", "Trash", Some(FolderRole::Trash), FolderKind::Label),
        ];
        let ids = |v: Vec<&FolderInfo>| v.into_iter().map(|x| x.folder.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(candidates(&folders, Some("INBOX"), false)), ["Label_1", "TRASH"]);
        assert_eq!(ids(candidates(&folders, Some("INBOX"), true)), ["Label_1"]);
    }

    #[test]
    fn fuzzy_prefers_prefix_and_word_starts() {
        assert!(fuzzy("q4", "Q4 Planning").unwrap() > fuzzy("q4", "Archive Q4").unwrap());
        assert!(fuzzy("plan", "Q4 Planning").is_some());
        assert!(fuzzy("xyz", "Q4 Planning").is_none());
        assert!(fuzzy("טיול", "טיולים 2026").is_some());
        assert_eq!(hits("inv", "Invoices"), [0, 1, 2]);
        assert_eq!(hits("iv", "In-house vendors"), [0, 9]);
        assert!(hits("xyz", "Invoices").is_empty());
        assert_eq!(runs("Invoices", &[0, 1, 2]), [("Inv".to_string(), true), ("oices".to_string(), false)]);
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
