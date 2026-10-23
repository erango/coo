use dioxus::prelude::*;
use pigeon_core::model::FolderRole;
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::{Icon, act, select};
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, OutState, Outgoing};

/// Accounts a mailbox shows mail from.
fn scope_accounts(mb: &Mailbox, all: &[String]) -> Vec<String> {
    match mb {
        Mailbox::Unified(_) => all.to_vec(),
        Mailbox::Folder { account_id, .. } => vec![account_id.clone()],
    }
}

/// Outgoing rows belong in the inbox/sent/drafts views of their account.
pub fn shows_outgoing(mb: &Mailbox, role_of_folder: Option<FolderRole>, draft_account: &str) -> bool {
    match mb {
        Mailbox::Unified(r) => matches!(r, FolderRole::Inbox | FolderRole::Sent | FolderRole::Drafts),
        Mailbox::Folder { account_id, .. } => {
            account_id == draft_account
                && matches!(role_of_folder, Some(FolderRole::Inbox | FolderRole::Sent | FolderRole::Drafts))
        }
    }
}

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

    let mailbox = s.mailbox.read().clone();
    let account_ids: Vec<String> = s.accounts.read().iter().map(|a| a.account.id.clone()).collect();
    let in_scope = scope_accounts(&mailbox, &account_ids);
    let cold = s.accounts.read().iter().any(|a| in_scope.contains(&a.account.id) && !a.first_sync_done);
    let sync = s.sync.read().clone();
    let expired: Vec<String> = sync.needs_sign_in.iter().filter(|a| in_scope.contains(a)).cloned().collect();
    let offline = sync.offline;
    let folder_role = match &mailbox {
        Mailbox::Folder { account_id, folder_id } => s
            .accounts
            .read()
            .iter()
            .find(|a| &a.account.id == account_id)
            .and_then(|a| a.folders.iter().find(|f| &f.folder.id == folder_id))
            .and_then(|f| f.folder.role),
        Mailbox::Unified(_) => None,
    };
    let outgoing: Vec<Outgoing> = s
        .outgoing
        .read()
        .iter()
        .filter(|o| shows_outgoing(&mailbox, folder_role, &o.draft.from.account_id))
        .cloned()
        .collect();

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
            } else if items.is_empty() && outgoing.is_empty() && cold {
                div { class: "list", role: "listbox", "aria-busy": "true",
                    for i in 0..9 {
                        div { class: "row row--skeleton", key: "sk{i}",
                            span { class: "skeleton" } span { class: "skeleton" } span { class: "skeleton" }
                        }
                    }
                }
            } else if items.is_empty() && outgoing.is_empty() && expired.is_empty() {
                Empty { syncing: s.sync.read().syncing || s.sync.read().last_ok.is_none() }
            } else {
                div { class: "list", role: "listbox", "data-auth": (!expired.is_empty()).then_some("expired"),
                    for o in outgoing.iter() {
                        OutgoingRow { key: "out-{o.draft.id}", out: o.clone(), many }
                    }
                    {rows.into_iter()}
                }
            }
            if let Some(acct) = expired.first() {
                AuthCard { account_id: acct.clone() }
            }
            if offline {
                div { class: "list-footer",
                    Icon { name: "offline", class: "icon--sm" }
                    span { class: "t-caption", "Offline · changes sync when you’re back" }
                }
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
            "data-stale": s.sync.read().needs_sign_in.iter().any(|a| a == item.account_id()).then_some("true"),
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

#[component]
fn OutgoingRow(out: Outgoing, many: bool) -> Element {
    let s = use_context::<AppState>();
    let d = out.draft.clone();
    let hue = s.hue(&d.from.account_id);
    let to = d.to.iter().chain(&d.cc).map(|a| a.display().to_string()).collect::<Vec<_>>().join(", ");
    let subject = if d.subject.is_empty() { "(no subject)".to_string() } else { d.subject.clone() };
    let state = match &out.state {
        OutState::Sending | OutState::Sent(_) => "sending",
        OutState::Failed(_) => "failed",
        OutState::Queued(_) => "queued",
    };
    let letter = d.from.address.email.chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or('?');
    let (retry, open) = (d.clone(), d.clone());
    rsx! {
        div { class: "row", role: "option", "data-hue": "{hue}", "data-state": "{state}",
            onclick: move |_| { let mut dr = s.draft; dr.set(Some(open.clone())); },
            div { class: "row__line",
                if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
                span { class: "row__who", dir: "auto", "To: {to}" }
                span { class: "row__meta",
                    if let OutState::Queued(at) = out.state { time { class: "row__time", "{format::hhmm(at)}" } }
                }
            }
            div { class: "row__subject", dir: "auto", "{subject}" }
            match out.state.clone() {
                OutState::Sending | OutState::Sent(_) => rsx! { div { class: "row__snippet", "Sending…" } },
                OutState::Failed(err) => rsx! {
                    div { class: "row__snippet", title: "{err}",
                        "Failed to send · "
                        a { href: "#", onclick: move |e| {
                                e.prevent_default();
                                e.stop_propagation();
                                s.track_outgoing(retry.clone(), OutState::Sending);
                                send_cmd(Cmd::Send(retry.clone()));
                            },
                            "Retry"
                        }
                        span { class: "t-3", style: "font-weight:400", " · Saved to Drafts" }
                    }
                },
                OutState::Queued(at) => rsx! {
                    div { class: "row__snippet", "Scheduled · {format::long_time(at, chrono::Local::now())}" }
                },
            }
        }
    }
}

#[component]
fn AuthCard(account_id: String) -> Element {
    let s = use_context::<AppState>();
    let acct = s.accounts.read().iter().find(|a| a.account.id == account_id).map(|a| a.account.clone());
    let Some(acct) = acct else { return rsx! {} };
    let provider = acct.provider;
    let label = match provider {
        ProviderKind::Microsoft => "Microsoft 365",
        ProviderKind::Google => "Google",
        ProviderKind::ICloud => "iCloud",
    };
    rsx! {
        div { class: "auth-card", role: "alertdialog",
            div { class: "auth-card__icon", Icon { name: "lock" } }
            div { class: "auth-card__title", "Sign in to {acct.email} again" }
            div { class: "auth-card__body",
                "Your {label} sign-in expired. Mail stays on this Mac; new mail and your changes wait until you sign in."
            }
            button { class: "btn btn--primary", r#type: "button",
                onclick: move |_| {
                    if provider == ProviderKind::ICloud {
                        let (mut open, mut form) = (s.settings_open, s.icloud_form);
                        form.set(true);
                        open.set(true);
                    } else {
                        let mut si = s.signing_in;
                        si.set(true);
                        send_cmd(Cmd::Login(provider));
                    }
                },
                "Sign in again"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_rows_show_in_inbox_sent_drafts_of_their_account() {
        assert!(shows_outgoing(&Mailbox::Unified(FolderRole::Inbox), None, "a"));
        assert!(!shows_outgoing(&Mailbox::Unified(FolderRole::Trash), None, "a"));
        let f = Mailbox::Folder { account_id: "a".into(), folder_id: "x".into() };
        assert!(shows_outgoing(&f, Some(FolderRole::Sent), "a"));
        assert!(!shows_outgoing(&f, Some(FolderRole::Sent), "b"));
        assert!(!shows_outgoing(&f, None, "a"));
    }

    #[test]
    fn scope_is_all_accounts_for_unified() {
        let all = vec!["a".to_string(), "b".to_string()];
        assert_eq!(scope_accounts(&Mailbox::Unified(FolderRole::Inbox), &all), all);
        assert_eq!(scope_accounts(&Mailbox::Folder { account_id: "b".into(), folder_id: "f".into() }, &all), ["b"]);
    }
}
