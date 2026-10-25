//! Search-box autocomplete: what the word at the caret is completing (an
//! operator name, or an operator's value), the fixed suggestions for it, and
//! the text after a pick. Offsets are byte offsets into the query; the UI
//! converts from the input's UTF-16 caret with [`utf16_to_byte`].

use std::ops::Range;

use chrono::{Days, Months, NaiveDate};

use crate::model::Folder;
use crate::search::quote;

/// The operators that take a value (or a fixed word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    From,
    To,
    Has,
    Is,
    In,
    Account,
    Before,
    After,
}

impl Op {
    fn parse(name: &str) -> Option<Op> {
        Some(match name.to_lowercase().as_str() {
            "from" => Op::From,
            "to" | "cc" => Op::To,
            "has" => Op::Has,
            "is" => Op::Is,
            "in" => Op::In,
            "account" => Op::Account,
            "before" => Op::Before,
            "after" => Op::After,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A bare word that may become an operator (empty between words).
    Operator(String),
    /// The value typed after `op:` so far, quotes removed.
    Value(Op, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub target: Target,
    /// What a pick replaces: the bare word, or the whole value after the colon.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Replaces the completion's range.
    pub insert: String,
    pub label: String,
    pub detail: String,
}

impl Suggestion {
    fn new(insert: impl Into<String>, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Suggestion { insert: insert.into(), label: label.into(), detail: detail.into() }
    }
}

/// Byte spans of the query's words, split as `search::parse` splits them
/// (whitespace, except inside `"quotes"`).
fn spans(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if let Some(s) = start.take() {
                out.push(s..i);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(s..text.len());
    }
    out
}

/// What the caret is completing, if anything. Between words (or in an empty
/// box) that's an empty operator. A bare word completes only with the caret
/// at its end; an operator's value wherever the caret is in it, and a pick
/// replaces the whole value.
pub fn at(text: &str, caret: usize) -> Option<Completion> {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let Some(span) = spans(text).into_iter().find(|r| r.start <= caret && caret <= r.end) else {
        return Some(Completion { target: Target::Operator(String::new()), range: caret..caret });
    };
    let word = &text[span.clone()];
    match word.split_once(':') {
        None => (caret == span.end).then(|| Completion { target: Target::Operator(word.to_string()), range: span }),
        Some((name, value)) => {
            let colon = span.start + name.len() + 1;
            if caret < colon {
                return None;
            }
            let op = Op::parse(name)?;
            Some(Completion { target: Target::Value(op, value.replace('"', "")), range: colon..span.end })
        }
    }
}

/// The query after picking `insert` for `c`, and the caret (byte offset)
/// after it. An operator (`from:`) leaves the caret on its colon, ready for
/// the value; anything else is a finished word and gets a space after it.
pub fn apply(text: &str, c: &Completion, insert: &str) -> (String, usize) {
    let (head, tail) = (&text[..c.range.start], &text[c.range.end..]);
    let mut out = format!("{head}{insert}");
    let mut caret = out.len();
    if !insert.ends_with(':') {
        match tail.chars().next() {
            Some(ws) if ws.is_whitespace() => caret += ws.len_utf8(),
            _ => {
                out.push(' ');
                caret += 1;
            }
        }
    }
    out.push_str(tail);
    (out, caret)
}

/// An operator is still waiting for its value (`from:` while a contact is
/// being picked): the query isn't worth a server search yet.
pub fn has_pending_operator(text: &str) -> bool {
    spans(text).into_iter().any(|r| text[r].strip_suffix(':').is_some_and(|op| Op::parse(op).is_some()))
}

/// Operators with a one-line description each, in the order they're offered.
pub const OPERATORS: [(&str, &str); 9] = [
    ("from:", "sender name or address"),
    ("to:", "recipient name or address"),
    ("has:attachment", "messages with files"),
    ("is:unread", "not read yet"),
    ("is:flagged", "flagged or starred"),
    ("in:", "folder or label"),
    ("account:", "one account only"),
    ("after:", "on or after a date"),
    ("before:", "before a date"),
];

/// Operators starting with `partial` (all of them for an empty one).
pub fn operators(partial: &str) -> Vec<Suggestion> {
    let p = partial.to_lowercase();
    OPERATORS
        .iter()
        .filter(|(op, _)| op.starts_with(&p) && op.len() > p.len())
        .map(|(op, d)| Suggestion::new(*op, *op, *d))
        .collect()
}

fn word_prefix(hay: &str, p: &str) -> bool {
    hay.to_lowercase().split_whitespace().any(|w| w.starts_with(p))
}

/// Suggestions for operators whose values don't depend on the mailbox:
/// `is:`, `has:`, and dates relative to `today` for `before:`/`after:`.
/// Values already typed out in full aren't offered again.
pub fn fixed_values(op: Op, partial: &str, today: NaiveDate) -> Vec<Suggestion> {
    let p = partial.trim().to_lowercase();
    let all: Vec<Suggestion> = match op {
        Op::Is => vec![
            Suggestion::new("unread", "unread", "not read yet"),
            Suggestion::new("read", "read", "already read"),
            Suggestion::new("flagged", "flagged", "flagged or starred"),
        ],
        Op::Has => vec![Suggestion::new("attachment", "attachment", "messages with files")],
        Op::Before | Op::After => [
            (Some(today), "today"),
            (today.checked_sub_days(Days::new(1)), "yesterday"),
            (today.checked_sub_days(Days::new(7)), "a week ago"),
            (today.checked_sub_months(Months::new(1)), "a month ago"),
        ]
        .into_iter()
        .filter_map(|(d, what)| d.map(|d| d.format("%Y-%m-%d").to_string()).map(|d| Suggestion::new(&d, &d, what)))
        .collect(),
        _ => Vec::new(),
    };
    all.into_iter().filter(|s| s.insert != p && (s.insert.starts_with(&p) || word_prefix(&s.detail, &p))).collect()
}

/// `in:` values: standard folders by role (once, however many accounts have
/// one), then other folders/labels by name, quoted when they have spaces.
/// `account` (the query's `account:` value) limits them to matching accounts.
pub fn folder_values<'a>(
    folders: impl IntoIterator<Item = (&'a str, &'a Folder)>,
    account: Option<&str>,
    partial: &str,
) -> Vec<Suggestion> {
    let p = partial.trim().to_lowercase();
    let account = account.map(str::to_lowercase);
    let mut out: Vec<Suggestion> = Vec::new();
    for (account_id, f) in folders {
        if account.as_ref().is_some_and(|a| !account_id.to_lowercase().contains(a.as_str())) {
            continue;
        }
        let s = match f.role {
            Some(r) => {
                let detail = if f.name.eq_ignore_ascii_case(r.as_str()) { String::new() } else { f.name.clone() };
                Suggestion::new(r.as_str(), r.as_str(), detail)
            }
            None => Suggestion::new(quote(&f.name), f.name.clone(), f.kind.as_str()),
        };
        let matches = s.label.to_lowercase().contains(&p) || s.detail.to_lowercase().contains(&p);
        if matches && s.label.to_lowercase() != p && !out.iter().any(|o| o.insert.eq_ignore_ascii_case(&s.insert)) {
            out.push(s);
        }
    }
    out
}

/// `account:` values from (address, label) pairs; the label is the
/// nickname, or the address when there's none.
pub fn account_values(accounts: &[(String, String)], partial: &str) -> Vec<Suggestion> {
    let p = partial.trim().to_lowercase();
    accounts
        .iter()
        .filter(|(email, label)| {
            let e = email.to_lowercase();
            e != p && (e.contains(&p) || label.to_lowercase().contains(&p))
        })
        .map(|(email, label)| {
            let detail = if label == email { String::new() } else { email.clone() };
            Suggestion::new(email.to_lowercase(), label.clone(), detail)
        })
        .collect()
}

/// A UTF-16 offset (what an `<input>`'s `selectionStart` counts) as a byte
/// offset into `text`.
pub fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if n >= units {
            return i;
        }
        n += c.len_utf16();
    }
    text.len()
}

pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text.char_indices().take_while(|(i, _)| *i < byte).map(|(_, c)| c.len_utf16()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FolderKind, FolderRole};

    fn target(text: &str, caret: usize) -> Option<Target> {
        at(text, caret).map(|c| c.target)
    }

    #[test]
    fn what_the_caret_completes() {
        assert_eq!(target("", 0), Some(Target::Operator(String::new())));
        assert_eq!(target("fr", 2), Some(Target::Operator("fr".into())));
        assert_eq!(target("invoice ", 8), Some(Target::Operator(String::new())));
        // Mid-word: not completing (a pick would eat the rest of the word).
        assert_eq!(target("fred", 2), None);
        assert_eq!(target("from:da", 7), Some(Target::Value(Op::From, "da".into())));
        assert_eq!(target("from:", 5), Some(Target::Value(Op::From, String::new())));
        assert_eq!(target("cc:x", 4), Some(Target::Value(Op::To, "x".into())));
        // Caret on the operator name, or an unknown operator / URL: nothing.
        assert_eq!(target("from:dana", 2), None);
        assert_eq!(target("https://x.com", 13), None);
    }

    #[test]
    fn caret_in_the_middle_of_the_query() {
        let text = "invoice from:da has:attachment";
        let c = at(text, 15).unwrap();
        assert_eq!(c.target, Target::Value(Op::From, "da".into()));
        assert_eq!(&text[c.range.clone()], "da");
        let (out, caret) = apply(text, &c, "dana@x.co.il");
        assert_eq!(out, "invoice from:dana@x.co.il has:attachment");
        // Onto the existing space, not a doubled one.
        assert_eq!(&out[caret..], "has:attachment");
        // Anywhere in the value replaces the whole value.
        let c = at(text, 14).unwrap();
        assert_eq!(c.range, 13..15);
    }

    #[test]
    fn quoted_values() {
        let text = r#"in:"Q4 Pl"#;
        let c = at(text, text.len()).unwrap();
        assert_eq!(c.target, Target::Value(Op::In, "Q4 Pl".into()));
        let (out, caret) = apply(text, &c, &quote("Q4 Planning"));
        assert_eq!(out, r#"in:"Q4 Planning" "#);
        assert_eq!(caret, out.len());
        assert_eq!(crate::search::parse(&out).folder.as_deref(), Some("Q4 Planning"));
        // Inside a closed quote too.
        let text = r#"from:"Dana Cohen" x"#;
        assert_eq!(target(text, 10), Some(Target::Value(Op::From, "Dana Cohen".into())));
    }

    #[test]
    fn hebrew_text() {
        let text = "שלום fr";
        let c = at(text, text.len()).unwrap();
        assert_eq!(c.target, Target::Operator("fr".into()));
        let (out, caret) = apply(text, &c, "from:");
        assert_eq!((out.as_str(), caret), ("שלום from:", out.len()));
        let c = at(&out, caret).unwrap();
        let (out, caret) = apply(&out, &c, "dana@x.co.il");
        assert_eq!(out, "שלום from:dana@x.co.il ");
        assert_eq!(caret, out.len());
        // A Hebrew value, caret before the trailing word.
        let text = "from:דנ קבלה";
        let caret = "from:דנ".len();
        assert_eq!(target(text, caret), Some(Target::Value(Op::From, "דנ".into())));
        // A caret inside a multi-byte char is clamped back to its start.
        assert_eq!(target("דנה", 3), None);
    }

    #[test]
    fn operator_picks() {
        let c = at("is", 2).unwrap();
        assert_eq!(apply("is", &c, "is:unread"), ("is:unread ".to_string(), 10));
        let c = at("invoice  x", 8).unwrap();
        assert_eq!(c.range, 8..8);
        assert_eq!(apply("invoice  x", &c, "from:"), ("invoice from: x".to_string(), 13));
    }

    #[test]
    fn pending_operators() {
        assert!(has_pending_operator("invoice from:"));
        assert!(has_pending_operator("IN: x"));
        assert!(!has_pending_operator("from:dana re: hello"));
        assert!(!has_pending_operator(""));
    }

    #[test]
    fn operator_suggestions() {
        let ops = |p: &str| operators(p).into_iter().map(|s| s.insert).collect::<Vec<_>>();
        assert_eq!(ops("").len(), OPERATORS.len());
        assert_eq!(ops("f"), ["from:"]);
        assert_eq!(ops("I"), ["is:unread", "is:flagged", "in:"]);
        assert_eq!(ops("a"), ["account:", "after:"]);
        assert!(ops("invoice").is_empty());
        assert!(ops("is:unread").is_empty());
    }

    #[test]
    fn fixed_value_suggestions() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let vals = |op, p: &str| fixed_values(op, p, today).into_iter().map(|s| s.insert).collect::<Vec<_>>();
        assert_eq!(vals(Op::Is, ""), ["unread", "read", "flagged"]);
        assert_eq!(vals(Op::Is, "un"), ["unread"]);
        assert!(vals(Op::Is, "unread").is_empty(), "already typed out");
        assert_eq!(vals(Op::Has, "a"), ["attachment"]);
        assert_eq!(vals(Op::After, ""), ["2026-03-31", "2026-03-30", "2026-03-24", "2026-02-28"]);
        assert_eq!(vals(Op::Before, "yes"), ["2026-03-30"]);
        assert_eq!(vals(Op::Before, "week"), ["2026-03-24"]);
        assert_eq!(vals(Op::After, "2026-02"), ["2026-02-28"]);
        // Every preset parses.
        for d in vals(Op::After, "") {
            assert!(crate::search::parse(&format!("after:{d}")).after.is_some());
        }
        assert!(vals(Op::From, "").is_empty());
    }

    #[test]
    fn folder_suggestions() {
        let f = |name: &str, role, kind| Folder { id: name.into(), name: name.into(), parent_id: None, role, kind };
        let ms = [
            f("Inbox", Some(FolderRole::Inbox), FolderKind::Folder),
            f("Sent Items", Some(FolderRole::Sent), FolderKind::Folder),
            f("Q4 Planning", None, FolderKind::Folder),
        ];
        let g = [f("INBOX", Some(FolderRole::Inbox), FolderKind::Label), f("קבלות", None, FolderKind::Label)];
        let all =
            || ms.iter().map(|x| ("microsoft:me@work.com", x)).chain(g.iter().map(|x| ("google:me@gmail.com", x)));
        let ins = |v: Vec<Suggestion>| v.into_iter().map(|s| s.insert).collect::<Vec<_>>();
        assert_eq!(ins(folder_values(all(), None, "")), ["inbox", "sent", "\"Q4 Planning\"", "קבלות"]);
        assert_eq!(folder_values(all(), None, "")[1].detail, "Sent Items");
        assert_eq!(ins(folder_values(all(), Some("gmail"), "")), ["inbox", "קבלות"]);
        assert_eq!(ins(folder_values(all(), None, "q4 p")), ["\"Q4 Planning\""]);
        assert_eq!(ins(folder_values(all(), None, "items")), ["sent"], "matches the folder's own name");
        assert_eq!(ins(folder_values(all(), None, "קב")), ["קבלות"]);
        assert!(folder_values(all(), None, "inbox").is_empty(), "already typed out");
    }

    #[test]
    fn account_suggestions() {
        let accounts =
            vec![("Me@Work.com".to_string(), "Work".to_string()), ("me@gmail.com".into(), "me@gmail.com".into())];
        let v = account_values(&accounts, "wo");
        assert_eq!(
            (v[0].insert.as_str(), v[0].label.as_str(), v[0].detail.as_str()),
            ("me@work.com", "Work", "Me@Work.com")
        );
        assert_eq!(account_values(&accounts, "").len(), 2);
        assert_eq!(account_values(&accounts, "gmail")[0].detail, "");
    }

    #[test]
    fn utf16_offsets() {
        let text = "שלום 📎 from:";
        // Hebrew letters are 1 unit / 2 bytes; the emoji 2 units / 4 bytes.
        assert_eq!(utf16_to_byte(text, 5), "שלום ".len());
        assert_eq!(utf16_to_byte(text, 7), "שלום 📎".len());
        assert_eq!(utf16_to_byte(text, 999), text.len());
        for b in [0, "שלום ".len(), "שלום 📎".len(), text.len()] {
            assert_eq!(utf16_to_byte(text, byte_to_utf16(text, b)), b);
        }
    }
}
