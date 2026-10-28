//! Keyboard shortcuts: one table, in two styles (Settings › Keyboard
//! shortcuts). Gmail style is single keys (R, A, F, E…); Outlook style is
//! Outlook for Mac's ⌘ and ⌃ keys. The key handlers, the tooltips, the
//! menus and the Settings list all read from here, so they can't drift.

use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Gmail,
    Outlook,
}

impl Style {
    pub fn from_pref(v: &str) -> Self {
        if v == "outlook" { Style::Outlook } else { Style::Gmail }
    }

    pub fn pref(self) -> &'static str {
        match self {
            Style::Gmail => "gmail",
            Style::Outlook => "outlook",
        }
    }
}

/// The style in use; components that read it re-render when it changes.
pub static STYLE: GlobalSignal<Style> = Signal::global(|| Style::from_pref(&crate::prefs::get().shortcut_style));

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Mail,
    Calendar,
    Palette,
    Find,
    Search,
    SearchAll,
    Next,
    Prev,
    Settings,
    Close,
    Compose,
    NewEvent,
    Reply,
    ReplyAll,
    Forward,
    Send,
    SelectAll,
    Archive,
    Delete,
    Move,
    Label,
    Spam,
    Flag,
    MarkRead,
    MarkUnread,
    Undo,
    Sync,
    Join,
    Bigger,
    Smaller,
    ActualSize,
}

/// (action, Settings group, what it does, Gmail-style keys, Outlook-style
/// keys). Keys are the bridge's names; the first one is the one shown in
/// tooltips and menus.
type Row = (Do, &'static str, &'static str, &'static [&'static str], &'static [&'static str]);

const TABLE: &[Row] = &[
    (Do::Mail, "Get around", "Mail", &["meta+1"], &["meta+1"]),
    (Do::Calendar, "Get around", "Calendar", &["meta+2"], &["meta+2"]),
    (Do::Palette, "Get around", "Command palette", &["meta+k"], &["meta+k"]),
    (Do::Find, "Get around", "Find in the open conversation (else search this folder)", &["meta+f"], &["meta+f"]),
    (Do::Search, "Get around", "Search this folder", &["/"], &[]),
    (Do::SearchAll, "Get around", "Search all mail", &["meta+alt+f"], &["meta+alt+f"]),
    (Do::Next, "Get around", "Next conversation", &["j", "ArrowDown"], &["ArrowDown", "ctrl+]"]),
    (Do::Prev, "Get around", "Previous conversation", &["k", "ArrowUp"], &["ArrowUp", "ctrl+["]),
    (Do::Settings, "Get around", "Settings", &["meta+,"], &["meta+,"]),
    (Do::Close, "Get around", "Close (composer, search, settings)", &["Escape"], &["Escape"]),
    (Do::Compose, "Write", "New message", &["c", "meta+n"], &["meta+n"]),
    (Do::NewEvent, "Write", "New event (in Calendar)", &["c"], &["meta+n"]),
    (Do::Reply, "Write", "Reply", &["r"], &["meta+r"]),
    (Do::ReplyAll, "Write", "Reply all", &["a"], &["meta+shift+r"]),
    (Do::Forward, "Write", "Forward", &["f"], &["meta+j"]),
    (Do::Send, "Write", "Send", &["meta+Enter"], &["meta+Enter"]),
    (Do::SelectAll, "Organize", "Select every message in the folder (list focused)", &["meta+a"], &["meta+a"]),
    (Do::Archive, "Organize", "Archive", &["e"], &["ctrl+e"]),
    (Do::Delete, "Organize", "Delete", &["#"], &["Backspace", "Delete", "meta+Backspace"]),
    (Do::Move, "Organize", "Move to folder", &["v"], &["meta+shift+m"]),
    (Do::Label, "Organize", "Label (Gmail)", &["l"], &[]),
    (Do::Spam, "Organize", "Mark as spam / not spam", &["!"], &["meta+shift+j"]),
    (Do::Flag, "Organize", "Flag / unflag", &["s"], &["meta+shift+l"]),
    (Do::MarkRead, "Organize", "Mark as read", &["I"], &["meta+t"]),
    (Do::MarkUnread, "Organize", "Mark as unread", &["U"], &["meta+shift+t"]),
    (Do::Undo, "Organize", "Undo the last action", &["z", "meta+z"], &["meta+z"]),
    (Do::Sync, "App", "Get new mail (sync now)", &["meta+shift+n"], &["meta+shift+n"]),
    (Do::Join, "App", "Join the meeting that's on now", &["J"], &["J"]),
    (Do::Bigger, "App", "Bigger text", &["zoom-in"], &["zoom-in"]),
    (Do::Smaller, "App", "Smaller text", &["zoom-out"], &["zoom-out"]),
    (Do::ActualSize, "App", "Actual text size", &["zoom-reset"], &["zoom-reset"]),
];

/// Settings › Keyboard shortcuts sections, in order.
pub const GROUPS: [&str; 4] = ["Get around", "Write", "Organize", "App"];

/// Keys the bridge always forwards, outside the table (Enter presses the
/// update dialog's button).
const EXTRA_KEYS: &[&str] = &["Enter"];

fn keys_of(row: &Row, style: Style) -> &'static [&'static str] {
    match style {
        Style::Gmail => row.3,
        Style::Outlook => row.4,
    }
}

/// What `key` does in `style`. In Calendar, New event takes its key (C or
/// ⌘N) from New message.
pub fn action(style: Style, key: &str, calendar: bool) -> Option<Do> {
    let hit = |d: Do| TABLE.iter().any(|r| r.0 == d && keys_of(r, style).contains(&key));
    if calendar && hit(Do::NewEvent) {
        return Some(Do::NewEvent);
    }
    TABLE.iter().find(|r| r.0 != Do::NewEvent && keys_of(r, style).contains(&key)).map(|r| r.0)
}

/// Every key the bridge should forward in `style`.
pub fn bridge_keys(style: Style) -> Vec<&'static str> {
    let mut out: Vec<&str> = TABLE.iter().flat_map(|r| keys_of(r, style).iter().copied()).collect();
    out.extend(EXTRA_KEYS);
    out.retain(|k| !k.starts_with("zoom-"));
    out.sort_unstable();
    out.dedup();
    out
}

/// JS that hands the bridge the current style's keys.
pub fn bridge_js(style: Style) -> String {
    format!(
        "window.__pigeonKeys && window.__pigeonKeys({})",
        serde_json::to_string(&bridge_keys(style)).unwrap_or_default()
    )
}

/// A bridge key as printed on a keycap: "meta+shift+r" is ⇧⌘R, "J" is ⇧J.
pub fn display(key: &str) -> String {
    let special = |k: &str| -> Option<&'static str> {
        Some(match k {
            "zoom-in" => "⌘+",
            "zoom-out" => "⌘−",
            "zoom-reset" => "⌘0",
            "Escape" => "Esc",
            "Enter" => "↵",
            "Backspace" => "⌫",
            "Delete" => "⌦",
            "ArrowDown" => "↓",
            "ArrowUp" => "↑",
            _ => return None,
        })
    };
    if let Some(s) = special(key) {
        return s.to_string();
    }
    let parts: Vec<&str> = key.split('+').collect();
    // "meta+," and the like: the last piece may itself be a "+"-free symbol.
    let (mods, base) = parts.split_at(parts.len() - 1);
    let base = base[0];
    let mut out = String::new();
    for (m, sym) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("meta", "⌘")] {
        if mods.contains(&m) {
            out.push_str(sym);
        }
    }
    match special(base) {
        Some(s) => out.push_str(s),
        None if base.len() == 1 && base.chars().all(|c| c.is_ascii_uppercase()) => {
            out.push('⇧');
            out.push_str(base);
        }
        None => out.push_str(&base.to_uppercase()),
    }
    if out.contains("⇧⇧") {
        out = out.replace("⇧⇧", "⇧");
    }
    out
}

/// The keycap shown for `what` in `style` ("" when it has no key).
pub fn hint_in(style: Style, what: Do) -> String {
    TABLE.iter().find(|r| r.0 == what).and_then(|r| keys_of(r, style).first()).map(|k| display(k)).unwrap_or_default()
}

/// The keycap for `what` in the current style (subscribes the caller).
pub fn hint(what: Do) -> String {
    hint_in(*STYLE.read(), what)
}

/// "Reply (⌘R)": a button tooltip with its current key.
pub fn tip(label: &str, what: Do) -> String {
    match hint(what) {
        h if h.is_empty() => label.to_string(),
        h => format!("{label} ({h})"),
    }
}

/// Settings rows for `style`: (group, keys as shown, what it does).
pub fn rows(style: Style) -> Vec<(&'static str, String, &'static str)> {
    TABLE
        .iter()
        .filter(|r| !keys_of(r, style).is_empty())
        .map(|r| {
            let keys: Vec<String> = keys_of(r, style).iter().map(|k| display(k)).collect();
            (r.1, keys.join(" · "), r.2)
        })
        .collect()
}

/// Switch styles: saved, shown everywhere, and handed to every window's bridge.
pub fn set_style(style: Style) {
    crate::prefs::update(move |p| p.shortcut_style = style.pref().to_string());
    *STYLE.write() = style;
    document::eval(&bridge_js(style));
    crate::menu::show_shortcuts(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycaps() {
        assert_eq!(display("meta+shift+r"), "⇧⌘R");
        assert_eq!(display("r"), "R");
        assert_eq!(display("J"), "⇧J");
        assert_eq!(display("ctrl+e"), "⌃E");
        assert_eq!(display("meta+Backspace"), "⌘⌫");
        assert_eq!(display("meta+alt+f"), "⌥⌘F");
        assert_eq!(display("meta+,"), "⌘,");
        assert_eq!(display("ctrl+]"), "⌃]");
        assert_eq!(display("#"), "#");
        assert_eq!(display("meta+Enter"), "⌘↵");
    }

    #[test]
    fn each_style_has_its_own_keys() {
        assert_eq!(action(Style::Gmail, "r", false), Some(Do::Reply));
        assert_eq!(action(Style::Outlook, "r", false), None);
        assert_eq!(action(Style::Outlook, "meta+r", false), Some(Do::Reply));
        assert_eq!(action(Style::Gmail, "meta+r", false), None);
        assert_eq!(action(Style::Outlook, "meta+j", false), Some(Do::Forward));
        assert_eq!(action(Style::Outlook, "ctrl+e", false), Some(Do::Archive));
        assert_eq!(action(Style::Gmail, "e", false), Some(Do::Archive));
        assert_eq!(action(Style::Gmail, "c", false), Some(Do::Compose));
        assert_eq!(action(Style::Gmail, "c", true), Some(Do::NewEvent));
        assert_eq!(action(Style::Outlook, "meta+n", true), Some(Do::NewEvent));
        assert_eq!(action(Style::Outlook, "meta+n", false), Some(Do::Compose));
        assert_eq!(hint_in(Style::Outlook, Do::ReplyAll), "⇧⌘R");
        assert_eq!(hint_in(Style::Gmail, Do::ReplyAll), "A");
        assert_eq!(hint_in(Style::Outlook, Do::Label), "");
    }

    #[test]
    fn no_key_does_two_things() {
        for style in [Style::Gmail, Style::Outlook] {
            for k in bridge_keys(style) {
                let owners: Vec<_> =
                    TABLE.iter().filter(|r| r.0 != Do::NewEvent && keys_of(r, style).contains(&k)).collect();
                assert!(owners.len() <= 1, "{style:?}: {k} is bound twice");
            }
        }
    }
}
