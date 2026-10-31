//! Keyboard shortcuts: one table, in two styles (Settings › Keyboard
//! shortcuts). Gmail style is single keys (R, A, F, E…); Outlook style is
//! Outlook for Mac's ⌘ and ⌃ keys. The key handlers, the tooltips, the
//! menus and the Settings list all read from here, so they can't drift.
//! An action with no key in a style can get one of the user's own
//! (`custom_shortcuts` in prefs, per style), checked against every other
//! key Coo uses, macOS's standard ones and the system's own shortcuts.

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
    Reuse,
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
    Phishing,
    SaveEml,
    OpenWindow,
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
    (Do::OpenWindow, "Get around", "Open the message in a new window", &[], &[]),
    (Do::Close, "Get around", "Close (composer, search, settings)", &["Escape"], &["Escape"]),
    (Do::Compose, "Write", "New message", &["c", "meta+n"], &["meta+n"]),
    (Do::NewEvent, "Write", "New event (in Calendar)", &["c"], &["meta+n"]),
    (Do::Reply, "Write", "Reply", &["r"], &["meta+r"]),
    (Do::ReplyAll, "Write", "Reply all", &["a"], &["meta+shift+r"]),
    (Do::Forward, "Write", "Forward", &["f"], &["meta+j"]),
    (Do::Reuse, "Write", "Reuse message", &[], &[]),
    (Do::Send, "Write", "Send", &["meta+Enter"], &["meta+Enter"]),
    (Do::SelectAll, "Organize", "Select every message in the folder (list focused)", &["meta+a"], &["meta+a"]),
    (Do::Archive, "Organize", "Archive", &["e"], &["meta+e"]),
    (Do::Delete, "Organize", "Delete", &["#"], &["Backspace", "Delete", "meta+Backspace"]),
    (Do::Move, "Organize", "Move to folder", &["v"], &["meta+shift+m"]),
    (Do::Label, "Organize", "Label (Gmail)", &["l"], &[]),
    (Do::Spam, "Organize", "Mark as spam / not spam", &["!"], &["meta+shift+j"]),
    (Do::Flag, "Organize", "Flag / unflag", &["s"], &["meta+shift+l"]),
    (Do::MarkRead, "Organize", "Mark as read", &["I"], &["meta+t"]),
    (Do::MarkUnread, "Organize", "Mark as unread", &["U"], &["meta+shift+t"]),
    (Do::Phishing, "Organize", "Report phishing", &[], &[]),
    (Do::SaveEml, "Organize", "Save as .eml…", &[], &[]),
    (Do::Undo, "Organize", "Undo the last action (also after its notice is gone)", &["meta+z"], &["meta+z"]),
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

type Customs = std::collections::BTreeMap<String, String>;

/// The user's own keys, as `style:Action` → key (prefs `custom_shortcuts`).
/// Plain state (key handling runs outside components); components that
/// show keys read `CUSTOM_REV` to re-render on a change.
static CUSTOM: std::sync::LazyLock<std::sync::RwLock<Customs>> = std::sync::LazyLock::new(|| {
    std::sync::RwLock::new(if cfg!(test) { Customs::new() } else { crate::prefs::get().custom_shortcuts })
});
pub static CUSTOM_REV: GlobalSignal<u64> = Signal::global(|| 0);

fn customs() -> Customs {
    CUSTOM.read().map(|c| c.clone()).unwrap_or_default()
}

fn custom_id(style: Style, what: Do) -> String {
    format!("{}:{what:?}", style.pref())
}

/// The user's key for `what` in `style`, if it has one.
fn custom_key(customs: &Customs, style: Style, what: Do) -> Option<String> {
    customs.get(&custom_id(style, what)).cloned()
}

/// Whether `what` has no built-in key in `style` (so it can take the user's).
#[cfg(test)]
fn customizable(style: Style, what: Do) -> bool {
    TABLE.iter().any(|r| r.0 == what && keys_of(r, style).is_empty())
}

/// `what`'s keys in `style`: its own, else the user's.
fn keys(customs: &Customs, style: Style, row: &Row) -> Vec<String> {
    match keys_of(row, style) {
        [] => custom_key(customs, style, row.0).into_iter().collect(),
        ks => ks.iter().map(|k| k.to_string()).collect(),
    }
}

/// What `key` does in `style`. In Calendar, New event takes its key (C or
/// ⌘N) from New message.
pub fn action(style: Style, key: &str, calendar: bool) -> Option<Do> {
    action_in(&customs(), style, key, calendar)
}

fn action_in(customs: &Customs, style: Style, key: &str, calendar: bool) -> Option<Do> {
    let has = |r: &Row| keys(customs, style, r).iter().any(|k| k == key);
    if calendar && TABLE.iter().any(|r| r.0 == Do::NewEvent && has(r)) {
        return Some(Do::NewEvent);
    }
    TABLE.iter().find(|r| r.0 != Do::NewEvent && has(r)).map(|r| r.0)
}

/// Every key the bridge should forward in `style`.
pub fn bridge_keys(style: Style) -> Vec<String> {
    bridge_keys_in(&customs(), style)
}

fn bridge_keys_in(customs: &Customs, style: Style) -> Vec<String> {
    let mut out: Vec<String> = TABLE.iter().flat_map(|r| keys(customs, style, r)).collect();
    out.extend(EXTRA_KEYS.iter().map(|k| k.to_string()));
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
    let customs = customs();
    TABLE
        .iter()
        .find(|r| r.0 == what)
        .and_then(|r| keys(&customs, style, r).into_iter().next())
        .map(|k| display(&k))
        .unwrap_or_default()
}

/// The keycap for `what` in the current style (subscribes the caller).
pub fn hint(what: Do) -> String {
    let _ = CUSTOM_REV.read();
    hint_in(*STYLE.read(), what)
}

/// "Reply (⌘R)": a button tooltip with its current key.
pub fn tip(label: &str, what: Do) -> String {
    match hint(what) {
        h if h.is_empty() => label.to_string(),
        h => format!("{label} ({h})"),
    }
}

/// One Settings row: its group, keys as shown ("" for none yet), what it
/// does, and whether the key is the user's own to set.
pub struct ShortcutRow {
    pub group: &'static str,
    pub keys: String,
    pub what: &'static str,
    pub action: Do,
    pub custom: bool,
}

/// Settings rows for `style`: every action with a key, and every one that
/// can take the user's.
pub fn rows(style: Style) -> Vec<ShortcutRow> {
    let _ = CUSTOM_REV.read();
    let customs = customs();
    TABLE
        .iter()
        .filter(|r| r.0 != Do::NewEvent || !keys_of(r, style).is_empty())
        .map(|r| {
            let keys: Vec<String> = keys(&customs, style, r).iter().map(|k| display(k)).collect();
            ShortcutRow {
                group: r.1,
                keys: keys.join(" · "),
                what: r.2,
                action: r.0,
                custom: keys_of(r, style).is_empty(),
            }
        })
        .collect()
}

/// Keys macOS and every Mac app reserve (the Coo and Edit menus, window
/// keys, text size), with what they do.
const RESERVED: &[(&str, &str)] = &[
    ("meta+q", "Quit"),
    ("meta+w", "Close Window"),
    ("meta+h", "Hide Coo"),
    ("meta+alt+h", "Hide Others"),
    ("meta+m", "Minimize"),
    ("meta+c", "Copy"),
    ("meta+v", "Paste"),
    ("meta+x", "Cut"),
    ("meta+shift+z", "Redo"),
    ("meta+`", "Next window"),
    ("ctrl+meta+f", "Full Screen"),
    ("meta+=", "Bigger text"),
    ("meta+-", "Smaller text"),
    ("meta+0", "Actual size"),
    ("Escape", "Close"),
    ("Enter", "Open"),
    ("Tab", "Move focus"),
    (" ", "Scroll"),
];

/// Why `key` can't be `what`'s shortcut in `style`, if it can't: another
/// Coo action has it, macOS reserves it, or a system shortcut uses it.
pub fn conflict(style: Style, what: Do, key: &str) -> Option<String> {
    conflict_in(&customs(), style, what, key, &crate::syskeys::taken)
}

fn conflict_in(customs: &Customs, style: Style, what: Do, key: &str, system: &dyn Fn(&str) -> bool) -> Option<String> {
    if key == "alt" || key.starts_with("alt+") {
        return Some("⌥ shortcuts need ⌘ too (⌥⌘…)".into());
    }
    if let Some((_, does)) = RESERVED.iter().find(|(k, _)| *k == key) {
        return Some(format!("{} is {does} in every Mac app", display(key)));
    }
    let owner = TABLE.iter().find(|r| r.0 != what && keys(customs, style, r).iter().any(|k| k == key));
    if let Some(r) = owner {
        return Some(format!("{} is already {}", display(key), r.2));
    }
    // ⌘A selects all, the zoom keys are handled before the table.
    if key == "meta+a" {
        return Some(format!("{} is Select All", display(key)));
    }
    if system(key) {
        return Some(format!("{} is a macOS shortcut (System Settings › Keyboard › Keyboard Shortcuts)", display(key)));
    }
    None
}

/// Give `what` the user's `key` in `style` (None removes it).
pub fn set_custom(style: Style, what: Do, key: Option<String>) {
    let id = custom_id(style, what);
    if let Ok(mut c) = CUSTOM.write() {
        match key {
            Some(k) => c.insert(id, k),
            None => c.remove(&id),
        };
    }
    *CUSTOM_REV.write() += 1;
    let saved = customs();
    crate::prefs::update(move |p| p.custom_shortcuts = saved);
    document::eval(&bridge_js(style));
    crate::menu::show_shortcuts(style);
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
        assert_eq!(action(Style::Outlook, "meta+e", false), Some(Do::Archive));
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
    fn custom_keys_fill_only_empty_actions_and_never_collide() {
        let mut c = std::collections::BTreeMap::new();
        c.insert(custom_id(Style::Outlook, Do::Reuse), "meta+shift+u".to_string());
        assert_eq!(action_in(&c, Style::Outlook, "meta+shift+u", false), Some(Do::Reuse));
        assert_eq!(action_in(&c, Style::Gmail, "meta+shift+u", false), None, "per style");
        assert!(bridge_keys_in(&c, Style::Outlook).contains(&"meta+shift+u".to_string()));
        assert!(customizable(Style::Outlook, Do::Label) && !customizable(Style::Gmail, Do::Label));
        let none = |_: &str| false;
        assert!(conflict_in(&c, Style::Outlook, Do::SaveEml, "meta+r", &none).unwrap().contains("Reply"));
        assert!(conflict_in(&c, Style::Outlook, Do::SaveEml, "meta+shift+u", &none).unwrap().contains("Reuse"));
        assert_eq!(conflict_in(&c, Style::Outlook, Do::Reuse, "meta+shift+u", &none), None, "its own key");
        assert!(conflict_in(&c, Style::Gmail, Do::Reuse, "meta+q", &none).unwrap().contains("Quit"));
        assert!(conflict_in(&c, Style::Gmail, Do::Reuse, "alt+r", &none).is_some());
        assert!(
            conflict_in(&c, Style::Gmail, Do::Reuse, "meta+shift+3", &|k| k == "meta+shift+3")
                .unwrap()
                .contains("macOS")
        );
        assert!(conflict_in(&c, Style::Gmail, Do::Reuse, "U", &none).unwrap().contains("Mark as unread"));
        assert_eq!(conflict_in(&c, Style::Gmail, Do::Reuse, "u", &none), None);
    }

    #[test]
    fn no_key_does_two_things() {
        for style in [Style::Gmail, Style::Outlook] {
            for k in bridge_keys_in(&Default::default(), style) {
                let owners: Vec<_> =
                    TABLE.iter().filter(|r| r.0 != Do::NewEvent && keys_of(r, style).contains(&k.as_str())).collect();
                assert!(owners.len() <= 1, "{style:?}: {k} is bound twice");
            }
        }
    }
}
