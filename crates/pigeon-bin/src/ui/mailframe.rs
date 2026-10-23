//! Building the sandboxed `srcdoc` for an email body.

const ADAPTED_CSS: &str = include_str!("../../../../design/mail-adapted.css");

/// (text, muted, link) per theme, from tokens.css.
const DARK: (&str, &str, &str) = ("#EDEEF0", "#A3A6AE", "#7AAEFF");
const LIGHT: (&str, &str, &str) = ("#16171A", "#52555C", "#2459C7");

/// Authored-layout email (newsletters, receipts) that should keep its own
/// white background ("paper") rather than adapt to the theme.
pub fn is_complex(html: &str) -> bool {
    let l = html.to_ascii_lowercase();
    l.contains("bgcolor")
        || l.contains("background-color")
        || l.contains("background:")
        || l.matches("<table").count() > 1
}

pub fn srcdoc(html: &str, dark: bool, adapted: bool, allow_remote: bool) -> String {
    let img_src = if allow_remote { "data: https: http:" } else { "data:" };
    let style = if adapted {
        let (fg, muted, link) = if dark { DARK } else { LIGHT };
        ADAPTED_CSS
            .replace("--mail-fg: #EDEEF0", &format!("--mail-fg: {fg}"))
            .replace("--mail-muted: #A3A6AE", &format!("--mail-muted: {muted}"))
            .replace("--mail-link: #7AAEFF", &format!("--mail-link: {link}"))
    } else {
        "html,body{margin:0;background:#fff;color:#111;font:15px/1.5 -apple-system,system-ui,sans-serif}\
         body{padding:16px}img{max-width:100%;height:auto}"
            .to_string()
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src {img_src}; font-src data:\">\
         <base target=\"_blank\"><style>{style}</style></head><body dir=\"auto\">{html}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_blocks_remote_until_allowed() {
        let blocked = srcdoc("<p>x</p>", true, true, false);
        assert!(blocked.contains("img-src data:;"));
        assert!(srcdoc("<p>x</p>", true, true, true).contains("img-src data: https: http:"));
        assert!(blocked.contains("default-src 'none'"));
    }

    #[test]
    fn adapted_colors_follow_theme() {
        assert!(srcdoc("", false, true, false).contains("--mail-fg: #16171A"));
        assert!(srcdoc("", true, true, false).contains("--mail-fg: #EDEEF0"));
        assert!(!srcdoc("", true, false, false).contains("--mail-fg"));
    }

    #[test]
    fn complexity_heuristic() {
        assert!(is_complex(r##"<table bgcolor="#fff"><tr><td>x</td></tr></table>"##));
        assert!(!is_complex("<p dir=auto>שלום</p><blockquote>hi</blockquote>"));
    }
}
