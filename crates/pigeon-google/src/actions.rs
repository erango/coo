//! Mail mutations: label changes via `messages.batchModify` (≤1000 ids per
//! call), trash/untrash per message.

use futures::future::try_join_all;
use pigeon_core::http::{client, send};
use pigeon_core::model::MailOp;
use pigeon_core::provider::MailActions;
use pigeon_core::{Error, Result};
use serde_json::{Value, json};

use crate::{GMAIL, Google};

const MODIFY_MAX: usize = 1000;
const CONCURRENCY: usize = 5;

/// (addLabelIds, removeLabelIds) for label-style ops.
fn label_change(op: &MailOp) -> Option<(Vec<String>, Vec<String>)> {
    let one = |label: &str, on: bool| {
        let l = vec![label.to_string()];
        if on { (l, vec![]) } else { (vec![], l) }
    };
    Some(match op {
        // Read = no UNREAD label.
        MailOp::SetRead { read, .. } => one("UNREAD", !read),
        MailOp::SetFlagged { flagged, .. } => one("STARRED", *flagged),
        MailOp::Relabel { add, remove, .. } => (add.clone(), remove.clone()),
        MailOp::MoveTo { folder_id, .. } => (vec![folder_id.clone()], vec![]),
        MailOp::Trash { .. } | MailOp::Untrash { .. } => return None,
    })
}

impl Google {
    async fn post(&self, url: &str, body: &Value) -> Result<()> {
        let token = self.tokens.access_token().await?;
        match send(|| client().post(url).bearer_auth(&token).json(body)).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop),
        }
    }
}

impl MailActions for Google {
    async fn apply(&self, op: &MailOp) -> Result<()> {
        if let Some((add, remove)) = label_change(op) {
            for ids in op.ids().chunks(MODIFY_MAX) {
                let body = json!({ "ids": ids, "addLabelIds": add, "removeLabelIds": remove });
                self.post(&format!("{GMAIL}/messages/batchModify"), &body).await?;
            }
            return Ok(());
        }
        let verb = match op {
            MailOp::Trash { .. } => "trash",
            MailOp::Untrash { .. } => "untrash",
            _ => return Err(Error::Other("unhandled Gmail op".into())),
        };
        let empty = json!({});
        for chunk in op.ids().chunks(CONCURRENCY) {
            let urls: Vec<String> = chunk.iter().map(|id| format!("{GMAIL}/messages/{id}/{verb}")).collect();
            try_join_all(urls.iter().map(|u| self.post(u, &empty))).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_changes() {
        let ids = vec!["a".to_string()];
        assert_eq!(
            label_change(&MailOp::SetRead { ids: ids.clone(), read: true }),
            Some((vec![], vec!["UNREAD".into()]))
        );
        assert_eq!(
            label_change(&MailOp::SetFlagged { ids: ids.clone(), flagged: true }),
            Some((vec!["STARRED".into()], vec![]))
        );
        assert_eq!(label_change(&MailOp::Trash { ids }), None);
    }
}
