//! Search queries: Gmail-style operators parsed once, run locally (store) and
//! translated for server search (Gmail `q`, Graph KQL `$search`).

use chrono::NaiveDate;

use crate::model::FolderRole;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchQuery {
    /// Free-text terms (full-text, prefix-matched).
    pub text: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub has_attachment: bool,
    /// `Some(true)` = is:unread, `Some(false)` = is:read.
    pub unread: Option<bool>,
    pub flagged: bool,
    pub role: Option<FolderRole>,
    /// Matches account ids/addresses containing this.
    pub account: Option<String>,
    /// Epoch ms, exclusive upper bound.
    pub before: Option<i64>,
    /// Epoch ms, inclusive lower bound.
    pub after: Option<i64>,
}

impl SearchQuery {
    pub fn is_empty(&self) -> bool {
        *self == SearchQuery::default()
    }
}

/// Split on whitespace, keeping `"quoted phrases"` (also after `op:`) whole.
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn date_ms(s: &str) -> Option<i64> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").or_else(|_| NaiveDate::parse_from_str(s, "%Y/%m/%d")).ok()?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

pub fn parse(input: &str) -> SearchQuery {
    let mut q = SearchQuery::default();
    for tok in tokens(input) {
        let Some((op, val)) = tok.split_once(':').filter(|(op, v)| !op.is_empty() && !v.is_empty()) else {
            q.text.push(tok);
            continue;
        };
        let v = val.to_lowercase();
        let handled = match op.to_lowercase().as_str() {
            "from" => {
                q.from.push(val.to_string());
                true
            }
            "to" | "cc" => {
                q.to.push(val.to_string());
                true
            }
            "has" if v.starts_with("attachment") => {
                q.has_attachment = true;
                true
            }
            "is" => match v.as_str() {
                "unread" => {
                    q.unread = Some(true);
                    true
                }
                "read" => {
                    q.unread = Some(false);
                    true
                }
                "flagged" | "starred" => {
                    q.flagged = true;
                    true
                }
                _ => false,
            },
            "in" => match FolderRole::parse(&v).or(if v == "deleted" { Some(FolderRole::Trash) } else { None }) {
                Some(r) => {
                    q.role = Some(r);
                    true
                }
                None => false,
            },
            "account" => {
                q.account = Some(v);
                true
            }
            "before" => date_ms(&v).map(|d| q.before = Some(d)).is_some(),
            "after" => date_ms(&v).map(|d| q.after = Some(d)).is_some(),
            _ => false,
        };
        if !handled {
            q.text.push(tok);
        }
    }
    q
}

fn quote(v: &str) -> String {
    let v = v.replace('"', "");
    if v.contains(char::is_whitespace) { format!("\"{v}\"") } else { v }
}

fn ymd(ms: i64, sep: &str) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format(&format!("%Y{sep}%m{sep}%d")).to_string())
        .unwrap_or_default()
}

/// Gmail `q` syntax.
pub fn to_gmail(q: &SearchQuery) -> String {
    let mut parts: Vec<String> = q.text.iter().map(|t| quote(t)).collect();
    parts.extend(q.from.iter().map(|f| format!("from:{}", quote(f))));
    parts.extend(q.to.iter().map(|t| format!("to:{}", quote(t))));
    if q.has_attachment {
        parts.push("has:attachment".into());
    }
    match q.unread {
        Some(true) => parts.push("is:unread".into()),
        Some(false) => parts.push("is:read".into()),
        None => {}
    }
    if q.flagged {
        parts.push("is:starred".into());
    }
    if let Some(r) = q.role {
        parts.push(format!("in:{}", if r == FolderRole::Starred { "starred" } else { r.as_str() }));
    }
    if let Some(b) = q.before {
        parts.push(format!("before:{}", ymd(b, "/")));
    }
    if let Some(a) = q.after {
        parts.push(format!("after:{}", ymd(a, "/")));
    }
    parts.join(" ")
}

/// Graph `$search` (KQL). Unread/flagged/folder aren't searchable there; the
/// local query re-applies them to whatever comes back.
pub fn to_graph(q: &SearchQuery) -> String {
    let mut parts: Vec<String> = q.text.iter().map(|t| quote(t)).collect();
    parts.extend(q.from.iter().map(|f| format!("from:{}", quote(f))));
    parts.extend(q.to.iter().map(|t| format!("to:{}", quote(t))));
    if q.has_attachment {
        parts.push("hasattachments:true".into());
    }
    if let Some(b) = q.before {
        parts.push(format!("received<{}", ymd(b, "-")));
    }
    if let Some(a) = q.after {
        parts.push(format!("received>={}", ymd(a, "-")));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operators_and_text() {
        let q = parse(
            r#"invoice from:"Dana Cohen" to:me@x.com has:attachment is:unread in:inbox account:work after:2026-01-02 שלום"#,
        );
        assert_eq!(q.text, ["invoice", "שלום"]);
        assert_eq!(q.from, ["Dana Cohen"]);
        assert_eq!(q.to, ["me@x.com"]);
        assert!(q.has_attachment && !q.flagged);
        assert_eq!(q.unread, Some(true));
        assert_eq!(q.role, Some(FolderRole::Inbox));
        assert_eq!(q.account.as_deref(), Some("work"));
        assert_eq!(q.after, Some(1_767_312_000_000));
    }

    #[test]
    fn unknown_operators_are_text() {
        let q = parse("re:hello is:weird https://x.com in:nowhere before:garbage");
        assert_eq!(q.text, ["re:hello", "is:weird", "https://x.com", "in:nowhere", "before:garbage"]);
        assert!(parse("   ").is_empty());
    }

    #[test]
    fn server_translations() {
        let q = parse(r#"deck from:"Priya Raman" has:attachment is:flagged before:2026-10-01"#);
        assert_eq!(to_gmail(&q), r#"deck from:"Priya Raman" has:attachment is:starred before:2026/10/01"#);
        assert_eq!(to_graph(&q), r#"deck from:"Priya Raman" hasattachments:true received<2026-10-01"#);
    }
}
