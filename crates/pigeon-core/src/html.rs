//! Email HTML sanitizing.
//!
//! Defense in depth: the UI renders bodies in a sandboxed frame (no scripts)
//! with a CSP whose `img-src` excludes the network until the user allows
//! remote content. Sanitizing here strips active content and tells the UI
//! whether a "load remote images" banner is worth showing.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub html: String,
    pub has_remote_content: bool,
}

const EXTRA_TAGS: &[&str] = &[
    "style",
    "font",
    "center",
    "big",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
    "figure",
    "figcaption",
    "picture",
    "source",
    "label",
    "caption",
    "colgroup",
    "col",
    "wbr",
    "bdi",
    "bdo",
];

const GENERIC_ATTRS: &[&str] = &[
    "style",
    "class",
    "id",
    "dir",
    "lang",
    "title",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "background",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "face",
    "size",
    "colspan",
    "rowspan",
    "role",
    "aria-label",
    "aria-hidden",
];

const REMOTE_URL_ATTRS: &[&str] = &["src", "srcset", "background", "poster"];
/// Elements that survive sanitizing and can load a URL by themselves.
/// (The filter also sees attributes of elements that are later dropped.)
const REMOTE_URL_ELEMENTS: &[&str] = &["img", "source", "table", "td", "th", "tr", "body", "div"];

pub fn sanitize(html: &str) -> Sanitized {
    let remote = Arc::new(AtomicBool::new(css_loads_remote(html)));
    let flag = remote.clone();

    let clean = ammonia::Builder::default()
        .add_tags(EXTRA_TAGS)
        .clean_content_tags(HashSet::from(["script", "title", "noscript", "template", "object", "iframe"]))
        .add_generic_attributes(GENERIC_ATTRS)
        .add_tag_attributes("img", &["src", "srcset", "alt", "width", "height"])
        .add_tag_attributes("source", &["srcset", "media", "type"])
        .add_tag_attributes("a", &["href", "name", "target"])
        .add_tag_attributes("td", &["nowrap"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "tel", "cid", "data"]))
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            let v = value.trim_start().to_ascii_lowercase();
            if REMOTE_URL_ELEMENTS.contains(&element) && REMOTE_URL_ATTRS.contains(&attribute) && v.contains("http") {
                flag.store(true, Ordering::Relaxed);
            }
            if attribute == "style" && css_loads_remote(value) {
                flag.store(true, Ordering::Relaxed);
            }
            // data: URLs only for inline images, never as link targets.
            if v.starts_with("data:") && !(element == "img" && attribute == "src" && v.starts_with("data:image/")) {
                return None;
            }
            Some(Cow::Borrowed(value))
        })
        .clean(html)
        .to_string();

    Sanitized { html: clean, has_remote_content: remote.load(Ordering::Relaxed) }
}

/// `url(http…)` / `@import` in inline or embedded CSS.
fn css_loads_remote(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    let squashed: String = lower.split_whitespace().collect();
    squashed.contains("url(http")
        || squashed.contains("url('http")
        || squashed.contains("url(\"http")
        || squashed.contains("url(//")
        || squashed.contains("@import")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_active_content_keeps_layout() {
        let s = sanitize(
            r##"<html><head><title>T</title><style>p{color:red}</style><script>alert(1)</script></head>
               <body onload="x()"><table bgcolor="#fff" cellpadding="4"><tr><td dir="rtl" style="color:blue">שלום</td></tr></table>
               <a href="javascript:alert(1)">bad</a><a href="https://ok.com">ok</a>
               <iframe src="https://evil"></iframe><form><input value=1></form></body></html>"##,
        );
        let h = &s.html;
        assert!(!h.contains("script") && !h.contains("alert") && !h.contains("onload") && !h.contains("iframe"));
        assert!(!h.contains(">T<"), "title text must not leak: {h}");
        assert!(h.contains("<style>p{color:red}</style>"), "{h}");
        assert!(h.contains(r##"dir="rtl""##) && h.contains(r##"bgcolor="#fff""##) && h.contains("שלום"));
        assert!(h.contains(r##"href="https://ok.com""##) && h.contains("noopener"));
        assert!(!h.contains("<input"));
        assert!(!s.has_remote_content, "{h}");
    }

    #[test]
    fn detects_remote_content() {
        assert!(sanitize(r##"<img src="https://t.co/pixel.gif">"##).has_remote_content);
        assert!(sanitize(r##"<table><tr><td background="http://x/bg.png">x</td></tr></table>"##).has_remote_content);
        assert!(sanitize(r##"<div style="background: URL( 'https://x/a.png')">"##).has_remote_content);
        assert!(sanitize(r##"<style>@import "https://x/a.css";</style>"##).has_remote_content);
        let local = sanitize(r##"<img src="cid:logo@x"><img src="data:image/png;base64,AAAA">"##);
        assert!(!local.has_remote_content);
        assert!(local.html.contains("cid:logo@x") && local.html.contains("data:image/png"));
    }

    #[test]
    fn data_urls_only_for_images() {
        let s = sanitize(r##"<a href="data:text/html,<script>x</script>">x</a>"##);
        assert!(!s.html.contains("data:"));
    }
}
