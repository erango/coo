//! Links in message text, for the reader to make clickable: web addresses
//! (with a scheme, `www.`, or a bare `tvtracker.online` whose ending is on
//! the public suffix list) and email addresses. And `mailto:` URLs, read
//! into the parts of a new message.

use std::ops::Range;

use crate::model::Address;
use crate::text::parse_address_list;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Where it goes: the text itself, `https://` added when it had no scheme.
    Web(String),
    /// An email address; a click starts a message to it.
    Email(String),
}

impl Link {
    pub fn href(&self) -> String {
        match self {
            Link::Web(url) => url.clone(),
            Link::Email(address) => format!("mailto:{address}"),
        }
    }
}

/// Text never part of a link: spaces, and what surrounds one in prose
/// or markup (`<…>`, quotes).
fn is_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '<' | '>' | '"' | '“' | '”' | '«' | '»' | '`')
}

/// Every link in `text`, at most one per space-separated word.
pub fn find(text: &str) -> Vec<(Range<usize>, Link)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let end = text[i..].find(is_break).map_or(text.len(), |e| i + e);
        if i < end
            && let Some((r, link)) = in_word(&text[i..end])
        {
            out.push((i + r.start..i + r.end, link));
        }
        i = end + text[end..].chars().next().map_or(0, char::len_utf8);
    }
    out
}

/// `text` as runs of plain text and links, in order.
pub fn split(text: &str) -> Vec<(&str, Option<Link>)> {
    let mut out = Vec::new();
    let mut at = 0;
    for (r, link) in find(text) {
        if at < r.start {
            out.push((&text[at..r.start], None));
        }
        out.push((&text[r.clone()], Some(link)));
        at = r.end;
    }
    if at < text.len() {
        out.push((&text[at..], None));
    }
    out
}

fn in_word(w: &str) -> Option<(Range<usize>, Link)> {
    // Same byte offsets as `w`: only ASCII letters change case.
    let lower = w.to_ascii_lowercase();
    let after_boundary = |p: usize| !w[..p].ends_with(|c: char| c.is_ascii_alphanumeric());
    for scheme in ["https://", "http://"] {
        if let Some(p) = lower.find(scheme).filter(|&p| after_boundary(p)) {
            let end = p + trim_end(&w[p..]);
            return (end > p + scheme.len()).then(|| (p..end, Link::Web(w[p..end].to_string())));
        }
    }
    if let Some(p) = lower.find("mailto:").filter(|&p| after_boundary(p)) {
        let at = p + "mailto:".len();
        let end = at + trim_end(&w[at..]);
        return is_email(&w[at..end]).then(|| (p..end, Link::Email(w[at..end].to_string())));
    }
    // Opening punctuation, or a Hebrew prefix letter, before the address.
    let start = w.find(|c: char| c.is_ascii_alphanumeric())?;
    let end = start + trim_end(&w[start..]);
    let t = &w[start..end];
    if t.contains('@') {
        return is_email(t).then(|| (start..end, Link::Email(t.to_string())));
    }
    let host = host_len(t)?;
    let rest = &t[host..];
    if !rest.is_empty() && !rest.starts_with(['/', '?', '#', ':']) {
        return None;
    }
    Some((start..end, Link::Web(format!("https://{t}"))))
}

fn is_email(t: &str) -> bool {
    let Some((local, domain)) = t.split_once('@') else { return false };
    !local.is_empty()
        && !local.starts_with('.')
        && local.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '\''))
        && host_len(domain) == Some(domain.len())
}

/// The length of the host name `t` starts with, when it is one: dotted
/// labels ending in a known public suffix, with a name before it.
/// "Hello.World" isn't a link: a capitalized ending reads as a sentence
/// run together, not an address.
fn host_len(t: &str) -> Option<usize> {
    let len = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(t.len());
    let host = &t[..len];
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || l.starts_with('-') || l.ends_with('-')) {
        return None;
    }
    let tld = labels[labels.len() - 1];
    let lower_tld = tld.chars().all(|c| c.is_ascii_lowercase());
    let upper_tld = tld.chars().all(|c| c.is_ascii_uppercase());
    if !(lower_tld || upper_tld) {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    psl::domain(lower.as_bytes()).filter(|d| d.suffix().is_known())?;
    Some(len)
}

/// Trailing characters that end a sentence rather than the link, and a
/// closing bracket the link didn't open (`(see x.com)`).
fn trim_end(s: &str) -> usize {
    let mut end = s.len();
    while let Some(c) = s[..end].chars().next_back() {
        let unopened = |open: char, close: char| s[..end].matches(open).count() < s[..end].matches(close).count();
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' => true,
            ')' => unopened('(', ')'),
            ']' => unopened('[', ']'),
            '}' => unopened('{', '}'),
            c => !c.is_ascii(),
        };
        if !drop {
            break;
        }
        end -= c.len_utf8();
    }
    end
}

/// A `mailto:` URL's parts (RFC 6068).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailto {
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub body: String,
}

pub fn parse_mailto(url: &str) -> Option<Mailto> {
    let rest = url.get(..7).filter(|s| s.eq_ignore_ascii_case("mailto:")).map(|_| &url[7..])?;
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut m = Mailto { to: parse_address_list(&decode(to)), ..Mailto::default() };
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k.to_ascii_lowercase().as_str() {
            "to" => m.to.extend(parse_address_list(&v)),
            "cc" => m.cc.extend(parse_address_list(&v)),
            "bcc" => m.bcc.extend(parse_address_list(&v)),
            "subject" => m.subject = v,
            "body" => m.body = v.replace("\r\n", "\n"),
            _ => {}
        }
    }
    Some(m)
}

/// Percent-decoding only: in a `mailto:` URL "+" is itself (`me+tag@x.io`).
fn decode(s: &str) -> String {
    String::from_utf8_lossy(&crate::images::percent_decode(s)).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<(&str, String)> {
        find(text).into_iter().map(|(r, l)| (&text[r], l.href())).collect()
    }

    #[test]
    fn finds_web_addresses_with_and_without_a_scheme() {
        assert_eq!(
            links("See https://example.com/a?b=1&c=2, www.Coo.app and tvtracker.online/shows."),
            [
                ("https://example.com/a?b=1&c=2", "https://example.com/a?b=1&c=2".to_string()),
                ("www.Coo.app", "https://www.Coo.app".to_string()),
                ("tvtracker.online/shows", "https://tvtracker.online/shows".to_string()),
            ]
        );
        assert_eq!(
            links("(http://x.io/wiki/A_(b))."),
            [("http://x.io/wiki/A_(b)", "http://x.io/wiki/A_(b)".to_string())]
        );
        assert_eq!(links("<https://x.io>"), [("https://x.io", "https://x.io".to_string())]);
        assert_eq!(
            links("mail.tvtracker.online:8080/x"),
            [("mail.tvtracker.online:8080/x", "https://mail.tvtracker.online:8080/x".to_string())]
        );
    }

    #[test]
    fn leaves_words_that_only_look_like_addresses() {
        for text in
            ["report.pdf", "e.g. this", "version 1.2.3", "Hello.World", "end.Next", "a.zzzunknown", "https://", "x@y"]
        {
            assert_eq!(links(text), [], "{text}");
        }
    }

    #[test]
    fn finds_email_addresses() {
        assert_eq!(
            links("Write to Me+Tag@tvtracker.online, or (mailto:b.c@x.co.il)."),
            [
                ("Me+Tag@tvtracker.online", "mailto:Me+Tag@tvtracker.online".to_string()),
                ("mailto:b.c@x.co.il", "mailto:b.c@x.co.il".to_string()),
            ]
        );
        // A Hebrew prefix letter ("to") stays outside the link.
        assert_eq!(links("לdan@x.io"), [("dan@x.io", "mailto:dan@x.io".to_string())]);
    }

    #[test]
    fn splits_text_around_links() {
        assert_eq!(split("a x.io b"), [("a ", None), ("x.io", Some(Link::Web("https://x.io".into()))), (" b", None)]);
    }

    #[test]
    fn reads_mailto_urls() {
        let m = parse_mailto("MAILTO:a@x.io,b+c@y.io?cc=d@z.io&Subject=Hi%20there&body=Line%201%0D%0ALine+2").unwrap();
        assert_eq!(m.to.iter().map(|a| a.email.as_str()).collect::<Vec<_>>(), ["a@x.io", "b+c@y.io"]);
        assert_eq!(m.cc[0].email, "d@z.io");
        assert_eq!(m.subject, "Hi there");
        assert_eq!(m.body, "Line 1\nLine+2");
        assert_eq!(parse_mailto("https://x.io"), None);
        assert_eq!(parse_mailto("mailto:").unwrap().to, []);
    }
}
