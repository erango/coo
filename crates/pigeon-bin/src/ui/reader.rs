use std::collections::HashSet;

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_core::model::{MessageHeader, StoredBody};
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ReaderView};

use super::mailframe;
use super::{Icon, act, reply, selected_item};
use crate::format;
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Reader() -> Element {
    let s = use_context::<AppState>();
    let view = s.reader.read().clone();
    let Some(view) = view else {
        return rsx! {
            main { class: "pane pane--reader",
                div { class: "reader-toolbar" }
                div { class: "pane-empty", "No conversation selected" }
            }
        };
    };
    let item = selected_item(&s);
    let (pos, total) = {
        let list = s.list.read();
        let key = s.selected.read().clone();
        (key.and_then(|k| list.iter().position(|i| AppState::key(i) == k)).map(|p| p + 1).unwrap_or(0), list.len())
    };
    let flagged = item.as_ref().is_some_and(|i| i.header().is_flagged);
    let (i1, i2, i3, i4, i5, i6) = (item.clone(), item.clone(), item.clone(), item.clone(), item.clone(), item.clone());

    rsx! {
        main { class: "pane pane--reader",
            div { class: "reader-toolbar",
                button { class: "icon-btn", "aria-label": "Archive (E)", onclick: move |_| if let Some(i) = &i1 { act(s, i, MailAction::Archive) }, Icon { name: "archive", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": "Delete (#)", onclick: move |_| if let Some(i) = &i2 { act(s, i, MailAction::Trash) }, Icon { name: "trash", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": "Flag (S)",
                    onclick: move |_| if let Some(i) = &i3 { act(s, i, if flagged { MailAction::Unflag } else { MailAction::Flag }) },
                    Icon { name: "flag", class: "icon--lg" }
                }
                span { class: "spacer" }
                if total > 0 { span { class: "reader-toolbar__pos", "{pos} of {total}" } }
                span { class: "reader-toolbar__sep" }
                button { class: "icon-btn", "aria-label": "Reply (R)", onclick: move |_| if let Some(i) = &i4 { reply(i, ReplyKind::Reply) }, Icon { name: "reply", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": "Reply all (A)", onclick: move |_| if let Some(i) = &i5 { reply(i, ReplyKind::ReplyAll) }, Icon { name: "replyall", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": "Forward (F)", onclick: move |_| if let Some(i) = &i6 { reply(i, ReplyKind::Forward) }, Icon { name: "forward", class: "icon--lg" } }
            }
            Thread { key: "{view.account_id}|{view.thread_id}|{view.focus}", view: view.clone() }
        }
    }
}

#[component]
fn Thread(view: ReaderView) -> Element {
    let s = use_context::<AppState>();
    let mut expanded = use_signal(|| HashSet::from([view.focus.clone()]));
    let hue = s.hue(&view.account_id);
    let account_email = s
        .accounts
        .read()
        .iter()
        .find(|a| a.account.id == view.account_id)
        .map(|a| a.account.email.clone())
        .unwrap_or_default();
    let subject = view
        .messages
        .last()
        .map(|m| if m.subject.is_empty() { "(no subject)".to_string() } else { m.subject.clone() })
        .unwrap_or_default();
    let mut people: Vec<String> = Vec::new();
    for m in &view.messages {
        if let Some(f) = &m.from {
            let n = f.display().to_string();
            if !people.contains(&n) {
                people.push(n);
            }
        }
    }
    let count = view.messages.len();
    let draft = s.draft.read().clone();
    let inline_draft = draft
        .filter(|d| d.reply.as_ref().is_some_and(|r| r.account_id == view.account_id && r.thread_id == view.thread_id));

    rsx! {
        div { class: "reader",
            h2 { class: "thread-subject", dir: "auto", "{subject}" }
            div { class: "thread-meta",
                span { class: "tag", "data-hue": "{hue}", span { class: "dot dot--sm" } "{account_email}" }
                span { class: "thread-meta__people", dir: "auto",
                    "{people.join(\", \")}"
                    if count > 1 { " · {count} messages" }
                }
            }
            div { class: "thread",
                for m in view.messages.iter() {
                    if expanded.read().contains(&m.id) {
                        Message { key: "{m.id}", account_id: view.account_id.clone(), header: m.clone() }
                    } else {
                        {
                            let id = m.id.clone();
                            let who = m.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default();
                            rsx! {
                                div { class: "msg-collapsed", key: "{m.id}", onclick: move |_| { expanded.write().insert(id.clone()); },
                                    span { class: "avatar", "data-hue": "{hue}", "{format::initials(&who)}" }
                                    span { class: "msg-collapsed__who", dir: "auto", "{who}" }
                                    span { class: "msg-collapsed__snip", dir: "auto", "{m.snippet}" }
                                    span { class: "msg-collapsed__time", "{format::row_time(m.received_at, chrono::Local::now())}" }
                                }
                            }
                        }
                    }
                }
            }
            if view.single && count == 1 {
                {
                    let (account_id, thread_id) = (view.account_id.clone(), view.thread_id.clone());
                    rsx! {
                        div { class: "conversation-link",
                            Icon { name: "lines", class: "icon--sm" }
                            "Part of a conversation"
                            a { class: "conversation-link__action", href: "#",
                                onclick: move |e| {
                                    e.prevent_default();
                                    send_cmd(Cmd::OpenThread { account_id: account_id.clone(), thread_id: thread_id.clone() });
                                },
                                "View whole conversation"
                            }
                        }
                    }
                }
            }
        }
        div { style: "padding:12px 48px 20px;flex:none",
            match inline_draft {
                Some(d) => rsx! { super::compose::InlineReply { key: "{d.id}", draft: d } },
                None => {
                    let last = view.messages.last().map(|m| m.id.clone()).unwrap_or_default();
                    let account_id = view.account_id.clone();
                    let to = view.messages.last().and_then(|m| m.from.as_ref()).map(|a| a.display().to_string()).unwrap_or_default();
                    rsx! {
                        div { class: "inline-reply inline-reply--collapsed",
                            onclick: move |_| send_cmd(Cmd::StartReply { account_id: account_id.clone(), message_id: last.clone(), kind: ReplyKind::Reply }),
                            div { class: "inline-reply__editor", "data-placeholder": "Reply to {to}…  R reply · A reply all · F forward" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Message(account_id: String, header: MessageHeader) -> Element {
    let s = use_context::<AppState>();
    let hue = s.hue(&account_id);
    let key = (account_id.clone(), header.id.clone());
    let body = s.bodies.read().get(&key).cloned();
    let invite = body.as_ref().and_then(|b| b.invite.clone()).zip(s.invites.read().get(&key).cloned());
    let from = header.from.clone().unwrap_or_default();
    let to_line = header.to.iter().chain(&header.cc).map(|a| a.display().to_string()).collect::<Vec<_>>().join(", ");
    let time = format::long_time(header.received_at, chrono::Local::now());

    rsx! {
        article { class: "msg",
            div { class: "msg-head",
                span { class: "avatar avatar--32 avatar--ring", "data-hue": "{hue}", "{format::initials(from.display())}" }
                div {
                    div { class: "msg-head__from", dir: "auto", "{from.display()} " span { class: "msg-head__addr", "{from.email}" } }
                    div { class: "msg-head__to", dir: "auto", "to {to_line}" }
                }
                span { class: "msg-head__time", "{time}" }
            }
            if let Some((inv, view)) = invite {
                super::invite::InviteCard { account_id: account_id.clone(), invite: inv, view }
            }
            match body {
                None => rsx! { div { class: "plain-body t-3", "Loading…" } },
                Some(b) => rsx! { Body { account_id: account_id.clone(), message_id: header.id.clone(), body: b } },
            }
        }
    }
}

#[component]
fn Body(account_id: String, message_id: String, body: StoredBody) -> Element {
    let s = use_context::<AppState>();
    let mut remote_ok = s.remote_ok;
    let mut flip = s.render_flip;
    let allowed = remote_ok.read().contains(&message_id);
    let flipped = flip.read().contains(&message_id);
    let dark = s.dark();
    let attachments: Vec<_> = body.attachments.iter().filter(|a| !a.meta.is_inline).cloned().collect();
    let (id1, id2) = (message_id.clone(), message_id.clone());

    let content = match &body.html {
        Some(html) => {
            let adapted = mailframe::is_complex(html) == flipped;
            let doc = mailframe::srcdoc(html, &s.palette.read(), dark, adapted, allowed);
            rsx! {
                div { class: "mail-body",
                    button { class: "btn btn--ghost btn--sm mail-body__toggle", r#type: "button",
                        onclick: move |_| { let mut f = flip.write(); if !f.remove(&id1) { f.insert(id1.clone()); } },
                        if adapted { "Original colors" } else { "Match theme" }
                    }
                    iframe { class: "mail-frame", "data-render": if adapted { "adapted" } else { "paper" },
                        "sandbox": "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
                        srcdoc: "{doc}"
                    }
                }
            }
        }
        None => rsx! { div { class: "plain-body", dir: "auto", "{body.text.clone().unwrap_or_default()}" } },
    };

    rsx! {
        if body.has_remote_content && !allowed {
            div { class: "banner images-banner",
                Icon { name: "eye", class: "icon--sm" }
                span { class: "spacer", "Remote images are blocked to keep senders from tracking you." }
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| { remote_ok.write().insert(id2.clone()); },
                    "Load images"
                }
            }
        }
        {content}
        if !attachments.is_empty() {
            div { class: "attachments",
                for a in attachments {
                    {
                        let kind = match a.meta.content_type.as_deref().unwrap_or_default() {
                            ct if ct.contains("pdf") => "pdf",
                            ct if ct.contains("sheet") || ct.contains("excel") || ct.contains("csv") => "sheet",
                            ct if ct.contains("word") || ct.contains("document") => "doc",
                            _ => "file",
                        };
                        let icon = match kind { "pdf" => "pdf", "sheet" => "sheet", _ => "file" };
                        let size = a.meta.size.map(format::size).unwrap_or_default();
                        let (account_id, message_id, attachment_id) = (account_id.clone(), message_id.clone(), a.meta.id.clone());
                        rsx! {
                            div { class: "attachment", key: "{a.meta.id}", title: "Open",
                                onclick: move |_| send_cmd(Cmd::OpenAttachment {
                                    account_id: account_id.clone(), message_id: message_id.clone(), attachment_id: attachment_id.clone(),
                                }),
                                span { class: "attachment__icon", "data-kind": "{kind}", Icon { name: icon, class: "icon--sm" } }
                                div { style: "min-width:0",
                                    div { class: "attachment__name", dir: "auto", "{a.meta.name}" }
                                    div { class: "attachment__size", "{size}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
