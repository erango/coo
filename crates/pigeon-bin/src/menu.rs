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

use crate::shortcuts::{Do, Style};

pub const SETTINGS: &str = "pigeon-settings";
pub const AGENT_SETUP: &str = "pigeon-agent-setup";
pub const REPORT_BUG: &str = "pigeon-report-bug";
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

/// A View / Message / Calendar menu row: (menu id, title, the shortcut it
/// runs), or a separator (empty id). The keys themselves stay with the JS
/// bridge (only it knows whether an editor has focus, so ⌘⌫ or ⌘R in a text
/// field keep their text-editing meaning); the menu shows the current
/// style's key as display text, right-aligned by `align_shortcuts`. Rows
/// with no action are handled by id in ui/mod.rs `on_menu`.
type Row = (&'static str, &'static str, Option<Do>);
const SEP: Row = ("", "", None);

const VIEW_ROWS: &[Row] = &[
    ("pigeon-view-mail", "Mail", Some(Do::Mail)),
    ("pigeon-view-calendar", "Calendar", Some(Do::Calendar)),
    SEP,
    ("pigeon-view-palette", "Command Palette…", Some(Do::Palette)),
    SEP,
    (ZOOM_IN, "Bigger Text", Some(Do::Bigger)),
    (ZOOM_OUT, "Smaller Text", Some(Do::Smaller)),
    (ZOOM_RESET, "Actual Size", Some(Do::ActualSize)),
];

const MESSAGE_ROWS: &[Row] = &[
    ("pigeon-msg-new", "New Message", Some(Do::Compose)),
    ("pigeon-msg-sync", "Get New Mail", Some(Do::Sync)),
    ("pigeon-msg-find", "Find in Conversation", Some(Do::Find)),
    ("pigeon-msg-search", "Search This Folder", Some(Do::Search)),
    ("pigeon-msg-search-all", "Search All Mail", Some(Do::SearchAll)),
    SEP,
    ("pigeon-msg-next", "Next Conversation", Some(Do::Next)),
    ("pigeon-msg-prev", "Previous Conversation", Some(Do::Prev)),
    (OPEN_WINDOW, "Open in New Window", None),
    SEP,
    ("pigeon-msg-reply", "Reply", Some(Do::Reply)),
    ("pigeon-msg-reply-all", "Reply All", Some(Do::ReplyAll)),
    ("pigeon-msg-forward", "Forward", Some(Do::Forward)),
    SEP,
    ("pigeon-msg-archive", "Archive", Some(Do::Archive)),
    ("pigeon-msg-delete", "Delete", Some(Do::Delete)),
    ("pigeon-msg-flag", "Flag / Unflag", Some(Do::Flag)),
    ("pigeon-msg-read", "Mark as Read", Some(Do::MarkRead)),
    ("pigeon-msg-unread", "Mark as Unread", Some(Do::MarkUnread)),
    ("pigeon-msg-move", "Move to…", Some(Do::Move)),
    ("pigeon-msg-label", "Label…", Some(Do::Label)),
    SEP,
    ("pigeon-msg-spam", "Spam / Not Spam", Some(Do::Spam)),
    (REPORT_PHISHING, "Report Phishing", None),
    SEP,
    ("pigeon-msg-undo", "Undo Last Action", Some(Do::Undo)),
];

const CALENDAR_ROWS: &[Row] =
    &[(NEW_EVENT, "New Event", Some(Do::NewEvent)), ("pigeon-cal-join", "Join Current Meeting", Some(Do::Join))];

/// A row's menu title: "Label\tShortcut" (aligned by `align_shortcuts`).
fn title(label: &str, what: Option<Do>, style: Style) -> String {
    match what.map(|d| crate::shortcuts::hint_in(style, d)).filter(|k| !k.is_empty()) {
        Some(k) => format!("{label}\t{k}"),
        None => label.to_string(),
    }
}

fn submenu(name: &str, rows: &[Row], style: Style) -> Submenu {
    let menu = Submenu::new(name, true);
    for (id, label, what) in rows {
        if id.is_empty() {
            menu.append(&PredefinedMenuItem::separator()).expect("separator");
        } else {
            let item = MenuItem::with_id(*id, title(label, *what, style), true, None);
            menu.append(&item).expect("menu item");
            ITEMS.with(|i| i.borrow_mut().push((item, label, *what)));
        }
    }
    menu
}

/// The shortcut a View / Message / Calendar item runs.
pub fn action_for(id: &str) -> Option<Do> {
    [VIEW_ROWS, MESSAGE_ROWS, CALENDAR_ROWS]
        .iter()
        .flat_map(|rows| rows.iter())
        .find(|(i, _, _)| !i.is_empty() && *i == id)
        .and_then(|(_, _, d)| *d)
}

/// Show `style`'s keys in the menus (after a switch in Settings).
pub fn show_shortcuts(style: Style) {
    ITEMS.with(|items| {
        for (item, label, what) in items.borrow().iter() {
            item.set_text(title(label, *what, style));
        }
    });
    align_shortcuts();
}

thread_local! {
    /// The View / Message / Calendar items, to re-title on a style switch.
    static ITEMS: RefCell<Vec<(MenuItem, &'static str, Option<Do>)>> = const { RefCell::new(Vec::new()) };
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
    let style = Style::from_pref(&crate::prefs::get().shortcut_style);
    let view = submenu("View", VIEW_ROWS, style);
    let message = submenu("Message", MESSAGE_ROWS, style);
    let calendar = submenu("Calendar", CALENDAR_ROWS, style);
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
    let help = Submenu::with_items(
        "Help",
        true,
        &[
            &MenuItem::with_id(REPORT_BUG, "Report a Bug…", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(AGENT_SETUP, "Connect an AI Agent…", true, None),
        ],
    )
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
    fn menu_items_show_their_key() {
        assert_eq!(action_for("pigeon-msg-reply"), Some(Do::Reply));
        assert_eq!(action_for("pigeon-settings"), None);
        assert_eq!(title("Reply", Some(Do::Reply), Style::Outlook), "Reply\t⌘R");
        assert_eq!(title("Reply", Some(Do::Reply), Style::Gmail), "Reply\tR");
        assert_eq!(title("Label…", Some(Do::Label), Style::Outlook), "Label…");
    }
}
