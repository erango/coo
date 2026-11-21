//! Outlook inbox rules (`messageRule`): Coo's mail rules saved on the
//! server, so they keep working with Coo closed. Needs the
//! `MailboxSettings.ReadWrite` scope.
//! <https://learn.microsoft.com/en-us/graph/api/resources/messagerule>

use pigeon_core::http::{client, send, send_once};
use pigeon_core::model::Folder;
use pigeon_core::rules::{Place, ServerRule, Then, When};
use pigeon_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{BASE, Graph};

const RULES: &str = "/me/mailFolders/inbox/messageRules";

/// The `messageRule` body for a rule. Outlook rules can't flag, and
/// `quiet`/`needs_you` are Coo's own; the caller handles those. Trash wins
/// over a move (a deleted message has nowhere else to go), and a label
/// (folder) over archiving.
pub fn to_outlook_rule(name: &str, when: &When, then: &Then, archive_folder: Option<&str>, sequence: u32) -> Value {
    let mut conditions = Map::new();
    for (key, values) in [
        ("senderContains", &when.from),
        ("recipientContains", &when.to),
        ("subjectContains", &when.subject),
        ("bodyOrSubjectContains", &when.words),
    ] {
        let values: Vec<&str> = values.iter().map(|v| v.trim()).filter(|v| !v.is_empty()).collect();
        if !values.is_empty() {
            conditions.insert(key.into(), json!(values));
        }
    }
    if when.has_attachment {
        conditions.insert("hasAttachments".into(), json!(true));
    }

    let mut actions = Map::new();
    if then.trash {
        actions.insert("delete".into(), json!(true));
    } else if let Some(label) = &then.label {
        actions.insert("moveToFolder".into(), json!(label.id));
    } else if then.archive
        && let Some(folder) = archive_folder
    {
        actions.insert("moveToFolder".into(), json!(folder));
    }
    if then.read {
        actions.insert("markAsRead".into(), json!(true));
    }
    actions.insert("stopProcessingRules".into(), json!(false));

    json!({
        "displayName": name,
        "sequence": sequence,
        "isEnabled": true,
        "conditions": conditions,
        "actions": actions,
    })
}

/// Set to something other than its "unset" value (Graph may send
/// `false`, `null` or `[]` for predicates a rule doesn't use).
fn present(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => o.values().any(present),
        Value::Number(_) => true,
    }
}

fn strings(v: &Value) -> impl Iterator<Item = String> + '_ {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// A rule as Coo sees it: for rules made in Outlook as well as Coo's.
/// None only when it has no id.
pub fn from_outlook_rule(v: &Value, folders: &[Folder], archive_folder: Option<&str>) -> Option<ServerRule> {
    let id = v.get("id")?.as_str().filter(|s| !s.is_empty())?.to_string();
    let name = v.get("displayName").and_then(Value::as_str).unwrap_or_default().to_string();
    let enabled = v.get("isEnabled").and_then(Value::as_bool).unwrap_or(true);
    let mut when = When::default();
    let mut then = Then::default();
    let mut extra = v.get("exceptions").is_some_and(present);

    if let Some(conditions) = v.get("conditions").and_then(Value::as_object) {
        for (key, value) in conditions {
            match key.as_str() {
                "senderContains" => when.from.extend(strings(value)),
                "fromAddresses" => when.from.extend(
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| r.pointer("/emailAddress/address").and_then(Value::as_str))
                        .filter(|a| !a.is_empty())
                        .map(String::from),
                ),
                "recipientContains" => when.to.extend(strings(value)),
                "subjectContains" => when.subject.extend(strings(value)),
                "bodyOrSubjectContains" => when.words.extend(strings(value)),
                "hasAttachments" => when.has_attachment = value.as_bool().unwrap_or(false),
                _ => extra |= present(value),
            }
        }
    }

    if let Some(actions) = v.get("actions").and_then(Value::as_object) {
        for (key, value) in actions {
            match key.as_str() {
                "delete" => then.trash = value.as_bool().unwrap_or(false),
                "markAsRead" => then.read = value.as_bool().unwrap_or(false),
                "moveToFolder" => match value.as_str().filter(|s| !s.is_empty()) {
                    Some(f) if Some(f) == archive_folder => then.archive = true,
                    Some(f) => {
                        let name = folders.iter().find(|x| x.id == f).map_or(f, |x| x.name.as_str());
                        then.label = Some(Place { id: f.to_string(), name: name.to_string() });
                    }
                    None => {}
                },
                // Coo saves `false`; `true` would skip later rules.
                _ => extra |= present(value),
            }
        }
    }

    Some(ServerRule { id, name, when, then, extra, enabled })
}

/// A 403 from the rules endpoints: the account's sign-in lacks
/// `MailboxSettings.ReadWrite` (or the tenant forbids it).
fn forbidden(e: Error) -> Error {
    if e.status() == Some(403) { Error::Other("Outlook hasn't allowed Coo to save rules".into()) } else { e }
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

impl Graph {
    /// Every inbox rule on the account, raw (see `from_outlook_rule`).
    pub async fn outlook_rules(&self) -> Result<Vec<Value>> {
        self.get_all(&format!("{BASE}{RULES}")).await.map_err(forbidden)
    }

    /// Save a rule (from `to_outlook_rule`); returns its id.
    pub async fn create_outlook_rule(&self, rule: &Value) -> Result<String> {
        let token = self.tokens.access_token().await?;
        // Single attempt: a retried create could leave two rules.
        let req = client().post(format!("{BASE}{RULES}")).bearer_auth(&token).json(rule);
        let created: Created = send_once(req).await.map_err(forbidden)?.json().await?;
        if created.id.is_empty() {
            return Err(Error::Other("Outlook saved the rule without an id".into()));
        }
        Ok(created.id)
    }

    /// Delete a rule; one that's already gone counts as done.
    pub async fn delete_outlook_rule(&self, id: &str) -> Result<()> {
        let token = self.tokens.access_token().await?;
        let url = format!("{BASE}{RULES}/{id}");
        match send(|| client().delete(&url).bearer_auth(&token)).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop).map_err(forbidden),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::FolderKind;

    fn folder(id: &str, name: &str) -> Folder {
        Folder { id: id.into(), name: name.into(), parent_id: None, role: None, kind: FolderKind::Folder }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn builds_rule() {
        let when = When {
            from: s(&["dana@acme.com", "@news.io"]),
            to: s(&["team"]),
            subject: s(&["weekly report"]),
            words: s(&["invoice", "past due"]),
            has_attachment: true,
        };
        let then = Then { archive: true, read: true, star: true, quiet: true, ..Default::default() };
        assert_eq!(
            to_outlook_rule("News", &when, &then, Some("ARCH"), 3),
            json!({
                "displayName": "News",
                "sequence": 3,
                "isEnabled": true,
                "conditions": {
                    "senderContains": ["dana@acme.com", "@news.io"],
                    "recipientContains": ["team"],
                    "subjectContains": ["weekly report"],
                    "bodyOrSubjectContains": ["invoice", "past due"],
                    "hasAttachments": true,
                },
                "actions": { "moveToFolder": "ARCH", "markAsRead": true, "stopProcessingRules": false },
            })
        );
    }

    #[test]
    fn move_precedence() {
        let when = When { from: s(&["a"]), ..Default::default() };
        let label = Some(Place { id: "F1".into(), name: "Receipts".into() });
        let both = Then { archive: true, label: label.clone(), ..Default::default() };
        assert_eq!(to_outlook_rule("", &when, &both, Some("ARCH"), 0)["actions"]["moveToFolder"], "F1");
        let trash = Then { trash: true, label, ..Default::default() };
        let r = to_outlook_rule("", &when, &trash, Some("ARCH"), 0);
        assert_eq!(r["actions"], json!({ "delete": true, "stopProcessingRules": false }));
        // No archive folder known: nothing to move to.
        let archive = Then { archive: true, ..Default::default() };
        assert_eq!(to_outlook_rule("", &when, &archive, None, 0)["actions"], json!({ "stopProcessingRules": false }));
    }

    #[test]
    fn round_trips() {
        let when = When {
            from: s(&["dana@acme.com"]),
            to: s(&["me@x.io"]),
            subject: s(&["weekly report", "digest"]),
            words: s(&["invoice"]),
            has_attachment: true,
        };
        let folders = [folder("F1", "Receipts"), folder("ARCH", "Archive")];
        for then in [
            Then { archive: true, read: true, ..Default::default() },
            Then { label: Some(Place { id: "F1".into(), name: "Receipts".into() }), ..Default::default() },
            Then { trash: true, ..Default::default() },
        ] {
            let mut r = to_outlook_rule("My rule", &when, &then, Some("ARCH"), 2);
            r["id"] = json!("AQAAA=");
            let back = from_outlook_rule(&r, &folders, Some("ARCH")).unwrap();
            assert_eq!(back.id, "AQAAA=");
            assert_eq!(back.name, "My rule");
            assert!(back.enabled);
            assert!(!back.extra, "{r}");
            assert_eq!(back.when, when);
            assert_eq!(back.then, then);
        }
    }

    #[test]
    fn reads_outlook_made_rule() {
        let r = json!({
            "id": "r1",
            "displayName": "From boss",
            "isEnabled": false,
            "sequence": 1,
            "hasError": false,
            "isReadOnly": false,
            "conditions": {
                "fromAddresses": [{ "emailAddress": { "name": "Boss", "address": "boss@acme.com" } }],
                "senderContains": ["@acme.io"],
                "importance": null,
                "isAutomaticForward": false,
                "subjectContains": [],
            },
            "actions": { "moveToFolder": "UNKNOWN", "stopProcessingRules": false, "forwardTo": [] },
            "exceptions": { "subjectContains": [] },
        });
        let back = from_outlook_rule(&r, &[], Some("ARCH")).unwrap();
        assert_eq!(back.name, "From boss");
        assert!(!back.enabled);
        assert_eq!(back.when.from, s(&["boss@acme.com", "@acme.io"]));
        assert_eq!(back.then.label, Some(Place { id: "UNKNOWN".into(), name: "UNKNOWN".into() }));
        assert!(!back.extra);
    }

    #[test]
    fn detects_extra() {
        let base = |conditions: Value, actions: Value, exceptions: Value| json!({ "id": "x", "conditions": conditions, "actions": actions, "exceptions": exceptions });
        let cases = [
            base(json!({}), json!({ "forwardTo": [{ "emailAddress": { "address": "a@b.c" } }] }), json!(null)),
            base(json!({}), json!({ "redirectTo": [{ "emailAddress": { "address": "a@b.c" } }] }), json!(null)),
            base(json!({}), json!({ "assignCategories": ["Red"] }), json!(null)),
            base(json!({}), json!({ "copyToFolder": "F9" }), json!(null)),
            base(json!({}), json!({ "markImportance": "high" }), json!(null)),
            base(json!({}), json!({ "permanentDelete": true }), json!(null)),
            base(json!({}), json!({ "stopProcessingRules": true }), json!(null)),
            base(json!({ "importance": "high" }), json!({}), json!(null)),
            base(json!({ "sentToMe": true }), json!({}), json!(null)),
            base(json!({}), json!({}), json!({ "senderContains": ["x"] })),
        ];
        for c in &cases {
            assert!(from_outlook_rule(c, &[], None).unwrap().extra, "{c}");
        }
    }

    #[test]
    fn needs_id() {
        assert!(from_outlook_rule(&json!({ "displayName": "x" }), &[], None).is_none());
    }
}
