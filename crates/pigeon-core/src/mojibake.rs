//! Repair UTF-8 text that was decoded in a legacy charset somewhere along
//! the way ("׳©׳œ׳•׳" for "שלום", "Ã©" for "é"): old Outlook versions,
//! mislabeled parts and forwards of already-broken mail all produce it.
//!
//! Best guess, and only when the text clearly carries the pattern: each run
//! of non-ASCII characters is mapped back to bytes through the suspect
//! charset and kept as UTF-8 only if those bytes are valid UTF-8. Real
//! multibyte UTF-8 never contains ASCII bytes, so runs never span words of
//! plain text. One exception: a non-breaking space (byte 0xA0, the second
//! byte of "נ" and others) usually comes back as a plain space or `&nbsp;`,
//! so it's taken as 0xA0 when a run ends mid-character.

use std::collections::HashMap;
use std::sync::OnceLock;

use encoding_rs::{Encoding, WINDOWS_1252, WINDOWS_1255};

/// Lead/continuation pairs needed before anything is touched.
const MIN_SIGNS: usize = 3;
/// Repaired runs needed to accept the result.
const MIN_FIXED: usize = 3;

/// Character → byte for the upper half of a single-byte charset.
fn table(enc: &'static Encoding) -> HashMap<char, u8> {
    let mut map = HashMap::new();
    let mut add = |enc: &'static Encoding, b: u8| {
        let byte = [b];
        let (s, _) = enc.decode_without_bom_handling(&byte);
        if let Some(c) = s.chars().next()
            && c != '\u{FFFD}'
        {
            map.entry(c).or_insert(b);
        }
    };
    for b in 0x80u8..=0xFF {
        add(enc, b);
    }
    // Bytes Hebrew Windows leaves undefined (0x9A, 0x9C, 0x9E, 0x9F…) come
    // out as their Western (1252) characters in the wild ("׳œ" for "ל").
    for b in 0x80u8..=0x9F {
        add(WINDOWS_1252, b);
    }
    // Undefined bytes some decoders pass through as C1 controls.
    for b in 0x80u8..=0x9F {
        map.entry(char::from(b)).or_insert(b);
    }
    map
}

fn tables() -> &'static [HashMap<char, u8>; 2] {
    static T: OnceLock<[HashMap<char, u8>; 2]> = OnceLock::new();
    T.get_or_init(|| [table(WINDOWS_1255), table(WINDOWS_1252)])
}

/// Bytes still owed by the last (incomplete) UTF-8 sequence in `b`.
fn incomplete(b: &[u8]) -> bool {
    let tail = &b[b.len().saturating_sub(3)..];
    for (i, &x) in tail.iter().enumerate().rev() {
        let need = match x {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            0x80..=0xBF => continue,
            _ => return false,
        };
        return tail.len() - i < need;
    }
    false
}

/// How many lead-byte characters are directly followed by a
/// continuation-byte character: the fingerprint of this mojibake.
fn signs(s: &str, map: &HashMap<char, u8>) -> usize {
    let mut n = 0;
    let mut prev: Option<u8> = None;
    for c in s.chars() {
        let b = map.get(&c).copied();
        if let (Some(0xC2..=0xF4), Some(0x80..=0xBF)) = (prev, b) {
            n += 1;
        }
        prev = b;
    }
    n
}

fn repair_with(s: &str, map: &HashMap<char, u8>) -> (String, usize) {
    let mut out = String::with_capacity(s.len());
    let mut fixed = 0;
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if !map.contains_key(&c) {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        // A run of mappable characters (plus 0xA0 stand-ins mid-character).
        let mut bytes = Vec::new();
        let mut end = 0;
        loop {
            let r = &rest[end..];
            if let Some(c) = r.chars().next()
                && let Some(&b) = map.get(&c)
            {
                bytes.push(b);
                end += c.len_utf8();
                continue;
            }
            if incomplete(&bytes) {
                if r.starts_with("&nbsp;") {
                    bytes.push(0xA0);
                    end += "&nbsp;".len();
                    continue;
                }
                if r.starts_with(' ') {
                    bytes.push(0xA0);
                    end += 1;
                    continue;
                }
            }
            break;
        }
        let run = &rest[..end];
        match std::str::from_utf8(&bytes) {
            Ok(good) if good != run => {
                out.push_str(good);
                fixed += 1;
            }
            _ => out.push_str(run),
        }
        rest = &rest[end..];
    }
    (out, fixed)
}

/// The repaired text, or None when `s` doesn't look like mojibake (or
/// repairing it doesn't work out).
pub fn repair(s: &str) -> Option<String> {
    if s.is_ascii() {
        return None;
    }
    tables()
        .iter()
        .filter(|map| signs(s, map) >= MIN_SIGNS)
        .map(|map| repair_with(s, map))
        .filter(|(_, fixed)| *fixed >= MIN_FIXED)
        .max_by_key(|(_, fixed)| *fixed)
        .map(|(text, _)| text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-8 text as it looks after a decode in `enc`.
    fn garble(s: &str, enc: &'static Encoding) -> String {
        enc.decode_without_bom_handling(s.as_bytes()).0.into_owned()
    }

    #[test]
    fn hebrew_read_as_windows_1255_comes_back() {
        let original = "שלום רב, מצורפת הצעת מחיר. נשמח לשמוע ממך";
        // Undefined 1255 bytes shown as their 1252 characters, as seen in
        // the wild: ל is D7 9C.
        let broken = garble(original, WINDOWS_1255).replace('\u{9c}', "œ").replace('\u{9e}', "ž");
        assert_ne!(broken, original);
        assert_eq!(repair(&broken).as_deref(), Some(original));
        // In HTML, with the 0xA0 byte of "נ" turned into &nbsp; or a space.
        let html = format!("<p dir=\"rtl\">{}</p>", broken.replace('\u{a0}', "&nbsp;"));
        assert_eq!(repair(&html), Some(format!("<p dir=\"rtl\">{original}</p>")));
        assert_eq!(repair(&broken.replace('\u{a0}', " ")).as_deref(), Some(original));
    }

    #[test]
    fn latin_read_as_windows_1252_comes_back() {
        let original = "Café crème, déjà vu — naïve résumé";
        assert_eq!(repair(&garble(original, WINDOWS_1252)).as_deref(), Some(original));
    }

    #[test]
    fn healthy_text_is_left_alone() {
        for s in [
            "שלום, מה שלומך? ג׳ירפה וצ׳יפס",
            "Café crème, déjà vu — naïve résumé",
            "Plain ASCII only",
            "Ünïcödé “quotes” and ₪100 × 3",
            "",
        ] {
            assert_eq!(repair(s), None, "{s}");
        }
    }

    #[test]
    fn one_stray_pair_is_not_enough() {
        let broken = garble("שלום", WINDOWS_1255);
        assert_eq!(repair(&format!("Hello {broken} there")), None);
    }

    #[test]
    fn incomplete_tail() {
        assert!(incomplete(&[0xD7]));
        assert!(!incomplete(&[0xD7, 0x90]));
        assert!(incomplete(&[0xE2, 0x80]));
        assert!(!incomplete(b"ab"));
    }
}
