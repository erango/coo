//! Keyboard shortcuts shown in Settings. A test checks this table against
//! the key set the bridge actually forwards, so the list can't drift.

/// (group, bridge key, label shown, what it does)
pub const SHORTCUTS: &[(&str, &str, &str, &str)] = &[
    ("Get around", "meta+1", "⌘1", "Mail"),
    ("Get around", "meta+2", "⌘2", "Calendar"),
    ("Get around", "meta+k", "⌘K", "Command palette"),
    ("Get around", "meta+f", "⌘F · /", "Search this folder"),
    ("Get around", "meta+alt+f", "⌥⌘F", "Search all mail"),
    ("Get around", "j", "J / ↓", "Next conversation"),
    ("Get around", "k", "K / ↑", "Previous conversation"),
    ("Get around", "meta+,", "⌘,", "Settings"),
    ("Get around", "Escape", "Esc", "Close (composer, search, settings)"),
    ("Write", "meta+n", "⌘N · C", "New message"),
    ("Write", "c", "C", "New event (in Calendar)"),
    ("Write", "meta+r", "⌘R · R", "Reply"),
    ("Write", "meta+shift+r", "⇧⌘R · A", "Reply all"),
    ("Write", "meta+shift+f", "⇧⌘F · F", "Forward"),
    ("Write", "meta+Enter", "⌘↵", "Send"),
    ("Organize", "meta+a", "⌘A", "Select every message in the folder (list focused)"),
    ("Organize", "ctrl+meta+a", "⌃⌘A · E", "Archive"),
    ("Organize", "meta+Backspace", "⌘⌫ · # · X", "Delete"),
    ("Organize", "v", "V", "Move to folder"),
    ("Organize", "l", "L", "Label (Gmail)"),
    ("Organize", "!", "!", "Mark as spam / not spam"),
    ("Organize", "meta+shift+l", "⇧⌘L · S", "Flag / unflag"),
    ("Organize", "meta+shift+u", "⇧⌘U · U", "Mark read / unread"),
    ("Organize", "z", "Z", "Undo the last action"),
    ("App", "meta+shift+n", "⇧⌘N", "Get new mail (sync now)"),
    ("App", "J", "⇧J", "Join the meeting that's on now"),
    ("App", "zoom-in", "⌘+", "Bigger text"),
    ("App", "zoom-out", "⌘−", "Smaller text"),
    ("App", "zoom-reset", "⌘0", "Actual text size"),
];

/// Settings › Keyboard shortcuts sections, in order.
pub const GROUPS: [&str; 4] = ["Get around", "Write", "Organize", "App"];

/// Keys the bridge forwards that are aliases of a documented one (shown in
/// its label, like the single-key extras next to the Mail-standard ⌘ keys),
/// or that nothing handles yet.
#[cfg(test)]
const NOT_LISTED: &[&str] =
    &["x", "#", "e", "r", "a", "f", "s", "u", "/", "ArrowDown", "ArrowUp", "meta+z", "Enter", "meta+shift+z"];

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge_keys() -> Vec<String> {
        let js = crate::bridge::JS;
        let start = js.find("const KEYS = new Set([").expect("KEYS in bridge") + "const KEYS = new Set([".len();
        let end = start + js[start..].find("])").expect("end of KEYS");
        // Every other piece between single quotes is a key ('meta+,' contains a comma).
        js[start..end].split('\'').skip(1).step_by(2).map(String::from).collect()
    }

    #[test]
    fn every_forwarded_key_is_documented() {
        let keys = bridge_keys();
        assert!(keys.len() > 20, "parsed {keys:?}");
        for k in &keys {
            assert!(
                SHORTCUTS.iter().any(|s| s.1 == k) || NOT_LISTED.contains(&k.as_str()),
                "bridge key {k:?} has no entry in SHORTCUTS"
            );
        }
        assert!(SHORTCUTS.iter().all(|s| GROUPS.contains(&s.0)), "every shortcut is in a shown section");
        for (_, k, _, _) in SHORTCUTS {
            assert!(keys.iter().any(|b| b == k) || k.starts_with("zoom-"), "{k:?} isn't forwarded by the bridge");
        }
    }
}
