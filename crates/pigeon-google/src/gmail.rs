//! Gmail sync. One scope for the whole mailbox:
//!
//! - bootstrap: snapshot `historyId`, then page `messages.list` over the
//!   retention window, fetching metadata for each id;
//! - incremental: `history.list` from the snapshot, re-fetching touched
//!   messages (their full label set is authoritative).
//!
//! Cursor encoding: `boot:{historyId}:{pageToken}` while bootstrapping,
//! `hist:{historyId}` when caught up, `histp:{historyId}:{pageToken}` mid-page.

use std::collections::HashMap;

use futures::{StreamExt, stream};
use pigeon_core::model::{Folder, FolderKind, FolderRole, MailChange, MessageHeader};
use pigeon_core::provider::{MailPage, MailProvider, MailSearch};
use pigeon_core::search::{SearchQuery, to_gmail};
use pigeon_core::text::{decode_entities, parse_address_list};
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{GMAIL, Google};

const SCOPE: &str = "mail";
const FETCH_CONCURRENCY: usize = 10;
const PAGE_SIZE: &str = "500";

/// Labels that are flags, not places.
const HIDDEN_LABELS: [&str; 2] = ["UNREAD", "CHAT"];

#[derive(Debug, PartialEq, Eq)]
enum Cursor<'a> {
    Fresh,
    Boot { history_id: &'a str, page: &'a str },
    History { history_id: &'a str, page: Option<&'a str> },
}

impl<'a> Cursor<'a> {
    fn parse(s: Option<&'a str>) -> Result<Self> {
        let Some(s) = s else { return Ok(Cursor::Fresh) };
        let bad = || Error::Other(format!("bad Gmail cursor {s}"));
        let (kind, rest) = s.split_once(':').ok_or_else(bad)?;
        Ok(match kind {
            "boot" => {
                let (history_id, page) = rest.split_once(':').ok_or_else(bad)?;
                Cursor::Boot { history_id, page }
            }
            "hist" => Cursor::History { history_id: rest, page: None },
            "histp" => {
                let (history_id, page) = rest.split_once(':').ok_or_else(bad)?;
                Cursor::History { history_id, page: Some(page) }
            }
            _ => return Err(bad()),
        })
    }
}

impl MailProvider for Google {
    async fn folders(&self) -> Result<Vec<Folder>> {
        #[derive(Deserialize)]
        struct Labels {
            #[serde(default)]
            labels: Vec<Label>,
        }
        let resp: Labels = self.get(&format!("{GMAIL}/labels"), &[]).await?;
        Ok(normalize_labels(resp.labels))
    }

    fn mail_scopes(&self, _folders: &[Folder]) -> Vec<String> {
        vec![SCOPE.to_string()]
    }

    async fn mail_page(&self, _scope: &str, cursor: Option<&str>) -> Result<MailPage> {
        match Cursor::parse(cursor)? {
            Cursor::Fresh => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Profile {
                    history_id: String,
                }
                let p: Profile = self.get(&format!("{GMAIL}/profile"), &[]).await?;
                self.bootstrap_page(&p.history_id, None).await
            }
            Cursor::Boot { history_id, page } => self.bootstrap_page(history_id, Some(page)).await,
            Cursor::History { history_id, page } => self.history_page(history_id, page).await,
        }
    }
}

impl Google {
    async fn bootstrap_page(&self, history_id: &str, page: Option<&str>) -> Result<MailPage> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct List {
            #[serde(default)]
            messages: Vec<IdRef>,
            next_page_token: Option<String>,
        }
        let mut query =
            vec![("maxResults", PAGE_SIZE.to_string()), ("q", format!("newer_than:{}d", self.mail_window_days))];
        if let Some(p) = page {
            query.push(("pageToken", p.to_string()));
        }
        let list: List = self.get(&format!("{GMAIL}/messages"), &query).await?;
        let ids: Vec<String> = list.messages.into_iter().map(|m| m.id).collect();
        let changes = self.fetch_changes(ids).await?;
        // After the last list page, `more` stays true so the driver goes on to
        // replay history accumulated since the snapshot.
        let cursor = match list.next_page_token {
            Some(next) => format!("boot:{history_id}:{next}"),
            None => format!("hist:{history_id}"),
        };
        Ok(MailPage { changes, cursor, more: true })
    }

    async fn history_page(&self, history_id: &str, page: Option<&str>) -> Result<MailPage> {
        let mut query = vec![("startHistoryId", history_id.to_string()), ("maxResults", PAGE_SIZE.to_string())];
        if let Some(p) = page {
            query.push(("pageToken", p.to_string()));
        }
        let resp: HistoryList = match self.get(&format!("{GMAIL}/history"), &query).await {
            Err(e) if e.status() == Some(404) => return Err(Error::CursorExpired),
            r => r?,
        };
        let touched = touched_messages(&resp.history);
        let mut changes = Vec::new();
        let mut fetch = Vec::new();
        for (id, action) in touched {
            match action {
                Touch::Deleted => changes.push(MailChange::Delete { id }),
                Touch::Changed => fetch.push(id),
            }
        }
        changes.extend(self.fetch_changes(fetch).await?);
        Ok(match resp.next_page_token {
            Some(next) => MailPage { changes, cursor: format!("histp:{history_id}:{next}"), more: true },
            None => MailPage { changes, cursor: format!("hist:{}", resp.history_id), more: false },
        })
    }

    /// Fetch metadata for each id; ids that vanished meanwhile become deletes.
    async fn fetch_changes(&self, ids: Vec<String>) -> Result<Vec<MailChange>> {
        let results: Vec<(String, Result<RawMessage>)> = stream::iter(ids)
            .map(|id| async move {
                let query = [
                    ("format", "metadata".to_string()),
                    ("metadataHeaders", "From".to_string()),
                    ("metadataHeaders", "To".to_string()),
                    ("metadataHeaders", "Cc".to_string()),
                    ("metadataHeaders", "Subject".to_string()),
                    ("metadataHeaders", "Message-ID".to_string()),
                ];
                let r = self.get(&format!("{GMAIL}/messages/{id}"), &query).await;
                (id, r)
            })
            .buffered(FETCH_CONCURRENCY)
            .collect()
            .await;
        results
            .into_iter()
            .map(|(id, r)| match r {
                Ok(raw) => Ok(MailChange::Upsert(to_header(raw))),
                Err(e) if e.status() == Some(404) => Ok(MailChange::Delete { id }),
                Err(e) => Err(e),
            })
            .collect()
    }
}

impl MailSearch for Google {
    async fn search(&self, q: &SearchQuery, limit: u32) -> Result<Vec<MailChange>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            messages: Vec<IdRef>,
        }
        let query = to_gmail(q);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let params = [("q", query), ("maxResults", limit.clamp(1, 100).to_string())];
        let list: List = self.get(&format!("{GMAIL}/messages"), &params).await?;
        self.fetch_changes(list.messages.into_iter().map(|m| m.id).collect()).await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Label {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

fn system_label(id: &str) -> Option<(Option<FolderRole>, &'static str)> {
    Some(match id {
        "INBOX" => (Some(FolderRole::Inbox), "Inbox"),
        "SENT" => (Some(FolderRole::Sent), "Sent"),
        "DRAFT" => (Some(FolderRole::Drafts), "Drafts"),
        "TRASH" => (Some(FolderRole::Trash), "Trash"),
        "SPAM" => (Some(FolderRole::Spam), "Spam"),
        "STARRED" => (Some(FolderRole::Starred), "Starred"),
        "IMPORTANT" => (Some(FolderRole::Important), "Important"),
        "CATEGORY_PERSONAL" => (None, "Primary"),
        "CATEGORY_SOCIAL" => (None, "Social"),
        "CATEGORY_PROMOTIONS" => (None, "Promotions"),
        "CATEGORY_UPDATES" => (None, "Updates"),
        "CATEGORY_FORUMS" => (None, "Forums"),
        _ => return None,
    })
}

fn normalize_labels(labels: Vec<Label>) -> Vec<Folder> {
    // Nested user labels are "Parent/Child".
    let by_name: HashMap<String, String> = labels.iter().map(|l| (l.name.clone(), l.id.clone())).collect();
    labels
        .into_iter()
        .filter(|l| !HIDDEN_LABELS.contains(&l.id.as_str()))
        .map(|l| {
            let system = l.kind.as_deref() == Some("system");
            let (role, name, parent_id) = match system_label(&l.id) {
                Some((role, name)) => (role, name.to_string(), None),
                None if system => (None, l.name, None),
                None => {
                    let (parent, leaf) = match l.name.rsplit_once('/') {
                        Some((parent, leaf)) => (by_name.get(parent).cloned(), leaf.to_string()),
                        None => (None, l.name.clone()),
                    };
                    (None, leaf, parent)
                }
            };
            Folder { id: l.id, name, parent_id, role, kind: FolderKind::Label }
        })
        .collect()
}

#[derive(Deserialize)]
struct IdRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryList {
    #[serde(default)]
    history: Vec<History>,
    history_id: String,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct MessageRef {
    message: IdRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    #[serde(default)]
    messages_added: Vec<MessageRef>,
    #[serde(default)]
    messages_deleted: Vec<MessageRef>,
    #[serde(default)]
    labels_added: Vec<MessageRef>,
    #[serde(default)]
    labels_removed: Vec<MessageRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    Changed,
    Deleted,
}

/// Net effect per message across history records, in order (last wins), with
/// first-seen ordering preserved.
fn touched_messages(history: &[History]) -> Vec<(String, Touch)> {
    let mut order: Vec<String> = Vec::new();
    let mut state: HashMap<String, Touch> = HashMap::new();
    let mut mark = |id: &str, t: Touch| {
        if state.insert(id.to_string(), t).is_none() {
            order.push(id.to_string());
        }
    };
    for h in history {
        for m in h.messages_added.iter().chain(&h.labels_added).chain(&h.labels_removed) {
            mark(&m.message.id, Touch::Changed);
        }
        for m in &h.messages_deleted {
            mark(&m.message.id, Touch::Deleted);
        }
    }
    order
        .into_iter()
        .map(|id| {
            let t = state[&id];
            (id, t)
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMessage {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(default)]
    snippet: String,
    internal_date: Option<String>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    mime_type: Option<String>,
    #[serde(default)]
    headers: Vec<Header>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

fn to_header(m: RawMessage) -> MessageHeader {
    let header = |name: &str| {
        m.payload
            .as_ref()
            .and_then(|p| p.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)))
            .map(|h| h.value.as_str())
    };
    let has = |label: &str| m.label_ids.iter().any(|l| l == label);
    MessageHeader {
        thread_id: m.thread_id.clone(),
        internet_message_id: header("Message-ID").map(String::from),
        subject: header("Subject").unwrap_or_default().to_string(),
        from: header("From").and_then(|v| parse_address_list(v).into_iter().next()),
        to: header("To").map(parse_address_list).unwrap_or_default(),
        cc: header("Cc").map(parse_address_list).unwrap_or_default(),
        snippet: decode_entities(&m.snippet),
        received_at: m.internal_date.as_deref().and_then(|d| d.parse().ok()).unwrap_or(0),
        is_read: !has("UNREAD"),
        is_flagged: has("STARRED"),
        // `format=metadata` has no part tree; a top-level multipart/mixed is
        // a good-enough attachment signal until bodies are fetched.
        has_attachments: m.payload.as_ref().and_then(|p| p.mime_type.as_deref()) == Some("multipart/mixed"),
        is_draft: has("DRAFT"),
        folder_ids: m.label_ids.iter().filter(|l| !HIDDEN_LABELS.contains(&l.as_str())).cloned().collect(),
        id: m.id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip() {
        assert_eq!(Cursor::parse(None).unwrap(), Cursor::Fresh);
        assert_eq!(Cursor::parse(Some("boot:42:tok")).unwrap(), Cursor::Boot { history_id: "42", page: "tok" });
        assert_eq!(Cursor::parse(Some("hist:42")).unwrap(), Cursor::History { history_id: "42", page: None });
        assert_eq!(
            Cursor::parse(Some("histp:42:p:2")).unwrap(),
            Cursor::History { history_id: "42", page: Some("p:2") }
        );
        assert!(Cursor::parse(Some("nope")).is_err());
    }

    #[test]
    fn labels_normalize() {
        let labels: Vec<Label> = serde_json::from_str(
            r#"[
              {"id": "INBOX", "name": "INBOX", "type": "system"},
              {"id": "UNREAD", "name": "UNREAD", "type": "system"},
              {"id": "CATEGORY_PROMOTIONS", "name": "CATEGORY_PROMOTIONS", "type": "system"},
              {"id": "Label_1", "name": "Clients", "type": "user"},
              {"id": "Label_2", "name": "Clients/Acme", "type": "user"}
            ]"#,
        )
        .unwrap();
        let f = normalize_labels(labels);
        assert_eq!(f.len(), 4);
        assert_eq!((f[0].name.as_str(), f[0].role), ("Inbox", Some(FolderRole::Inbox)));
        assert_eq!(f[1].name, "Promotions");
        assert_eq!((f[3].name.as_str(), f[3].parent_id.as_deref()), ("Acme", Some("Label_1")));
    }

    #[test]
    fn history_net_effect() {
        let h: Vec<History> = serde_json::from_str(
            r#"[
              {"messagesAdded": [{"message": {"id": "a"}}, {"message": {"id": "b"}}]},
              {"labelsRemoved": [{"message": {"id": "c"}}]},
              {"messagesDeleted": [{"message": {"id": "a"}}]}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            touched_messages(&h),
            vec![("a".into(), Touch::Deleted), ("b".into(), Touch::Changed), ("c".into(), Touch::Changed)]
        );
    }

    #[test]
    fn metadata_to_header() {
        let raw: RawMessage = serde_json::from_str(
            r#"{
              "id": "m1", "threadId": "t1", "labelIds": ["INBOX", "UNREAD", "STARRED"],
              "snippet": "Here&#39;s the deck", "internalDate": "1761206400000",
              "payload": {"mimeType": "multipart/mixed", "headers": [
                {"name": "From", "value": "\"Cohen, Dana\" <dana@x.co.il>"},
                {"name": "To", "value": "me@gmail.com, Other <o@y.com>"},
                {"name": "Subject", "value": "Re: שלום"},
                {"name": "Message-ID", "value": "<abc@mail.gmail.com>"}
              ]}
            }"#,
        )
        .unwrap();
        let h = to_header(raw);
        assert_eq!(h.snippet, "Here's the deck");
        assert_eq!(h.from.as_ref().unwrap().display(), "Cohen, Dana");
        assert_eq!(h.to.len(), 2);
        assert_eq!(h.subject, "Re: שלום");
        assert!(!h.is_read && h.is_flagged && h.has_attachments);
        assert_eq!(h.folder_ids, ["INBOX", "STARRED"]);
        assert_eq!(h.received_at, 1_761_206_400_000);
    }
}
