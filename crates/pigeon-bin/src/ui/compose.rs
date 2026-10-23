//! Composer: inline reply in the reader and the new-message overlay.
//!
//! The editor holds only what the user types; the quoted original lives in
//! `draft.html` and is appended at send time. Every editor block carries
//! `dir="auto"` (set by the bridge), and Ctrl+Right/Left Shift force RTL/LTR.

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, Sender};
use pigeon_core::model::Address;
use pigeon_core::text::parse_address_list;
use pigeon_engine::service::{Cmd, Toast};

use super::Icon;
use super::recipients::{RecipientInput, is_valid};
use crate::bridge;
use crate::send_cmd;
use crate::state::AppState;

fn editor_id(d: &Draft) -> String {
    format!("editor-{}", d.id)
}

/// Send the open draft (⌘↵): read the editor, attach the quote, go.
pub fn send_current(s: AppState) {
    let Some(draft) = s.draft.read().clone() else { return };
    if *s.sending.read() {
        return;
    }
    let all = || draft.to.iter().chain(&draft.cc).chain(&draft.bcc);
    let problem = if all().next().is_none() {
        Some("Add at least one recipient".to_string())
    } else {
        all().find(|a| !is_valid(&a.email)).map(|a| format!("“{}” isn't a valid address", a.email))
    };
    if let Some(message) = problem {
        s.push_toast(Toast { id: crate::state::next_token(), message, error: true, undo: None });
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

#[derive(Clone, Copy, PartialEq)]
enum Field {
    To,
    Cc,
    Bcc,
}

/// Write one recipient list into the open draft.
fn set_field(s: AppState, field: Field, list: Vec<Address>) {
    let mut draft = s.draft;
    if let Some(d) = draft.write().as_mut() {
        match field {
            Field::To => d.to = list,
            Field::Cc => d.cc = list,
            Field::Bcc => d.bcc = list,
        }
    }
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
    let mut state = s.draft;
    let mut show_cc = use_signal(|| !draft.cc.is_empty());
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        move || bridge::focus(&id)
    });
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let sending = *s.sending.read();

    rsx! {
        div { class: "inline-reply",
            div { class: "inline-reply__head", style: "height:auto;min-height:44px",
                FromPicker {}
                span { class: "t-3", style: "font-size:12.5px", "to" }
                RecipientInput { id: "reply-to-{current.id}", value: current.to.clone(),
                    on_change: move |l| set_field(s, Field::To, l) }
                if !*show_cc.read() {
                    button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| show_cc.set(true), "Cc" }
                }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Discard draft",
                    onclick: move |_| state.set(None),
                    Icon { name: "x" }
                }
            }
            if *show_cc.read() {
                div { class: "inline-reply__head", style: "height:auto;min-height:40px",
                    span { class: "t-3", style: "font-size:12.5px;padding-left:4px", "cc" }
                    RecipientInput { id: "reply-cc-{current.id}", value: current.cc.clone(),
                        on_change: move |l| set_field(s, Field::Cc, l) }
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

/// Debug builds: `PIGEON_DEBUG_OPEN=compose:<to list>[|<partial>]` fills
/// the first new draft for screenshots.
fn debug_compose() -> Option<(Vec<Address>, String)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let v = std::env::var("PIGEON_DEBUG_OPEN").ok()?;
    let rest = v.strip_prefix("compose:")?;
    let (list, partial) = rest.split_once('|').unwrap_or((rest, ""));
    Some((parse_address_list(list), partial.to_string()))
}

#[component]
fn Composer(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    use_senders(s);
    let mut state = s.draft;
    let mut subject = use_signal(|| draft.subject.clone());
    let debug = use_hook(|| {
        static USED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let d = debug_compose().filter(|_| !USED.swap(true, std::sync::atomic::Ordering::Relaxed));
        if let Some((to, _)) = &d {
            set_field(s, Field::To, to.clone());
        }
        d.map(|(_, partial)| partial).unwrap_or_default()
    });
    let mut show_cc_bcc = use_signal(|| !draft.cc.is_empty() || !draft.bcc.is_empty());
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        let focus_to = draft.to.is_empty();
        let to_id = format!("composer-to-{}", draft.id);
        move || bridge::focus(if focus_to { &to_id } else { &id })
    });
    let sending = *s.sending.read();
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let title = if current.subject.is_empty() { "New message".to_string() } else { current.subject.clone() };

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
                        RecipientInput { id: "composer-to-{current.id}", value: current.to.clone(),
                            placeholder: "Name or address", initial_text: debug.clone(),
                            on_change: move |l| set_field(s, Field::To, l) }
                        if !*show_cc_bcc.read() {
                            button { class: "btn btn--ghost btn--sm composer-field__aside", r#type: "button",
                                onclick: move |_| show_cc_bcc.set(true), "Cc Bcc" }
                        }
                    }
                    if *show_cc_bcc.read() {
                        div { class: "composer-field",
                            span { class: "composer-field__label", "Cc" }
                            RecipientInput { id: "composer-cc-{current.id}", value: current.cc.clone(),
                                on_change: move |l| set_field(s, Field::Cc, l) }
                        }
                        div { class: "composer-field",
                            span { class: "composer-field__label", "Bcc" }
                            RecipientInput { id: "composer-bcc-{current.id}", value: current.bcc.clone(),
                                on_change: move |l| set_field(s, Field::Bcc, l) }
                        }
                    }
                    div { class: "composer-field composer-field--subject",
                        input { class: "input--bare", dir: "auto", value: "{subject}", placeholder: "Subject",
                            oninput: move |e| {
                                subject.set(e.value());
                                if let Some(d) = state.write().as_mut() { d.subject = e.value(); }
                            }
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
