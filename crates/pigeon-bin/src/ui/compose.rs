//! Composer: inline reply in the reader and the new-message overlay.
//!
//! The editor holds only what the user types; the quoted original lives in
//! `draft.html` and is appended at send time. Every editor block carries
//! `dir="auto"` (set by the bridge), and Ctrl+Right/Left Shift force RTL/LTR.

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Weekday};
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use pigeon_core::compose::{AttachmentSource, Draft, DraftAttachment, Sender};
use pigeon_core::model::Address;
use pigeon_core::text::parse_address_list;
use pigeon_engine::service::{Cmd, Toast};

use super::Icon;
use super::recipients::{RecipientInput, is_valid};
use crate::bridge;
use crate::send_cmd;
use crate::state::{AppState, OutState};

fn editor_id(d: &Draft) -> String {
    format!("editor-{}", d.id)
}

const SIGNATURE_MARK: &str = "<div data-coo-signature";

/// A plain-text signature as one auto-direction block (lines joined by
/// <br>, so it can be found and stripped again).
pub fn signature_html(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = t.lines().map(pigeon_core::text::escape_html).collect();
    format!("{SIGNATURE_MARK} dir=\"auto\"><br>{}</div>", lines.join("<br>"))
}

/// Remove a signature block added by `signature_html`.
pub fn strip_signature(html: &str) -> String {
    let Some(start) = html.find(SIGNATURE_MARK) else { return html.to_string() };
    match html[start..].find("</div>") {
        Some(end) => format!("{}{}", &html[..start], &html[start + end + "</div>".len()..]),
        None => html[..start].to_string(),
    }
}

/// What goes out: typed text, then the sender's signature, then the quote.
pub fn compose_html(typed: &str, signature: &str, quote: &str) -> String {
    format!("{}{}{quote}", strip_signature(typed), signature_html(signature))
}

pub fn signature_for(email: &str) -> String {
    crate::prefs::get().signatures.get(&email.to_lowercase()).cloned().unwrap_or_default()
}

fn at_local(t: DateTime<Local>, day_offset: i64, h: u32, m: u32) -> i64 {
    let d = t.date_naive() + Duration::days(day_offset);
    Local
        .from_local_datetime(&d.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time")))
        .earliest()
        .map(|x| x.timestamp_millis())
        .unwrap_or_else(|| (t + Duration::days(day_offset)).timestamp_millis())
}

/// Send-later presets: (label, hint, epoch ms). "Later today" only while
/// there's a sensible later today (3 h out, before 20:00).
pub fn send_later_options(now: DateTime<Local>) -> Vec<(&'static str, String, i64)> {
    let mut out = Vec::new();
    let later = now + Duration::hours(3);
    if later.date_naive() == now.date_naive() && later.time() < NaiveTime::from_hms_opt(20, 0, 0).expect("valid") {
        let ms = at_local(now, 0, later.time().format("%H").to_string().parse().unwrap_or(0), 0);
        out.push(("Later today", crate::format::hhmm(ms), ms));
    }
    let tomorrow = at_local(now, 1, 8, 0);
    out.push(("Tomorrow morning", format!("{} 08:00", (now + Duration::days(1)).format("%a")), tomorrow));
    let to_monday = match now.weekday() {
        Weekday::Mon => 7,
        w => 7 - i64::from(w.num_days_from_monday()),
    };
    if to_monday > 1 {
        out.push((
            "Monday morning",
            format!("Mon {} 08:00", (now + Duration::days(to_monday)).format("%-d %b")),
            at_local(now, to_monday, 8, 0),
        ));
    }
    out
}

/// Parse the `<input type=datetime-local>` value (local time).
pub fn parse_local_datetime(v: &str) -> Option<i64> {
    let t = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M").ok()?;
    Local.from_local_datetime(&t).earliest().map(|x| x.timestamp_millis())
}

/// Send the open draft (⌘↵): read the editor, attach signature + quote,
/// and go out after the undo-send window.
pub fn send_current(s: AppState) {
    send_at(s, None);
}

/// Send now (after the undo window) or at `later` (send later).
pub fn send_at(s: AppState, later: Option<i64>) {
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
        s.push_toast(Toast { id: crate::state::next_token(), message, error: true, undo: None, cancel_send: None });
        return;
    }
    let mut sending = s.sending;
    sending.set(true);
    spawn(async move {
        let typed = bridge::editor_html(&editor_id(&draft)).await;
        let mut d = draft;
        d.body_html = compose_html(&typed, &signature_for(&d.from.address.email), "");
        let now = pigeon_core::now_ms();
        let undo_secs = i64::from(crate::prefs::get().undo_send_seconds);
        let (mut open, mut sending) = (s.draft, s.sending);
        match later {
            Some(at) => {
                s.track_outgoing(d.clone(), OutState::Queued(at));
                send_cmd(Cmd::Schedule { draft: d, at, undo_window: false });
                open.set(None);
                sending.set(false);
            }
            None if undo_secs > 0 => {
                s.track_outgoing(d.clone(), OutState::Sending);
                send_cmd(Cmd::Schedule { draft: d, at: now + undo_secs * 1000, undo_window: true });
                open.set(None);
                sending.set(false);
            }
            None => {
                s.track_outgoing(d.clone(), OutState::Sending);
                send_cmd(Cmd::Send(d));
            }
        }
    });
}

/// Autosave after typing pauses (the bridge sends the editor's id).
pub fn autosave(s: AppState, editor: &str) {
    let Some(draft) = s.draft.read().clone() else { return };
    if editor_id(&draft) != editor || *s.sending.read() {
        return;
    }
    let editor = editor.to_string();
    let mut state = s.draft;
    spawn(async move {
        let typed = bridge::editor_html(&editor).await;
        // Keep the open composer in step so a later send/close has it.
        if let Some(d) = state.write().as_mut()
            && d.id == draft.id
        {
            d.body_html = typed.clone();
        }
        let mut d = draft;
        d.body_html = typed;
        send_cmd(Cmd::SaveDraft(d));
    });
}

/// Close the composer, saving what's in the editor (here and, after a
/// moment, on the server).
pub fn save_and_close(s: AppState) {
    let Some(draft) = s.draft.read().clone() else { return };
    let mut state = s.draft;
    state.set(None);
    spawn(async move {
        let typed = bridge::editor_html(&editor_id(&draft)).await;
        let mut d = draft;
        if !typed.is_empty() {
            d.body_html = typed;
        }
        send_cmd(Cmd::SaveDraft(d));
    });
}

/// Throw the draft away, including its server copy.
fn discard(s: AppState) {
    let Some(draft) = s.draft.read().clone() else { return };
    let mut state = s.draft;
    state.set(None);
    send_cmd(Cmd::DiscardDraft { draft_id: draft.id });
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

/// Add picked/dropped files to the open draft.
fn add_files(s: AppState, files: Vec<dioxus::html::FileData>) {
    let mut draft = s.draft;
    if let Some(d) = draft.write().as_mut() {
        for f in files {
            let path = f.path();
            if path.as_os_str().is_empty()
                || d.attachments
                    .iter()
                    .any(|a| matches!(&a.source, AttachmentSource::File { path: p } if std::path::Path::new(p) == path))
            {
                continue;
            }
            d.attachments.push(DraftAttachment {
                name: f.name(),
                content_type: None,
                source: AttachmentSource::File { path: path.to_string_lossy().into_owned() },
            });
        }
    }
}

fn file_size(a: &DraftAttachment) -> String {
    match &a.source {
        AttachmentSource::File { path } => {
            std::fs::metadata(path).map(|m| crate::format::size(m.len())).unwrap_or_default()
        }
        AttachmentSource::Message { .. } => "forwarded".into(),
    }
}

#[component]
fn Attachments() -> Element {
    let s = use_context::<AppState>();
    let mut draft = s.draft;
    let atts = draft.read().as_ref().map(|d| d.attachments.clone()).unwrap_or_default();
    if atts.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "composer__attachments",
            for (i, a) in atts.into_iter().enumerate() {
                div { class: "attachment", key: "{i}-{a.name}",
                    span { class: "attachment__icon", "data-kind": "file", Icon { name: "file", class: "icon--sm" } }
                    div { style: "min-width:0;flex:1",
                        div { class: "attachment__name", dir: "auto", "{a.name}" }
                        div { class: "attachment__size", "{file_size(&a)}" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Remove attachment",
                        onclick: move |_| { if let Some(d) = draft.write().as_mut() && i < d.attachments.len() { d.attachments.remove(i); } },
                        Icon { name: "x", class: "icon--sm" }
                    }
                }
            }
        }
    }
}

/// Paperclip button: a hidden file input (Dioxus desktop opens the native
/// picker and reports real paths).
#[component]
fn AttachButton(id: String) -> Element {
    let s = use_context::<AppState>();
    rsx! {
        label { class: "icon-btn icon-btn--sm", r#for: "{id}", title: "Attach files", "aria-label": "Attach files",
            Icon { name: "clip" }
        }
        input { id: "{id}", r#type: "file", multiple: true, style: "display:none",
            onchange: move |e| add_files(s, e.files()) }
    }
}

#[component]
fn SendButton() -> Element {
    let s = use_context::<AppState>();
    let mut menu = use_signal(debug_extras);
    let mut pick = use_signal(String::new);
    let sending = *s.sending.read();
    let options = send_later_options(Local::now());
    rsx! {
        span { style: "position:relative",
            div { class: "split",
                button { class: "btn btn--primary", r#type: "button", disabled: sending,
                    onclick: move |_| send_current(s),
                    if sending { "Sending…" } else { "Send" }
                    kbd { class: "kbd", "⌘↵" }
                }
                button { class: "btn btn--primary split__more", r#type: "button", "aria-label": "Send later",
                    "aria-expanded": "{*menu.read()}",
                    onclick: move |_| { let v = *menu.read(); menu.set(!v); },
                    Icon { name: "chev-down", class: "icon--sm" }
                }
            }
            if *menu.read() {
                div { class: "menu send-later", style: "position:absolute;right:0;bottom:40px;z-index:30",
                    div { class: "menu__label", "Send later" }
                    for (label, hint, at) in options {
                        div { class: "menu-item", role: "menuitem", key: "{label}",
                            onclick: move |_| { menu.set(false); send_at(s, Some(at)); },
                            Icon { name: "clock", class: "icon--sm" }
                            "{label}"
                            span { class: "menu-item__hint", "{hint}" }
                        }
                    }
                    div { class: "menu-sep" }
                    div { class: "menu-item", style: "gap:8px",
                        Icon { name: "cal", class: "icon--sm" }
                        input { class: "input input--sm", r#type: "datetime-local", style: "flex:1",
                            value: "{pick}", oninput: move |e| pick.set(e.value()) }
                        button { class: "btn btn--secondary btn--sm", r#type: "button",
                            disabled: parse_local_datetime(&pick.read()).is_none_or(|t| t <= pigeon_core::now_ms()),
                            onclick: move |_| {
                                if let Some(at) = parse_local_datetime(&pick.read()) { menu.set(false); send_at(s, Some(at)); }
                            },
                            "Schedule"
                        }
                    }
                }
            }
        }
    }
}

/// The sender's signature as it will be appended (swaps with From).
#[component]
fn SignaturePreview() -> Element {
    let s = use_context::<AppState>();
    let email = s.draft.read().as_ref().map(|d| d.from.address.email.clone()).unwrap_or_default();
    let sig = signature_for(&email);
    if sig.trim().is_empty() {
        return rsx! {};
    }
    rsx! { div { class: "composer__signature", dir: "auto", style: "white-space:pre-wrap", "{sig}" } }
}

/// Drop target: files dragged over the composer show the design's drop zone.
fn drop_handlers(
    s: AppState,
    mut dragging: Signal<bool>,
) -> (EventHandler<DragEvent>, EventHandler<DragEvent>, EventHandler<DragEvent>) {
    (
        EventHandler::new(move |e: DragEvent| {
            e.prevent_default();
            dragging.set(true);
        }),
        EventHandler::new(move |_e: DragEvent| dragging.set(false)),
        EventHandler::new(move |e: DragEvent| {
            e.prevent_default();
            dragging.set(false);
            add_files(s, e.files());
        }),
    )
}

#[component]
pub fn InlineReply(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    use_senders(s);
    let state = s.draft;
    let mut show_cc = use_signal(|| !draft.cc.is_empty());
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        move || bridge::focus(&id)
    });
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let dragging = use_signal(|| false);
    let (over, leave, drop) = drop_handlers(s, dragging);

    rsx! {
        div { class: "inline-reply", "data-dragging": dragging.read().then_some("true"),
            ondragover: move |e| over.call(e), ondragleave: move |e| leave.call(e), ondrop: move |e| drop.call(e),
            div { class: "inline-reply__head", style: "height:auto;min-height:44px",
                FromPicker {}
                span { class: "t-3", style: "font-size:12.5px", "to" }
                RecipientInput { id: "reply-to-{current.id}", value: current.to.clone(),
                    on_change: move |l| set_field(s, Field::To, l) }
                if !*show_cc.read() {
                    button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| show_cc.set(true), "Cc" }
                }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Discard draft",
                    onclick: move |_| discard(s),
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
            div { class: "dropzone", Icon { name: "upload" } "Drop to attach" span { class: "dropzone__meta", "files are added to this reply" } }
            div { class: "inline-reply__editor", id: "{id}", contenteditable: "true", "data-editor": "true",
                dir: "auto", "data-placeholder": "Write a reply…  Ctrl+Right Shift for RTL",
                dangerous_inner_html: "{draft.body_html}",
            }
            div { style: "padding:0 16px", SignaturePreview {} }
            if !current.html.is_empty() {
                button { class: "inline-reply__quoted", r#type: "button", title: "Quoted text is included", "•••" }
            }
            Attachments {}
            div { class: "inline-reply__footer",
                AttachButton { id: "attach-{current.id}" }
                span { class: "spacer" }
                SendButton {}
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

/// Debug builds: PIGEON_DEBUG_EXTRAS=1 with the compose hook shows the
/// composer extras for screenshots (attachment, send-later menu, format bar).
fn debug_extras() -> bool {
    cfg!(debug_assertions) && std::env::var("PIGEON_DEBUG_EXTRAS").is_ok_and(|v| v == "1")
}

#[component]
fn Composer(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    use_senders(s);
    use_hook({
        let id = editor_id(&draft);
        move || {
            if debug_extras() {
                let mut d = s.draft;
                if let Some(d) = d.write().as_mut() {
                    d.subject = "Q4 deck".into();
                    d.attachments.push(DraftAttachment {
                        name: "Q4-roadmap-v3.pdf".into(),
                        content_type: None,
                        source: AttachmentSource::File {
                            path: "/System/Library/CoreServices/SystemVersion.plist".into(),
                        },
                    });
                }
                document::eval(&format!(
                    "setTimeout(()=>window.__pigeonFormatBarDemo&&window.__pigeonFormatBarDemo({id:?}),1200)"
                ));
            }
        }
    });
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
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let title = if current.subject.is_empty() { "New message".to_string() } else { current.subject.clone() };
    let dragging = use_signal(|| false);
    let (over, leave, drop) = drop_handlers(s, dragging);

    rsx! {
        div { class: "composer-overlay",
            div { class: "composer", "data-dragging": dragging.read().then_some("true"),
                ondragover: move |e| over.call(e), ondragleave: move |e| leave.call(e), ondrop: move |e| drop.call(e),
                div { class: "composer__titlebar",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)",
                        onclick: move |_| save_and_close(s),
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
                div { class: "dropzone", Icon { name: "upload" } "Drop to attach" span { class: "dropzone__meta", "files are added to this message" } }
                div { class: "composer__body",
                    div { class: "composer__editor", id: "{id}", contenteditable: "true", "data-editor": "true", dir: "auto",
                        dangerous_inner_html: "{draft.body_html}" }
                    SignaturePreview {}
                }
                Attachments {}
                div { class: "composer__footer",
                    AttachButton { id: "attach-{current.id}" }
                    span { class: "t-caption", "Ctrl+Right Shift: RTL · Ctrl+Left Shift: LTR" }
                    span { class: "spacer" }
                    SendButton {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_round_trip_and_swap() {
        let html = compose_html("<div dir=\"auto\">hi</div>", "Dana\nשלום & co", "<blockquote>q</blockquote>");
        assert!(html.contains("<div data-coo-signature dir=\"auto\"><br>Dana<br>שלום &amp; co</div>"));
        assert!(html.ends_with("<blockquote>q</blockquote>"));
        // Re-sending (e.g. after undo) swaps instead of stacking signatures.
        let again = compose_html(&strip_signature(&html), "Other", "");
        assert_eq!(again.matches(SIGNATURE_MARK).count(), 1);
        assert!(again.contains("Other") && !again.contains("Dana"));
        assert_eq!(signature_html("   "), "");
    }

    #[test]
    fn send_later_presets() {
        let fri_10 = Local.with_ymd_and_hms(2026, 10, 23, 10, 0, 0).unwrap();
        let o = send_later_options(fri_10);
        let labels: Vec<&str> = o.iter().map(|x| x.0).collect();
        assert_eq!(labels, ["Later today", "Tomorrow morning", "Monday morning"]);
        assert_eq!(o[0].2, Local.with_ymd_and_hms(2026, 10, 23, 13, 0, 0).unwrap().timestamp_millis());
        assert_eq!(o[2].2, Local.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap().timestamp_millis());
        // Evening: no "later today". Sunday: Monday is tomorrow, so no duplicate.
        let fri_19 = Local.with_ymd_and_hms(2026, 10, 23, 19, 0, 0).unwrap();
        assert!(send_later_options(fri_19).iter().all(|x| x.0 != "Later today"));
        let sun = Local.with_ymd_and_hms(2026, 10, 25, 9, 0, 0).unwrap();
        assert!(send_later_options(sun).iter().all(|x| x.0 != "Monday morning"));
        assert!(parse_local_datetime("2026-10-24T09:30").is_some());
        assert!(parse_local_datetime("tomorrow").is_none());
    }
}
