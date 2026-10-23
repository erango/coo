//! Exchange drafts: `POST /me/messages` with a base64 MIME body creates a
//! draft in Drafts (and keeps In-Reply-To/References, which the JSON API
//! can't set). There's no MIME update, so a save creates the new version
//! first and then deletes the old one; the id changes each time.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pigeon_core::http::{client, send, send_once};
use pigeon_core::provider::DraftSync;
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{BASE, Graph, PREFER};

/// The MIME request is capped at 4 MB by Graph.
const MIME_MAX: usize = 3 * 1024 * 1024;

#[derive(Deserialize)]
struct Created {
    id: String,
}

impl DraftSync for Graph {
    async fn put_draft(&self, mime: &[u8], existing: Option<&str>, _thread_id: Option<&str>) -> Result<String> {
        if mime.len() > MIME_MAX {
            return Err(Error::Other("draft too large to save on the server; it's kept on this Mac".into()));
        }
        let token = self.tokens.access_token().await?;
        // Single attempt: a retried create could leave two drafts.
        let req = client()
            .post(format!("{BASE}/me/messages"))
            .bearer_auth(&token)
            .header("Prefer", PREFER)
            .header("Content-Type", "text/plain")
            .body(STANDARD.encode(mime));
        let created: Created = send_once(req).await?.json().await?;
        if let Some(old) = existing
            && old != created.id
            && let Err(e) = self.delete_draft(old).await
        {
            tracing::warn!(error = %e, "could not remove the previous draft version");
        }
        Ok(created.id)
    }

    async fn delete_draft(&self, id: &str) -> Result<()> {
        let token = self.tokens.access_token().await?;
        let url = format!("{BASE}/me/messages/{id}");
        match send(|| client().delete(&url).bearer_auth(&token).header("Prefer", PREFER)).await {
            Err(e) if e.status() == Some(404) => Ok(()),
            r => r.map(drop),
        }
    }

    async fn draft_for_message(&self, message_id: &str) -> Result<Option<String>> {
        // Exchange drafts are messages; same id.
        Ok(Some(message_id.to_string()))
    }

    async fn send_draft(&self, id: &str, mime: &[u8], thread_id: Option<&str>) -> Result<()> {
        let id = self.put_draft(mime, Some(id), thread_id).await?;
        let token = self.tokens.access_token().await?;
        let req = client()
            .post(format!("{BASE}/me/messages/{id}/send"))
            .bearer_auth(&token)
            .header("Prefer", PREFER)
            .header("Content-Length", "0");
        send_once(req).await.map(drop)
    }
}
