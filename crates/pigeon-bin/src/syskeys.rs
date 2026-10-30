//! The system's own keyboard shortcuts (System Settings › Keyboard ›
//! Keyboard Shortcuts: Spotlight, screenshots, Mission Control…), so a
//! custom Coo shortcut doesn't take one. Read through Carbon's
//! `CopySymbolicHotKeys`, which lists the enabled ones with their virtual
//! key code and modifiers.

/// Whether a bridge key ("meta+shift+3") is one of the system's enabled
/// shortcuts. Keys without ⌘, ⌃ or ⌥ never are.
pub fn taken(key: &str) -> bool {
    let Some(combo) = combo(key) else { return false };
    if combo.1 & (CMD | CTRL | OPT) == 0 {
        return false;
    }
    mac::hotkeys().contains(&combo)
}

// Carbon modifier bits, as symbolic hotkeys store them.
const CMD: u32 = 0x100;
const SHIFT: u32 = 0x200;
const OPT: u32 = 0x800;
const CTRL: u32 = 0x1000;
const MODS: u32 = CMD | SHIFT | OPT | CTRL;

/// A bridge key as (ANSI virtual key code, Carbon modifiers); None for a
/// key that has no fixed code (a shifted symbol like "!").
fn combo(key: &str) -> Option<(u16, u32)> {
    let parts: Vec<&str> = key.split('+').collect();
    let (mods, base) = parts.split_at(parts.len().checked_sub(1)?);
    let mut m = 0;
    for p in mods {
        m |= match *p {
            "meta" => CMD,
            "shift" => SHIFT,
            "alt" => OPT,
            "ctrl" => CTRL,
            _ => return None,
        };
    }
    let mut base = base[0].to_string();
    // "J" is ⇧J.
    if base.len() == 1 && base.chars().all(|c| c.is_ascii_uppercase()) {
        m |= SHIFT;
        base = base.to_lowercase();
    }
    Some((code(&base)?, m))
}

fn code(base: &str) -> Option<u16> {
    const LETTERS: &[(char, u16)] = &[
        ('a', 0),
        ('s', 1),
        ('d', 2),
        ('f', 3),
        ('h', 4),
        ('g', 5),
        ('z', 6),
        ('x', 7),
        ('c', 8),
        ('v', 9),
        ('b', 11),
        ('q', 12),
        ('w', 13),
        ('e', 14),
        ('r', 15),
        ('y', 16),
        ('t', 17),
        ('1', 18),
        ('2', 19),
        ('3', 20),
        ('4', 21),
        ('6', 22),
        ('5', 23),
        ('=', 24),
        ('9', 25),
        ('7', 26),
        ('-', 27),
        ('8', 28),
        ('0', 29),
        (']', 30),
        ('o', 31),
        ('u', 32),
        ('[', 33),
        ('i', 34),
        ('p', 35),
        ('l', 37),
        ('j', 38),
        ('\'', 39),
        ('k', 40),
        (';', 41),
        ('\\', 42),
        (',', 43),
        ('/', 44),
        ('n', 45),
        ('m', 46),
        ('.', 47),
        (' ', 49),
        ('`', 50),
    ];
    let named = match base {
        "Enter" => Some(36),
        "Tab" => Some(48),
        "Backspace" => Some(51),
        "Escape" => Some(53),
        "ArrowLeft" => Some(123),
        "ArrowRight" => Some(124),
        "ArrowDown" => Some(125),
        "ArrowUp" => Some(126),
        _ => None,
    };
    let mut chars = base.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => LETTERS.iter().find(|(l, _)| *l == c).map(|(_, k)| *k),
        _ => named,
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn CopySymbolicHotKeys(out: *mut *const AnyObject) -> i32;
    }

    /// The enabled system shortcuts as (key code, modifiers).
    pub fn hotkeys() -> Vec<(u16, u32)> {
        let mut out = Vec::new();
        let mut array: *const AnyObject = std::ptr::null();
        // SAFETY: Carbon's documented call; on success it hands us a +1
        // CFArray of CFDictionaries (toll-free NSArray / NSDictionary), which
        // `Retained::from_raw` takes ownership of.
        let array: Option<Retained<NSArray<NSDictionary<NSString, AnyObject>>>> = unsafe {
            if CopySymbolicHotKeys(&mut array) != 0 || array.is_null() {
                return out;
            }
            Retained::from_raw(array as *mut NSArray<NSDictionary<NSString, AnyObject>>)
        };
        let Some(array) = array else { return out };
        let number = |d: &NSDictionary<NSString, AnyObject>, k: &str| -> Option<i64> {
            let v = d.objectForKey(&NSString::from_str(k))?;
            v.downcast_ref::<NSNumber>().map(|n| n.longLongValue())
        };
        for d in array.iter() {
            if number(&d, "kHISymbolicHotKeyEnabled") != Some(1) {
                continue;
            }
            let (Some(code), Some(mods)) =
                (number(&d, "kHISymbolicHotKeyCode"), number(&d, "kHISymbolicHotKeyModifiers"))
            else {
                continue;
            };
            out.push((code as u16, mods as u32 & super::MODS));
        }
        out
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn hotkeys() -> Vec<(u16, u32)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_keys_map_to_key_codes() {
        assert_eq!(combo("meta+shift+3"), Some((20, CMD | SHIFT)));
        assert_eq!(combo("ctrl+ArrowUp"), Some((126, CTRL)));
        assert_eq!(combo("J"), Some((38, SHIFT)));
        assert_eq!(combo("meta+ "), Some((49, CMD)));
        assert_eq!(combo("!"), None);
        assert!(!taken("r"), "plain keys are never system shortcuts");
    }
}
