//! Sending raw MIME via Gmail, and send-as aliases.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pigeon_core::Result;
use pigeon_core::http::{client, send_once};
use pigeon_core::model::Address;
use pigeon_core::provider::MailSender;
use serde::Deserialize;
use serde_json::json;

use crate::{GMAIL, Google};

/// Above this, use the media upload endpoint (no threadId, 35 MB limit).
const JSON_SEND_MAX: usize = 3 * 1024 * 1024;
const UPLOAD_URL: &str = "https://gmail.googleapis.com/upload/gmail/v1/users/me/messages/send?uploadType=media";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendAsList {
    #[serde(default)]
    send_as: Vec<SendAs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendAs {
    send_as_email: String,
    display_name: Option<String>,
    #[serde(default)]
    is_primary: bool,
    verification_status: Option<String>,
}

fn aliases(list: SendAsList) -> Vec<Address> {
    list.send_as
        .into_iter()
        .filter(|s| !s.is_primary && s.verification_status.as_deref().is_none_or(|v| v == "accepted"))
        .map(|s| Address::new(s.display_name.as_deref(), &s.send_as_email))
        .collect()
}

impl MailSender for Google {
    async fn send_as(&self) -> Result<Vec<Address>> {
        Ok(aliases(self.get(&format!("{GMAIL}/settings/sendAs"), &[]).await?))
    }

    async fn send_mime(&self, mime: &[u8], thread_id: Option<&str>) -> Result<()> {
        let token = self.tokens.access_token().await?;
        let req = if mime.len() <= JSON_SEND_MAX {
            let mut body = json!({ "raw": URL_SAFE_NO_PAD.encode(mime) });
            if let Some(t) = thread_id {
                body["threadId"] = json!(t);
            }
            client().post(format!("{GMAIL}/messages/send")).bearer_auth(&token).json(&body)
        } else {
            client().post(UPLOAD_URL).bearer_auth(&token).header("Content-Type", "message/rfc822").body(mime.to_vec())
        };
        send_once(req).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_aliases() {
        let list: SendAsList = serde_json::from_str(
            r#"{"sendAs": [
              {"sendAsEmail": "me@gmail.com", "isPrimary": true},
              {"sendAsEmail": "support@biz.com", "displayName": "Biz Support", "verificationStatus": "accepted"},
              {"sendAsEmail": "pending@biz.com", "verificationStatus": "pending"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(aliases(list), vec![Address::new(Some("Biz Support"), "support@biz.com")]);
    }
}
