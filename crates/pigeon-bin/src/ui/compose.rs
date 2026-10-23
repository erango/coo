//! Composer: inline reply in the reader and the new-message overlay.
//!
//! The editor holds only what the user types; the quoted original lives in
//! `draft.html` and is appended at send time. Every editor block carries
//! `dir="auto"` (set by the bridge), and Ctrl+Right/Left Shift force RTL/LTR.

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, Sender};
use pigeon_core::model::Address;
use pigeon_core::text::parse_address_list;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::bridge;
use crate::send_cmd;
use crate::state::AppState;

fn editor_id(d: &Draft) -> String {
    format!("editor-{}", d.id)
}

fn join(list: &[Address]) -> String {
    list.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{n} <{}>", a.email),
            None => a.email.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Send the open draft (⌘↵): read the editor, attach the quote, go.
pub fn send_current(s: AppState) {
    let Some(draft) = s.draft.read().clone() else { return };
    if *s.sending.read() {
        return;
    }
    let mut sending = s.sending;
    sending.set(true);
    spawn(async move {
        let typed = bridge::editor_html(&editor_id(&draft)).await;
        let mut d = draft;
        d.html = format!("{typed}{}", d.html);
        send_cmd(Cmd::Send(d));
    });
}

fn use_senders(s: AppState) {
    use_hook(move || {
        if s.senders.read().is_empty() {
            send_cmd(Cmd::LoadSenders);
        }
    });
}

#[component]
fn FromPicker() -> Element {
    let s = use_context::<AppState>();
    let mut open = use_signal(|| false);
    let mut draft = s.draft;
    let Some(d) = draft.read().clone() else { return rsx! {} };
    let hue = s.hue(&d.from.account_id);
    let senders = s.senders.read().clone();
    let accounts = s.accounts.read().clone();

    rsx! {
        span { style: "position:relative",
            button { class: "from-picker", r#type: "button", "data-hue": "{hue}",
                onclick: move |_| { let v = *open.read(); open.set(!v); },
                span { class: "dot" }
                span { dir: "auto", "{d.from.address.name.clone().unwrap_or_default()}" }
                span { class: "from-picker__addr", "{d.from.address.email}" }
                Icon { name: "chev-down", class: "icon--xs" }
            }
            if *open.read() {
                div { class: "from-menu",
                    if senders.is_empty() { div { class: "from-menu__group", "Loading addresses…" } }
                    for a in accounts {
                        {
                            let mine: Vec<Sender> = senders.iter().filter(|x| x.account_id == a.account.id).cloned().collect();
                            rsx! {
                                if !mine.is_empty() { div { class: "from-menu__group", "{a.account.email}" } }
                                for sender in mine {
                                    {
                                        let selected = sender == d.from;
                                        let label = sender.address.email.clone();
                                        let name = sender.address.name.clone().unwrap_or_default();
                                        rsx! {
                                            button { class: "from-menu__item", r#type: "button", "data-hue": "{a.hue}",
                                                "aria-selected": "{selected}",
                                                onclick: move |_| {
                                                    if let Some(d) = draft.write().as_mut() { d.switch_from(sender.clone()); }
                                                    open.set(false);
                                                },
                                                span { class: "dot" }
                                                span { dir: "auto", "{name}" }
                                                span { class: "from-menu__addr", "{label}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// "Replying from X: received at Y" when From crosses accounts.
fn cross_hint(s: &AppState, d: &Draft) -> Option<String> {
    let r = d.reply.as_ref()?;
    if !d.crosses_accounts() {
        return None;
    }
    let received = s.accounts.read().iter().find(|a| a.account.id == r.account_id).map(|a| a.account.email.clone())?;
    Some(format!("Replying from {}, received at {received}. Threading headers are kept.", d.from.address.email))
}

#[component]
pub fn InlineReply(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    use_senders(s);
    let mut to = use_signal(|| join(&draft.to));
    let mut cc = use_signal(|| join(&draft.cc));
    let mut state = s.draft;
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        move || bridge::focus(&id)
    });
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let sending = *s.sending.read();

    // Keep recipients in the draft as they're edited.
    let mut sync_fields = move || {
        if let Some(d) = state.write().as_mut() {
            d.to = parse_address_list(&to.read());
            d.cc = parse_address_list(&cc.read());
        }
    };

    rsx! {
        div { class: "inline-reply",
            div { class: "inline-reply__head",
                FromPicker {}
                span { class: "t-3", style: "font-size:12.5px", "to" }
                input { class: "input--bare inline-reply__to", dir: "auto", value: "{to}",
                    oninput: move |e| { to.set(e.value()); sync_fields(); }
                }
                if !cc.read().is_empty() {
                    span { class: "t-3", style: "font-size:12.5px", "cc" }
                    input { class: "input--bare inline-reply__to", dir: "auto", value: "{cc}",
                        oninput: move |e| { cc.set(e.value()); sync_fields(); }
                    }
                }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Discard draft",
                    onclick: move |_| state.set(None),
                    Icon { name: "x" }
                }
            }
            if let Some(h) = hint { div { class: "cross-account-hint", dir: "auto", "{h}" } }
            div { class: "inline-reply__editor", id: "{id}", contenteditable: "true", "data-editor": "true",
                dir: "auto", "data-placeholder": "Write a reply…  Ctrl+Right Shift for RTL",
            }
            if !current.html.is_empty() {
                button { class: "inline-reply__quoted", r#type: "button", title: "Quoted text is included", "•••" }
            }
            div { class: "inline-reply__footer",
                span { class: "spacer" }
                div { class: "split",
                    button { class: "btn btn--primary", r#type: "button", disabled: sending,
                        onclick: move |_| send_current(s),
                        if sending { "Sending…" } else { "Send" }
                        kbd { class: "kbd", "⌘↵" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn ComposerOverlay() -> Element {
    let s = use_context::<AppState>();
    let draft = s.draft.read().clone();
    let reader = s.reader.read().clone();
    let Some(d) = draft else { return rsx! {} };
    // Replies to the open thread are inline; everything else gets the window.
    let inline =
        d.reply.as_ref().is_some_and(|r| {
            reader.as_ref().is_some_and(|v| v.account_id == r.account_id && v.thread_id == r.thread_id)
        }) && *s.mode.read() == crate::state::Mode::Mail;
    if inline {
        return rsx! {};
    }
    rsx! { Composer { key: "{d.id}", draft: d } }
}

#[component]
fn Composer(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    use_senders(s);
    let mut state = s.draft;
    let mut to = use_signal(|| join(&draft.to));
    let mut cc = use_signal(|| join(&draft.cc));
    let mut subject = use_signal(|| draft.subject.clone());
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        let focus_to = draft.to.is_empty();
        move || bridge::focus(if focus_to { "composer-to" } else { &id })
    });
    let sending = *s.sending.read();
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let title = if current.subject.is_empty() { "New message".to_string() } else { current.subject.clone() };

    let mut sync_fields = move || {
        if let Some(d) = state.write().as_mut() {
            d.to = parse_address_list(&to.read());
            d.cc = parse_address_list(&cc.read());
            d.subject = subject.read().clone();
        }
    };

    rsx! {
        div { class: "composer-overlay",
            div { class: "composer",
                div { class: "composer__titlebar",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)",
                        onclick: move |_| {
                            if let Some(d) = state.read().clone() { send_cmd(Cmd::SaveDraft(d)); }
                            state.set(None);
                        },
                        Icon { name: "x" }
                    }
                    span { class: "composer__title", dir: "auto", "{title}" }
                    span { class: "composer__saved", "Draft" }
                }
                div { class: "composer__fields",
                    div { class: "composer-field",
                        span { class: "composer-field__label", "From" }
                        FromPicker {}
                    }
                    if let Some(h) = hint { div { class: "cross-account-hint", style: "padding:6px 0", "{h}" } }
                    div { class: "composer-field",
                        span { class: "composer-field__label", "To" }
                        input { class: "input--bare", id: "composer-to", style: "flex:1", dir: "auto", value: "{to}",
                            placeholder: "name@example.com, …",
                            oninput: move |e| { to.set(e.value()); sync_fields(); }
                        }
                    }
                    div { class: "composer-field",
                        span { class: "composer-field__label", "Cc" }
                        input { class: "input--bare", style: "flex:1", dir: "auto", value: "{cc}",
                            oninput: move |e| { cc.set(e.value()); sync_fields(); }
                        }
                    }
                    div { class: "composer-field composer-field--subject",
                        input { class: "input--bare", dir: "auto", value: "{subject}", placeholder: "Subject",
                            oninput: move |e| { subject.set(e.value()); sync_fields(); }
                        }
                    }
                }
                div { class: "composer__body",
                    div { class: "composer__editor", id: "{id}", contenteditable: "true", "data-editor": "true", dir: "auto" }
                }
                div { class: "composer__footer",
                    span { class: "t-caption", "Ctrl+Right Shift: RTL · Ctrl+Left Shift: LTR" }
                    span { class: "spacer" }
                    button { class: "btn btn--primary", r#type: "button", disabled: sending,
                        onclick: move |_| send_current(s),
                        if sending { "Sending…" } else { "Send" }
                        kbd { class: "kbd", "⌘↵" }
                    }
                }
            }
        }
    }
}
