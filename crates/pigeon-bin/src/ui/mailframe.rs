//! Building the sandboxed `srcdoc` for an email body.

use crate::themes;

const ADAPTED_CSS: &str = include_str!("../../../../design/mail-adapted.css");

/// What a message body shows: its HTML, its plain text, or nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Shown<'a> {
    Html(&'a str),
    Text(&'a str),
    Empty,
}

/// HTML with nothing a reader would see (no text, images or tables).
pub fn html_is_blank(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    if ["<img", "<table", "<svg", "<video", "<hr"].iter().any(|t| lower.contains(t)) {
        return false;
    }
    pigeon_core::text::html_to_text(html).chars().all(|c| c.is_whitespace() || c == '\u{a0}' || c == '\u{200b}')
}

/// Prefer the HTML part, but not an empty one when the text part has
/// content, and report an empty message instead of showing a blank frame.
pub fn shown<'a>(html: Option<&'a str>, text: Option<&'a str>) -> Shown<'a> {
    let text = text.filter(|t| !t.trim().is_empty());
    match html {
        Some(h) if !html_is_blank(h) => Shown::Html(h),
        _ => text.map_or(Shown::Empty, Shown::Text),
    }
}

/// Authored-layout email (newsletters, receipts) that should keep its own
/// white background ("paper") rather than adapt to the theme.
pub fn is_complex(html: &str) -> bool {
    let l = html.to_ascii_lowercase();
    l.contains("bgcolor")
        || l.contains("background-color")
        || l.contains("background:")
        || l.matches("<table").count() > 1
}

/// Free-text terms of a search query worth highlighting: lowercased,
/// deduped, longest first (so "invoice" wins over "in").
pub fn match_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = pigeon_core::search::parse(query)
        .text
        .into_iter()
        .map(|t| t.trim_matches(|c: char| c == '*' || c == '"').to_lowercase())
        .filter(|t| t.chars().count() >= 2)
        .collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.chars().count()));
    terms.dedup();
    terms
}

/// Byte length of `term` matched case-insensitively at the start of `s`.
fn match_at(s: &str, term: &str) -> Option<usize> {
    let mut it = s.char_indices();
    for tc in term.chars() {
        let (_, c) = it.next()?;
        if c.to_lowercase().ne(tc.to_lowercase()) {
            return None;
        }
    }
    Some(it.next().map_or(s.len(), |(i, _)| i))
}

/// Split plain text into (segment, is_match) runs.
pub fn split_matches<'a>(text: &'a str, terms: &[String]) -> Vec<(&'a str, bool)> {
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < text.len() {
        if let Some(len) = terms.iter().find_map(|t| match_at(&text[i..], t)) {
            if start < i {
                out.push((&text[start..i], false));
            }
            out.push((&text[i..i + len], true));
            i += len;
            start = i;
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if start < text.len() {
        out.push((&text[start..], false));
    }
    out
}

/// Wrap matches in text nodes of (sanitized) HTML with `<mark>`; tags,
/// entities and style/script/title content are left alone. Returns the new
/// HTML and the number of matches. Only the original characters plus our
/// own `<mark>` tags are emitted, so it can't open a sanitization hole.
pub fn highlight_html(html: &str, terms: &[String]) -> (String, usize) {
    if terms.is_empty() {
        return (html.to_string(), 0);
    }
    let mut out = String::with_capacity(html.len() + 64);
    let mut count = 0;
    let mut rest = html;
    let mut skip_until: Option<String> = None;
    while !rest.is_empty() {
        let lt = rest.find('<').unwrap_or(rest.len());
        let text = &rest[..lt];
        if skip_until.is_some() {
            out.push_str(text);
        } else {
            // Don't match across an entity: highlight between them only.
            let mut t = text;
            while !t.is_empty() {
                let amp = t.find('&').unwrap_or(t.len());
                for (seg, m) in split_matches(&t[..amp], terms) {
                    if m {
                        count += 1;
                        out.push_str("<mark data-pg-match>");
                        out.push_str(seg);
                        out.push_str("</mark>");
                    } else {
                        out.push_str(seg);
                    }
                }
                t = &t[amp..];
                if !t.is_empty() {
                    let end = t.find(';').filter(|&e| e <= 12).map_or(1, |e| e + 1);
                    out.push_str(&t[..end]);
                    t = &t[end..];
                }
            }
        }
        rest = &rest[lt..];
        if rest.is_empty() {
            break;
        }
        // A tag (or comment) through its closing '>', honoring quotes.
        let end = if rest.starts_with("<!--") {
            rest.find("-->").map_or(rest.len(), |e| e + 3)
        } else {
            let mut q: Option<char> = None;
            let mut e = rest.len();
            for (i, c) in rest.char_indices().skip(1) {
                match (q, c) {
                    (Some(open), c) if c == open => q = None,
                    (None, '"' | '\'') => q = Some(c),
                    (None, '>') => {
                        e = i + 1;
                        break;
                    }
                    _ => {}
                }
            }
            e
        };
        let tag = &rest[..end];
        let name = tag
            .trim_start_matches('<')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
            .collect::<String>()
            .to_ascii_lowercase();
        match &skip_until {
            Some(close) if name == *close => skip_until = None,
            None if matches!(name.as_str(), "style" | "script" | "title" | "textarea") => {
                skip_until = Some(format!("/{name}"))
            }
            _ => {}
        }
        out.push_str(tag);
        rest = &rest[end..];
    }
    (out, count)
}

const MARK_CSS: &str = "mark{background:#FFE58A;color:#111;border-radius:3px;padding:0 1px}mark[data-current]{background:#1F8A70;color:#fff}";

/// The frame is sized to its content, so the content can't size itself to
/// the frame: mail that sets `html`/`body` to `height:100%` (or a min-height)
/// grew the frame on every fit, endlessly or at least never back down.
/// `:root` keeps these above the mail's own rules, `!important` included.
const NATURAL_HEIGHT_CSS: &str =
    "html:root,html:root>body{height:auto!important;min-height:0!important;max-height:none!important}";

/// `palette` + `dark` pick the adapted text/muted/link colors (the frame
/// can't see the app's custom properties).
pub fn srcdoc(html: &str, palette: &str, dark: bool, adapted: bool, allow_remote: bool) -> String {
    let img_src = if allow_remote { "data: https: http:" } else { "data:" };
    let style = if adapted {
        let (fg, muted, link) = themes::mail_colors(palette, dark);
        ADAPTED_CSS
            .replace("--mail-fg: #EDEEF0", &format!("--mail-fg: {fg}"))
            .replace("--mail-muted: #A3A6AE", &format!("--mail-muted: {muted}"))
            .replace("--mail-link: #7AAEFF", &format!("--mail-link: {link}"))
    } else {
        "html,body{margin:0;background:#fff;color:#111;font:15px/1.5 -apple-system,system-ui,sans-serif}\
         body{padding:16px}img{max-width:100%;height:auto}"
            .to_string()
    };
    // Pin the frame's color scheme to the app's theme (not the system's):
    // WebKit paints an opaque backdrop behind a frame whose scheme differs
    // from its embedder's, which turned adapted mail into dark text on
    // black whenever the app theme and macOS appearance disagreed.
    let scheme = if adapted && dark { "dark" } else { "light" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"color-scheme\" content=\"{scheme}\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src {img_src}; font-src data:\">\
         <base target=\"_blank\"><style>{style}{MARK_CSS}{NATURAL_HEIGHT_CSS}:root{{color-scheme:{scheme}}}</style></head><body dir=\"auto\">{html}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bodies_are_recognized() {
        // The shape of a real message whose sender sent only a subject.
        let empty = "<html><head><meta charset=\"utf-8\"></head><body></body></html>\r\n";
        assert_eq!(shown(Some(empty), Some("\r\n")), Shown::Empty);
        assert_eq!(shown(Some("\n"), Some("\r\n")), Shown::Empty);
        assert_eq!(shown(Some("<div><br></div>"), Some("Hi there")), Shown::Text("Hi there"));
        assert_eq!(shown(Some("<p>שלום</p>"), None), Shown::Html("<p>שלום</p>"));
        assert_eq!(shown(Some(r#"<img src="cid:x">"#), None), Shown::Html(r#"<img src="cid:x">"#));
        assert_eq!(shown(None, Some("  ")), Shown::Empty);
        assert_eq!(shown(Some("<p>&nbsp;</p>"), None), Shown::Empty);
    }

    #[test]
    fn frame_color_scheme_follows_the_app_theme() {
        // Light app on a dark Mac: the frame must not pick "dark".
        let light = srcdoc("<p>x</p>", "pigeon", false, true, false);
        assert!(light.contains(":root{color-scheme:light}") && !light.contains("color-scheme:dark"));
        assert!(srcdoc("<p>x</p>", "pigeon", true, true, false).contains(":root{color-scheme:dark}"));
        // Paper mode is always light.
        assert!(srcdoc("<p>x</p>", "pigeon", true, false, false).contains(":root{color-scheme:light}"));
    }

    #[test]
    fn csp_blocks_remote_until_allowed() {
        let blocked = srcdoc("<p>x</p>", "pigeon", true, true, false);
        assert!(blocked.contains("img-src data:;"));
        assert!(srcdoc("<p>x</p>", "pigeon", true, true, true).contains("img-src data: https: http:"));
        assert!(blocked.contains("default-src 'none'"));
    }

    #[test]
    fn adapted_colors_follow_theme_and_palette() {
        assert!(srcdoc("", "pigeon", false, true, false).contains("--mail-fg: #16171A"));
        assert!(srcdoc("", "pigeon", true, true, false).contains("--mail-fg: #EDEEF0"));
        assert!(!srcdoc("", "pigeon", true, false, false).contains("--mail-fg"));
        let (fg, _, link) = themes::mail_colors("dracula", true);
        let doc = srcdoc("", "dracula", true, true, false);
        assert!(doc.contains(&format!("--mail-fg: {fg}")) && doc.contains(&format!("--mail-link: {link}")));
        assert!(!doc.contains("#EDEEF0"));
    }

    #[test]
    fn highlights_text_not_markup() {
        let terms = match_terms("invoice from:dana Q4 a");
        assert_eq!(terms, ["invoice", "q4"]);
        let html = r#"<style>.invoice{color:red}</style><p class="invoice" title="Invoice">Your INVOICE for Q4 &amp; invoices</p><!-- invoice -->"#;
        let (out, n) = highlight_html(html, &terms);
        assert_eq!(n, 3);
        assert!(out.starts_with(r#"<style>.invoice{color:red}</style><p class="invoice" title="Invoice">"#));
        assert!(out.contains(
            "<mark data-pg-match>INVOICE</mark> for <mark data-pg-match>Q4</mark> &amp; <mark data-pg-match>invoice</mark>s"
        ));
        assert!(out.ends_with("<!-- invoice -->"));
        // Hebrew, case-insensitive non-ASCII, and no terms.
        let (he, n) = highlight_html("<p>הזמנה מספר 4471</p>", &match_terms("הזמנה"));
        assert_eq!((n, he.as_str()), (1, "<p><mark data-pg-match>הזמנה</mark> מספר 4471</p>"));
        assert_eq!(highlight_html("<p>ÉCOLE</p>", &match_terms("école")).1, 1);
        assert_eq!(highlight_html("<p>x</p>", &[]), ("<p>x</p>".to_string(), 0));
        assert_eq!(split_matches("a Q4 b", &terms), [("a ", false), ("Q4", true), (" b", false)]);
    }

    #[test]
    fn complexity_heuristic() {
        assert!(is_complex(r##"<table bgcolor="#fff"><tr><td>x</td></tr></table>"##));
        assert!(!is_complex("<p dir=auto>שלום</p><blockquote>hi</blockquote>"));
    }
}
