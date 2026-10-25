//! Small text helpers shared by providers.

use crate::model::Address;

/// Parse an RFC 5322 address list header value (`"A, B" <a@x>, b@y`).
/// Lenient: anything without an `@` is dropped.
pub fn parse_address_list(value: &str) -> Vec<Address> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_quotes, mut in_angle) = (false, false);
    for ch in value.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '<' if !in_quotes => in_angle = true,
            '>' if !in_quotes => in_angle = false,
            ',' if !in_quotes && !in_angle => {
                out.extend(parse_address(&cur));
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    out.extend(parse_address(&cur));
    out
}

fn parse_address(part: &str) -> Option<Address> {
    let part = part.trim();
    let (name, email) = match (part.rfind('<'), part.rfind('>')) {
        (Some(l), Some(r)) if l < r => (Some(part[..l].trim().trim_matches('"').trim()), part[l + 1..r].trim()),
        _ => (None, part.trim_matches('"')),
    };
    email.contains('@').then(|| Address::new(name, email))
}

/// Decode the handful of HTML entities providers put in plain-text snippets.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // Search bytes: `;` is ASCII, so its index is always a char boundary
        // (slicing the str at a fixed byte offset is not — Hebrew is 2 bytes).
        let Some(end) = rest.as_bytes().iter().take(12).position(|&b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Cut `s` to at most `max_bytes`, backing off to a char boundary.
pub fn truncate_utf8(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut cut = max_bytes;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
}

/// One-line preview text: invisible formatting characters dropped (marketing
/// mail pads its preheader with zero-width joiners, soft hyphens and the like,
/// which otherwise overflow the row and leave a stray ellipsis), whitespace
/// runs collapsed to one space, ends trimmed.
pub fn preview_text(s: &str) -> String {
    let invisible = |c: char| {
        matches!(c,
            '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}'
            | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}'
            | '\u{2800}' | '\u{3164}' | '\u{FEFF}' | '\u{FFA0}')
    };
    let mut out = String::with_capacity(s.len());
    for word in s.split(|c: char| c.is_whitespace()) {
        let word: String = word.chars().filter(|&c| !invisible(c)).collect();
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out
}

/// Turn free text into a safe FTS5 query: every term becomes a quoted prefix
/// match, all terms must match.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Plain text to composer HTML: one auto-direction block per line, so mixed
/// Hebrew/English paragraphs each pick their own direction.
pub fn text_to_html(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.trim().is_empty() {
                "<div dir=\"auto\"><br></div>".to_string()
            } else {
                format!("<div dir=\"auto\">{}</div>", escape_html(l))
            }
        })
        .collect()
}

/// Rough HTML to text for the text/plain alternative and previews.
pub fn html_to_text(html: &str) -> String {
    const BREAKS: [&str; 10] = ["br", "/p", "/div", "/li", "/tr", "/h1", "/h2", "/h3", "/blockquote", "/table"];
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut skip_until: Option<&str> = None;
    while let Some(lt) = rest.find('<') {
        if skip_until.is_none() {
            out.push_str(&rest[..lt]);
        }
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = rest[lt + 1..lt + gt].trim().to_ascii_lowercase();
        let name = tag.split(|c: char| c.is_whitespace() || c == '/' && !tag.starts_with('/')).next().unwrap_or("");
        match skip_until {
            Some(end) if name == end => skip_until = None,
            Some(_) => {}
            None if name == "style" || name == "script" || name == "head" => {
                skip_until = Some(match name {
                    "style" => "/style",
                    "script" => "/script",
                    _ => "/head",
                })
            }
            None if BREAKS.contains(&name) => out.push('\n'),
            None => {}
        }
        rest = &rest[lt + gt + 1..];
    }
    if skip_until.is_none() {
        out.push_str(rest);
    }
    let text = decode_entities(&out);
    // Collapse runs of blank lines.
    let mut lines: Vec<&str> = Vec::new();
    for l in text.lines().map(str::trim_end) {
        if l.trim().is_empty() && lines.last().is_some_and(|p| p.trim().is_empty()) {
            continue;
        }
        lines.push(l);
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_drops_preheader_padding() {
        let zoom = "Download, test, host - your Zoom journey begins. \u{200C}\u{00A0}\u{034F} \u{200C}\u{00A0}\u{FEFF}\u{00AD} \u{2800}";
        assert_eq!(preview_text(zoom), "Download, test, host - your Zoom journey begins.");
        assert_eq!(preview_text("  a\n\tb\u{2007}c  "), "a b c");
        assert_eq!(preview_text("שלום עולם"), "שלום עולם");
        assert_eq!(preview_text("\u{200B}\u{200B}"), "");
    }

    #[test]
    fn address_list_handles_quotes_and_bare_addresses() {
        let got = parse_address_list(r#""Doe, Jane" <jane@x.com>, bob@y.org, Al <al@z.io>"#);
        assert_eq!(
            got,
            vec![
                Address::new(Some("Doe, Jane"), "jane@x.com"),
                Address::new(None, "bob@y.org"),
                Address::new(Some("Al"), "al@z.io"),
            ]
        );
    }

    #[test]
    fn address_list_drops_junk_and_redundant_names() {
        assert_eq!(parse_address_list("undisclosed-recipients:;"), vec![]);
        assert_eq!(parse_address_list("a@b.c <a@b.c>"), vec![Address::new(None, "a@b.c")]);
    }

    #[test]
    fn entities_decode() {
        assert_eq!(decode_entities("Tom &amp; Jerry&#39;s &lt;3 &#x263A; &bogus; &"), "Tom & Jerry's <3 ☺ &bogus; &");
    }

    #[test]
    fn entities_next_to_multibyte_text() {
        // Regression: sync panicked slicing a Gmail snippet mid-character.
        assert_eq!(decode_entities("&quot;דן תחבורה ציבורית&quot; שם"), "\"דן תחבורה ציבורית\" שם");
        assert_eq!(decode_entities("a &שלום; b"), "a &שלום; b");
        assert_eq!(decode_entities("&#1513;"), "ש");
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let mut s = "שלום".to_string(); // 8 bytes
        truncate_utf8(&mut s, 3);
        assert_eq!(s, "ש");
        let mut s = "abc".to_string();
        truncate_utf8(&mut s, 10);
        assert_eq!(s, "abc");
    }

    #[test]
    fn fts_query_quotes_terms() {
        assert_eq!(fts_query(r#"  inv"oice  acme "#).as_deref(), Some(r#""invoice"* "acme"*"#));
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn html_text_conversions() {
        assert_eq!(escape_html(r#"<a href="x">&'"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        assert_eq!(
            text_to_html("שלום\n\nhello <b>"),
            "<div dir=\"auto\">שלום</div><div dir=\"auto\"><br></div><div dir=\"auto\">hello &lt;b&gt;</div>"
        );
        assert_eq!(
            html_to_text(
                "<html><head><style>p{}</style></head><div dir=auto>שלום</div><div><br></div><div>a &amp; b</div><p>x<br>y</p>"
            ),
            "שלום\n\na & b\nx\ny"
        );
    }
}
