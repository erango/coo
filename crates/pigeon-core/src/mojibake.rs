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

/// Hebrew letters are D7 90..D7 AA. Three of them can't survive the trip:
/// 0x90 (א) and 0x9D (ם) are undefined in both Windows charsets and get
/// dropped or turned into a space, and 0xA0 (נ) is a non-breaking space
/// that often becomes a plain one. Each spot where that happened (a bare
/// D7 lead) is a `Slot`, resolved afterwards from the words around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Between two letters: א (ם only ends a word; נ's byte would be there).
    Mid,
    /// At the end of a word (before punctuation, a line end or the end).
    End,
    /// Followed by a space: ם or א ending a word, or א / נ whose byte
    /// became that space.
    EndSpace,
}

/// Placeholder for a slot in the decoded text (private-use, never in mail).
const SLOT: char = '\u{E000}';

impl Slot {
    /// (letter, whether it takes the following space), best guess first.
    fn options(self) -> &'static [(char, bool)] {
        match self {
            Slot::Mid => &[('א', false)],
            Slot::End => &[('ם', false), ('א', false)],
            Slot::EndSpace => &[('ם', false), ('א', true), ('א', false), ('נ', true)],
        }
    }
}

/// Decides whether a Hebrew word exists (the app sets macOS's spell
/// checker). Without one, slots take their first option, or א for the
/// common words below.
static WORD_JUDGE: OnceLock<fn(&str) -> bool> = OnceLock::new();

pub fn set_word_judge(judge: fn(&str) -> bool) {
    let _ = WORD_JUDGE.set(judge);
}

/// Frequent words ending in א, for when there's no spell checker.
const ENDS_IN_ALEF: &[&str] = &[
    "לא",
    "הוא",
    "היא",
    "אבא",
    "אמא",
    "סבא",
    "בא",
    "נא",
    "מלא",
    "קרא",
    "יצא",
    "מצא",
    "נמצא",
    "שוא",
    "דווקא",
    "אלא",
    "רופא",
    "ממילא",
    "הבא",
    "תא",
    "מבוא",
    "שלא",
    "ולא",
    "כשלא",
    "פלא",
    "חטא",
];

fn is_hebrew_letter(c: char) -> bool {
    ('\u{5d0}'..='\u{5ea}').contains(&c)
}

fn repair_with(s: &str, map: &HashMap<char, u8>, judge: Option<fn(&str) -> bool>) -> (String, usize) {
    let hebrew = map.get(&'\u{5f3}') == Some(&0xD7);
    let mut out = String::with_capacity(s.len());
    let mut fixed = 0;
    let mut rest = s;
    // A lost letter: the bare lead goes, the placeholder's bytes take its place.
    let slot = |bytes: &mut Vec<u8>, kinds: &mut Vec<Slot>, kind: Slot| {
        bytes.pop();
        bytes.extend_from_slice(SLOT.encode_utf8(&mut [0; 3]).as_bytes());
        kinds.push(kind);
    };
    let mut slots: Vec<(usize, Slot)> = Vec::new();
    while let Some(c) = rest.chars().next() {
        if !map.contains_key(&c) {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        // A run of mappable characters (plus &nbsp; for a 0xA0 mid-character).
        let mut bytes = Vec::new();
        let mut kinds = Vec::new();
        let mut end = 0;
        loop {
            let r = &rest[end..];
            if let Some(c) = r.chars().next()
                && let Some(&b) = map.get(&c)
            {
                if hebrew && b == 0xD7 && bytes.last() == Some(&0xD7) {
                    slot(&mut bytes, &mut kinds, Slot::Mid);
                }
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
                if hebrew && bytes.last() == Some(&0xD7) {
                    let kind = if r.starts_with(' ') { Slot::EndSpace } else { Slot::End };
                    slot(&mut bytes, &mut kinds, kind);
                }
            }
            break;
        }
        let run = &rest[..end];
        match std::str::from_utf8(&bytes) {
            Ok(good) if good != run => {
                let mut k = kinds.into_iter();
                for (i, ch) in good.char_indices() {
                    if ch == SLOT
                        && let Some(kind) = k.next()
                    {
                        slots.push((out.len() + i, kind));
                    }
                }
                out.push_str(good);
                fixed += 1;
            }
            _ => out.push_str(run),
        }
        rest = &rest[end..];
    }
    if slots.is_empty() {
        return (out, fixed);
    }
    (fill_slots(&out, &slots, judge), fixed)
}

/// Bounds of the word (Hebrew letters and slots) around `at`.
fn word_bounds(cs: &[char], at: usize) -> (usize, usize) {
    let part = |c: char| is_hebrew_letter(c) || c == SLOT;
    let mut from = at;
    while from > 0 && part(cs[from - 1]) {
        from -= 1;
    }
    let mut to = (at + 1).min(cs.len());
    while to < cs.len() && part(cs[to]) {
        to += 1;
    }
    (from, to)
}

/// Punctuation that never follows a space (unlike "-" or "(").
const CLOSING: &[char] = &[',', '.', ':', ';', '?', '!', ')', ']'];

/// More slots than this in one stretch: no search, best guesses only.
const MAX_CLUSTER: usize = 5;

/// Pick the slots' letters. Slots in the same stretch of words (a lost
/// letter can merge or split words, so neighbours decide together) try
/// every combination of their options; the one with the fewest made-up
/// words wins, ties going to the likelier options. Without a judge each slot
/// takes its first option (א for the common words ending in it).
fn fill_slots(text: &str, slots: &[(usize, Slot)], judge: Option<fn(&str) -> bool>) -> String {
    let mut cs: Vec<char> = text.chars().collect();
    let char_at: HashMap<usize, usize> = text.char_indices().enumerate().map(|(ci, (bi, _))| (bi, ci)).collect();
    let mut found: Vec<(usize, Slot)> = slots.iter().filter_map(|(b, k)| char_at.get(b).map(|&c| (c, *k))).collect();
    found.sort_by_key(|(c, _)| *c);
    // Bytes dropped outright leave two leads side by side (a Mid slot);
    // without any, this mail turned its lost bytes into spaces.
    let spaced = !found.iter().any(|(_, k)| *k == Slot::Mid);
    // Neighbouring slots: only letters and at most one space between them.
    let mut clusters: Vec<Vec<(usize, Slot)>> = Vec::new();
    for s in found {
        if let Some(last) = clusters.last_mut().and_then(|c| c.last().copied())
            && cs[last.0 + 1..s.0].iter().all(|&c| is_hebrew_letter(c) || c == ' ')
            && cs[last.0 + 1..s.0].iter().filter(|&&c| c == ' ').count() <= 1
        {
            clusters.last_mut().unwrap().push(s);
            continue;
        }
        clusters.push(vec![s]);
    }
    // Right to left, so earlier indices stay valid as text changes length.
    for cluster in clusters.iter().rev() {
        let (first, last) = (cluster[0].0, cluster[cluster.len() - 1].0);
        let from = word_bounds(&cs, first).0;
        let mut to = word_bounds(&cs, last).1;
        if cs.get(to) == Some(&' ') {
            to = word_bounds(&cs, (to + 1).min(cs.len().saturating_sub(1))).1.max(to);
        }
        let region = &cs[from..to];
        let render = |pick: &[usize]| -> Vec<char> {
            let mut out = Vec::with_capacity(region.len());
            let mut k = 0;
            let mut skip_space = false;
            for (off, &c) in region.iter().enumerate() {
                if skip_space {
                    skip_space = false;
                    if c == ' ' {
                        continue;
                    }
                }
                if k < cluster.len() && from + off == cluster[k].0 {
                    let (letter, takes) = cluster[k].1.options()[pick[k]];
                    out.push(letter);
                    // A lost letter ending a word (ם always, א here) whose
                    // byte became a space: that space, when followed by
                    // another space, punctuation or the end, was the
                    // letter's byte, not a word break, so it goes.
                    let at = from + off;
                    let byte_space = matches!(letter, 'ם' | 'א')
                        && cs.get(at + 1) == Some(&' ')
                        && cs.get(at + 2).is_none_or(|&c| c.is_whitespace() || CLOSING.contains(&c));
                    skip_space = takes || byte_space;
                    k += 1;
                } else {
                    out.push(c);
                }
            }
            out
        };
        let default: Vec<usize> = cluster
            .iter()
            .map(|&(at, kind)| {
                // No judge: the common words ending in א keep it.
                let (wf, _) = word_bounds(&cs, at);
                let word: String =
                    cs[wf..at].iter().map(|&c| if c == SLOT { 'א' } else { c }).chain(std::iter::once('א')).collect();
                let alef = kind.options().iter().position(|o| *o == ('א', false));
                match alef {
                    Some(a) if kind != Slot::Mid && ENDS_IN_ALEF.contains(&word.as_str()) => a,
                    _ => 0,
                }
            })
            .collect();
        let pick = match judge {
            Some(judge) if cluster.len() <= MAX_CLUSTER => {
                // Lower is better: words that don't exist (a lone letter
                // never does), then, where lost bytes became spaces, more
                // words (a space next to a slot is likelier its byte than a
                // break: "מאושר", not "מם ושר").
                let score = |p: &[usize]| {
                    let s: String = render(p).into_iter().collect();
                    let words: Vec<&str> = s.split(|c: char| !is_hebrew_letter(c)).filter(|w| !w.is_empty()).collect();
                    let wrong = words.iter().filter(|w| w.chars().count() < 2 || !judge(w)).count();
                    (wrong, if spaced { words.len() } else { 0 })
                };
                let sizes: Vec<usize> = cluster.iter().map(|(_, k)| k.options().len()).collect();
                let mut best = default.clone();
                let mut best_score = score(&best);
                let mut p = vec![0; cluster.len()];
                'all: loop {
                    let sc = score(&p);
                    if sc < best_score {
                        best_score = sc;
                        best = p.clone();
                    }
                    // Next combination (odometer).
                    for d in (0..p.len()).rev() {
                        p[d] += 1;
                        if p[d] < sizes[d] {
                            continue 'all;
                        }
                        p[d] = 0;
                    }
                    break;
                }
                best
            }
            _ => default,
        };
        let replaced = render(&pick);
        cs.splice(from..to, replaced);
    }
    cs.into_iter().collect()
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
        .map(|map| repair_with(s, map, WORD_JUDGE.get().copied()))
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
        // In HTML, with the 0xA0 byte of "נ" written as &nbsp;.
        let html = format!("<p dir=\"rtl\">{}</p>", broken.replace('\u{a0}', "&nbsp;"));
        assert_eq!(repair(&html), Some(format!("<p dir=\"rtl\">{original}</p>")));
    }

    #[test]
    fn dropped_alef_and_final_mem_are_restored() {
        // א (D7 90) and ם (D7 9D) have bytes neither charset defines; mail
        // often loses them entirely.
        let original = "שלום אבא, מה שלומם היום? אני בבית";
        let broken: String =
            garble(original, WINDOWS_1255).chars().filter(|c| !matches!(c, '\u{90}' | '\u{9d}')).collect();
        assert_ne!(broken, garble(original, WINDOWS_1255));
        assert_eq!(repair(&broken).as_deref(), Some(original));
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
    fn a_spell_checker_picks_between_alef_mem_and_nun() {
        // A lost א became a space (the common case the guess got wrong), a
        // lost ם was dropped at a word's end, and נ's nbsp became a space.
        const WORDS: &[&str] = &["אני", "מאוד", "שמח", "לשמוע", "שלום", "חברים", "נא", "לענות", "היום"];
        fn judge(w: &str) -> bool {
            WORDS.contains(&w)
        }
        let original = "שלום חברים, אני מאוד שמח לשמוע. נא לענות היום";
        let broken = garble(original, WINDOWS_1255)
            .replace('\u{90}', " ") // א's byte turned into a space
            .replace('\u{9d}', "") // ם's byte dropped
            .replace('\u{a0}', " "); // נ's nbsp became a plain space
        assert_eq!(repair_with(&broken, &tables()[0], Some(judge)).0, original);
    }

    #[test]
    fn final_mem_whose_byte_became_a_space_adds_no_space() {
        // ם's byte turned into a space: "שלום," must not become "שלום ,",
        // nor "שלום אני" get two spaces.
        let original = "שלום, שלום אני";
        let broken = garble(original, WINDOWS_1255).replace('\u{9d}', " ");
        assert_eq!(repair(&format!("{broken} {broken}")).as_deref(), Some(format!("{original} {original}").as_str()));
    }

    #[test]
    fn a_space_next_to_a_lost_alef_was_its_byte() {
        // Real mail: every lost א turned into a space, which read as a
        // word break ("מם ושר", "מם ת:", "ם ודה", "ם תא").
        const WORDS: &[&str] = &[
            "הי",
            "בוקר",
            "טוב",
            "מאושר",
            "מם",
            "ושר",
            "תודה",
            "רבה",
            "מאת",
            "אודה",
            "לקבלת",
            "אתם",
            "תוסיפו",
            "ציטוט",
            "תא",
            "את",
        ];
        fn judge(w: &str) -> bool {
            WORDS.contains(&w)
        }
        for original in ["הי בוקר טוב מאושר תודה רבה", "מאת: תודה רבה", "אודה לקבלת ציטוט", "אתם תוסיפו ציטוט"]
        {
            let broken = garble(original, WINDOWS_1255).replace(['\u{90}', '\u{9d}'], " ");
            assert_eq!(repair_with(&broken, &tables()[0], Some(judge)).0, original);
        }
    }

    #[test]
    fn incomplete_tail() {
        assert!(incomplete(&[0xD7]));
        assert!(!incomplete(&[0xD7, 0x90]));
        assert!(incomplete(&[0xE2, 0x80]));
        assert!(!incomplete(b"ab"));
    }
}
