//! Gmail filters (`users.settings.filters`): Coo's mail rules saved on the
//! server, so they keep working with Coo closed. Filters can't be edited in
//! place; a changed rule is a delete plus a create.
//! <https://developers.google.com/gmail/api/reference/rest/v1/users.settings.filters>

use pigeon_core::http::{client, send, send_once};
use pigeon_core::model::Folder;
use pigeon_core::rules::{Place, ServerRule, Then, When};
use pigeon_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{GMAIL, Google};

/// System labels a filter can add that Coo has no action for.
const FOREIGN_ADD: &[&str] = &["IMPORTANT", "SPAM", "UNREAD", "INBOX", "SENT", "DRAFT", "CHAT"];

/// Quote a value with spaces so Gmail reads it as one phrase.
fn quote(v: &str) -> String {
    let v = v.trim();
    if v.chars().any(char::is_whitespace) && !(v.starts_with('"') && v.ends_with('"')) {
        format!("\"{}\"", v.replace('"', ""))
    } else {
        v.to_string()
    }
}

fn any_of(values: &[String]) -> Option<String> {
    let parts: Vec<String> = values.iter().map(|v| quote(v)).filter(|v| !v.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

/// The filter resource body (`{criteria, action}`) for a rule. `quiet` and
/// `needs_you` are Coo's own and never go to the server.
pub fn to_gmail_filter(when: &When, then: &Then) -> Value {
    let mut criteria = Map::new();
    for (key, values) in [("from", &when.from), ("to", &when.to), ("subject", &when.subject)] {
        if let Some(s) = any_of(values) {
            criteria.insert(key.into(), json!(s));
        }
    }
    if let Some(s) = any_of(&when.words) {
        criteria.insert("query".into(), json!(format!("({s})")));
    }
    if when.has_attachment {
        criteria.insert("hasAttachment".into(), json!(true));
    }

    let mut add: Vec<String> = Vec::new();
    let mut remove: Vec<String> = Vec::new();
    if then.archive {
        remove.push("INBOX".into());
    }
    if then.read {
        remove.push("UNREAD".into());
    }
    if then.star {
        add.push("STARRED".into());
    }
    if then.trash {
        add.push("TRASH".into());
    }
    if let Some(label) = &then.label {
        add.push(label.id.clone());
    }
    let mut action = Map::new();
    if !add.is_empty() {
        action.insert("addLabelIds".into(), json!(add));
    }
    if !remove.is_empty() {
        action.insert("removeLabelIds".into(), json!(remove));
    }
    json!({ "criteria": criteria, "action": action })
}

/// Split `a OR "b c" OR (d)` into its values: quotes and parentheses go,
/// a quoted phrase stays whole. Gmail's `{a b}` (any of) is read the same.
/// The bool is true when the text had something Coo can't represent
/// (AND, a negation, a search operator).
fn split_or(s: &str) -> (Vec<String>, bool) {
    let mut s = s.trim();
    let mut braces = false;
    while s.len() >= 2
        && ((s.starts_with('(') && s.ends_with(')')) || (s.starts_with('{') && s.ends_with('}')))
        && balanced(&s[1..s.len() - 1])
    {
        braces |= s.starts_with('{');
        s = s[1..s.len() - 1].trim();
    }
    // Tokens, keeping quoted phrases whole.
    let mut tokens: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '"' => {
                if in_quote {
                    tokens.push((std::mem::take(&mut cur), true));
                } else if !cur.trim().is_empty() {
                    tokens.push((std::mem::take(&mut cur), false));
                }
                cur.clear();
                in_quote = !in_quote;
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    tokens.push((std::mem::take(&mut cur), false));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        tokens.push((cur, in_quote));
    }

    let mut values = Vec::new();
    let mut extra = false;
    let mut last_was_value = false;
    for (tok, quoted) in tokens {
        if !quoted && (tok == "OR" || tok == "|") {
            last_was_value = false;
            continue;
        }
        let tok = if quoted { tok.trim().to_string() } else { tok.trim_matches(|c| c == '(' || c == ')').to_string() };
        if tok.is_empty() {
            continue;
        }
        if !quoted && (tok == "AND" || tok.starts_with('-') || tok.contains(':')) {
            extra = true;
        }
        // Two values in a row without OR mean "both" in Gmail, unless in braces.
        if last_was_value && !braces {
            extra = true;
        }
        values.push(tok);
        last_was_value = true;
    }
    (values, extra)
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Set to something other than its "unset" value.
fn present(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(_) => true,
    }
}

fn label_ids(v: &Value) -> impl Iterator<Item = &str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str)
}

/// A filter as a rule: for filters made in Gmail itself as well as Coo's.
/// None only when it has no id.
pub fn from_gmail_filter(v: &Value, labels: &[Folder]) -> Option<ServerRule> {
    let id = v.get("id")?.as_str().filter(|s| !s.is_empty())?.to_string();
    let mut when = When::default();
    let mut then = Then::default();
    let mut extra = false;

    if let Some(criteria) = v.get("criteria").and_then(Value::as_object) {
        for (key, value) in criteria {
            let text = value.as_str().unwrap_or_default();
            let target = match key.as_str() {
                "from" => &mut when.from,
                "to" => &mut when.to,
                "subject" => &mut when.subject,
                "query" => &mut when.words,
                "hasAttachment" => {
                    when.has_attachment = value.as_bool().unwrap_or(false);
                    continue;
                }
                _ => {
                    extra |= present(value);
                    continue;
                }
            };
            let (values, odd) = split_or(text);
            extra |= odd;
            *target = values;
        }
    }

    if let Some(action) = v.get("action").and_then(Value::as_object) {
        for (key, value) in action {
            match key.as_str() {
                "removeLabelIds" => {
                    for l in label_ids(value) {
                        match l {
                            "INBOX" => then.archive = true,
                            "UNREAD" => then.read = true,
                            _ => extra = true,
                        }
                    }
                }
                "addLabelIds" => {
                    for l in label_ids(value) {
                        match l {
                            "STARRED" => then.star = true,
                            "TRASH" => then.trash = true,
                            l if FOREIGN_ADD.contains(&l) || l.starts_with("CATEGORY_") => extra = true,
                            // Coo files into one label per rule.
                            _ if then.label.is_some() => extra = true,
                            l => {
                                let name = labels.iter().find(|f| f.id == l).map_or(l, |f| f.name.as_str());
                                then.label = Some(Place { id: l.to_string(), name: name.to_string() });
                            }
                        }
                    }
                }
                _ => extra |= present(value),
            }
        }
    }

    Some(ServerRule { id, name: String::new(), when, then, extra, enabled: true })
}

#[derive(Deserialize)]
struct FilterList {
    #[serde(default)]
    filter: Vec<Value>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

impl Google {
    /// Every filter on the account, raw (see `from_gmail_filter`).
    pub async fn gmail_filters(&self) -> Result<Vec<Value>> {
        let list: FilterList = self.get(&format!("{GMAIL}/settings/filters"), &[]).await?;
        Ok(list.filter)
    }

    /// Save a filter (from `to_gmail_filter`); returns its id.
    pub async fn create_gmail_filter(&self, filter: &Value) -> Result<String> {
        let token = self.tokens.access_token().await?;
        // Single attempt: a retried create could leave two filters.
        let req = client().post(format!("{GMAIL}/settings/filters")).bearer_auth(&token).json(filter);
        let created: Created = send_once(req).await?.json().await?;
        if created.id.is_empty() {
            return Err(Error::Other("Gmail saved the filter without an id".into()));
        }
        Ok(created.id)
    }

    /// Delete a filter; one that's already gone counts as done.
    pub async fn delete_gmail_filter(&self, id: &str) -> Result<()> {
        let token = self.tokens.access_token().await?;
        let url = format!("{GMAIL}/settings/filters/{id}");
        match send(|| client().delete(&url).bearer_auth(&token)).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::FolderKind;

    fn label(id: &str, name: &str) -> Folder {
        Folder { id: id.into(), name: name.into(), parent_id: None, role: None, kind: FolderKind::Label }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn builds_filter() {
        let when = When {
            from: s(&["dana@acme.com", "@news.io"]),
            to: s(&["team"]),
            subject: s(&["weekly report", "digest"]),
            words: s(&["invoice", "past due"]),
            has_attachment: true,
        };
        let then = Then {
            archive: true,
            read: true,
            star: true,
            label: Some(Place { id: "Label_7".into(), name: "Receipts".into() }),
            quiet: true,
            needs_you: true,
            ..Default::default()
        };
        let f = to_gmail_filter(&when, &then);
        assert_eq!(
            f,
            json!({
                "criteria": {
                    "from": "dana@acme.com OR @news.io",
                    "to": "team",
                    "subject": "\"weekly report\" OR digest",
                    "query": "(invoice OR \"past due\")",
                    "hasAttachment": true,
                },
                "action": {
                    "addLabelIds": ["STARRED", "Label_7"],
                    "removeLabelIds": ["INBOX", "UNREAD"],
                }
            })
        );
    }

    #[test]
    fn omits_empty_fields() {
        let when = When { from: s(&["a@b.c"]), ..Default::default() };
        let then = Then { trash: true, ..Default::default() };
        assert_eq!(
            to_gmail_filter(&when, &then),
            json!({ "criteria": { "from": "a@b.c" }, "action": { "addLabelIds": ["TRASH"] } })
        );
    }

    #[test]
    fn round_trips() {
        let when = When {
            from: s(&["dana@acme.com", "@news.io"]),
            to: s(&["me@x.io"]),
            subject: s(&["weekly report"]),
            words: s(&["invoice", "past due"]),
            has_attachment: true,
        };
        let then = Then {
            archive: true,
            read: true,
            star: true,
            label: Some(Place { id: "Label_7".into(), name: "Receipts".into() }),
            ..Default::default()
        };
        let mut f = to_gmail_filter(&when, &then);
        f["id"] = json!("ANe1Bmj");
        let r = from_gmail_filter(&f, &[label("Label_7", "Receipts")]).unwrap();
        assert_eq!(r.id, "ANe1Bmj");
        assert_eq!(r.name, "");
        assert!(r.enabled);
        assert!(!r.extra);
        assert_eq!(r.when, when);
        assert_eq!(r.then, then);
    }

    #[test]
    fn reads_gmail_made_filter() {
        let f = json!({
            "id": "x1",
            "criteria": { "from": "(a@x.io OR \"Dana Cohen\" OR b@y.io)", "query": "{foo bar}" },
            "action": { "addLabelIds": ["Label_unknown", "TRASH"] }
        });
        let r = from_gmail_filter(&f, &[]).unwrap();
        assert_eq!(r.when.from, s(&["a@x.io", "Dana Cohen", "b@y.io"]));
        assert_eq!(r.when.words, s(&["foo", "bar"]));
        assert_eq!(r.then.label, Some(Place { id: "Label_unknown".into(), name: "Label_unknown".into() }));
        assert!(r.then.trash);
        assert!(!r.extra);
    }

    #[test]
    fn splits_or() {
        assert_eq!(split_or("a OR b"), (s(&["a", "b"]), false));
        assert_eq!(split_or("\"a b\" OR c"), (s(&["a b", "c"]), false));
        assert_eq!(split_or("((a OR b))"), (s(&["a", "b"]), false));
        assert_eq!(split_or("one"), (s(&["one"]), false));
        assert_eq!(split_or(""), (vec![], false));
        // Two terms without OR mean both: not representable.
        assert!(split_or("a b").1);
        assert!(split_or("a OR -b").1);
        assert!(split_or("from:x OR y").1);
    }

    #[test]
    fn detects_extra() {
        let cases = [
            json!({ "id": "1", "criteria": { "from": "a", "negatedQuery": "b" }, "action": {} }),
            json!({ "id": "2", "criteria": { "size": 1000, "sizeComparison": "larger" }, "action": {} }),
            json!({ "id": "3", "criteria": { "excludeChats": true }, "action": {} }),
            json!({ "id": "4", "criteria": { "from": "a" }, "action": { "forward": "x@y.io" } }),
            json!({ "id": "5", "criteria": { "from": "a" }, "action": { "removeLabelIds": ["SPAM"] } }),
            json!({ "id": "6", "criteria": { "from": "a" }, "action": { "addLabelIds": ["IMPORTANT"] } }),
            json!({ "id": "7", "criteria": { "from": "a" }, "action": { "addLabelIds": ["CATEGORY_SOCIAL"] } }),
            json!({ "id": "8", "criteria": { "from": "a" }, "action": { "addLabelIds": ["L1", "L2"] } }),
        ];
        for c in &cases {
            assert!(from_gmail_filter(c, &[]).unwrap().extra, "{c}");
        }
        let plain = json!({ "id": "9", "criteria": { "from": "a", "excludeChats": false }, "action": {} });
        assert!(!from_gmail_filter(&plain, &[]).unwrap().extra);
    }

    #[test]
    fn needs_id() {
        assert!(from_gmail_filter(&json!({ "criteria": { "from": "a" } }), &[]).is_none());
        let r = from_gmail_filter(&json!({ "id": "z" }), &[]).unwrap();
        assert_eq!(r.when, When::default());
        assert_eq!(r.then, Then::default());
    }
}
