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
use crate::shortcuts::{Do, hint, tip};
use crate::state::AppState;

/// The reading pane: the open conversation, or a docked new message
/// (Settings › Composing › Docked).
#[component]
pub fn Reader() -> Element {
    let s = use_context::<AppState>();
    if !*s.docked_hidden.read()
        && let Some(d) = super::compose::docked_draft(&s)
    {
        return rsx! {
            main { class: "pane pane--reader pane--composing",
                super::compose::Composer { key: "{d.id}", draft: d, docked: true }
            }
        };
    }
    if !super::multi::items(&s).is_empty() {
        return rsx! { super::multi::MultiPane {} };
    }
    rsx! { ThreadReader {} }
}

/// Nothing open. With mail in the list, the app's bird waits on its perch,
/// looking toward the list; with an empty list the list's own (sleeping) bird
/// is the only one and this pane stays blank.
#[component]
fn ReaderEmpty() -> Element {
    let s = use_context::<AppState>();
    let items = s.visible_items();
    if items.is_empty() {
        return rsx! { div { class: "pane-empty" } };
    }
    let unread = items
        .iter()
        .filter(|i| match i {
            pigeon_engine::service::ListItem::Thread(t) => t.unread_count > 0,
            pigeon_engine::service::ListItem::Message(m) => !m.header.is_read,
        })
        .count();
    let lede = match unread {
        0 => "Choose one on the left, or start at the top.".to_string(),
        1 => "1 unread here. Choose one on the left, or start at the top.".to_string(),
        n => format!("{n} unread here. Choose one on the left, or start at the top."),
    };
    rsx! {
        div { class: "empty-state reader-empty",
            div { class: "perch perch--awake",
                svg { view_box: "4 4 60 56", "aria-hidden": "true", dangerous_inner_html: super::list::AWAKE_BIRD }
            }
            div { class: "empty-state__title", "Pick a conversation" }
            div { class: "empty-state__body", "{lede}" }
            div { class: "reader-empty__keys",
                span { kbd { class: "kbd", {hint(Do::Next)} } kbd { class: "kbd", {hint(Do::Prev)} } "Move" }
                span { kbd { class: "kbd", {hint(Do::Compose)} } "New message" }
                span { kbd { class: "kbd", "⌘F" } "Search this folder" }
                span { kbd { class: "kbd", "⌘K" } "Commands" }
            }
        }
    }
}

#[component]
fn ThreadReader() -> Element {
    let s = use_context::<AppState>();
    let view = s.reader.read().clone();
    let Some(view) = view else {
        return rsx! {
            main { class: "pane pane--reader",
                div { class: "reader-toolbar", super::compose::DockedDraftChip {} }
                ReaderEmpty {}
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
    // In Trash, mail is only restored (Move) or deleted for good (#).
    let in_trash = super::trash_view(&s).is_some();
    let (i1, i2, i3, i4, i5, i6) = (item.clone(), item.clone(), item.clone(), item.clone(), item.clone(), item.clone());
    let (i7, i8) = (item.clone(), item.clone());
    // Spam, or Not spam in the spam folder (phishing is under More).
    let spam = item.as_ref().and_then(|i| {
        super::spam::for_items(&s, std::slice::from_ref(i)).into_iter().find(|a| *a != MailAction::Phishing)
    });

    rsx! {
        main { class: "pane pane--reader",
            div { class: "reader-toolbar",
                if !in_trash {
                button { class: "icon-btn", "aria-label": tip("Archive", Do::Archive), onclick: move |_| if let Some(i) = &i1 { act(s, i, MailAction::Archive) }, Icon { name: "archive", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": tip("Delete", Do::Delete), onclick: move |_| if let Some(i) = &i2 { act(s, i, MailAction::Trash) }, Icon { name: "trash", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": tip("Flag", Do::Flag),
                    onclick: move |_| if let Some(i) = &i3 { act(s, i, if flagged { MailAction::Unflag } else { MailAction::Flag }) },
                    Icon { name: "flag", class: "icon--lg" }
                }
                }
                button { class: "icon-btn", "aria-label": tip("Move", Do::Move), onclick: move |_| super::move_picker::open(s, false), Icon { name: "folder", class: "icon--lg" } }
                if let Some(spam) = spam {
                    {
                        let (icon, tip) = if spam == MailAction::NotSpam {
                            ("inbox", tip("Not spam", Do::Spam))
                        } else {
                            ("alert", tip("Mark as spam", Do::Spam))
                        };
                        rsx! {
                            button { class: "icon-btn", "aria-label": "{tip}",
                                onclick: move |_| if let Some(i) = &i7 { act(s, i, spam.clone()) },
                                Icon { name: icon, class: "icon--lg" }
                            }
                        }
                    }
                }
                // Everything else for this message: the row's right-click menu.
                button { class: "icon-btn", "aria-label": "More",
                    onclick: move |e| if let Some(i) = &i8 {
                        let p = e.client_coordinates();
                        super::row_menu::open(s, i, p.x - 8.0, p.y + 14.0);
                    },
                    Icon { name: "more", class: "icon--lg" }
                }
                super::compose::DockedDraftChip {}
                span { class: "spacer" }
                if total > 0 { span { class: "reader-toolbar__pos", "{pos} of {total}" } }
                span { class: "reader-toolbar__sep" }
                button { class: "icon-btn", "aria-label": tip("Reply", Do::Reply), onclick: move |_| if let Some(i) = &i4 { reply(i, ReplyKind::Reply) }, Icon { name: "reply", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": tip("Reply all", Do::ReplyAll), onclick: move |_| if let Some(i) = &i5 { reply(i, ReplyKind::ReplyAll) }, Icon { name: "replyall", class: "icon--lg" } }
                button { class: "icon-btn", "aria-label": tip("Forward", Do::Forward), onclick: move |_| if let Some(i) = &i6 { reply(i, ReplyKind::Forward) }, Icon { name: "forward", class: "icon--lg" } }
            }
            Thread { key: "{view.account_id}|{view.thread_id}|{view.focus}", view: view.clone() }
        }
    }
}

/// The thread subject: selectable like any text, plus a copy button that
/// shows on hover and turns into a check for a moment once copied.
#[component]
fn SubjectLine(subject: String) -> Element {
    let mut copied = use_signal(|| false);
    let text = subject.clone();
    let (icon, tip) = if *copied.read() { ("check", "Copied") } else { ("copy", "Copy subject") };
    rsx! {
        div { class: "thread-subject-row",
            h2 { class: "thread-subject selectable", dir: "auto", "{subject}" }
            button { class: "icon-btn icon-btn--xs thread-subject__copy", r#type: "button", title: tip,
                "data-copied": if *copied.read() { "true" },
                onclick: move |_| {
                    let js = format!("navigator.clipboard.writeText({})", serde_json::to_string(&text).unwrap_or_default());
                    document::eval(&js);
                    copied.set(true);
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        copied.set(false);
                    });
                },
                Icon { name: icon, class: "icon--sm" }
            }
        }
    }
}

/// A conversation: the main reader's, or a message window's (`windowed`:
/// no inline reply, replies open in a compose window).
#[component]
pub(super) fn Thread(view: ReaderView, #[props(default)] windowed: bool) -> Element {
    let s = use_context::<AppState>();
    let window_want = try_use_context::<Signal<super::reader_window::Want>>();
    // The newest message is always open, plus the one the user picked.
    let latest_id = latest_message_id(&view.messages).unwrap_or_default();
    let mut expanded = use_signal(|| HashSet::from([view.focus.clone(), latest_id.clone()]));
    let hue = s.hue(&view.account_id);
    let account_email = s
        .accounts
        .read()
        .iter()
        .find(|a| a.account.id == view.account_id)
        .map(|a| s.account_label(&a.account.id, &a.account.email))
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
    // Newest on top by default (Settings-free toggle in the thread header).
    let mut newest_first = use_signal(|| crate::prefs::get().thread_newest_first);
    let ordered: Vec<MessageHeader> =
        if *newest_first.read() { view.messages.iter().rev().cloned().collect() } else { view.messages.clone() };
    let terms = search_terms(&s);
    let open_ids: Vec<String> =
        view.messages.iter().filter(|m| expanded.read().contains(&m.id)).map(|m| m.id.clone()).collect();
    let matches = match_count(&s, &view.account_id, &open_ids, &terms);
    let finding = !windowed && s.find.read().is_some();
    // The reply being written here: the main window's, or this message
    // window's own (its compose slot).
    let slot = use_context::<super::compose::ComposeSlot>();
    let draft = slot.draft.read().clone();
    let inline_draft = draft
        .filter(|_| !super::compose::reply_window_mode())
        .filter(|d| d.reply.as_ref().is_some_and(|r| r.account_id == view.account_id && r.thread_id == view.thread_id));
    // A reply draft at the end of the conversation opens for editing right
    // away (inline, or in its own window per Settings › Composing); while it's
    // open the editor stands in for its read-only copy.
    let latest_draft = view.messages.last().filter(|m| m.is_draft).map(|m| m.id.clone());
    use_hook({
        let (account_id, latest_draft) = (view.account_id.clone(), latest_draft.clone());
        let mine = inline_draft.is_some();
        move || {
            if let Some(message_id) = latest_draft.filter(|_| !windowed && !mine) {
                send_cmd(Cmd::EditDraft { account_id, message_id });
            }
        }
    });
    let ordered: Vec<MessageHeader> =
        if inline_draft.is_some() { ordered.into_iter().filter(|m| !m.is_draft).collect() } else { ordered };
    let count = if inline_draft.is_some() { ordered.len() } else { count };

    rsx! {
        div { class: "reader",
            if finding {
                FindBar { total: matches }
            }
            SubjectLine { subject: subject.clone() }
            div { class: "thread-meta",
                span { class: "tag", "data-hue": "{hue}", span { class: "dot dot--sm" } "{account_email}" }
                // A single message is just a message: no conversation summary.
                if count > 1 {
                    span { class: "thread-meta__people", dir: "auto", "{people.join(\", \")} · {count} messages" }
                }
                span { class: "spacer" }
                if matches > 0 && !finding {
                    MatchNav { total: matches }
                }
                if count > 1 {
                    {
                        let nf = *newest_first.read();
                        let icon: &'static str = if nf { "chev-down" } else { "chev-up" };
                        rsx! {
                            button { class: "thread-order", r#type: "button",
                                title: if nf { "Showing newest first. Click for oldest first." } else { "Showing oldest first. Click for newest first." },
                                onclick: move |_| {
                                    let v = !*newest_first.read();
                                    newest_first.set(v);
                                    crate::prefs::update(move |p| p.thread_newest_first = v);
                                },
                                Icon { name: icon, class: "icon--xs" }
                                if nf { "Newest first" } else { "Oldest first" }
                            }
                        }
                    }
                }
            }
            div { class: "thread",
                for m in ordered.iter() {
                    if count == 1 || expanded.read().contains(&m.id) {
                        Message { key: "{m.id}", account_id: view.account_id.clone(), header: m.clone(), windowed }
                    } else {
                        {
                            let id = m.id.clone();
                            let who = m.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default();
                            rsx! {
                                div { class: "msg-collapsed", key: "{m.id}", onclick: move |_| { expanded.write().insert(id.clone()); },
                                    super::avatar::Avatar { email: m.from.as_ref().map(|a| a.email.clone()).unwrap_or_default(), label: who.clone() }
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
                                    let (account_id, thread_id) = (account_id.clone(), thread_id.clone());
                                    match window_want {
                                        Some(want) => super::reader_window::show_whole(want, account_id, thread_id),
                                        None => send_cmd(Cmd::OpenThread { account_id, thread_id }),
                                    }
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
                    let start = move |kind: ReplyKind| {
                        let (account_id, message_id) = (account_id.clone(), last.clone());
                        if windowed {
                            super::reader_window::reply_here(s, account_id, message_id, kind)
                        } else {
                            send_cmd(Cmd::StartReply { account_id, message_id, kind })
                        }
                    };
                    rsx! {
                        div { class: "reply-bar",
                            button { class: "reply-bar__main", r#type: "button", onclick: move |_| start(ReplyKind::Reply),
                                Icon { name: "reply", class: "icon--sm" }
                                span { class: "reply-bar__text", dir: "auto", "Reply to {to}…" }
                            }
                            div { class: "reply-bar__keys", "aria-hidden": "true",
                                span { class: "reply-bar__key", kbd { class: "kbd", {hint(Do::Reply)} } "Reply" }
                                span { class: "reply-bar__key", kbd { class: "kbd", {hint(Do::ReplyAll)} } "Reply all" }
                                span { class: "reply-bar__key", kbd { class: "kbd", {hint(Do::Forward)} } "Forward" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Message(account_id: String, header: MessageHeader, #[props(default)] windowed: bool) -> Element {
    let s = use_context::<AppState>();
    let key = (account_id.clone(), header.id.clone());
    let body = s.bodies.read().get(&key).cloned();
    let invite = body.as_ref().and_then(|b| b.invite.clone()).zip(s.invites.read().get(&key).cloned());
    let from = header.from.clone().unwrap_or_default();
    let time = format::long_time(header.received_at, chrono::Local::now());

    rsx! {
        article { class: "msg",
            div { class: "msg-head",
                super::avatar::Avatar { email: from.email.clone(), label: from.display().to_string(), class: "avatar--32 avatar--ring" }
                div {
                    div { class: "msg-head__from", dir: "auto",
                        super::sender_card::Person { account_id: account_id.clone(), message_id: Some(header.id.clone()), addr: from.clone(), sender: true,
                            "{from.display()} " span { class: "msg-head__addr", "{from.email}" }
                        }
                    }
                    Recipients { label: "to", list: header.to.clone(), account_id: account_id.clone(), message_id: header.id.clone() }
                    if !header.cc.is_empty() { Recipients { label: "cc", list: header.cc.clone(), account_id: account_id.clone(), message_id: header.id.clone() } }
                }
                // Drafts open for editing on their own in the main window; a
                // message window keeps the button.
                if header.is_draft && windowed {
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

/// "to Dana Cohen <dana@x.io>, … +4 more" — every name with its real
/// address; long lists collapse until clicked.
#[component]
fn Recipients(
    label: &'static str,
    list: Vec<pigeon_core::model::Address>,
    account_id: String,
    message_id: String,
) -> Element {
    let mut open = use_signal(|| false);
    let shown = if *open.read() { list.len() } else { format::recipients_shown(list.len(), 3) };
    let more = list.len() - shown;
    rsx! {
        div { class: "msg-head__to", dir: "auto",
            span { class: "msg-head__label", "{label} " }
            for (i, a) in list.iter().take(shown).enumerate() {
                span { key: "{i}",
                    if i > 0 { ", " }
                    super::sender_card::Person { account_id: account_id.clone(), message_id: Some(message_id.clone()), addr: a.clone(),
                        class: "msg-head__rcpt", "{format::with_address(a)}"
                    }
                }
            }
            if more > 0 {
                button { class: "link-btn msg-head__more", r#type: "button", onclick: move |_| open.set(true), "+{more} more" }
            }
        }
    }
}

/// Highlight terms: what Find (⌘F) looks for, as one phrase, or the
/// search's words while the message was opened from search.
fn search_terms(s: &AppState) -> Vec<String> {
    if let Some(q) = s.find.read().as_ref() {
        let q = q.trim().to_lowercase();
        return if q.is_empty() { Vec::new() } else { vec![q] };
    }
    s.search.read().as_ref().map(|st| mailframe::match_terms(&st.query)).unwrap_or_default()
}

/// The most recently received message (ties: the later one in thread order).
pub fn latest_message_id(messages: &[MessageHeader]) -> Option<String> {
    messages.iter().enumerate().max_by_key(|(i, m)| (m.received_at, *i)).map(|(_, m)| m.id.clone())
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
        show_match(i);
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

/// Find in the open conversation (⌘F): matches are marked like search
/// matches; ↵ and ⇧↵ step through them, Esc closes.
#[component]
fn FindBar(total: usize) -> Element {
    let s = use_context::<AppState>();
    let mut find = s.find;
    let mut text = use_signal(|| find.peek().clone().unwrap_or_default());
    let mut current = use_signal(|| 0usize);
    let mut typed = use_signal(|| 0u64);
    // New text, new matches: start again at the first once the frames reload.
    use_effect(move || {
        let _ = find.read();
        current.set(0);
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            show_match(0);
        });
    });
    let i = (*current.read()).min(total.saturating_sub(1));
    let mut go = move |next: bool| {
        if total == 0 {
            return;
        }
        let n = if next {
            (i + 1) % total
        } else if i == 0 {
            total - 1
        } else {
            i - 1
        };
        current.set(n);
        show_match(n);
    };
    let has_text = !text.read().trim().is_empty();
    rsx! {
        div { class: "find-bar", role: "search",
            Icon { name: "search", class: "icon--sm" }
            input { class: "find-bar__input", id: "find-input", r#type: "text", dir: "auto",
                placeholder: "Find in this conversation", autocomplete: "off", spellcheck: "false",
                value: "{text}",
                oninput: move |e| {
                    text.set(e.value());
                    // Each change re-marks the frames: wait for a pause in typing.
                    let n = *typed.peek() + 1;
                    typed.set(n);
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                        if *typed.peek() == n {
                            find.set(Some(text.peek().clone()));
                        }
                    });
                },
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        e.prevent_default();
                        if text.peek().as_str() != find.peek().as_deref().unwrap_or_default() {
                            find.set(Some(text.peek().clone()));
                        } else {
                            go(!e.modifiers().shift());
                        }
                    }
                },
            }
            span { class: "find-bar__count",
                if !has_text { "" } else if total == 0 { "No matches" } else { "{i + 1} of {total}" }
            }
            button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Previous match (⇧↵)",
                disabled: total == 0, onclick: move |_| go(false), Icon { name: "chev-up", class: "icon--sm" } }
            button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Next match (↵)",
                disabled: total == 0, onclick: move |_| go(true), Icon { name: "chev-down", class: "icon--sm" } }
            button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Close (Esc)",
                onclick: move |_| find.set(None), Icon { name: "x", class: "icon--sm" } }
        }
    }
}

/// Make match `i` the current one (marks in the reader and its same-origin
/// srcdoc mail frames) and scroll to it.
fn show_match(i: usize) {
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
    let files: Vec<super::attachments::File> =
        attachments.iter().map(|a| (a.meta.id.clone(), a.meta.name.clone())).collect();
    let total: u64 = attachments.iter().filter_map(|a| a.meta.size).sum();
    let att_menu = use_signal(|| None::<super::attachments::Menu>);
    let (save_account, save_message, save_files) = (account_id.clone(), message_id.clone(), files.clone());
    let (id1, id2) = (message_id.clone(), message_id.clone());

    let terms = search_terms(&s);
    let content = match mailframe::shown(body.html.as_deref(), body.text.as_deref()) {
        mailframe::Shown::Empty => rsx! { div { class: "plain-body" } },
        mailframe::Shown::Html(html) => {
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
        mailframe::Shown::Text(text) => {
            let runs: Vec<(String, bool)> =
                mailframe::split_matches(text, &terms).into_iter().map(|(t, m)| (t.to_string(), m)).collect();
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

    let on_top = prefs.attachments_position != "bottom";
    let files_view = rsx! {
        if attachments.len() > 1 {
            div { class: "attachments__head",
                Icon { name: "clip", class: "icon--sm" }
                span { class: "attachments__count", "{attachments.len()} attachments" }
                if total > 0 { span { class: "attachments__total", "· {format::size(total)}" } }
                button { class: "attachments__save", r#type: "button", title: "Save every attachment to a folder",
                    onclick: move |_| super::attachments::save_all(save_account.clone(), save_message.clone(), save_files.clone()),
                    Icon { name: "download", class: "icon--sm" }
                    "Save all…"
                }
            }
        }
        if !attachments.is_empty() {
            super::attachments::AttachmentMenu { account_id: account_id.clone(), message_id: message_id.clone(), all: files.clone(), menu: att_menu }
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
                        let file = (a.meta.id.clone(), a.meta.name.clone());
                        let mut menu = att_menu;
                        let ringed = menu.read().as_ref().is_some_and(|m| m.file.0 == a.meta.id);
                        let drag = serde_json::json!({
                            "account_id": account_id, "message_id": message_id,
                            "attachment_id": a.meta.id, "name": a.meta.name,
                        })
                        .to_string();
                        rsx! {
                            div { class: "attachment", key: "{a.meta.id}", title: "Open, or drag to a folder to save",
                                "data-drag-out": "{drag}",
                                "data-context": ringed.then_some("true"),
                                oncontextmenu: move |e| {
                                    e.prevent_default();
                                    let p = e.client_coordinates();
                                    menu.set(Some(super::attachments::Menu { x: p.x, y: p.y, file: file.clone() }));
                                },
                                onclick: move |_| send_cmd(Cmd::OpenAttachment {
                                    account_id: account_id.clone(), message_id: message_id.clone(), attachment_id: attachment_id.clone(),
                                    how: Default::default(),
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
    };

    rsx! {
        if on_top { {files_view.clone()} }
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
        if !on_top { {files_view} }
    }
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    fn h(id: &str, at: i64) -> MessageHeader {
        MessageHeader { id: id.into(), received_at: at, ..Default::default() }
    }

    #[test]
    fn latest_is_by_time_not_position() {
        assert_eq!(latest_message_id(&[h("a", 3), h("b", 1), h("c", 2)]).as_deref(), Some("a"));
        assert_eq!(latest_message_id(&[h("a", 1), h("b", 1)]).as_deref(), Some("b"));
        assert_eq!(latest_message_id(&[]), None);
    }
}
