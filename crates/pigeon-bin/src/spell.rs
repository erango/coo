//! macOS's Hebrew spell checker as the judge for repairing garbled Hebrew
//! (`pigeon_core::mojibake`): where a letter was lost in transit, the
//! candidate that makes real words wins.

use std::collections::HashMap;
use std::sync::Mutex;

/// Install the judge (once, at startup), when this Mac has a Hebrew
/// dictionary; without one the repair keeps its plain guesses.
pub fn install() {
    if imp::has_hebrew() {
        pigeon_core::mojibake::set_word_judge(hebrew_word);
    }
}

fn hebrew_word(word: &str) -> bool {
    static CACHE: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Ok(mut c) = CACHE.lock()
        && let Some(&known) = c.get_or_insert_with(HashMap::new).get(word)
    {
        return known;
    }
    let ok = imp::check(word);
    if let Ok(mut c) = CACHE.lock() {
        let map = c.get_or_insert_with(HashMap::new);
        if map.len() > 20_000 {
            map.clear();
        }
        map.insert(word.to_string(), ok);
    }
    ok
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSNotFound, NSString};

    /// Hebrew listed and actually working (a listed language can lack its
    /// dictionary, as on CI runners): a known word passes, a misspelling
    /// doesn't.
    pub fn has_hebrew() -> bool {
        NSSpellChecker::sharedSpellChecker().availableLanguages().iter().any(|l| l.to_string() == "he")
            && check("שלום")
            && !check("שלוא")
    }

    pub fn check(word: &str) -> bool {
        let checker = NSSpellChecker::sharedSpellChecker();
        let (w, lang) = (NSString::from_str(word), NSString::from_str("he"));
        // SAFETY: a null word-count pointer is allowed.
        let miss = unsafe {
            checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                &w,
                0,
                Some(&lang),
                false,
                0,
                std::ptr::null_mut(),
            )
        };
        miss.location == NSNotFound as usize
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn check(_word: &str) -> bool {
        false
    }

    pub fn has_hebrew() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn knows_hebrew_words() {
        // CI runners may have no Hebrew dictionary.
        if !super::imp::has_hebrew() {
            return;
        }
        assert!(super::imp::check("שלום"));
        assert!(!super::imp::check("שלוא"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn repairs_real_garbled_sentences() {
        if !super::imp::has_hebrew() {
            return;
        }
        super::install();
        // Sentences from a real garbled mail: lost א and ם bytes became
        // spaces (the user's corrections).
        let original = "הי בוקר טוב מאושר תודה רבה\nמאת: אודה לקבלת ציטוט. אתם תוסיפו ציטוט\n\
                        בוקר טוב, האם המאמר מאושר לפרסום?\nאנא אשרו את המאמר";
        let bytes: Vec<u8> = original.bytes().map(|b| if matches!(b, 0x90 | 0x9D) { b' ' } else { b }).collect();
        let broken = encoding_rs::WINDOWS_1255.decode_without_bom_handling(&bytes).0.into_owned();
        assert_eq!(pigeon_core::mojibake::repair(&broken).as_deref(), Some(original));
    }
}
