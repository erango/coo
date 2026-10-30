//! Quoted history in a message: the earlier messages a reply carries
//! (Gmail, Apple Mail, Outlook, and plain-text `>` quoting), found and
//! folded into one collapsible block. Each earlier message gets a small
//! "who and when" line in place of its "On …, … wrote:" line, and older
//! levels nest inside it. Pure: sanitized HTML (or text) in, the same out
//! with the quote restructured; when nothing is recognized the caller keeps
//! its original. The look is the mail frame's CSS (pigeon-bin mailframe.rs).
//!
//! The folded structure:
//!
//! ```html
//! <details class="coo-quoted" data-count="2">
//!   <summary class="coo-quoted__pill">…Quoted: 2 earlier messages…</summary>
//!   <div class="coo-q">
//!     <div class="coo-q__who"><span class="coo-q__av">Y</span><b>Yaron Frost</b><span>Fri, Oct 30 · 08:50</span></div>
//!     <div class="coo-q__body">… <div class="coo-q">…</div></div>
//!   </div>
//! </details>
//! ```
//!
//! Levels past the second sit in their own collapsed
//! `<details class="coo-quoted coo-quoted--deeper">` ("1 more quoted message").

use kuchikiki::NodeRef;
use kuchikiki::traits::TendrilSink;

/// Who wrote a quoted message, and when (shortened), from its attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub name: String,
    pub when: Option<String>,
}

impl Who {
    /// The avatar's letter.
    pub fn initial(&self) -> String {
        self.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default()
    }
}

/// "wrote:" in the languages mail clients write it in.
const WROTE: &[&str] = &[
    "wrote:",
    "wrote :",
    "schrieb:",
    "a écrit :",
    "a écrit:",
    "escribió:",
    "scrisse:",
    "escreveu:",
    "schreef:",
    "skrev:",
    "kirjoitti:",
    "napisał:",
    "написал:",
    "כתב:",
    "כתבה:",
    "כתב/ה:",
];

fn squash(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '\u{a0}').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Whether `text` reads like an "On …, X wrote:" line.
pub fn is_attribution(text: &str) -> bool {
    let t = squash(text);
    t.chars().count() < 400 && WROTE.iter().any(|w| t.ends_with(w))
}

/// Whether `text` is an Outlook reply header ("From: … Sent: …").
fn is_outlook_header(text: &str) -> bool {
    let t = squash(text);
    t.starts_with("From:") && (t.contains("Sent:") || t.contains("Date:")) && t.chars().count() < 800
}

/// The date shortened for a small label: no year, no zone, "at" as a dot.
fn short_when(s: &str) -> Option<String> {
    let mut t = s.trim().trim_end_matches(',').trim().to_string();
    for zone in [" UTC", " GMT"] {
        t = t.replace(zone, "");
    }
    t = t.replace(", at ", " · ").replace(" at ", " · ");
    // Drop a four-digit year (", 2026" or " 2026").
    let mut out = String::new();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let year = |at: usize| {
            at + 4 <= chars.len()
                && chars[at..at + 4].iter().all(|c| c.is_ascii_digit())
                && (chars[at] == '1' || chars[at] == '2')
                && chars.get(at + 4).is_none_or(|c| !c.is_ascii_digit())
        };
        if chars[i] == ',' && chars.get(i + 1) == Some(&' ') && year(i + 2) {
            i += 6;
            continue;
        }
        if chars[i] == ' ' && year(i + 1) && i > 0 && !chars[i - 1].is_ascii_digit() {
            i += 5;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    let out = squash(&out).trim_matches(|c: char| c == ',' || c == ' ').to_string();
    (!out.is_empty()).then_some(out)
}

/// Read "On Fri, Oct 30, 2026 at 5:47 AM Yaron Frost <y@x.io> wrote:" (and
/// Apple's and Coo's own shapes, and Outlook's "From: … Sent: …").
pub fn parse_attribution(text: &str) -> Option<Who> {
    let t = squash(text);
    if is_outlook_header(&t) {
        let field = |label: &str, ends: &[&str]| -> Option<String> {
            let start = t.find(label)? + label.len();
            let rest = &t[start..];
            let end = ends.iter().filter_map(|e| rest.find(e)).min().unwrap_or(rest.len());
            Some(rest[..end].trim().to_string())
        };
        let from = field("From:", &["Sent:", "Date:", "To:", "Cc:", "Subject:"])?;
        let name = from.split('<').next().unwrap_or("").trim().trim_matches('"').to_string();
        let name = if name.is_empty() { from.trim_matches(|c| c == '<' || c == '>').to_string() } else { name };
        let when = field("Sent:", &["To:", "Cc:", "Subject:"])
            .or_else(|| field("Date:", &["To:", "Cc:", "Subject:"]))
            .and_then(|w| short_when(&w));
        return Some(Who { name, when });
    }
    let marker = WROTE.iter().find(|w| t.ends_with(*w))?;
    let mut head = t[..t.len() - marker.len()].trim().trim_end_matches(',').trim();
    for on in ["On ", "Le ", "Am ", "El ", "Il ", "Em ", "Op ", "Den "] {
        if let Some(rest) = head.strip_prefix(on) {
            head = rest;
            break;
        }
    }
    // "… Name <addr>": the name is what precedes the address, after the date.
    let (before, email) = match (head.rfind('<'), head.rfind('>')) {
        (Some(lt), Some(gt)) if gt > lt => (head[..lt].trim(), Some(head[lt + 1..gt].trim())),
        _ => (head, None),
    };
    let (date, name) = match before.rfind(", ") {
        Some(i) => (&before[..i], before[i + 2..].trim()),
        None => ("", before),
    };
    // Gmail has no comma before the name: "…, 2026 at 5:47 AM Yaron Frost".
    let words: Vec<&str> = name.split(' ').collect();
    let timeish = |w: &str| {
        w.contains(':')
            || ["AM", "PM", "am", "pm", "UTC", "GMT"].contains(&w)
            || w.chars().all(|c| c.is_ascii_digit())
            || w == "at"
    };
    let lead = words.iter().take_while(|w| timeish(w)).count();
    let (date, name) = if lead > 0 {
        (format!("{date}, {}", words[..lead].join(" ")), words[lead..].join(" "))
    } else {
        (date.to_string(), name.to_string())
    };
    let name = name.trim().trim_matches('"').to_string();
    let name = if name.is_empty() { email.unwrap_or_default().to_string() } else { name };
    if name.is_empty() {
        return None;
    }
    Some(Who { name, when: short_when(&date) })
}

// ---- DOM helpers ----

type Handle = NodeRef;

fn tag(n: &Handle) -> Option<String> {
    n.as_element().map(|e| e.name.local.to_string())
}

fn attr(n: &Handle, key: &str) -> Option<String> {
    n.as_element().and_then(|e| e.attributes.borrow().get(key).map(str::to_string))
}

fn has_class(n: &Handle, class: &str) -> bool {
    attr(n, "class").is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

/// Text with `<br>` as line breaks (attribution lines are often wrapped).
fn text_of(n: &Handle) -> String {
    let mut out = String::new();
    fn walk(n: &Handle, out: &mut String) {
        if let Some(t) = n.as_text() {
            out.push_str(&t.borrow());
            return;
        }
        match tag(n).as_deref() {
            Some("br") => out.push('\n'),
            Some("style" | "script") => {}
            _ => {
                for c in n.children() {
                    walk(&c, out);
                }
            }
        }
    }
    walk(n, &mut out);
    out
}

fn is_blank(n: &Handle) -> bool {
    if let Some(t) = n.as_text() {
        return t.borrow().chars().all(|c| c.is_whitespace() || c == '\u{a0}');
    }
    n.as_comment().is_some() || tag(n).as_deref() == Some("br")
}

/// A new element from a snippet of our own markup.
fn make(html: &str) -> Handle {
    let doc = kuchikiki::parse_html().one(html).document_node;
    let first = doc.select_first("body > *").map(|n| n.as_node().clone()).expect("snippet");
    first.detach();
    first
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The nearest earlier sibling that isn't blank (whitespace, `<br>`).
fn prev_content_sibling(n: &Handle) -> Option<Handle> {
    let mut p = n.previous_sibling();
    while let Some(s) = p {
        if !is_blank(&s) {
            return Some(s);
        }
        p = s.previous_sibling();
    }
    None
}

// ---- Finding a quote ----

/// One recognized earlier message: where it goes, what to drop (its
/// attribution and wrappers), and what it quoted.
struct Found {
    anchor: Handle,
    drop: Vec<Handle>,
    who: Option<Who>,
    content: Vec<Handle>,
}

fn find(container: &Handle) -> Option<Found> {
    let kids: Vec<Handle> = container.children().collect();
    for n in kids {
        let Some(name) = tag(&n) else { continue };
        if matches!(name.as_str(), "style" | "script" | "head" | "title") {
            continue;
        }
        if let Some(f) = recognize(&n, &name) {
            return Some(f);
        }
        if let Some(f) = find(&n) {
            return Some(f);
        }
    }
    None
}

fn recognize(n: &Handle, name: &str) -> Option<Found> {
    // Gmail: <div class="gmail_quote"> holding the attribution
    // (div.gmail_attr) and the <blockquote class="gmail_quote">.
    if name != "blockquote" && has_class(n, "gmail_quote") {
        let kids: Vec<Handle> = n.children().collect();
        let attr_node = kids.iter().find(|c| has_class(c, "gmail_attr")).cloned();
        let who = attr_node.as_ref().and_then(|a| parse_attribution(&text_of(a)));
        let mut content = Vec::new();
        for c in kids {
            if attr_node.as_ref().is_some_and(|a| *a == c) {
                continue;
            }
            if tag(&c).as_deref() == Some("blockquote") {
                content.extend(c.children());
            } else if !is_blank(&c) {
                content.push(c);
            }
        }
        return Some(Found { anchor: n.clone(), drop: vec![n.clone()], who, content });
    }
    // A blockquote right after an "On …, X wrote:" line (Apple Mail, iOS,
    // older Gmail, and Coo's own replies), or Gmail's own blockquote.
    if name == "blockquote" {
        let prev = prev_content_sibling(n);
        let prev_attr = prev.filter(|p| is_attribution(&text_of(p)));
        if prev_attr.is_some() || has_class(n, "gmail_quote") || attr(n, "type").as_deref() == Some("cite") {
            let who = prev_attr.as_ref().and_then(|p| parse_attribution(&text_of(p)));
            let mut drop = vec![n.clone()];
            drop.extend(prev_attr);
            let content = n.children().collect();
            return Some(Found { anchor: n.clone(), drop, who, content });
        }
        return None;
    }
    // Outlook: "From: … Sent: …" (id divRplyFwdMsg on the web; a bordered
    // div on the desktop), and everything after it is the earlier message.
    if attr(n, "id").as_deref() == Some("divRplyFwdMsg")
        || (matches!(name, "div" | "p") && is_outlook_header(&text_of(n)))
    {
        // The header may be wrapped in single-child divs: the quote is what
        // follows the outermost of them.
        let mut head = n.clone();
        while let Some(p) = head.parent() {
            let only = p.children().filter(|c| !is_blank(c)).count() == 1;
            if only && tag(&p).is_some_and(|t| matches!(t.as_str(), "div" | "p" | "font")) {
                head = p;
            } else {
                break;
            }
        }
        let content: Vec<Handle> = head.following_siblings().collect();
        if content.iter().all(is_blank) {
            return None;
        }
        let mut drop = vec![head.clone()];
        if let Some(hr) = prev_content_sibling(&head).filter(|h| tag(h).as_deref() == Some("hr")) {
            drop.push(hr);
        }
        return Some(Found { anchor: head, drop, who: parse_attribution(&text_of(n)), content });
    }
    None
}

// ---- Folding ----

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { one.to_string() } else { format!("{n} {many}") }
}

fn summary_html(more: &str, less: &str) -> String {
    format!(
        "<summary class=\"coo-quoted__pill\"><span class=\"coo-quoted__dots\" aria-hidden=\"true\"></span>\
         <span class=\"coo-quoted__more\">{}</span><span class=\"coo-quoted__less\">{}</span></summary>",
        esc(more),
        esc(less)
    )
}

fn who_html(who: &Who) -> String {
    let when = who.when.as_deref().map(|w| format!("<span>{}</span>", esc(w))).unwrap_or_default();
    format!(
        "<div class=\"coo-q__who\"><span class=\"coo-q__av\" aria-hidden=\"true\">{}</span><b dir=\"auto\">{}</b>{when}</div>",
        esc(&who.initial()),
        esc(&who.name)
    )
}

/// Fold the first quote in `container` (and, inside it, the ones it
/// quotes). Returns how many earlier messages were folded.
fn fold(container: &Handle, level: usize) -> usize {
    let Some(found) = find(container) else { return 0 };
    let who = found.who.as_ref().map(who_html).unwrap_or_default();
    let block = make(&format!("<div class=\"coo-q\">{who}<div class=\"coo-q__body\"></div></div>"));
    let body = block.select_first(".coo-q__body").map(|n| n.as_node().clone()).expect("body");
    // The block takes the quote's place; then its content moves in.
    found.anchor.insert_before(block.clone());
    for c in found.content {
        body.append(c);
    }
    for d in &found.drop {
        d.detach();
    }
    let inner = fold(&body, level + 1);
    let total = 1 + inner;
    let wrap = match level {
        1 => {
            let more = if total == 1 && found.who.is_none() {
                "Quoted text".to_string()
            } else {
                format!("Quoted: {}", plural(total, "1 earlier message", "earlier messages"))
            };
            Some(format!(
                "<details class=\"coo-quoted\" data-count=\"{total}\">{}</details>",
                summary_html(&more, "Hide quoted text")
            ))
        }
        3 => Some(format!(
            "<details class=\"coo-quoted coo-quoted--deeper\">{}</details>",
            summary_html(&plural(total, "1 more quoted message", "more quoted messages"), "Hide")
        )),
        _ => None,
    };
    if let Some(w) = wrap {
        let d = make(&w);
        block.insert_before(d.clone());
        d.append(block);
    }
    total
}

/// `html` with its quoted history folded, and how many earlier messages it
/// holds; None when no quote is recognized (show the original then).
pub fn fold_html(html: &str) -> Option<(String, usize)> {
    let lower = html.to_ascii_lowercase();
    // Cheap pre-check: nothing to do without one of the shapes we know.
    if !["gmail_quote", "<blockquote", "divrplyfwdmsg", "from:"].iter().any(|k| lower.contains(k)) {
        return None;
    }
    let doc = kuchikiki::parse_html().one(html).document_node;
    let body = doc.select_first("body").ok()?.as_node().clone();
    let n = fold(&body, 1);
    if n == 0 {
        return None;
    }
    let mut out = String::new();
    if let Ok(head) = doc.select_first("head") {
        for c in head.as_node().children() {
            out.push_str(&c.to_string());
        }
    }
    for c in body.children() {
        out.push_str(&c.to_string());
    }
    Some((out, n))
}

// ---- Plain text ----

/// A quoted message in a plain-text body: its attribution, its own lines,
/// and the message it quoted in turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQuote {
    pub who: Option<Who>,
    pub lines: Vec<String>,
    pub inner: Option<Box<TextQuote>>,
}

impl TextQuote {
    /// Earlier messages in this chain.
    pub fn depth(&self) -> usize {
        1 + self.inner.as_ref().map_or(0, |q| q.depth())
    }
}

/// Split a plain-text body at its `>`-quoted history: the text before it,
/// and the quote chain. None when there's no quoted block.
pub fn split_text(text: &str) -> Option<(String, TextQuote)> {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|l| l.trim_start().starts_with('>'))?;
    let last = lines.iter().rposition(|l| l.trim_start().starts_with('>'))?;
    // The attribution: the line before, or the two before joined (Gmail wraps).
    let prev: Vec<usize> = (0..first).rev().filter(|&i| !lines[i].trim().is_empty()).take(2).collect();
    let joined = |a: usize, b: usize| format!("{} {}", lines[b], lines[a]);
    let (start, who) = match prev.as_slice() {
        [a, ..] if is_attribution(lines[*a]) && parse_attribution(lines[*a]).is_some() => {
            (*a, parse_attribution(lines[*a]))
        }
        [a, b, ..] if is_attribution(&joined(*a, *b)) && parse_attribution(&joined(*a, *b)).is_some() => {
            (*b, parse_attribution(&joined(*a, *b)))
        }
        [a, ..] if is_attribution(lines[*a]) => (*a, None),
        _ => (first, None),
    };
    let main = lines[..start].join("\n").trim_end().to_string();
    let quoted: Vec<String> = lines[first..=last]
        .iter()
        .map(|l| {
            let l = l.trim_start();
            let l = l.strip_prefix('>').unwrap_or(l);
            l.strip_prefix(' ').unwrap_or(l).to_string()
        })
        .collect();
    let inner_text = quoted.join("\n");
    let quote = match split_text(&inner_text) {
        Some((m, q)) => TextQuote { who, lines: m.lines().map(str::to_string).collect(), inner: Some(Box::new(q)) },
        None => TextQuote { who, lines: quoted, inner: None },
    };
    Some((main, quote))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(name: &str, when: &str) -> Option<Who> {
        Some(Who { name: name.into(), when: Some(when.into()) })
    }

    #[test]
    fn attributions_read_in_every_shape() {
        assert_eq!(
            parse_attribution("On Fri, Oct 30, 2026 at 5:47\u{202f}AM Yaron Frost <yaron@example.com> wrote:"),
            who("Yaron Frost", "Fri, Oct 30 · 5:47 AM")
        );
        assert_eq!(
            parse_attribution("On Fri, Oct 30, 2026 at 08:50 UTC, Yaron Frost <yaron@example.com> wrote:"),
            who("Yaron Frost", "Fri, Oct 30 · 08:50")
        );
        assert_eq!(
            parse_attribution("On Oct 30, 2026, at 08:50, Dana Cohen <dana@example.com> wrote:"),
            who("Dana Cohen", "Oct 30 · 08:50")
        );
        assert_eq!(
            parse_attribution(
                "From: Ava Chen <ava@example.com>\nSent: Friday, October 30, 2026 8:50 AM\nTo: Sam\nSubject: Re: x"
            ),
            who("Ava Chen", "Friday, October 30 8:50 AM")
        );
        assert_eq!(parse_attribution("On Tue, Ann wrote:").map(|w| w.name), Some("Ann".into()));
        assert!(parse_attribution("Thanks, see you then.").is_none());
        assert_eq!(Who { name: "yaron".into(), when: None }.initial(), "Y");
    }

    #[test]
    fn gmail_nested_quotes_fold_with_attributions() {
        let html = r#"<div dir="ltr">Thanks!</div><br><div class="gmail_quote"><div dir="ltr" class="gmail_attr">On Fri, Oct 30, 2026 at 08:50 Yaron Frost &lt;y@example.com&gt; wrote:<br></div><blockquote class="gmail_quote" style="border-left:1px solid #ccc;padding-left:1ex">Hi Eran,<br><div class="gmail_quote"><div class="gmail_attr">On Thu, Oct 29, 2026 at 5:47 PM Eran Goldin &lt;e@example.com&gt; wrote:<br></div><blockquote class="gmail_quote">Earlier words</blockquote></div></blockquote></div>"#;
        let (out, n) = fold_html(html).expect("folded");
        assert_eq!(n, 2);
        assert!(out.starts_with(r#"<div dir="ltr">Thanks!</div>"#));
        assert!(out.contains(r#"<details class="coo-quoted" data-count="2">"#));
        assert!(out.contains("Quoted: 2 earlier messages"));
        assert!(out.contains("<b dir=\"auto\">Yaron Frost</b><span>Fri, Oct 30 · 08:50</span>"));
        assert!(out.contains("<b dir=\"auto\">Eran Goldin</b>"));
        assert!(out.contains("Earlier words"));
        assert!(!out.contains("wrote:"), "attribution lines are replaced: {out}");
        assert!(!out.contains("blockquote"), "the quote's own border goes: {out}");
        // The second level nests inside the first.
        let first = out.find("Yaron Frost").unwrap();
        let second = out.find("Eran Goldin").unwrap();
        assert!(first < second);
    }

    #[test]
    fn apple_and_coo_blockquotes_fold() {
        let html = "<p>Sounds good.</p><div>On Oct 30, 2026, at 08:50, Dana Cohen &lt;d@example.com&gt; wrote:</div><blockquote style=\"margin:0 0 0 .8ex\"><div>Can we meet at 3?</div></blockquote>";
        let (out, n) = fold_html(html).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("Quoted: 1 earlier message"));
        assert!(out.contains("Dana Cohen") && out.contains("Can we meet at 3?"));
        assert!(!out.contains("wrote:"));
    }

    #[test]
    fn outlook_reply_header_folds_what_follows() {
        let html = r#"<div>See below.</div><hr style="display:inline-block;width:98%"><div id="divRplyFwdMsg" dir="ltr"><font><b>From:</b> Ava Chen &lt;a@example.com&gt;<br><b>Sent:</b> Friday, October 30, 2026 8:50 AM<br><b>To:</b> Sam<br><b>Subject:</b> Flights</font><div>&nbsp;</div></div><div>Booked the 7:40.</div>"#;
        let (out, n) = fold_html(html).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("<b dir=\"auto\">Ava Chen</b>"));
        assert!(out.contains("Booked the 7:40."));
        assert!(!out.contains("<hr") && !out.contains("divRplyFwdMsg"));
        assert!(out.find("See below.").unwrap() < out.find("coo-quoted").unwrap());
    }

    #[test]
    fn deep_chains_collapse_past_the_second_level() {
        let mut html = String::from("Top");
        for i in 0..4 {
            html.push_str(&format!(
                "<div>On Mon, Oct {}, 2026 at 9:00 AM P{i} &lt;p@example.com&gt; wrote:</div><blockquote>level {i}",
                20 + i
            ));
        }
        html.push_str(&"</blockquote>".repeat(4));
        let (out, n) = fold_html(&html).unwrap();
        assert_eq!(n, 4);
        assert!(out.contains("coo-quoted--deeper"));
        assert!(out.contains("2 more quoted messages"));
    }

    #[test]
    fn plain_quotes_and_unknown_html_are_left_alone() {
        assert!(fold_html("<p>Hello</p>").is_none());
        // A blockquote used for emphasis, with no attribution, isn't history.
        assert!(fold_html("<p>As they say:</p><blockquote>Less is more.</blockquote>").is_none());
    }

    #[test]
    fn text_quotes_split_into_levels() {
        let text = "Thanks!\n\nOn Fri, Oct 30, 2026 at 08:50 Yaron Frost <y@example.com>\nwrote:\n> Hi Eran,\n> device: iPhone 13\n>\n> On Thu, Oct 29, 2026 at 5:47 PM Eran <e@example.com> wrote:\n>> Earlier\n";
        let (main, q) = split_text(text).unwrap();
        assert_eq!(main, "Thanks!");
        assert_eq!(q.depth(), 2);
        assert_eq!(q.who.as_ref().unwrap().name, "Yaron Frost");
        assert_eq!(q.lines.first().map(String::as_str), Some("Hi Eran,"));
        let inner = q.inner.unwrap();
        assert_eq!(inner.who.unwrap().name, "Eran");
        assert_eq!(inner.lines, ["Earlier"]);
        assert!(split_text("No quotes here").is_none());
    }
}
