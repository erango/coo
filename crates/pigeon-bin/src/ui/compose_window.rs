//! New-message composer in its own native window (Settings › Composing ›
//! "In a separate window", the default).
//!
//! Each window is a separate VirtualDom. It shares the main window's
//! `AppState` (signals are thread-local handles and every window runs on the
//! UI thread; a subscriber is woken through its own window's scheduler), and
//! owns its draft in a `ComposeSlot`. Commands go out through the global
//! `send_cmd`, so drafts save, send, schedule and undo exactly as before; the
//! undo-send toast and the outgoing row show in the main window.
//!
//! Lifetime: closing the window (Esc, its close button, Send) empties the
//! slot, and the root closes the window when that happens. Closing the main
//! window first saves every open draft and closes their windows, since their
//! `AppState` lives in the main window.

use std::cell::RefCell;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{Config, DesktopContext, LogicalSize, WindowBuilder, WindowCloseBehaviour};
use dioxus::prelude::*;
use pigeon_core::compose::Draft;

use super::compose::{self, ComposeSlot};
use crate::bridge::{self, Msg};
use crate::state::AppState;

struct Open {
    draft_id: String,
    window: DesktopContext,
    slot: ComposeSlot,
}

thread_local! {
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// Window title for a draft.
pub fn title(subject: &str) -> String {
    if subject.trim().is_empty() { "New message".to_string() } else { subject.trim().to_string() }
}

/// Open `draft` in its own window, or bring its window forward if it's
/// already open (e.g. the same draft reopened).
pub fn open(s: AppState, draft: Draft) {
    let existing = OPEN.with(|o| o.borrow().iter().find(|w| w.draft_id == draft.id).map(|w| w.window.clone()));
    if let Some(w) = existing {
        w.window.set_visible(true);
        w.window.set_focus();
        return;
    }
    let mut window = WindowBuilder::new()
        .with_title(title(&draft.subject))
        .with_inner_size(LogicalSize::new(760.0, 680.0))
        .with_min_inner_size(LogicalSize::new(520.0, 420.0));
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
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_background_color((17, 18, 21, 255));
    let dom = VirtualDom::new_with_props(ComposeWindow, ComposeWindowProps { app: s, draft });
    // Creating a window is deferred to the event loop (not from inside a
    // webview callback); nothing to wait for here.
    let _ = dioxus::desktop::window().new_window(dom, cfg);
}

/// Any compose window open (keeps the Dock icon while the main window hides).
pub fn any_open() -> bool {
    OPEN.with(|o| !o.borrow().is_empty())
}

/// The main window is closing: save every open draft and close its window
/// (they read the main window's state, which is about to go away).
pub fn close_all() {
    let open = OPEN.with(|o| std::mem::take(&mut *o.borrow_mut()));
    for w in open {
        compose::save_now(w.slot);
        w.window.set_close_behavior(WindowCloseBehaviour::WindowCloses);
        w.window.close();
    }
}

#[derive(Props, Clone)]
pub struct ComposeWindowProps {
    app: AppState,
    draft: Draft,
}

impl PartialEq for ComposeWindowProps {
    fn eq(&self, other: &Self) -> bool {
        self.draft.id == other.draft.id
    }
}

#[allow(non_snake_case)]
fn ComposeWindow(props: ComposeWindowProps) -> Element {
    let s = use_context_provider(|| props.app);
    let first = props.draft.clone();
    let slot = use_context_provider(move || ComposeSlot {
        draft: Signal::new(Some(first)),
        sending: Signal::new(false),
        notice: Signal::new(None),
        windowed: true,
    });

    use_hook(move || {
        let window = dioxus::desktop::window();
        let draft_id = slot.draft.peek().as_ref().map(|d| d.id.clone()).unwrap_or_default();
        OPEN.with(|o| o.borrow_mut().push(Open { draft_id, window, slot }));
        spawn(bridge_loop(s, slot));
    });
    use_drop(move || {
        let me = dioxus::desktop::window().id();
        OPEN.with(|o| o.borrow_mut().retain(|w| w.window.id() != me));
    });

    // The close button: save (or drop an empty draft) like Mail, then close.
    dioxus::desktop::use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            compose::save_and_close(slot);
        }
    });

    // Draft gone (sent, scheduled, saved, discarded): close the window.
    use_effect(move || {
        if slot.draft.read().is_none() {
            let me = dioxus::desktop::window();
            OPEN.with(|o| o.borrow_mut().retain(|w| w.window.id() != me.id()));
            me.set_close_behavior(WindowCloseBehaviour::WindowCloses);
            me.close();
        }
    });

    // Title follows the subject (Window menu, Mission Control, ⌘`).
    use_effect(move || {
        if let Some(d) = slot.draft.read().as_ref() {
            dioxus::desktop::window().set_title(&title(&d.subject));
        }
    });

    // Light/dark and the color theme follow the main window.
    use_effect(move || {
        let theme = *s.theme.read();
        let palette = s.palette.read().clone();
        bridge::set_theme(theme.as_pref());
        bridge::set_palette(&palette);
    });

    let Some(d) = slot.draft.read().clone() else { return rsx! {} };
    rsx! {
        compose::Composer { key: "{d.id}", draft: d }
        div { style: "display:none", dangerous_inner_html: super::ICONS }
        div { style: "display:none", dangerous_inner_html: super::APP_ICONS }
    }
}

/// This window's half of the JS bridge: composer keys, autosave, links.
async fn bridge_loop(s: AppState, slot: ComposeSlot) {
    let mut eval = document::eval(&bridge::script());
    while let Ok(msg) = eval.recv::<Msg>().await {
        match msg.k.as_str() {
            "key" => match msg.v.as_str() {
                "meta+Enter" => compose::send_current(s, slot),
                "Escape" => compose::save_and_close(slot),
                "meta+n" => super::new_message(s),
                "zoom-in" => super::zoom::change(s, super::zoom::Step::Bigger),
                "zoom-out" => super::zoom::change(s, super::zoom::Step::Smaller),
                "zoom-reset" => super::zoom::change(s, super::zoom::Step::Reset),
                _ => {}
            },
            "draft-input" => compose::autosave(slot, &msg.v),
            "ctx-img" => crate::ctxmenu::set_image(&msg.v),
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

    #[test]
    fn window_titles() {
        assert_eq!(title(""), "New message");
        assert_eq!(title("  Q4 deck "), "Q4 deck");
    }
}
