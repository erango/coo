//! macOS's Hebrew spell checker as the judge for repairing garbled Hebrew
//! (`pigeon_core::mojibake`): where a letter was lost in transit, the
//! candidate that makes real words wins.

use std::collections::HashMap;
use std::sync::Mutex;

/// Install the judge (once, at startup).
pub fn install() {
    pigeon_core::mojibake::set_word_judge(hebrew_word);
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
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn knows_hebrew_words() {
        assert!(super::imp::check("שלום"));
        assert!(!super::imp::check("שלוא"));
    }
}
