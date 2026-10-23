//! Graph bodies: message `body` + `internetMessageHeaders` (threading), the
//! attachment list, and inline attachments fetched eagerly for `cid:` images.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pigeon_core::http::{client, send};
use pigeon_core::model::{AttachmentMeta, MessageBody};
use pigeon_core::provider::BodyProvider;
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{BASE, Graph};

const INLINE_EAGER_MAX: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyMessage {
    body: Option<ItemBody>,
    #[serde(default)]
    internet_message_headers: Vec<MessageHeader>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemBody {
    content_type: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize)]
struct MessageHeader {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attachment {
    id: String,
    name: Option<String>,
    content_type: Option<String>,
    size: Option<u64>,
    #[serde(default)]
    is_inline: bool,
    #[serde(rename = "@odata.type")]
    odata_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileAttachment {
    content_id: Option<String>,
    content_bytes: Option<String>,
}

fn message_ids(value: &str) -> Vec<String> {
    value.split_whitespace().filter(|t| t.starts_with('<') && t.ends_with('>')).map(String::from).collect()
}

fn split_body(body: Option<ItemBody>) -> (Option<String>, Option<String>) {
    let Some(b) = body else { return (None, None) };
    let content = b.content.filter(|c| !c.is_empty());
    match b.content_type.as_deref() {
        Some("html") => (content, None),
        _ => (None, content),
    }
}

fn header<'a>(headers: &'a [MessageHeader], name: &str) -> Option<&'a str> {
    headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

impl BodyProvider for Graph {
    async fn body(&self, message_id: &str) -> Result<MessageBody> {
        let msg_url = format!("{BASE}/me/messages/{message_id}?$select=body,internetMessageHeaders");
        let att_url =
            format!("{BASE}/me/messages/{message_id}/attachments?$select=id,name,contentType,size,isInline&$top=100");
        let (msg, atts) = futures::join!(self.get::<BodyMessage>(&msg_url), self.get_all::<Attachment>(&att_url));
        let msg = msg?;
        // Item attachments (attached emails/events) have no bytes to inline.
        let atts: Vec<Attachment> = atts?;

        let mut attachments = Vec::with_capacity(atts.len());
        let mut inline_data = Vec::new();
        for a in atts {
            let is_file = a.odata_type.as_deref().is_none_or(|t| t.ends_with("fileAttachment"));
            let mut meta = AttachmentMeta {
                name: a.name.unwrap_or_else(|| "attachment".into()),
                content_type: a.content_type,
                size: a.size,
                content_id: None,
                is_inline: a.is_inline,
                id: a.id,
            };
            if is_file && a.is_inline && a.size.unwrap_or(0) <= INLINE_EAGER_MAX {
                let url = format!("{BASE}/me/messages/{message_id}/attachments/{}", meta.id);
                let file: FileAttachment = self.get(&url).await?;
                meta.content_id = file.content_id.map(|c| c.trim_start_matches('<').trim_end_matches('>').to_string());
                if let Some(bytes) = file.content_bytes {
                    let bytes =
                        STANDARD.decode(bytes).map_err(|e| Error::Other(format!("bad base64 from Graph: {e}")))?;
                    inline_data.push((meta.id.clone(), bytes));
                }
            }
            attachments.push(meta);
        }

        let (html, text) = split_body(msg.body);
        let headers = &msg.internet_message_headers;
        Ok(MessageBody {
            html,
            text,
            attachments,
            inline_data,
            in_reply_to: header(headers, "In-Reply-To").and_then(|v| message_ids(v).into_iter().next()),
            references: header(headers, "References").map(message_ids).unwrap_or_default(),
        })
    }

    async fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let url = format!("{BASE}/me/messages/{message_id}/attachments/{attachment_id}/$value");
        let token = self.tokens.access_token().await?;
        Ok(send(|| client().get(&url).bearer_auth(&token)).await?.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_headers() {
        let m: BodyMessage = serde_json::from_str(
            r#"{"body": {"contentType": "html", "content": "<p dir=\"rtl\">שלום</p>"},
                "internetMessageHeaders": [
                  {"name": "In-Reply-To", "value": "<b@x>"},
                  {"name": "References", "value": "<a@x>\r\n <b@x>"}
                ]}"#,
        )
        .unwrap();
        let headers = &m.internet_message_headers;
        assert_eq!(message_ids(header(headers, "references").unwrap()), ["<a@x>", "<b@x>"]);
        let (html, text) = split_body(m.body);
        assert!(html.unwrap().contains("שלום"));
        assert_eq!(text, None);

        let (html, text) = split_body(Some(ItemBody { content_type: Some("text".into()), content: Some("hi".into()) }));
        assert_eq!((html, text.as_deref()), (None, Some("hi")));
    }
}
