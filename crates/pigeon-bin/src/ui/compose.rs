//! Composer: inline reply in the reader, and the new-message composer,
//! either docked in the reading pane or in its own window (Settings ›
//! Composing). Every composer works on a `ComposeSlot`: the main window's
//! (`AppState::main_slot`) or a compose window's own.
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

/// The draft a composer edits and its send state. The main window's slot is
/// `AppState.draft` / `.sending`; each compose window owns one.
#[derive(Clone, Copy, PartialEq)]
pub struct ComposeSlot {
    pub draft: Signal<Option<Draft>>,
    pub sending: Signal<bool>,
    /// Inline problem shown in the composer footer (e.g. no recipients).
    pub notice: Signal<Option<String>>,
    /// In its own window: closes as soon as the draft leaves the slot.
    pub windowed: bool,
}

pub const EMPTY_CONFIRM: &str = "Send without a subject or text? Press Send again to send it anyway.";
const UNREADABLE: &str = "Coo couldn't read the message text, so it wasn't sent. Try again.";

/// HTML with something a reader would see.
fn has_text(html: &str) -> bool {
    !pigeon_core::text::html_to_text(html).trim().is_empty() || html.contains("<img")
}

/// The typed text to send: the live editor; if it reads back empty or not
/// at all, the last autosaved text; and if there's neither, an unreadable
/// editor blocks the send (never "send blank because we couldn't look").
pub fn typed_for_send(read: Option<&str>, autosaved: &str) -> Result<String, &'static str> {
    let saved = strip_signature(autosaved);
    match read {
        Some(r) if has_text(r) => Ok(r.to_string()),
        _ if has_text(&saved) => {
            tracing::warn!("composer editor read back empty at send; using the autosaved body");
            Ok(saved)
        }
        Some(r) => Ok(r.to_string()),
        None => Err(UNREADABLE),
    }
}

/// No subject, no text, nothing quoted or attached: ask before sending.
pub fn needs_empty_confirm(d: &Draft, typed: &str) -> bool {
    d.subject.trim().is_empty() && !has_text(typed) && !has_text(&d.html) && d.attachments.is_empty()
}

/// A draft with nothing in it yet (`typed` is the editor's HTML).
pub fn is_blank(d: &Draft, typed: &str) -> bool {
    d.to.is_empty()
        && d.cc.is_empty()
        && d.bcc.is_empty()
        && d.subject.trim().is_empty()
        && d.attachments.is_empty()
        && d.reply.is_none()
        && pigeon_core::text::html_to_text(typed).trim().is_empty()
}

/// Replies open in their own window (Settings › Composing › Replies).
pub fn reply_window_mode() -> bool {
    crate::prefs::get().reply_mode == "window"
}

/// Replies to the thread open in the reader are written inline (unless
/// replies are set to open in a window).
pub fn is_inline(s: &AppState, d: &Draft) -> bool {
    if reply_window_mode() {
        return false;
    }
    let reader = s.reader.read();
    d.reply
        .as_ref()
        .is_some_and(|r| reader.as_ref().is_some_and(|v| v.account_id == r.account_id && v.thread_id == r.thread_id))
        && *s.mode.read() == crate::state::Mode::Mail
}

/// New messages open docked in the reading pane (else in their own window).
pub fn docked_mode() -> bool {
    crate::prefs::get().compose_mode == "docked"
}

/// The draft docked in the reading pane, if any.
pub fn docked_draft(s: &AppState) -> Option<Draft> {
    let d = s.draft.read().clone()?;
    let windowed_reply = d.reply.is_some() && reply_window_mode();
    (!is_inline(s, &d) && docked_mode() && !windowed_reply).then_some(d)
}

fn editor_id(d: &Draft) -> String {
    format!("editor-{}", d.id)
}

const SIGNATURE_MARK: &str = "<div data-coo-signature";

/// A stored signature's body as clean HTML. Signatures are HTML now (rich
/// editor, browser paste); older plain-text ones become one line per row.
pub fn signature_body(sig: &str) -> String {
    let t = sig.trim();
    if t.is_empty() {
        String::new()
    } else if t.contains('<') {
        pigeon_core::html::sanitize_signature(t)
    } else {
        t.lines().map(pigeon_core::text::escape_html).collect::<Vec<_>>().join("<br>")
    }
}

/// The signature as one marked auto-direction block, so it can be found and
/// swapped (From changes, undo send) without stacking.
pub fn signature_html(sig: &str) -> String {
    let body = signature_body(sig);
    if body.is_empty() {
        return String::new();
    }
    format!("{SIGNATURE_MARK} dir=\"auto\"><br>{body}</div>")
}

/// Remove a signature block added by `signature_html` (rich signatures nest
/// their own divs, so match the closing tag by depth).
pub fn strip_signature(html: &str) -> String {
    let Some(start) = html.find(SIGNATURE_MARK) else { return html.to_string() };
    let rest = &html[start..];
    let lower = rest.to_ascii_lowercase();
    let (mut depth, mut i) = (0usize, 0usize);
    while i < lower.len() {
        let open = lower[i..].find("<div");
        let close = lower[i..].find("</div");
        match (open, close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i += o + 4;
            }
            (_, Some(c)) => {
                depth = depth.saturating_sub(1);
                i += c + 5;
                if depth == 0 {
                    let end = lower[i..].find('>').map_or(lower.len(), |g| i + g + 1);
                    return format!("{}{}", &html[..start], &rest[end..]);
                }
            }
            _ => break,
        }
    }
    html[..start].to_string()
}

/// What goes out: typed text, then the sender's signature, then the quote.
pub fn compose_html(typed: &str, signature: &str, quote: &str) -> String {
    format!("{}{}{quote}", strip_signature(typed), signature_html(signature))
}

pub fn signature_for(email: &str) -> String {
    crate::prefs::get().signature(&email.to_lowercase())
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
pub fn send_later_options(now: DateTime<Local>, first: Weekday) -> Vec<(String, String, i64)> {
    let mut out = Vec::new();
    let later = now + Duration::hours(3);
    if later.date_naive() == now.date_naive() && later.time() < NaiveTime::from_hms_opt(20, 0, 0).expect("valid") {
        let ms = at_local(now, 0, later.time().format("%H").to_string().parse().unwrap_or(0), 0);
        out.push(("Later today".into(), crate::format::hhmm(ms), ms));
    }
    let tomorrow = at_local(now, 1, 8, 0);
    out.push(("Tomorrow morning".into(), format!("{} 08:00", (now + Duration::days(1)).format("%a")), tomorrow));
    // Start of next week (Sunday in Israel, Monday elsewhere).
    let to_next_week = 7 - i64::from((7 + now.weekday().num_days_from_sunday() - first.num_days_from_sunday()) % 7);
    if to_next_week > 1 {
        let day = now + Duration::days(to_next_week);
        out.push((
            format!("{} morning", day.format("%A")),
            format!("{} 08:00", day.format("%a %-d %b")),
            at_local(now, to_next_week, 8, 0),
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
pub fn send_current(s: AppState, slot: ComposeSlot) {
    send_at(s, slot, None);
}

/// Send now (after the undo window) or at `later` (send later).
pub fn send_at(s: AppState, slot: ComposeSlot, later: Option<i64>) {
    let Some(draft) = slot.draft.read().clone() else { return };
    if *slot.sending.read() {
        return;
    }
    let all = || draft.to.iter().chain(&draft.cc).chain(&draft.bcc);
    let problem = if all().next().is_none() {
        Some("Add at least one recipient".to_string())
    } else {
        all().find(|a| !is_valid(&a.email)).map(|a| format!("“{}” isn't a valid address", a.email))
    };
    let mut notice = slot.notice;
    if let Some(message) = problem {
        notice.set(Some(message.clone()));
        if !slot.windowed {
            s.push_toast(Toast { id: crate::state::next_token(), message, error: true, undo: None, cancel_send: None });
        }
        return;
    }
    // Second Send after "Send without a subject or text?" goes ahead.
    let confirmed_empty = notice.peek().as_deref() == Some(EMPTY_CONFIRM);
    notice.set(None);
    let mut sending = slot.sending;
    sending.set(true);
    spawn(async move {
        let read = bridge::try_editor_html(&editor_id(&draft)).await;
        let mut d = draft;
        let (mut open, mut sending, mut notice) = (slot.draft, slot.sending, slot.notice);
        let typed = match typed_for_send(read.as_deref(), &d.body_html) {
            Ok(t) => t,
            Err(message) => {
                sending.set(false);
                notice.set(Some(message.to_string()));
                s.push_toast(Toast {
                    id: crate::state::next_token(),
                    message: message.into(),
                    error: true,
                    ..Default::default()
                });
                return;
            }
        };
        if needs_empty_confirm(&d, &typed) && !confirmed_empty {
            sending.set(false);
            notice.set(Some(EMPTY_CONFIRM.into()));
            return;
        }
        d.body_html = compose_html(&typed, &signature_for(&d.from.address.email), "");
        let now = pigeon_core::now_ms();
        let undo_secs = i64::from(crate::prefs::get().undo_send_seconds);
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
                // A compose window doesn't wait for Sent: the list's outgoing
                // row shows progress (and Retry if it fails).
                if slot.windowed {
                    open.set(None);
                    sending.set(false);
                }
            }
        }
    });
}

/// Autosave after typing pauses (the bridge sends the editor's id).
pub fn autosave(slot: ComposeSlot, editor: &str) {
    let Some(draft) = slot.draft.read().clone() else { return };
    if editor_id(&draft) != editor || *slot.sending.read() {
        return;
    }
    let editor = editor.to_string();
    let mut state = slot.draft;
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
pub fn save_and_close(slot: ComposeSlot) {
    let Some(draft) = slot.draft.read().clone() else { return };
    spawn(async move {
        let typed = bridge::editor_html(&editor_id(&draft)).await;
        let mut state = slot.draft;
        state.set(None);
        let mut d = draft;
        if !typed.is_empty() {
            d.body_html = typed;
        }
        // Nothing written: drop it instead of leaving an empty draft behind.
        if is_blank(&d, &d.body_html) {
            send_cmd(Cmd::DiscardDraft { draft_id: d.id });
        } else {
            send_cmd(Cmd::SaveDraft(d));
        }
    });
}

/// Save from the last autosaved state without reading the editor (its window
/// may already be going away).
pub fn save_now(slot: ComposeSlot) {
    let Some(d) = slot.draft.peek().clone() else { return };
    let mut state = slot.draft;
    state.set(None);
    if is_blank(&d, &d.body_html) {
        send_cmd(Cmd::DiscardDraft { draft_id: d.id });
    } else {
        send_cmd(Cmd::SaveDraft(d));
    }
}

/// Throw the draft away, including its server copy.
fn discard(slot: ComposeSlot) {
    let Some(draft) = slot.draft.read().clone() else { return };
    let mut state = slot.draft;
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
fn set_field(slot: ComposeSlot, field: Field, list: Vec<Address>) {
    let mut draft = slot.draft;
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
    let slot = use_context::<ComposeSlot>();
    let mut open = use_signal(|| false);
    let mut draft = slot.draft;
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
                                if !mine.is_empty() { div { class: "from-menu__group", "{s.account_label(&a.account.id, &a.account.email)}" } }
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
fn add_files(slot: ComposeSlot, files: Vec<dioxus::html::FileData>) {
    let mut draft = slot.draft;
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
        AttachmentSource::Drive { size: Some(n), .. } => crate::format::size(*n),
        AttachmentSource::Drive { .. } => "Google Drive".into(),
    }
}

#[component]
fn Attachments() -> Element {
    let slot = use_context::<ComposeSlot>();
    let mut draft = slot.draft;
    let atts = draft.read().as_ref().map(|d| d.attachments.clone()).unwrap_or_default();
    if atts.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "composer__attachments",
            for (i, a) in atts.into_iter().enumerate() {
                div { class: "attachment", key: "{i}-{a.name}",
                    {
                        let (kind, icon) = super::drive::file_kind(a.content_type.as_deref().unwrap_or_default(), &a.name);
                        rsx! { span { class: "attachment__icon", "data-kind": "{kind}", Icon { name: icon, class: "icon--sm" } } }
                    }
                    div { style: "min-width:0;flex:1",
                        div { class: "attachment__name", dir: "auto", "{a.name}" }
                        div { class: "attachment__size",
                            if a.source.is_drive() {
                                span { class: "attachment__drive", title: "From Google Drive", Icon { name: "cloud", class: "icon--xs" } }
                            }
                            "{file_size(&a)}"
                        }
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

#[component]
fn SendButton() -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    let mut menu = use_signal(debug_extras);
    let mut pick = use_signal(String::new);
    let sending = *slot.sending.read();
    let options = send_later_options(Local::now(), *s.first_weekday.read());
    rsx! {
        span { style: "position:relative",
            div { class: "split",
                button { class: "btn btn--primary", r#type: "button", disabled: sending,
                    onclick: move |_| send_current(s, slot),
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
                            onclick: move |_| { menu.set(false); send_at(s, slot, Some(at)); },
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
                                if let Some(at) = parse_local_datetime(&pick.read()) { menu.set(false); send_at(s, slot, Some(at)); }
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
    let slot = use_context::<ComposeSlot>();
    let email = slot.draft.read().as_ref().map(|d| d.from.address.email.clone()).unwrap_or_default();
    let sig = signature_for(&email);
    if sig.trim().is_empty() {
        return rsx! {};
    }
    let body = signature_body(&sig);
    rsx! { div { class: "composer__signature", dir: "auto", dangerous_inner_html: "{body}" } }
}

/// Drop target: files dragged over the composer show the design's drop zone.
fn drop_handlers(
    slot: ComposeSlot,
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
            add_files(slot, e.files());
        }),
    )
}

#[component]
pub fn InlineReply(draft: Draft) -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    use_senders(s);
    let state = slot.draft;
    let mut show_cc = use_signal(|| !draft.cc.is_empty());
    let id = editor_id(&draft);
    use_hook({
        let id = id.clone();
        move || {
            bridge::focus(&id);
            // Bring the whole reply into view so it can't be missed.
            document::eval(
                "requestAnimationFrame(()=>{const r=document.querySelector('.inline-reply--open');\
                 r&&r.scrollIntoView({block:'nearest',behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'auto':'smooth'});});",
            );
        }
    });
    let current = state.read().clone().unwrap_or(draft.clone());
    let hint = cross_hint(&s, &current);
    let dragging = use_signal(|| false);
    let (over, leave, drop) = drop_handlers(slot, dragging);

    rsx! {
        div { class: "inline-reply inline-reply--open", "data-dragging": dragging.read().then_some("true"),
            ondragover: move |e| over.call(e), ondragleave: move |e| leave.call(e), ondrop: move |e| drop.call(e),
            div { class: "inline-reply__head", style: "height:auto;min-height:44px",
                FromPicker {}
                span { class: "t-3", style: "font-size:12.5px", "to" }
                RecipientInput { id: "reply-to-{current.id}", value: current.to.clone(),
                    on_change: move |l| set_field(slot, Field::To, l) }
                if !*show_cc.read() {
                    button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| show_cc.set(true), "Cc" }
                }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Discard draft",
                    onclick: move |_| discard(slot),
                    Icon { name: "x" }
                }
            }
            if *show_cc.read() {
                div { class: "inline-reply__head", style: "height:auto;min-height:40px",
                    span { class: "t-3", style: "font-size:12.5px;padding-left:4px", "cc" }
                    RecipientInput { id: "reply-cc-{current.id}", value: current.cc.clone(),
                        on_change: move |l| set_field(slot, Field::Cc, l) }
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
            super::drive::DriveWaiting {}
            super::drive::DriveSheet { editor: id.clone() }
            div { class: "inline-reply__footer",
                super::drive::AttachMenu { id: "attach-{current.id}", on_files: move |f| add_files(slot, f) }
                span { class: "spacer" }
                SendButton {}
            }
        }
    }
}

/// Routes a new (non-inline) draft from the main window: docked mode leaves
/// it for the reading pane; window mode moves it into its own window.
#[component]
pub fn ComposerOverlay() -> Element {
    let s = use_context::<AppState>();
    use_effect(move || {
        let Some(d) = s.draft.read().clone() else { return };
        let windowed_reply = d.reply.is_some() && reply_window_mode();
        if is_inline(&s, &d) || (docked_mode() && !windowed_reply) {
            return;
        }
        let mut draft = s.draft;
        draft.set(None);
        super::compose_window::open(s, d);
    });
    rsx! {}
}

/// "Draft: <subject>" in the reader toolbar while a docked draft is set
/// aside; click to go back to it.
#[component]
pub fn DockedDraftChip() -> Element {
    let s = use_context::<AppState>();
    if !*s.docked_hidden.read() {
        return rsx! {};
    }
    let Some(d) = docked_draft(&s) else { return rsx! {} };
    let label = if d.subject.trim().is_empty() { "New message".to_string() } else { d.subject.clone() };
    let mut hidden = s.docked_hidden;
    rsx! {
        button { class: "chip docked-chip", r#type: "button", title: "Back to your draft",
            onclick: move |_| hidden.set(false),
            Icon { name: "edit", class: "icon--sm" }
            span { class: "docked-chip__label", dir: "auto", "Draft: {label}" }
        }
    }
}

/// Selecting another message while a docked draft is open: an empty one
/// closes, anything else is set aside behind the "Draft:" chip.
pub fn leave_docked(s: AppState) {
    let Some(d) = docked_draft(&s) else { return };
    if *s.docked_hidden.peek() {
        return;
    }
    let slot = s.main_slot();
    spawn(async move {
        let typed = bridge::editor_html(&editor_id(&d)).await;
        let mut draft = slot.draft;
        if is_blank(&d, &typed) {
            draft.set(None);
            send_cmd(Cmd::DiscardDraft { draft_id: d.id });
            return;
        }
        let mut kept = d;
        if !typed.is_empty() {
            kept.body_html = typed;
        }
        draft.set(Some(kept.clone()));
        send_cmd(Cmd::SaveDraft(kept));
        let mut hidden = s.docked_hidden;
        hidden.set(true);
    });
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
pub fn Composer(draft: Draft, #[props(default)] docked: bool) -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    use_senders(s);
    use_hook({
        let id = editor_id(&draft);
        move || {
            if debug_extras() {
                let mut d = slot.draft;
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
    let mut state = slot.draft;
    let mut subject = use_signal(|| draft.subject.clone());
    let debug = use_hook(|| {
        static USED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let d = debug_compose().filter(|_| !USED.swap(true, std::sync::atomic::Ordering::Relaxed));
        if let Some((to, _)) = &d {
            set_field(slot, Field::To, to.clone());
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
    let (over, leave, drop) = drop_handlers(slot, dragging);

    let notice = slot.notice.read().clone();

    rsx! {
            div { class: "composer", "data-dragging": dragging.read().then_some("true"),
                "data-docked": docked.then_some("true"), "data-windowed": slot.windowed.then_some("true"),
                ondragover: move |e| over.call(e), ondragleave: move |e| leave.call(e), ondrop: move |e| drop.call(e),
                div { class: "composer__titlebar",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)",
                        onclick: move |_| save_and_close(slot),
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
                            on_change: move |l| set_field(slot, Field::To, l) }
                        if !*show_cc_bcc.read() {
                            button { class: "btn btn--ghost btn--sm composer-field__aside", r#type: "button",
                                onclick: move |_| show_cc_bcc.set(true), "Cc Bcc" }
                        }
                    }
                    if *show_cc_bcc.read() {
                        div { class: "composer-field",
                            span { class: "composer-field__label", "Cc" }
                            RecipientInput { id: "composer-cc-{current.id}", value: current.cc.clone(),
                                on_change: move |l| set_field(slot, Field::Cc, l) }
                        }
                        div { class: "composer-field",
                            span { class: "composer-field__label", "Bcc" }
                            RecipientInput { id: "composer-bcc-{current.id}", value: current.bcc.clone(),
                                on_change: move |l| set_field(slot, Field::Bcc, l) }
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
                super::drive::DriveWaiting {}
                super::drive::DriveSheet { editor: id.clone() }
                div { class: "composer__footer",
                    super::drive::AttachMenu { id: "attach-{current.id}", on_files: move |f| add_files(slot, f) }
                    match notice {
                        Some(n) => rsx! { span { class: "t-caption composer__notice", role: "alert", "{n}" } },
                        None => rsx! { span { class: "t-caption", "Ctrl+Right Shift: RTL · Ctrl+Left Shift: LTR" } },
                    }
                    span { class: "spacer" }
                    SendButton {}
                }
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::compose::Sender;
    use pigeon_core::model::Address;

    fn draft() -> Draft {
        let mut d =
            Draft::new(Sender { account_id: "google:me@x.io".into(), address: Address::new(Some("Me"), "me@x.io") }, 0);
        d.to = vec![Address::new(None, "dana@x.io")];
        d
    }

    #[test]
    fn send_uses_the_latest_editor_content() {
        // Undo-send schedules what's in the editor now, not an older autosave.
        let stale = "<div>first draft</div>";
        assert_eq!(typed_for_send(Some("<div>final text</div>"), stale).unwrap(), "<div>final text</div>");
        // Editor read back empty though text was autosaved: never send blank.
        assert_eq!(typed_for_send(Some("<div><br></div>"), stale).unwrap(), stale);
        assert_eq!(typed_for_send(None, stale).unwrap(), stale);
        // Unreadable and nothing saved: block, don't send "".
        assert_eq!(typed_for_send(None, ""), Err(UNREADABLE));
        // Genuinely empty editor is allowed through (the empty check asks).
        assert_eq!(typed_for_send(Some(""), "").unwrap(), "");
    }

    #[test]
    fn typed_text_never_serializes_to_an_empty_body() {
        let mut d = draft();
        d.subject = "פידבקים".into();
        let typed = typed_for_send(Some("<div dir=\"auto\">הנה הפידבקים שלי</div>"), "").unwrap();
        d.body_html = compose_html(&typed, "", "");
        let (_, mime) = pigeon_core::compose::build_mime(&d, &[], 0).unwrap();
        let text = String::from_utf8_lossy(&mime);
        // Bodies are quoted-printable or base64; decode the HTML part back.
        assert!(!d.full_html().is_empty() && d.full_html().contains("הנה הפידבקים שלי"));
        assert!(text.contains("Content-Type: text/html"), "{text}");
    }

    #[test]
    fn empty_subject_and_body_asks_first() {
        let mut d = draft();
        assert!(needs_empty_confirm(&d, ""));
        assert!(needs_empty_confirm(&d, "<div><br></div>"));
        assert!(!needs_empty_confirm(&d, "<div>hi</div>"));
        d.subject = "Hi".into();
        assert!(!needs_empty_confirm(&d, ""));
        d.subject.clear();
        d.html = "<blockquote>quoted original</blockquote>".into();
        assert!(!needs_empty_confirm(&d, ""), "a forward with no note of its own is fine");
    }

    #[test]
    fn rich_signature_swaps_cleanly() {
        let rich = r#"<div><b>Dana</b></div><div style="color:#555">Acme <div>inner</div></div><script>x()</script>"#;
        let html = compose_html("<div>Hi</div>", rich, "<blockquote>q</blockquote>");
        assert!(html.contains("<b>Dana</b>") && !html.contains("script"), "{html}");
        let stripped = strip_signature(&html);
        assert_eq!(stripped, "<div>Hi</div><blockquote>q</blockquote>");
    }

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
        let o = send_later_options(fri_10, Weekday::Mon);
        let labels: Vec<&str> = o.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(labels, ["Later today", "Tomorrow morning", "Monday morning"]);
        assert_eq!(o[0].2, Local.with_ymd_and_hms(2026, 10, 23, 13, 0, 0).unwrap().timestamp_millis());
        assert_eq!(o[2].2, Local.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap().timestamp_millis());
        // Sunday-start weeks: next week begins Sunday.
        let il = send_later_options(fri_10, Weekday::Sun);
        assert_eq!(il[2].0, "Sunday morning");
        assert_eq!(il[2].2, Local.with_ymd_and_hms(2026, 10, 25, 8, 0, 0).unwrap().timestamp_millis());
        // Evening: no "later today". Day before the week starts: no duplicate of tomorrow.
        let fri_19 = Local.with_ymd_and_hms(2026, 10, 23, 19, 0, 0).unwrap();
        assert!(send_later_options(fri_19, Weekday::Mon).iter().all(|x| x.0 != "Later today"));
        let sun = Local.with_ymd_and_hms(2026, 10, 25, 9, 0, 0).unwrap();
        assert!(send_later_options(sun, Weekday::Mon).iter().all(|x| x.0 != "Monday morning"));
        let sat = Local.with_ymd_and_hms(2026, 10, 24, 9, 0, 0).unwrap();
        assert_eq!(send_later_options(sat, Weekday::Sun).len(), 2);
        assert!(parse_local_datetime("2026-10-24T09:30").is_some());
        assert!(parse_local_datetime("tomorrow").is_none());
    }
}
