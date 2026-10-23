//! Keyboard shortcuts shown in Settings. A test checks this table against
//! the key set the bridge actually forwards, so the list can't drift.

/// (group, bridge key, label shown, what it does)
pub const SHORTCUTS: &[(&str, &str, &str, &str)] = &[
    ("Anywhere", "meta+k", "⌘K", "Command palette"),
    ("Anywhere", "meta+1", "⌘1", "Mail"),
    ("Anywhere", "meta+2", "⌘2", "Calendar"),
    ("Anywhere", "meta+n", "⌘N", "New message"),
    ("Anywhere", "meta+r", "⌘R", "Sync now"),
    ("Anywhere", "meta+,", "⌘,", "Settings"),
    ("Anywhere", "J", "⇧J", "Join the meeting that's on now"),
    ("Anywhere", "zoom-in", "⌘+", "Bigger text"),
    ("Anywhere", "zoom-out", "⌘−", "Smaller text"),
    ("Anywhere", "zoom-reset", "⌘0", "Actual text size"),
    ("Anywhere", "Escape", "Esc", "Close (composer, search, settings)"),
    ("Anywhere", "z", "Z", "Undo the last action"),
    ("Mail", "j", "J / ↓", "Next conversation"),
    ("Mail", "k", "K / ↑", "Previous conversation"),
    ("Mail", "e", "E", "Archive"),
    ("Mail", "#", "# / X", "Delete"),
    ("Mail", "s", "S", "Flag / unflag"),
    ("Mail", "u", "U", "Mark read / unread"),
    ("Mail", "r", "R", "Reply"),
    ("Mail", "a", "A", "Reply all"),
    ("Mail", "f", "F", "Forward"),
    ("Mail", "v", "V", "Move to folder"),
    ("Mail", "l", "L", "Label (Gmail)"),
    ("Mail", "/", "/", "Search"),
    ("Mail", "c", "C", "New message (new event in Calendar)"),
    ("Composer", "meta+Enter", "⌘↵", "Send"),
];

/// Keys the bridge forwards that are aliases of a documented one, or that
/// nothing handles yet.
#[cfg(test)]
const NOT_LISTED: &[&str] = &["x", "ArrowDown", "ArrowUp", "meta+z", "Enter", "meta+shift+z"];

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
        for (_, k, _, _) in SHORTCUTS {
            assert!(keys.iter().any(|b| b == k) || k.starts_with("zoom-"), "{k:?} isn't forwarded by the bridge");
        }
    }
}
