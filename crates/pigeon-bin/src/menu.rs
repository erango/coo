//! The native menu bar. On macOS the Edit menu is what routes ⌘C/⌘V/⌘X/⌘A/⌘Z
//! to text fields in the web view, and the app menu carries ⌘Q: launching
//! with no menu (`with_menu(None)`) silently disabled all of them.
//!
//! Key ownership: ⌘, is on the menu item (opening Settings twice is harmless
//! if the web view also sees it). ⌘+ / ⌘- / ⌘0 stay with the JS bridge, which
//! also handles them inside editors; the View items have no accelerators so
//! nothing fires twice.

use dioxus::desktop::muda::accelerator::{Accelerator, Code, Modifiers};
use std::cell::RefCell;

use dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

pub const SETTINGS: &str = "pigeon-settings";
pub const AGENT_SETUP: &str = "pigeon-agent-setup";
pub const CHECK_UPDATES: &str = "pigeon-check-updates";
pub const ABOUT: &str = "pigeon-about";
pub const RESTART_TO_UPDATE: &str = "pigeon-restart-to-update";
pub const ZOOM_IN: &str = "pigeon-zoom-in";
pub const ZOOM_OUT: &str = "pigeon-zoom-out";
pub const ZOOM_RESET: &str = "pigeon-zoom-reset";
/// Message › Report Phishing (no shortcut: it can't be taken back).
pub const REPORT_PHISHING: &str = "pigeon-msg-phishing";

/// Mailbox and Message menu items: (menu id, title, bridge key it runs).
/// Titles show the shortcut like the View items do; the keys themselves stay
/// with the JS bridge (only it knows whether an editor has focus, so ⌘⌫ or
/// ⌘R in a text field keep their text-editing meaning).
const MESSAGE_ITEMS: &[(&str, &str, &str)] = &[
    ("pigeon-msg-new", "New Message          ⌘N", "meta+n"),
    ("pigeon-msg-sync", "Get New Mail         ⇧⌘N", "meta+shift+n"),
    ("pigeon-msg-search", "Search                ⌘F", "meta+f"),
    ("pigeon-msg-reply", "Reply                   ⌘R", "meta+r"),
    ("pigeon-msg-reply-all", "Reply All             ⇧⌘R", "meta+shift+r"),
    ("pigeon-msg-forward", "Forward              ⇧⌘F", "meta+shift+f"),
    ("pigeon-msg-archive", "Archive               ⌃⌘A", "ctrl+meta+a"),
    ("pigeon-msg-delete", "Delete                 ⌘⌫", "meta+Backspace"),
    ("pigeon-msg-flag", "Flag / Unflag        ⇧⌘L", "meta+shift+l"),
    ("pigeon-msg-unread", "Mark Read / Unread ⇧⌘U", "meta+shift+u"),
    ("pigeon-msg-move", "Move to…               V", "v"),
    ("pigeon-msg-spam", "Spam / Not Spam        !", "!"),
];

/// The bridge key a Message menu item runs.
pub fn key_for(id: &str) -> Option<&'static str> {
    MESSAGE_ITEMS.iter().find(|(i, _, _)| *i == id).map(|(_, _, k)| *k)
}

thread_local! {
    /// The Coo menu, and its "Restart to Update" item while one is shown.
    static APP_MENU: RefCell<Option<(Submenu, Option<MenuItem>)>> = const { RefCell::new(None) };
}

/// "Restart to Update to x.y.z" title.
pub fn restart_title(version: &str) -> String {
    format!("Restart to Update to {version}")
}

/// Show (Some) or remove (None) "Restart to Update to x.y.z" under About.
pub fn set_restart(version: Option<&str>) {
    APP_MENU.with(|m| {
        let mut m = m.borrow_mut();
        let Some((app, item)) = m.as_mut() else { return };
        match (version, item.as_ref()) {
            (Some(v), Some(existing)) => existing.set_text(restart_title(v)),
            (Some(v), None) => {
                let it = MenuItem::with_id(RESTART_TO_UPDATE, restart_title(v), true, None);
                if app.insert(&it, 1).is_ok() {
                    *item = Some(it);
                }
            }
            (None, Some(existing)) => {
                let _ = app.remove(existing);
                *item = None;
            }
            (None, None) => {}
        }
    });
}

pub fn build() -> Menu {
    let settings =
        MenuItem::with_id(SETTINGS, "Settings…", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)));
    let app = Submenu::with_items(
        "Coo",
        true,
        &[
            // Our own About (credits link + update status), not muda's.
            &MenuItem::with_id(ABOUT, "About Coo", true, None),
            &MenuItem::with_id(CHECK_UPDATES, "Check for Updates…", true, None),
            &PredefinedMenuItem::separator(),
            &settings,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )
    .expect("app menu");
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .expect("edit menu");
    let view = Submenu::with_items(
        "View",
        true,
        &[
            &MenuItem::with_id(ZOOM_IN, "Bigger Text    ⌘+", true, None),
            &MenuItem::with_id(ZOOM_OUT, "Smaller Text   ⌘−", true, None),
            &MenuItem::with_id(ZOOM_RESET, "Actual Size    ⌘0", true, None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::fullscreen(None),
        ],
    )
    .expect("view menu");
    let message_items: Vec<MenuItem> =
        MESSAGE_ITEMS.iter().map(|(id, title, _)| MenuItem::with_id(*id, *title, true, None)).collect();
    let message = Submenu::new("Message", true);
    for (i, item) in message_items.iter().enumerate() {
        // Separators after Search and after Forward group like Mail does.
        if i == 3 || i == 6 {
            message.append(&PredefinedMenuItem::separator()).expect("separator");
        }
        message.append(item).expect("message item");
    }
    message.append(&MenuItem::with_id(REPORT_PHISHING, "Report Phishing", true, None)).expect("phishing item");
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::close_window(None),
        ],
    )
    .expect("window menu");
    let help =
        Submenu::with_items("Help", true, &[&MenuItem::with_id(AGENT_SETUP, "Connect an AI Agent…", true, None)])
            .expect("help menu");

    let menu = Menu::new();
    menu.append_items(&[&app, &edit, &view, &message, &window, &help]).expect("menu bar");
    APP_MENU.with(|m| *m.borrow_mut() = Some((app.clone(), None)));
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_menu_runs_documented_shortcuts() {
        for (id, _, key) in MESSAGE_ITEMS {
            assert_eq!(key_for(id), Some(*key));
            assert!(crate::shortcuts::SHORTCUTS.iter().any(|s| s.1 == *key), "{key} isn't in Settings › Shortcuts");
        }
        assert_eq!(key_for("pigeon-settings"), None);
    }
}
