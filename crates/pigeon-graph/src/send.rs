//! Sending raw MIME via `POST /me/sendMail` (base64 body, `text/plain`). Graph's
//! JSON message API can't set `In-Reply-To`, and `/reply` only works inside
//! the same mailbox; MIME works for replies from any account.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pigeon_core::http::{client, send_once};
use pigeon_core::model::Address;
use pigeon_core::provider::MailSender;
use pigeon_core::{Error, Result};

use crate::{BASE, Graph};

/// sendMail caps the request at 4 MB; bigger needs an upload session.
const MIME_MAX: usize = 3 * 1024 * 1024;

impl MailSender for Graph {
    async fn send_as(&self) -> Result<Vec<Address>> {
        // Shared mailboxes / send-as delegates come later.
        Ok(Vec::new())
    }

    async fn send_mime(&self, mime: &[u8], _thread_id: Option<&str>) -> Result<()> {
        if mime.len() > MIME_MAX {
            return Err(Error::Other("messages over 3 MB can't be sent from Microsoft accounts yet".into()));
        }
        let token = self.tokens.access_token().await?;
        let req = client()
            .post(format!("{BASE}/me/sendMail"))
            .bearer_auth(&token)
            .header("Content-Type", "text/plain")
            .body(STANDARD.encode(mime));
        send_once(req).await.map(drop)
    }
}
