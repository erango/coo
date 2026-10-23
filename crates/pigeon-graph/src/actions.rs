//! Mail mutations via JSON `$batch` (up to 20 requests per call).

use pigeon_core::http::{client, send};
use pigeon_core::model::MailOp;
use pigeon_core::provider::MailActions;
use pigeon_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{BASE, Graph, PREFER};

const BATCH_MAX: usize = 20;

/// One sub-request: method, path relative to `/v1.0`, JSON body.
type SubRequest = (&'static str, String, Value);

#[derive(Deserialize)]
struct BatchResponse {
    responses: Vec<SubResponse>,
}

#[derive(Deserialize)]
struct SubResponse {
    id: String,
    status: u16,
    #[serde(default)]
    body: Value,
}

fn requests(op: &MailOp) -> Result<Vec<SubRequest>> {
    let each = |method: &'static str, path: fn(&str) -> String, body: Value| {
        op.ids().iter().map(move |id| (method, path(id), body.clone())).collect::<Vec<_>>()
    };
    let patch = |id: &str| format!("/me/messages/{id}");
    let mv = |id: &str| format!("/me/messages/{id}/move");
    Ok(match op {
        MailOp::SetRead { read, .. } => each("PATCH", patch, json!({ "isRead": read })),
        MailOp::SetFlagged { flagged, .. } => {
            each("PATCH", patch, json!({ "flag": { "flagStatus": if *flagged { "flagged" } else { "notFlagged" } } }))
        }
        MailOp::MoveTo { folder_id, .. } => each("POST", mv, json!({ "destinationId": folder_id })),
        MailOp::Trash { .. } => each("POST", mv, json!({ "destinationId": "deleteditems" })),
        MailOp::Untrash { .. } => each("POST", mv, json!({ "destinationId": "inbox" })),
        MailOp::Relabel { .. } => return Err(Error::Other("Exchange folders have no labels".into())),
    })
}

/// First failure worth reporting; 404 counts as done (message already gone).
fn check(responses: &[SubResponse]) -> Result<()> {
    // Prefer surfacing a retryable status so the whole (idempotent) op is retried.
    let failed = responses.iter().filter(|r| !(200..300).contains(&r.status) && r.status != 404);
    let worst = failed.max_by_key(|r| matches!(r.status, 429 | 500..=599));
    match worst {
        None => Ok(()),
        Some(r) => Err(Error::Api { status: r.status, url: format!("$batch#{}", r.id), body: r.body.to_string() }),
    }
}

impl MailActions for Graph {
    async fn apply(&self, op: &MailOp) -> Result<()> {
        let reqs = requests(op)?;
        for chunk in reqs.chunks(BATCH_MAX) {
            let body = json!({
                "requests": chunk.iter().enumerate().map(|(i, (method, url, body))| json!({
                    "id": i.to_string(),
                    "method": method,
                    "url": url,
                    "headers": { "Content-Type": "application/json", "Prefer": PREFER },
                    "body": body,
                })).collect::<Vec<_>>()
            });
            let token = self.tokens.access_token().await?;
            let resp: BatchResponse =
                send(|| client().post(format!("{BASE}/$batch")).bearer_auth(&token).json(&body)).await?.json().await?;
            check(&resp.responses)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_requests() {
        let r = requests(&MailOp::SetFlagged { ids: vec!["a".into(), "b".into()], flagged: false }).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!((r[1].0, r[1].1.as_str()), ("PATCH", "/me/messages/b"));
        assert_eq!(r[1].2, json!({"flag": {"flagStatus": "notFlagged"}}));
        let r = requests(&MailOp::MoveTo { ids: vec!["a".into()], folder_id: "F".into() }).unwrap();
        assert_eq!((r[0].0, r[0].1.as_str(), &r[0].2), ("POST", "/me/messages/a/move", &json!({"destinationId": "F"})));
        assert!(requests(&MailOp::Relabel { ids: vec![], add: vec![], remove: vec![] }).is_err());
    }

    #[test]
    fn batch_status_handling() {
        let parse = |s: &str| serde_json::from_str::<BatchResponse>(s).unwrap().responses;
        assert!(check(&parse(r#"{"responses": [{"id": "0", "status": 200}, {"id": "1", "status": 404}]}"#)).is_ok());
        let e = check(&parse(
            r#"{"responses": [{"id": "0", "status": 400, "body": {}}, {"id": "1", "status": 429, "body": {}}]}"#,
        ))
        .unwrap_err();
        assert_eq!(e.status(), Some(429), "retryable status wins");
    }
}
