//! Gmail drafts (`users.drafts`): create/update from raw MIME, delete, look
//! up the draft behind a synced message, and send a saved draft.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pigeon_core::http::{client, send, send_once};
use pigeon_core::provider::DraftSync;
use pigeon_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{GMAIL, Google};

/// The JSON API carries the MIME base64-encoded inside the request; keep
/// well under Gmail's request limit.
const DRAFT_MAX: usize = 3 * 1024 * 1024;

#[derive(Deserialize)]
struct DraftRef {
    id: String,
    #[serde(default)]
    message: Option<MessageRef>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DraftList {
    #[serde(default)]
    drafts: Vec<DraftRef>,
    next_page_token: Option<String>,
}

pub(crate) fn draft_body(mime: &[u8], id: Option<&str>, thread_id: Option<&str>) -> Value {
    let mut message = json!({ "raw": URL_SAFE_NO_PAD.encode(mime) });
    if let Some(t) = thread_id {
        message["threadId"] = json!(t);
    }
    let mut body = json!({ "message": message });
    if let Some(id) = id {
        body["id"] = json!(id);
    }
    body
}

fn find_draft(list: &[DraftRef], message_id: &str) -> Option<String> {
    list.iter().find(|d| d.message.as_ref().is_some_and(|m| m.id == message_id)).map(|d| d.id.clone())
}

impl DraftSync for Google {
    async fn put_draft(&self, mime: &[u8], existing: Option<&str>, thread_id: Option<&str>) -> Result<String> {
        if mime.len() > DRAFT_MAX {
            return Err(Error::Other("draft too large to save on the server; it's kept on this Mac".into()));
        }
        let token = self.tokens.access_token().await?;
        let resp = match existing {
            // Updates are idempotent, so the retrying client is fine.
            Some(id) => {
                let body = draft_body(mime, Some(id), thread_id);
                match send(|| client().put(format!("{GMAIL}/drafts/{id}")).bearer_auth(&token).json(&body)).await {
                    // Deleted elsewhere: recreate instead.
                    Err(e) if e.status() == Some(404) => None,
                    r => Some(r?),
                }
            }
            None => None,
        };
        let resp = match resp {
            Some(r) => r,
            // Single attempt: a retried create could leave two drafts.
            None => {
                let body = draft_body(mime, None, thread_id);
                send_once(client().post(format!("{GMAIL}/drafts")).bearer_auth(&token).json(&body)).await?
            }
        };
        let d: DraftRef = resp.json().await?;
        Ok(d.id)
    }

    async fn delete_draft(&self, id: &str) -> Result<()> {
        let token = self.tokens.access_token().await?;
        match send(|| client().delete(format!("{GMAIL}/drafts/{id}")).bearer_auth(&token)).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop),
        }
    }

    async fn draft_for_message(&self, message_id: &str) -> Result<Option<String>> {
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("maxResults", "500".to_string())];
            if let Some(p) = &page {
                q.push(("pageToken", p.clone()));
            }
            let list: DraftList = self.get(&format!("{GMAIL}/drafts"), &q).await?;
            if let Some(id) = find_draft(&list.drafts, message_id) {
                return Ok(Some(id));
            }
            match list.next_page_token {
                Some(p) => page = Some(p),
                None => return Ok(None),
            }
        }
    }

    async fn send_draft(&self, id: &str, mime: &[u8], thread_id: Option<&str>) -> Result<()> {
        // Save the final content first so the sent message is exactly it.
        let id = self.put_draft(mime, Some(id), thread_id).await?;
        let token = self.tokens.access_token().await?;
        send_once(client().post(format!("{GMAIL}/drafts/send")).bearer_auth(&token).json(&json!({ "id": id })))
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_carries_raw_thread_and_id() {
        let b = draft_body(b"Subject: x\r\n\r\nhi", Some("r-1"), Some("t1"));
        assert_eq!(b["id"], "r-1");
        assert_eq!(b["message"]["threadId"], "t1");
        assert_eq!(b["message"]["raw"], URL_SAFE_NO_PAD.encode(b"Subject: x\r\n\r\nhi"));
        assert!(draft_body(b"x", None, None).get("id").is_none());
    }

    #[test]
    fn finds_draft_behind_message() {
        let list: DraftList = serde_json::from_str(
            r#"{"drafts": [{"id": "r-1", "message": {"id": "m1", "threadId": "t"}}, {"id": "r-2", "message": {"id": "m2"}}]}"#,
        )
        .unwrap();
        assert_eq!(find_draft(&list.drafts, "m2").as_deref(), Some("r-2"));
        assert_eq!(find_draft(&list.drafts, "zz"), None);
    }
}
