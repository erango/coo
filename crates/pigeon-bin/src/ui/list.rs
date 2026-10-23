use dioxus::prelude::*;
use pigeon_core::model::FolderRole;
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use super::{Icon, act, select};
use crate::format;
use crate::state::AppState;

fn title(s: &AppState, mb: &Mailbox) -> (String, String) {
    match mb {
        Mailbox::Unified(role) => {
            let t = match role {
                FolderRole::Inbox => "Inbox",
                FolderRole::Sent => "Sent",
                FolderRole::Drafts => "Drafts",
                FolderRole::Trash => "Trash",
                FolderRole::Archive => "Archive",
                FolderRole::Spam => "Spam",
                FolderRole::Starred => "Starred",
                FolderRole::Important => "Important",
            };
            (t.to_string(), "All accounts".to_string())
        }
        Mailbox::Folder { account_id, folder_id } => {
            let accounts = s.accounts.read();
            let acct = accounts.iter().find(|a| &a.account.id == account_id);
            let name = acct
                .and_then(|a| a.folders.iter().find(|f| &f.folder.id == folder_id))
                .map(|f| f.folder.name.clone())
                .unwrap_or_default();
            (name, acct.map(|a| a.account.email.clone()).unwrap_or_default())
        }
    }
}

#[component]
pub fn MailList() -> Element {
    let s = use_context::<AppState>();
    let mut threaded = s.threaded;
    let items = s.list.read().clone();
    let (heading, sub) = if s.search.read().is_some() {
        ("Search".to_string(), "All accounts".to_string())
    } else {
        title(&s, &s.mailbox.read())
    };
    let now = chrono::Local::now();
    let many = s.accounts.read().len() > 4;

    let mut last_group = "";
    let mut rows = Vec::with_capacity(items.len() * 2);
    for item in &items {
        let g = format::group(item.header().received_at, now);
        if g != last_group {
            rows.push(rsx! { div { class: "list-group", key: "g-{g}", "{g}" } });
            last_group = g;
        }
        rows.push(rsx! { Row { key: "{AppState::key(item)}", item: item.clone(), many } });
    }

    rsx! {
        section { class: "pane pane--list", "aria-label": "{heading}",
            header { class: "pane-header",
                h1 { class: "pane-header__title", dir: "auto", "{heading}" }
                span { class: "pane-header__sub", "{sub}" }
                span { class: "spacer" }
                div { class: "segmented segmented--sm", role: "tablist", "aria-label": "Grouping",
                    button { role: "tab", "aria-selected": "{*threaded.read()}", onclick: move |_| threaded.set(true), "Threads" }
                    button { role: "tab", "aria-selected": "{!*threaded.read()}", onclick: move |_| threaded.set(false), "Messages" }
                }
            }
            if s.search.read().is_some() {
                super::search::SearchPane {}
            } else if items.is_empty() {
                Empty { syncing: s.sync.read().syncing || s.sync.read().last_ok.is_none() }
            } else {
                div { class: "list", role: "listbox", {rows.into_iter()} }
            }
            super::toasts::Toasts {}
        }
    }
}

#[component]
fn Empty(syncing: bool) -> Element {
    rsx! {
        div { class: "empty-state",
            div { class: "perch",
                svg { class: "icon", style: "width:88px;height:80px", r#use { href: "#i-pigeon" } }
                if !syncing {
                    span { class: "perch__z", "z" }
                    span { class: "perch__z", "z" }
                }
            }
            if syncing {
                div { class: "empty-state__title", "Fetching your mail…" }
                div { class: "empty-state__body", "Recent messages show up first; older mail fills in behind." }
            } else {
                div { class: "empty-state__title", "All caught up" }
                div { class: "empty-state__body", "Nothing left here. Enjoy the quiet." }
            }
        }
    }
}

#[component]
pub fn Row(item: ListItem, many: bool) -> Element {
    let s = use_context::<AppState>();
    let key = AppState::key(&item);
    let selected = s.selected.read().as_deref() == Some(key.as_str());
    let hue = s.hue(item.account_id());
    let h = item.header().clone();
    let (who, count, unread) = match &item {
        ListItem::Thread(t) => {
            let who = if t.participants.is_empty() { "(unknown)".to_string() } else { t.participants.join(", ") };
            (who, (t.message_count > 1).then_some(t.message_count), t.unread_count > 0)
        }
        ListItem::Message(m) => {
            (m.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default(), None, !m.header.is_read)
        }
    };
    let who = if h.is_draft { "Draft".to_string() } else { who };
    let letter = s
        .accounts
        .read()
        .iter()
        .find(|a| a.account.id == item.account_id())
        .and_then(|a| a.account.email.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?');
    let time = format::row_time(h.received_at, chrono::Local::now());
    let subject = if h.subject.is_empty() { "(no subject)".to_string() } else { h.subject.clone() };
    let (i1, i2, i3, i4, i5) = (item.clone(), item.clone(), item.clone(), item.clone(), item.clone());
    let flagged = h.is_flagged;
    // Inline invite only when the body is already loaded (never fetch for the list).
    let invite_key = (item.account_id().to_string(), h.id.clone());
    let row_invite = s
        .bodies
        .read()
        .get(&invite_key)
        .and_then(|b| b.invite.clone())
        .filter(|i| i.method == pigeon_core::model::InviteMethod::Request);
    let row_event = s.invites.read().get(&invite_key).and_then(|v| v.event.clone());

    rsx! {
        div { class: "row", role: "option", "data-hue": "{hue}",
            "data-unread": unread.then_some("true"),
            "data-flagged": flagged.then_some("true"),
            "data-state": h.is_draft.then_some("draft"),
            "aria-selected": "{selected}",
            "data-focused": selected.then_some("true"),
            onclick: move |_| select(s, &i1),
            div { class: "row__line",
                if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
                span { class: "row__who", dir: "auto", "{who}" }
                if let Some(n) = count { span { class: "row__count", "{n}" } }
                span { class: "row__meta",
                    if flagged { Icon { name: "flag", class: "row__flag" } }
                    if h.has_attachments { Icon { name: "clip" } }
                    time { class: "row__time", "{time}" }
                }
                span { class: "row__actions",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Archive (E)",
                        onclick: move |e| { e.stop_propagation(); act(s, &i2, MailAction::Archive) },
                        Icon { name: "archive" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Delete (#)",
                        onclick: move |e| { e.stop_propagation(); act(s, &i3, MailAction::Trash) },
                        Icon { name: "trash" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Flag (S)",
                        onclick: move |e| {
                            e.stop_propagation();
                            act(s, &i4, if flagged { MailAction::Unflag } else { MailAction::Flag })
                        },
                        Icon { name: "flag" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": if unread { "Mark read (U)" } else { "Mark unread (U)" },
                        onclick: move |e| {
                            e.stop_propagation();
                            act(s, &i5, if unread { MailAction::MarkRead } else { MailAction::MarkUnread })
                        },
                        Icon { name: "mailopen" }
                    }
                }
            }
            div { class: "row__subject", dir: "auto", "{subject}" }
            match row_invite {
                Some(inv) => rsx! { super::invite::RowInvite { invite: inv, event: row_event } },
                None => rsx! { div { class: "row__snippet", dir: "auto", "{h.snippet}" } },
            }
        }
    }
}
