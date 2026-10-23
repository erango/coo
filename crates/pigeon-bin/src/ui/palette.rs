//! ⌘K command palette: actions, navigation (every account's folders), and
//! live message hits, in one keyboard-driven list.

use std::time::Duration;

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_core::model::{FolderRole, MessageRow};
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ListItem, Mailbox};

use super::{Icon, act, reply, selected_item};
use crate::send_cmd;
use crate::state::{AppState, Mode, Theme, next_token};

#[derive(Clone, PartialEq)]
enum Action {
    Compose,
    Go(Mailbox),
    Mode(Mode),
    Sync,
    Theme(Theme),
    Threads(bool),
    Search(String),
    Open(Box<MessageRow>),
    Mail(MailAction),
    Reply(ReplyKind),
    Zoom(super::zoom::Step),
    Settings,
    AgentSetup,
}

#[derive(Clone, PartialEq)]
struct Item {
    section: &'static str,
    icon: &'static str,
    label: String,
    meta: String,
    keys: &'static str,
    action: Action,
}

fn item(
    section: &'static str,
    icon: &'static str,
    label: impl Into<String>,
    keys: &'static str,
    action: Action,
) -> Item {
    Item { section, icon, label: label.into(), meta: String::new(), keys, action }
}

/// Every word of the query appears in the label or meta.
fn matches(query: &str, it: &Item) -> bool {
    let hay = format!("{} {}", it.label, it.meta).to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

fn build(s: &AppState, query: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let q = query.trim();
    if !q.is_empty() {
        out.push(Item {
            meta: "local + servers".into(),
            ..item("Search", "search", format!("Search mail for “{q}”"), "↵", Action::Search(q.to_string()))
        });
    }

    let mut actions = vec![
        item("Actions", "edit", "New message", "C", Action::Compose),
        item("Actions", "sync", "Sync now", "⌘R", Action::Sync),
        item("Actions", "settings", "Settings", "⌘,", Action::Settings),
        Item {
            meta: "Claude Code, Claude Desktop, MCP".into(),
            ..item("Actions", "link", "Connect an AI agent…", "", Action::AgentSetup)
        },
        item("Actions", "lines", "Show threads", "", Action::Threads(true)),
        item("Actions", "list", "Show individual messages", "", Action::Threads(false)),
        item("Actions", "sun", "Theme: follow system", "", Action::Theme(Theme::System)),
        item("Actions", "sun", "Theme: dark", "", Action::Theme(Theme::Dark)),
        item("Actions", "sun", "Theme: light", "", Action::Theme(Theme::Light)),
        item("Actions", "cal", "Go to Calendar", "⌘2", Action::Mode(Mode::Calendar)),
        item("Actions", "mail", "Go to Mail", "⌘1", Action::Mode(Mode::Mail)),
        item("Actions", "text", "Bigger text", "⌘+", Action::Zoom(super::zoom::Step::Bigger)),
        item("Actions", "text", "Smaller text", "⌘-", Action::Zoom(super::zoom::Step::Smaller)),
        item("Actions", "text", "Actual text size", "⌘0", Action::Zoom(super::zoom::Step::Reset)),
    ];
    if selected_item(s).is_some() {
        actions.splice(
            0..0,
            [
                item("Actions", "reply", "Reply", "R", Action::Reply(ReplyKind::Reply)),
                item("Actions", "replyall", "Reply all", "A", Action::Reply(ReplyKind::ReplyAll)),
                item("Actions", "forward", "Forward", "F", Action::Reply(ReplyKind::Forward)),
                item("Actions", "archive", "Archive", "E", Action::Mail(MailAction::Archive)),
                item("Actions", "trash", "Delete", "#", Action::Mail(MailAction::Trash)),
                item("Actions", "flag", "Flag", "S", Action::Mail(MailAction::Flag)),
                item("Actions", "mailopen", "Mark unread", "U", Action::Mail(MailAction::MarkUnread)),
            ],
        );
    }
    out.extend(actions);

    let unified = [
        (FolderRole::Inbox, "inbox"),
        (FolderRole::Sent, "send"),
        (FolderRole::Drafts, "file"),
        (FolderRole::Trash, "trash"),
    ];
    for (role, icon) in unified {
        let name = format!("{}{}", role.as_str()[..1].to_uppercase(), &role.as_str()[1..]);
        out.push(Item {
            meta: "All accounts".into(),
            ..item("Go to", icon, name, "", Action::Go(Mailbox::Unified(role)))
        });
    }
    for a in s.accounts.read().iter() {
        for f in &a.folders {
            if f.folder.id.starts_with("CATEGORY_") {
                continue;
            }
            let icon = if f.folder.role == Some(FolderRole::Inbox) { "inbox" } else { "folder" };
            let mb = Mailbox::Folder { account_id: a.account.id.clone(), folder_id: f.folder.id.clone() };
            out.push(Item {
                meta: a.account.email.clone(),
                ..item("Go to", icon, f.folder.name.clone(), "", Action::Go(mb))
            });
        }
    }

    let mut filtered: Vec<Item> = out.into_iter().filter(|it| it.section == "Search" || matches(q, it)).collect();
    // Without a query, keep it short: actions + unified mailboxes.
    if q.is_empty() {
        filtered.retain(|it| it.section != "Go to" || it.meta == "All accounts");
    } else {
        filtered.truncate(14);
        for hit in s.palette_hits.read().iter().take(6) {
            let who = hit.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default();
            filtered.push(Item {
                meta: who,
                ..item("Messages", "mail", hit.header.subject.clone(), "", Action::Open(Box::new(hit.clone())))
            });
        }
    }
    filtered
}

fn run(mut s: AppState, action: Action) {
    s.palette_open.set(false);
    match action {
        Action::Compose => send_cmd(Cmd::NewDraft),
        Action::Go(mb) => {
            s.mode.set(Mode::Mail);
            s.search.set(None);
            s.mailbox.set(mb);
        }
        Action::Mode(m) => s.mode.set(m),
        Action::Sync => send_cmd(Cmd::SyncNow),
        Action::Settings => s.settings_open.set(true),
        Action::AgentSetup => s.agent_setup_open.set(true),
        Action::Theme(t) => s.theme.set(t),
        Action::Threads(t) => s.threaded.set(t),
        Action::Search(q) => {
            s.mode.set(Mode::Mail);
            super::search::open(s, &q);
        }
        Action::Open(row) => {
            s.mode.set(Mode::Mail);
            super::search::open(s, "");
            super::select(s, &ListItem::Message(*row));
        }
        Action::Mail(a) => {
            if let Some(it) = selected_item(&s) {
                act(s, &it, a);
            }
        }
        Action::Zoom(step) => super::zoom::change(s, step),
        Action::Reply(k) => {
            if let Some(it) = selected_item(&s) {
                reply(&it, k);
            }
        }
    }
}

#[component]
pub fn Palette() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.palette_open;
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);
    use_hook(|| crate::bridge::focus("palette-input"));
    let q = query.read().clone();
    let items = build(&s, &q);
    let n = items.len();
    let at = (*cursor.read()).min(n.saturating_sub(1));
    let chosen = items.get(at).cloned();

    let on_input = move |e: FormEvent| {
        let v = e.value();
        query.set(v.clone());
        cursor.set(0);
        let mut token_sig = s.palette_token;
        let token = next_token();
        token_sig.set(token);
        if v.trim().chars().count() >= 2 {
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if *token_sig.read() == token {
                    send_cmd(Cmd::Search { query: v, token, limit: 6 });
                }
            });
        }
    };

    let mut last = "";
    let mut rows = Vec::new();
    for (i, it) in items.into_iter().enumerate() {
        if it.section != last {
            rows.push(rsx! { div { class: "palette__section", key: "s-{it.section}", "{it.section}" } });
            last = it.section;
        }
        let selected = i == at;
        let action = it.action.clone();
        rows.push(rsx! {
            div { class: "palette-item", key: "{i}-{it.label}", "aria-selected": "{selected}",
                onmouseenter: move |_| cursor.set(i),
                onclick: move |_| run(s, action.clone()),
                Icon { name: it.icon }
                span { class: "palette-item__label", dir: "auto", "{it.label}" }
                span { class: "palette-item__meta", dir: "auto", "{it.meta}" }
                if !it.keys.is_empty() { span { class: "palette-item__keys", kbd { class: "kbd", "{it.keys}" } } }
            }
        });
    }

    rsx! {
        div { class: "palette-scrim", onclick: move |_| open.set(false),
            div { class: "palette", role: "dialog", onclick: move |e| e.stop_propagation(),
                div { class: "palette__input",
                    Icon { name: "search" }
                    input { id: "palette-input", dir: "auto", value: "{q}", autocomplete: "off",
                        placeholder: "Type a command, a folder, or search mail…",
                        oninput: on_input,
                        onkeydown: move |e| match e.key() {
                            Key::ArrowDown => { e.prevent_default(); cursor.set((at + 1).min(n.saturating_sub(1))); }
                            Key::ArrowUp => { e.prevent_default(); cursor.set(at.saturating_sub(1)); }
                            Key::Enter => { e.prevent_default(); if let Some(it) = &chosen { run(s, it.action.clone()); } }
                            _ => {}
                        },
                    }
                }
                div { class: "palette__list", {rows.into_iter()} }
                div { class: "palette__footer",
                    span { code { "↑↓" } " navigate" }
                    span { code { "↵" } " run" }
                    span { code { "esc" } " close" }
                    span { "Search supports " code { "from: to: has:attachment is:unread in:" } }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_is_all_words_case_insensitive() {
        let it = Item { meta: "sam@halyard.co".into(), ..item("Go to", "folder", "Q4 Planning", "", Action::Sync) };
        assert!(matches("q4 halyard", &it));
        assert!(matches("", &it));
        assert!(!matches("q4 gmail", &it));
    }
}
