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
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
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
