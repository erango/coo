//! Bundled fonts (SIL Open Font License; see assets/fonts/LICENSE-*.txt):
//! Inter (variable, all UI weights) and JetBrains Mono (regular + medium).
//! Embedded as data URLs in the startup `<style>` so the app never waits on
//! the network or flashes the fallback face. Hebrew isn't in Inter and falls
//! back to the system font, as before.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.woff2");
const MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.woff2");
const MONO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Medium.woff2");

fn face(family: &str, weight: &str, bytes: &[u8]) -> String {
    format!(
        "@font-face{{font-family:\"{family}\";font-style:normal;font-weight:{weight};font-display:block;\
         src:url(data:font/woff2;base64,{}) format(\"woff2\");}}",
        STANDARD.encode(bytes)
    )
}

/// `@font-face` rules; put them before tokens.css.
pub fn css() -> String {
    [
        face("Inter", "100 900", INTER),
        face("JetBrains Mono", "400", MONO_REGULAR),
        face("JetBrains Mono", "500", MONO_MEDIUM),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn faces_are_woff2_and_complete() {
        let css = super::css();
        assert_eq!(css.matches("@font-face").count(), 3);
        assert!(css.contains("font-family:\"Inter\"") && css.contains("font-family:\"JetBrains Mono\""));
        // woff2 magic "wOF2" base64-encodes to "d09GMg".
        assert_eq!(css.matches("base64,d09GMg").count(), 3);
    }
}
