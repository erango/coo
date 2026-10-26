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
/// Message › Open in New Window (double-click; no key).
pub const OPEN_WINDOW: &str = "pigeon-msg-open-window";
/// Calendar › New Event (C, in the calendar).
pub const NEW_EVENT: &str = "pigeon-cal-new-event";

/// A View / Message / Calendar menu row: (menu id, title, shortcut shown,
/// bridge key it runs), or a separator (empty id). The keys themselves stay
/// with the JS bridge (only it knows whether an editor has focus, so ⌘⌫ or
/// ⌘R in a text field keep their text-editing meaning); the shortcut is
/// display text, right-aligned by `align_shortcuts`. Rows with no bridge
/// key are handled by id in ui/mod.rs `on_menu`.
type Row = (&'static str, &'static str, &'static str, &'static str);
const SEP: Row = ("", "", "", "");

const VIEW_ROWS: &[Row] = &[
    ("pigeon-view-mail", "Mail", "⌘1", "meta+1"),
    ("pigeon-view-calendar", "Calendar", "⌘2", "meta+2"),
    SEP,
    ("pigeon-view-palette", "Command Palette…", "⌘K", "meta+k"),
    SEP,
    (ZOOM_IN, "Bigger Text", "⌘+", ""),
    (ZOOM_OUT, "Smaller Text", "⌘−", ""),
    (ZOOM_RESET, "Actual Size", "⌘0", ""),
];

const MESSAGE_ROWS: &[Row] = &[
    ("pigeon-msg-new", "New Message", "⌘N", "meta+n"),
    ("pigeon-msg-sync", "Get New Mail", "⇧⌘N", "meta+shift+n"),
    ("pigeon-msg-search", "Search This Folder", "⌘F", "meta+f"),
    ("pigeon-msg-search-all", "Search All Mail", "⌥⌘F", "meta+alt+f"),
    SEP,
    ("pigeon-msg-next", "Next Conversation", "J", "j"),
    ("pigeon-msg-prev", "Previous Conversation", "K", "k"),
    (OPEN_WINDOW, "Open in New Window", "", ""),
    SEP,
    ("pigeon-msg-reply", "Reply", "⌘R", "meta+r"),
    ("pigeon-msg-reply-all", "Reply All", "⇧⌘R", "meta+shift+r"),
    ("pigeon-msg-forward", "Forward", "⇧⌘F", "meta+shift+f"),
    SEP,
    ("pigeon-msg-archive", "Archive", "⌃⌘A", "ctrl+meta+a"),
    ("pigeon-msg-delete", "Delete", "⌘⌫", "meta+Backspace"),
    ("pigeon-msg-flag", "Flag / Unflag", "⇧⌘L", "meta+shift+l"),
    ("pigeon-msg-unread", "Mark Read / Unread", "⇧⌘U", "meta+shift+u"),
    ("pigeon-msg-move", "Move to…", "V", "v"),
    ("pigeon-msg-label", "Label…", "L", "l"),
    SEP,
    ("pigeon-msg-spam", "Spam / Not Spam", "!", "!"),
    (REPORT_PHISHING, "Report Phishing", "", ""),
    SEP,
    ("pigeon-msg-undo", "Undo Last Action", "Z", "z"),
];

const CALENDAR_ROWS: &[Row] =
    &[(NEW_EVENT, "New Event", "C", ""), ("pigeon-cal-join", "Join Current Meeting", "⇧J", "J")];

/// A row's menu title: "Label\tShortcut" (aligned by `align_shortcuts`).
fn title(label: &str, shortcut: &str) -> String {
    if shortcut.is_empty() { label.to_string() } else { format!("{label}\t{shortcut}") }
}

fn submenu(name: &str, rows: &[Row]) -> Submenu {
    let menu = Submenu::new(name, true);
    for (id, label, shortcut, _) in rows {
        if id.is_empty() {
            menu.append(&PredefinedMenuItem::separator()).expect("separator");
        } else {
            menu.append(&MenuItem::with_id(*id, title(label, shortcut), true, None)).expect("menu item");
        }
    }
    menu
}

/// The bridge key a View / Message / Calendar item runs.
pub fn key_for(id: &str) -> Option<&'static str> {
    [VIEW_ROWS, MESSAGE_ROWS, CALENDAR_ROWS]
        .iter()
        .flat_map(|rows| rows.iter())
        .find(|(i, _, _, k)| !i.is_empty() && *i == id && !k.is_empty())
        .map(|(_, _, _, k)| *k)
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
    let view = submenu("View", VIEW_ROWS);
    let message = submenu("Message", MESSAGE_ROWS);
    let calendar = submenu("Calendar", CALENDAR_ROWS);
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            // Here rather than in View: its native shortcut column can't
            // line up with View's display shortcuts (`align_shortcuts`).
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::close_window(None),
        ],
    )
    .expect("window menu");
    let help =
        Submenu::with_items("Help", true, &[&MenuItem::with_id(AGENT_SETUP, "Connect an AI Agent…", true, None)])
            .expect("help menu");

    let menu = Menu::new();
    menu.append_items(&[&app, &edit, &view, &message, &calendar, &window, &help]).expect("menu bar");
    APP_MENU.with(|m| *m.borrow_mut() = Some((app.clone(), None)));
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
    }
    menu
}

/// Right-align the display shortcuts ("Label\tShortcut" titles) in every
/// menu of the menu bar: each such item gets an attributed title with one
/// right tab stop just past its menu's widest label + shortcut. Run once the
/// menu bar is installed; plain titles (no tab) are left alone.
#[cfg(target_os = "macos")]
pub fn align_shortcuts() {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker, msg_send};
    use objc2_app_kit::{
        NSApplication, NSFont, NSFontAttributeName, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
        NSTextAlignment, NSTextTab,
    };
    use objc2_foundation::{NSArray, NSAttributedString, NSDictionary, NSSize, NSString};

    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
    let font = NSFont::menuFontOfSize(0.0);
    // SAFETY: NSMenu / NSMenuItem / NSAttributedString methods with their
    // documented signatures, on the main thread.
    unsafe {
        let width = |text: &str| -> f64 {
            let attrs = NSDictionary::from_slices(&[NSFontAttributeName], &[&*font as &AnyObject]);
            let s = NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(text),
                Some(&attrs),
            );
            let size: NSSize = msg_send![&*s, size];
            size.width
        };
        let n: isize = msg_send![&*bar, numberOfItems];
        for i in 0..n {
            let top: *mut AnyObject = msg_send![&*bar, itemAtIndex: i];
            let sub: *mut AnyObject = msg_send![top, submenu];
            if sub.is_null() {
                continue;
            }
            let count: isize = msg_send![sub, numberOfItems];
            let items: Vec<(*mut AnyObject, String, String)> = (0..count)
                .filter_map(|j| {
                    let item: *mut AnyObject = msg_send![sub, itemAtIndex: j];
                    let t: Option<Retained<NSString>> = msg_send![item, title];
                    let t = t?.to_string();
                    let (label, key) = t.split_once('\t')?;
                    Some((item, label.to_string(), key.to_string()))
                })
                .collect();
            if items.is_empty() {
                continue;
            }
            let widest_label = items.iter().map(|(_, l, _)| width(l)).fold(0.0, f64::max);
            let widest_key = items.iter().map(|(_, _, k)| width(k)).fold(0.0, f64::max);
            let stop = (widest_label + 28.0 + widest_key).ceil();
            let tab = NSTextTab::initWithTextAlignment_location_options(
                NSTextTab::alloc(),
                NSTextAlignment::Right,
                stop,
                &NSDictionary::new(),
            );
            let para = NSMutableParagraphStyle::new();
            para.setTabStops(Some(&NSArray::from_retained_slice(&[tab])));
            let attrs = NSDictionary::from_slices(
                &[NSFontAttributeName, NSParagraphStyleAttributeName],
                &[&*font as &AnyObject, &*para as &AnyObject],
            );
            for (item, label, key) in items {
                let text = NSString::from_str(&format!("{label}\t{key}"));
                let titled =
                    NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &text, Some(&attrs));
                let _: () = msg_send![item, setAttributedTitle: &*titled];
            }
        }
    }
}

/// Debug builds: pop up a menu-bar menu by title (screenshots of menus).
#[cfg(target_os = "macos")]
pub fn debug_pop(name: &str) {
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSPoint, NSString};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
    // SAFETY: NSMenu methods with their documented signatures, main thread.
    unsafe {
        let item: *mut AnyObject = msg_send![&*bar, itemWithTitle: &*NSString::from_str(name)];
        if item.is_null() {
            return;
        }
        let sub: *mut AnyObject = msg_send![item, submenu];
        let none: *mut AnyObject = std::ptr::null_mut();
        let _: bool =
            msg_send![sub, popUpMenuPositioningItem: none, atLocation: NSPoint::new(300.0, 700.0), inView: none];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn align_shortcuts() {}

#[cfg(not(target_os = "macos"))]
pub fn debug_pop(_name: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_items_run_documented_shortcuts() {
        for (id, label, _, key) in VIEW_ROWS.iter().chain(MESSAGE_ROWS).chain(CALENDAR_ROWS) {
            if id.is_empty() || key.is_empty() {
                continue;
            }
            assert_eq!(key_for(id), Some(*key), "{label}");
            assert!(crate::shortcuts::SHORTCUTS.iter().any(|s| s.1 == *key), "{key} isn't in Settings › Shortcuts");
        }
        assert_eq!(key_for("pigeon-settings"), None);
        assert_eq!(key_for(ZOOM_IN), None, "zoom is handled by id");
    }

    #[test]
    fn every_shortcut_has_a_menu_item() {
        // Esc, Send and the zoom keys live elsewhere (the composer, the bridge).
        let elsewhere = ["Escape", "meta+Enter", "meta+,", "zoom-in", "zoom-out", "zoom-reset", "c"];
        for (_, key, _, what) in crate::shortcuts::SHORTCUTS {
            if elsewhere.contains(key) {
                continue;
            }
            assert!(key_for(&id_for(key)).is_some(), "{what} ({key}) has no menu item");
        }
    }

    fn id_for(key: &str) -> String {
        VIEW_ROWS
            .iter()
            .chain(MESSAGE_ROWS)
            .chain(CALENDAR_ROWS)
            .find(|r| r.3 == key)
            .map(|r| r.0.to_string())
            .unwrap_or_default()
    }
}
