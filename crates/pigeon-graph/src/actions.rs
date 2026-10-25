//! Mail mutations via JSON `$batch` (up to 20 requests per call).

use pigeon_core::http::{client, send};
use pigeon_core::model::{MailOp, SpamReport};
use pigeon_core::provider::MailActions;
use pigeon_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{BASE, BETA, Graph, PREFER};

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
        // A report's move is ours (see `report_requests`).
        MailOp::MoveTo { folder_id, .. } | MailOp::Report { folder_id, .. } => {
            each("POST", mv, json!({ "destinationId": folder_id }))
        }
        MailOp::Trash { .. } => each("POST", mv, json!({ "destinationId": "deleteditems" })),
        MailOp::Untrash { .. } => each("POST", mv, json!({ "destinationId": "inbox" })),
        MailOp::Relabel { .. } => return Err(Error::Other("Exchange folders have no labels".into())),
    })
}

/// `message: reportMessage` sub-requests for a `Report` op, relative to
/// `/beta` (the action isn't in v1.0). The server is told not to move the
/// message: the move to `folder_id` follows as a v1.0 request, so where it
/// lands is ours, and a report the API refuses still moves it.
/// <https://learn.microsoft.com/en-us/graph/api/message-reportmessage?view=graph-rest-beta>
fn report_requests(op: &MailOp) -> Vec<SubRequest> {
    let MailOp::Report { ids, report, .. } = op else { return Vec::new() };
    let action = match report {
        SpamReport::Junk => "junk",
        SpamReport::NotJunk => "notJunk",
        SpamReport::Phishing => "phish",
    };
    ids.iter()
        .map(|id| {
            let body = json!({ "IsMessageMoveRequested": false, "ReportAction": action });
            ("POST", format!("/me/messages/{id}/reportMessage"), body)
        })
        .collect()
}

/// After a report batch: transient failures retry the whole (idempotent)
/// op; anything else (say the beta action is refused for this mailbox)
/// falls back to the plain move.
fn report_outcome(responses: &[SubResponse]) -> Result<()> {
    match check(responses) {
        Err(e) if matches!(e.status(), Some(401 | 429 | 500..=599)) => Err(e),
        Err(e) => {
            tracing::warn!(status = e.status(), "reportMessage refused; moving without a report");
            Ok(())
        }
        Ok(()) => Ok(()),
    }
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

impl Graph {
    /// Run sub-requests through `{base}/$batch`, up to `BATCH_MAX` per call.
    async fn batch(&self, base: &str, reqs: &[SubRequest]) -> Result<Vec<SubResponse>> {
        let mut out = Vec::with_capacity(reqs.len());
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
                send(|| client().post(format!("{base}/$batch")).bearer_auth(&token).json(&body)).await?.json().await?;
            out.extend(resp.responses);
        }
        Ok(out)
    }
}

impl MailActions for Graph {
    async fn apply(&self, op: &MailOp) -> Result<()> {
        let reports = report_requests(op);
        if !reports.is_empty() {
            report_outcome(&self.batch(BETA, &reports).await?)?;
        }
        check(&self.batch(BASE, &requests(op)?).await?)
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
    fn reports_go_to_beta_then_move_in_v1() {
        let op =
            MailOp::Report { ids: vec!["a".into(), "b".into()], report: SpamReport::Phishing, folder_id: "J".into() };
        let r = report_requests(&op);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].0, r[0].1.as_str()), ("POST", "/me/messages/a/reportMessage"));
        assert_eq!(r[0].2, json!({"IsMessageMoveRequested": false, "ReportAction": "phish"}));
        let not = MailOp::Report { ids: vec!["a".into()], report: SpamReport::NotJunk, folder_id: "I".into() };
        assert_eq!(report_requests(&not)[0].2["ReportAction"], "notJunk");
        let junk = MailOp::Report { ids: vec!["a".into()], report: SpamReport::Junk, folder_id: "J".into() };
        assert_eq!(report_requests(&junk)[0].2["ReportAction"], "junk");
        let mv = requests(&op).unwrap();
        assert_eq!((mv[1].1.as_str(), &mv[1].2), ("/me/messages/b/move", &json!({"destinationId": "J"})));
        assert!(report_requests(&MailOp::Trash { ids: vec!["a".into()] }).is_empty());
    }

    #[test]
    fn refused_reports_fall_back_to_the_move() {
        let parse = |s: &str| serde_json::from_str::<BatchResponse>(s).unwrap().responses;
        assert!(report_outcome(&parse(r#"{"responses": [{"id": "0", "status": 400, "body": {}}]}"#)).is_ok());
        assert!(report_outcome(&parse(r#"{"responses": [{"id": "0", "status": 403, "body": {}}]}"#)).is_ok());
        let e = report_outcome(&parse(r#"{"responses": [{"id": "0", "status": 503, "body": {}}]}"#)).unwrap_err();
        assert_eq!(e.status(), Some(503), "transient: retry the op");
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
