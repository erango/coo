//! A conversation in its own native window (double-click a list row, or
//! "Open in new window" in its right-click menu), filling the visible frame
//! of the screen the main window is on (not a macOS full-screen space).
//!
//! Like compose windows (see compose_window.rs), each is a separate
//! VirtualDom sharing the main window's `AppState`, running the bridge JS
//! itself (mail-frame sizing, links, keys). The engine keeps one reader
//! view, so opening a window also opens the conversation in the main
//! reader, and the window copies that view whenever the main reader shows
//! its conversation (so it keeps up with new messages). Replies are written
//! inline here too (Settings › Composing › Replies), in the window's own
//! compose slot: one started in the main window moves here when the window
//! opens, and goes back there when it closes.

use std::cell::RefCell;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{Config, DesktopContext, LogicalSize, WindowBuilder, WindowCloseBehaviour};
use dioxus::prelude::*;
use pigeon_core::compose::{Draft, ReplyKind};
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ListItem, ReaderView};

use super::compose::ComposeSlot;
use super::{Icon, act};
use crate::bridge::{self, Msg};
use crate::send_cmd;
use crate::shortcuts::{Do, tip};
use crate::state::AppState;

/// Screen rectangle in AppKit coordinates: (x, y, width, height), y up.
type Frame = (f64, f64, f64, f64);

struct Open {
    key: String,
    window: DesktopContext,
    /// The conversation shown (account id, thread id) and its inline reply.
    thread: (String, String),
    slot: ComposeSlot,
}

thread_local! {
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// What a window shows: a conversation, or one message (unthreaded list).
#[derive(Debug, Clone, PartialEq)]
pub enum Want {
    Thread { account_id: String, thread_id: String },
    Message { account_id: String, message_id: String },
}

impl Want {
    pub fn of(item: &ListItem) -> Want {
        match item {
            ListItem::Thread(t) => Want::Thread { account_id: t.account_id.clone(), thread_id: t.thread_id.clone() },
            ListItem::Message(m) => Want::Message { account_id: m.account_id.clone(), message_id: m.header.id.clone() },
        }
    }

    /// The engine's reader view is this window's conversation.
    pub fn shows(&self, v: &ReaderView) -> bool {
        match self {
            Want::Thread { account_id, thread_id } => {
                !v.single && v.account_id == *account_id && v.thread_id == *thread_id
            }
            Want::Message { account_id, message_id } => {
                v.single && v.account_id == *account_id && v.focus == *message_id
            }
        }
    }
}

/// Window title for a conversation.
pub fn title(subject: &str) -> String {
    if subject.trim().is_empty() { "(no subject)".to_string() } else { subject.trim().to_string() }
}

/// A new draft is the reply a message window asked for (`pending`: account
/// id, message id), so it opens in a compose window.
pub fn wants_window(pending: Option<&(String, String)>, d: &Draft) -> bool {
    match (pending, &d.reply) {
        (Some((account_id, message_id)), Some(r)) => r.account_id == *account_id && r.message_id == *message_id,
        _ => false,
    }
}

/// A reply that belongs in an open message window's inline reply (its
/// conversation is open there): handed to it. False: no such window.
pub fn take_reply(d: &Draft) -> bool {
    let Some(r) = &d.reply else { return false };
    let slot = OPEN
        .with(|o| o.borrow().iter().find(|w| w.thread.0 == r.account_id && w.thread.1 == r.thread_id).map(|w| w.slot));
    match slot {
        Some(slot) => {
            let mut draft = slot.draft;
            draft.set(Some(d.clone()));
            true
        }
        None => false,
    }
}

/// Start a reply from a message window.
pub fn reply_here(s: AppState, account_id: String, message_id: String, kind: ReplyKind) {
    let mut pending = s.reply_to_window;
    pending.set(Some((account_id.clone(), message_id.clone())));
    send_cmd(Cmd::StartReply { account_id, message_id, kind });
}

/// Switch a message window (its `Want`, from context) from one message to
/// its whole conversation.
pub fn show_whole(mut want: Signal<Want>, account_id: String, thread_id: String) {
    want.set(Want::Thread { account_id: account_id.clone(), thread_id: thread_id.clone() });
    send_cmd(Cmd::OpenThread { account_id, thread_id });
}

/// Open `item` in its own window, or bring its window forward.
pub fn open(s: AppState, item: ListItem) {
    let key = AppState::key(&item);
    let existing = OPEN.with(|o| o.borrow().iter().find(|w| w.key == key).map(|w| w.window.clone()));
    if let Some(w) = existing {
        w.window.set_visible(true);
        w.window.set_focus();
        return;
    }
    // The window shows the engine's reader view: open the item there too
    // (a double-click's first click already has).
    let want = Want::of(&item);
    let shown = s.reader.peek().as_ref().is_some_and(|v| want.shows(v));
    if !shown && s.selected.peek().as_deref() != Some(key.as_str()) {
        super::select(s, &item);
    }
    let main = dioxus::desktop::window();
    // A reply being written inline for this conversation moves to the
    // window, with what's in its editor right now.
    let thread = (item.account_id().to_string(), item.header().thread_id.clone());
    let inline = s.draft.peek().clone().filter(|d| {
        !super::compose::reply_window_mode()
            && d.reply.as_ref().is_some_and(|r| r.account_id == thread.0 && r.thread_id == thread.1)
    });
    match inline {
        Some(d) => {
            spawn(async move {
                let d = super::compose::with_editor_content(d).await;
                let mut draft = s.draft;
                draft.set(None);
                create(s, main, item, Some(d));
            });
        }
        None => create(s, main, item, None),
    }
}

fn create(s: AppState, main: DesktopContext, item: ListItem, reply: Option<Draft>) {
    let frame = visible_frame(&main);
    let (w, h) = frame.map(|f| (f.2, f.3)).unwrap_or((1100.0, 800.0));
    let mut window = WindowBuilder::new()
        .with_title(title(&item.header().subject))
        .with_inner_size(LogicalSize::new(w, h))
        .with_min_inner_size(LogicalSize::new(520.0, 420.0))
        // Shown once it's been placed (see `ReaderWindow`).
        .with_visible(false);
    #[cfg(target_os = "macos")]
    {
        use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
        window = window.with_titlebar_transparent(true).with_fullsize_content_view(true).with_title_hidden(true);
    }
    let cfg = Config::new()
        .with_window(window)
        .with_custom_head(crate::head_html())
        // Keep the app's menu bar (None leaves the existing NSApp menu alone).
        .with_menu(None)
        .with_close_behaviour(WindowCloseBehaviour::WindowCloses)
        .with_background_color((17, 18, 21, 255));
    let dom = VirtualDom::new_with_props(ReaderWindow, ReaderWindowProps { app: s, item, frame, reply });
    // Created on the event loop's next turn; nothing to wait for.
    let _ = main.new_window(dom, cfg);
}

/// Any message window open (keeps the Dock icon while the main window hides).
pub fn any_open() -> bool {
    OPEN.with(|o| !o.borrow().is_empty())
}

/// The main window is quitting: close every message window (they read its
/// state).
pub fn close_all() {
    let open = OPEN.with(|o| std::mem::take(&mut *o.borrow_mut()));
    for w in open {
        w.window.close();
    }
}

/// The window is closing with a reply open: it goes back to the main
/// window's inline reply (saved, unless empty).
fn hand_back(s: AppState, slot: ComposeSlot) {
    let Some(d) = slot.draft.peek().clone() else { return };
    let mut draft = slot.draft;
    draft.set(None);
    if super::compose::is_blank(&d, &d.body_html) {
        send_cmd(Cmd::DiscardDraft { draft_id: d.id });
        return;
    }
    // Back inline only where it shows: the main reader on its conversation.
    // Anywhere else it's saved to Drafts (else it would be lost).
    let here =
        s.reader.peek().as_ref().is_some_and(|v| {
            d.reply.as_ref().is_some_and(|r| r.account_id == v.account_id && r.thread_id == v.thread_id)
        });
    let mut main = s.draft;
    if here && main.peek().is_none() {
        if !super::compose::asks() {
            send_cmd(Cmd::SaveDraft(d.clone()));
        }
        main.set(Some(d));
    } else {
        send_cmd(Cmd::SaveDraft(d));
    }
}

/// This window's compose slot.
fn slot_of_window() -> Option<ComposeSlot> {
    let me = dioxus::desktop::window().id();
    OPEN.with(|o| o.borrow().iter().find(|w| w.window.id() == me).map(|w| w.slot))
}

fn close_me(s: AppState) {
    let me = dioxus::desktop::window();
    let slot = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let slot = o.iter().find(|w| w.window.id() == me.id()).map(|w| w.slot);
        o.retain(|w| w.window.id() != me.id());
        slot
    });
    if let Some(slot) = slot {
        hand_back(s, slot);
    }
    me.close();
}

/// The visible frame (no menu bar, no Dock) of the screen `w` is on.
#[cfg(target_os = "macos")]
fn visible_frame(w: &DesktopContext) -> Option<Frame> {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    // SAFETY: tao's NSWindow for a live window, used on the main thread.
    let ns = unsafe { (w.window.ns_window() as *const objc2_app_kit::NSWindow).as_ref() }?;
    let r = ns.screen()?.visibleFrame();
    Some((r.origin.x, r.origin.y, r.size.width, r.size.height))
}

#[cfg(not(target_os = "macos"))]
fn visible_frame(_: &DesktopContext) -> Option<Frame> {
    None
}

#[cfg(target_os = "macos")]
fn place(w: &DesktopContext, f: Frame) {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    // SAFETY: as in `visible_frame`.
    if let Some(ns) = unsafe { (w.window.ns_window() as *const objc2_app_kit::NSWindow).as_ref() } {
        ns.setFrame_display(NSRect::new(NSPoint::new(f.0, f.1), NSSize::new(f.2, f.3)), true);
    }
}

#[cfg(not(target_os = "macos"))]
fn place(w: &DesktopContext, _: Frame) {
    w.window.set_maximized(true);
}

#[derive(Props, Clone)]
pub struct ReaderWindowProps {
    app: AppState,
    item: ListItem,
    frame: Option<Frame>,
    /// An inline reply moved over from the main window.
    reply: Option<Draft>,
}

impl PartialEq for ReaderWindowProps {
    fn eq(&self, other: &Self) -> bool {
        AppState::key(&self.item) == AppState::key(&other.item)
    }
}

#[allow(non_snake_case)]
fn ReaderWindow(props: ReaderWindowProps) -> Element {
    crate::keymon::use_native_keys();
    super::zoom::use_live_zoom();
    let s = use_context_provider(|| props.app);
    let moved = props.reply.clone();
    let slot = use_context_provider(move || ComposeSlot {
        draft: Signal::new(moved),
        sending: Signal::new(false),
        notice: Signal::new(None),
        windowed: true,
    });
    let first = props.item.clone();
    let key = AppState::key(&first);
    let want = use_context_provider(|| Signal::new(Want::of(&first)));
    let mut view = use_signal(|| s.reader.peek().clone().filter(|v| want.peek().shows(v)));

    use_hook({
        let (key, item, frame) = (key.clone(), first.clone(), props.frame);
        move || {
            let window = dioxus::desktop::window();
            if let Some(f) = frame {
                place(&window, f);
            }
            window.set_visible(true);
            window.set_focus();
            let thread = (item.account_id().to_string(), item.header().thread_id.clone());
            OPEN.with(|o| o.borrow_mut().push(Open { key, window, thread, slot }));
            spawn(bridge_loop(s, item));
        }
    });
    use_drop(move || {
        let me = dioxus::desktop::window().id();
        OPEN.with(|o| o.borrow_mut().retain(|w| w.window.id() != me));
    });
    dioxus::desktop::use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, window_id, .. } = event
            && *window_id == dioxus::desktop::window().id()
        {
            let me = dioxus::desktop::window().id();
            OPEN.with(|o| o.borrow_mut().retain(|w| w.window.id() != me));
            hand_back(s, slot);
        }
    });

    // Keep up with the main reader while it shows this conversation.
    use_effect(move || {
        let current = s.reader.read();
        if let Some(v) = current.as_ref()
            && want.read().shows(v)
            && view.peek().as_ref() != Some(v)
        {
            view.set(Some(v.clone()));
        }
    });

    // Title: the conversation's subject (Window menu, Mission Control, ⌘`).
    let fallback = first.header().subject.clone();
    use_effect(move || {
        let subject = view.read().as_ref().and_then(|v| v.messages.last().map(|m| m.subject.clone()));
        dioxus::desktop::window().set_title(&title(subject.as_deref().unwrap_or(&fallback)));
    });

    // Light/dark and the color theme follow the main window.
    use_effect(move || {
        let theme = *s.theme.read();
        let palette = s.palette.read().clone();
        bridge::set_theme(theme.as_pref());
        bridge::set_palette(&palette);
    });

    // The row as the list has it now (flag state), else as opened.
    let item = s.visible_items().into_iter().find(|i| AppState::key(i) == key).unwrap_or(first);
    let flagged = item.header().is_flagged;
    let (i1, i2, i3, i4, i5, i6) = (item.clone(), item.clone(), item.clone(), item.clone(), item.clone(), item.clone());
    let current = view.read().clone();

    rsx! {
        div { class: "app reader-window",
            main { class: "pane pane--reader",
                div { class: "reader-toolbar",
                    button { class: "icon-btn", "aria-label": tip("Archive", Do::Archive), onclick: move |_| archive(s, &i1, MailAction::Archive), Icon { name: "archive", class: "icon--lg" } }
                    button { class: "icon-btn", "aria-label": tip("Delete", Do::Delete), onclick: move |_| archive(s, &i2, MailAction::Trash), Icon { name: "trash", class: "icon--lg" } }
                    button { class: "icon-btn", "aria-label": tip("Flag", Do::Flag),
                        onclick: move |_| act(s, &i3, if flagged { MailAction::Unflag } else { MailAction::Flag }),
                        Icon { name: "flag", class: "icon--lg" }
                    }
                    span { class: "spacer" }
                    button { class: "icon-btn", "aria-label": tip("Reply", Do::Reply), onclick: move |_| reply(s, &i4, ReplyKind::Reply), Icon { name: "reply", class: "icon--lg" } }
                    button { class: "icon-btn", "aria-label": tip("Reply all", Do::ReplyAll), onclick: move |_| reply(s, &i5, ReplyKind::ReplyAll), Icon { name: "replyall", class: "icon--lg" } }
                    button { class: "icon-btn", "aria-label": tip("Forward", Do::Forward), onclick: move |_| reply(s, &i6, ReplyKind::Forward), Icon { name: "forward", class: "icon--lg" } }
                }
                match current {
                    Some(v) => rsx! {
                        super::reader::Thread { key: "{v.account_id}|{v.thread_id}|{v.focus}", view: v.clone(), windowed: true }
                    },
                    None => rsx! { div { class: "reader", div { class: "plain-body t-3", "Loading…" } } },
                }
            }
        }
        super::sender_card::SenderCard {}
        div { style: "display:none", dangerous_inner_html: super::ICONS }
        div { style: "display:none", dangerous_inner_html: super::APP_ICONS }
    }
}

fn reply(s: AppState, item: &ListItem, kind: ReplyKind) {
    reply_here(s, item.account_id().to_string(), item.header().id.clone(), kind);
}

/// Archive or delete, then close the window: the conversation is gone.
fn archive(s: AppState, item: &ListItem, action: MailAction) {
    act(s, item, action);
    close_me(s);
}

/// This window's half of the JS bridge: message keys, links, mail-frame
/// sizing (the script itself), window drag, text size.
async fn bridge_loop(s: AppState, item: ListItem) {
    let js = format!("{}\n{}", bridge::JS, crate::shortcuts::bridge_js(*crate::shortcuts::STYLE.peek()));
    let mut eval = document::eval(&js);
    let key = AppState::key(&item);
    while let Ok(msg) = eval.recv::<Msg>().await {
        match msg.k.as_str() {
            "key" | "key-synth" => {
                let item = s.visible_items().into_iter().find(|i| AppState::key(i) == key).unwrap_or(item.clone());
                let Some(what) = crate::shortcuts::action(*crate::shortcuts::STYLE.peek(), &msg.v, false) else {
                    continue;
                };
                match what {
                    Do::Reply => reply(s, &item, ReplyKind::Reply),
                    Do::ReplyAll => reply(s, &item, ReplyKind::ReplyAll),
                    Do::Forward => reply(s, &item, ReplyKind::Forward),
                    Do::Archive => archive(s, &item, MailAction::Archive),
                    Do::Delete => archive(s, &item, MailAction::Trash),
                    Do::Spam => {
                        let spam = super::spam::for_items(&s, std::slice::from_ref(&item));
                        if let Some(a) = spam.into_iter().find(|a| *a != MailAction::Phishing) {
                            archive(s, &item, a);
                        }
                    }
                    Do::Flag => {
                        act(s, &item, if item.header().is_flagged { MailAction::Unflag } else { MailAction::Flag })
                    }
                    Do::MarkRead => act(s, &item, MailAction::MarkRead),
                    Do::MarkUnread => act(s, &item, MailAction::MarkUnread),
                    // Esc in a reply closes the reply (saved); otherwise the window.
                    Do::Close => match slot_of_window() {
                        Some(slot) if slot.draft.peek().is_some() => super::compose::save_and_close(slot),
                        _ => close_me(s),
                    },
                    Do::Compose => super::new_message(s),
                    Do::Bigger => super::zoom::change(s, super::zoom::Step::Bigger),
                    Do::Smaller => super::zoom::change(s, super::zoom::Step::Smaller),
                    Do::ActualSize => super::zoom::change(s, super::zoom::Step::Reset),
                    _ => {}
                }
            }
            // Drag by the toolbar, double-click it to zoom.
            "win" => super::window_action(&msg.v),
            "ctx-img" => crate::ctxmenu::set_image(&msg.v),
            "attach-drag" => crate::dragout::start(&msg.v),
            "link" => {
                let v = msg.v.to_ascii_lowercase();
                if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("mailto:") {
                    let _ = open::that_detached(&msg.v);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::compose::{ReplyContext, Sender};
    use pigeon_core::model::Address;

    fn view(thread_id: &str, focus: &str, single: bool) -> ReaderView {
        ReaderView {
            account_id: "a".into(),
            thread_id: thread_id.into(),
            messages: Vec::new(),
            focus: focus.into(),
            single,
        }
    }

    #[test]
    fn a_window_takes_only_its_own_conversation() {
        let t = Want::Thread { account_id: "a".into(), thread_id: "t1".into() };
        assert!(t.shows(&view("t1", "m9", false)));
        assert!(!t.shows(&view("t2", "m9", false)));
        assert!(!t.shows(&view("t1", "m9", true)), "a single message of it isn't the conversation");
        let m = Want::Message { account_id: "a".into(), message_id: "m9".into() };
        assert!(m.shows(&view("t1", "m9", true)));
        assert!(!m.shows(&view("t1", "m9", false)));
        assert!(!m.shows(&view("t1", "m8", true)));
        let other = Want::Thread { account_id: "b".into(), thread_id: "t1".into() };
        assert!(!other.shows(&view("t1", "m9", false)));
    }

    #[test]
    fn only_the_asked_for_reply_goes_to_a_window() {
        let from = Sender { account_id: "a".into(), address: Address::new(None, "me@x.io") };
        let mut d = Draft::new(from, 0);
        let pending = ("a".to_string(), "m9".to_string());
        assert!(!wants_window(Some(&pending), &d), "a new message isn't a reply");
        d.reply = Some(ReplyContext {
            kind: ReplyKind::Reply,
            account_id: "a".into(),
            message_id: "m9".into(),
            thread_id: "t1".into(),
            internet_message_id: None,
            references: Vec::new(),
        });
        assert!(wants_window(Some(&pending), &d));
        assert!(!wants_window(None, &d));
        assert!(!wants_window(Some(&("a".to_string(), "m8".to_string())), &d));
    }

    #[test]
    fn window_titles() {
        assert_eq!(title("  Q4 deck "), "Q4 deck");
        assert_eq!(title(""), "(no subject)");
    }
}
