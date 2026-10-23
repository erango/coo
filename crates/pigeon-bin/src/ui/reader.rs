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
                button { class: "icon-btn", "aria-label": "Move (V)", onclick: move |_| super::move_picker::open(s, false), Icon { name: "folder", class: "icon--lg" } }
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
    let terms = search_terms(&s);
    let open_ids: Vec<String> =
        view.messages.iter().filter(|m| expanded.read().contains(&m.id)).map(|m| m.id.clone()).collect();
    let matches = match_count(&s, &view.account_id, &open_ids, &terms);
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
                if matches > 0 {
                    span { class: "spacer" }
                    MatchNav { total: matches }
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
                if header.is_draft {
                    {
                        let (account_id, message_id) = (account_id.clone(), header.id.clone());
                        rsx! {
                            button { class: "btn btn--secondary btn--sm", r#type: "button",
                                onclick: move |_| send_cmd(Cmd::EditDraft { account_id: account_id.clone(), message_id: message_id.clone() }),
                                Icon { name: "edit", class: "icon--sm" }
                                "Edit draft"
                            }
                        }
                    }
                }
                span { class: "msg-head__time", "{time}" }
            }
            if let Some((inv, view)) = invite {
                super::invite::InviteCard { account_id: account_id.clone(), invite: inv, view }
            }
            match body {
                None => rsx! { div { class: "plain-body t-3", "Loading…" } },
                Some(b) => rsx! { Body { account_id: account_id.clone(), message_id: header.id.clone(),
                    sender: header.from.as_ref().map(|a| a.email.to_lowercase()).unwrap_or_default(), body: b } },
            }
        }
    }
}

/// Highlight terms while the message was opened from search.
fn search_terms(s: &AppState) -> Vec<String> {
    s.search.read().as_ref().map(|st| mailframe::match_terms(&st.query)).unwrap_or_default()
}

/// Matches in the expanded messages' loaded bodies.
fn match_count(s: &AppState, account_id: &str, ids: &[String], terms: &[String]) -> usize {
    if terms.is_empty() {
        return 0;
    }
    let bodies = s.bodies.read();
    ids.iter()
        .filter_map(|id| bodies.get(&(account_id.to_string(), id.clone())))
        .map(|b| match &b.html {
            Some(h) => mailframe::highlight_html(h, terms).1,
            None => {
                mailframe::split_matches(b.text.as_deref().unwrap_or_default(), terms).iter().filter(|r| r.1).count()
            }
        })
        .sum()
}

/// "2 of 5" with previous/next: moves `data-current` across the marks in
/// the reader (mail frames are same-origin srcdoc) and scrolls to it.
#[component]
fn MatchNav(total: usize) -> Element {
    let current = use_signal(|| 0usize);
    let go = move |i: usize| {
        let mut c = current;
        c.set(i);
        document::eval(&format!(
            "(()=>{{const r=document.querySelector('.pane--reader');if(!r)return;\
             const marks=[];for(const n of r.querySelectorAll('.plain-body mark[data-pg-match], iframe.mail-frame')){{\
             if(n.tagName==='IFRAME'){{try{{marks.push(...n.contentDocument.querySelectorAll('mark[data-pg-match]'))}}catch(_){{}}}}else marks.push(n);}}\
             marks.forEach((m,k)=>{{if(k==={i})m.setAttribute('data-current','');else m.removeAttribute('data-current');}});\
             const m=marks[{i}];if(!m)return;const f=m.ownerDocument.defaultView.frameElement;\
             const top=(f?f.getBoundingClientRect().top:0)+m.getBoundingClientRect().top;\
             const sc=r.querySelector('.reader');const box=(sc||r).getBoundingClientRect();\
             (sc||r).scrollBy({{top:top-box.top-box.height/3,behavior:'smooth'}});}})()"
        ));
    };
    // Mark the first match once the frames have loaded.
    use_effect(move || {
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            go(0);
        });
    });
    let i = *current.read();
    rsx! {
        div { class: "match-nav", role: "group", "aria-label": "Search matches",
            span { "{i + 1} of {total}" }
            button { class: "icon-btn icon-btn--xs", "aria-label": "Previous match",
                onclick: move |_| go(if i == 0 { total - 1 } else { i - 1 }), Icon { name: "chev-up", class: "icon--sm" } }
            button { class: "icon-btn icon-btn--xs", "aria-label": "Next match",
                onclick: move |_| go((i + 1) % total), Icon { name: "chev-down", class: "icon--sm" } }
        }
    }
}

#[component]
fn Body(account_id: String, message_id: String, sender: String, body: StoredBody) -> Element {
    let s = use_context::<AppState>();
    let mut remote_ok = s.remote_ok;
    let mut flip = s.render_flip;
    let prefs = crate::prefs::get();
    let trusted = prefs.remote_images_always || (!sender.is_empty() && prefs.remote_images_senders.contains(&sender));
    let allowed = trusted || remote_ok.read().contains(&message_id);
    let (always_id, always_sender) = (message_id.clone(), sender.clone());
    let flipped = flip.read().contains(&message_id);
    let dark = s.dark();
    let attachments: Vec<_> = body.attachments.iter().filter(|a| !a.meta.is_inline).cloned().collect();
    let (id1, id2) = (message_id.clone(), message_id.clone());

    let terms = search_terms(&s);
    let content = match &body.html {
        Some(html) => {
            let adapted = mailframe::is_complex(html) == flipped;
            let (marked, _) = mailframe::highlight_html(html, &terms);
            let doc = mailframe::srcdoc(&marked, &s.palette.read(), dark, adapted, allowed);
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
        None => {
            let text = body.text.clone().unwrap_or_default();
            let runs: Vec<(String, bool)> =
                mailframe::split_matches(&text, &terms).into_iter().map(|(t, m)| (t.to_string(), m)).collect();
            rsx! {
                div { class: "plain-body", dir: "auto",
                    // One inline run: .plain-body is a flex column.
                    div { dir: "auto", style: "unicode-bidi:plaintext",
                        for (i, (t, m)) in runs.into_iter().enumerate() {
                            if m { mark { key: "{i}", "data-pg-match": "true", "{t}" } } else { span { key: "{i}", "{t}" } }
                        }
                    }
                }
            }
        }
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
                if !sender.is_empty() {
                    button { class: "btn btn--ghost btn--sm", r#type: "button", title: "Settings › Privacy lists trusted senders",
                        onclick: move |_| {
                            let who = always_sender.clone();
                            crate::prefs::update(move |p| if !p.remote_images_senders.contains(&who) { p.remote_images_senders.push(who) });
                            remote_ok.write().insert(always_id.clone());
                        },
                        "Always from {sender}"
                    }
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
