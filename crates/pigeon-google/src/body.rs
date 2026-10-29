//! Gmail bodies via `messages.get?format=full`: walk the MIME part tree,
//! decode bodies in their declared charset, list attachments, and fetch
//! inline (cid) parts eagerly. Attachment ids are Gmail part ids, which are
//! stable; Gmail's own `attachmentId` is not, so it is re-resolved on download.

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use pigeon_core::model::{AttachmentMeta, MessageBody};
use pigeon_core::provider::BodyProvider;
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{GMAIL, Google};

/// Inline parts bigger than this are listed but not fetched eagerly.
const INLINE_EAGER_MAX: u64 = 2 * 1024 * 1024;

const B64URL: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Deserialize)]
struct FullMessage {
    payload: Option<Part>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(default)]
    part_id: String,
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    headers: Vec<Header>,
    #[serde(default)]
    body: PartBody,
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PartBody {
    #[serde(default)]
    size: u64,
    data: Option<String>,
    attachment_id: Option<String>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct AttachmentData {
    data: String,
}

impl Part {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
    }

    fn find(&self, part_id: &str) -> Option<&Part> {
        if self.part_id == part_id {
            return Some(self);
        }
        self.parts.iter().find_map(|p| p.find(part_id))
    }

    fn is_attachment(&self) -> bool {
        let disposition = self.header("Content-Disposition").unwrap_or_default().to_ascii_lowercase();
        !self.filename.is_empty()
            || disposition.starts_with("attachment")
            || (self.header("Content-ID").is_some() && !self.mime_type.starts_with("text/"))
    }
}

/// What the walk found; text bodies may still need fetching by attachment id.
#[derive(Default)]
struct Walk<'a> {
    html: Option<&'a Part>,
    text: Option<&'a Part>,
    /// iCalendar part of a meeting invite.
    calendar: Option<&'a Part>,
    attachments: Vec<&'a Part>,
}

fn walk<'a>(part: &'a Part, out: &mut Walk<'a>) {
    let is_ics =
        part.mime_type == "text/calendar" || part.mime_type == "application/ics" || part.filename.ends_with(".ics");
    if is_ics && out.calendar.is_none() {
        out.calendar = Some(part);
    }
    if part.mime_type.starts_with("multipart/") {
        for p in &part.parts {
            walk(p, out);
        }
    } else if part.mime_type == "text/calendar" && !part.is_attachment() {
        // Inline invite alternative: not a body, not an attachment.
    } else if part.is_attachment() {
        out.attachments.push(part);
    } else if part.mime_type == "text/html" && out.html.is_none() {
        out.html = Some(part);
    } else if part.mime_type == "text/plain" && out.text.is_none() {
        out.text = Some(part);
    } else if part.mime_type == "message/rfc822" || !part.mime_type.starts_with("text/") {
        out.attachments.push(part);
    }
}

fn charset(part: &Part) -> Option<String> {
    let ct = part.header("Content-Type")?;
    ct.split(';').map(str::trim).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("charset").then(|| v.trim().trim_matches('"').to_string())
    })
}

/// Bytes in the part's declared charset to a String (UTF-8 fallback).
fn decode_text(bytes: &[u8], charset: Option<&str>) -> String {
    let enc = charset.and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes())).unwrap_or(encoding_rs::UTF_8);
    enc.decode(bytes).0.into_owned()
}

fn b64(data: &str) -> Result<Vec<u8>> {
    B64URL.decode(data.trim()).map_err(|e| Error::Other(format!("bad base64 from Gmail: {e}")))
}

/// `References` / `In-Reply-To` values to a list of `<id>` tokens.
pub(crate) fn message_ids(value: &str) -> Vec<String> {
    value.split_whitespace().filter(|t| t.starts_with('<') && t.ends_with('>')).map(String::from).collect()
}

fn meta(part: &Part) -> AttachmentMeta {
    let content_id =
        part.header("Content-ID").map(|c| c.trim().trim_start_matches('<').trim_end_matches('>').to_string());
    let disposition = part.header("Content-Disposition").unwrap_or_default().to_ascii_lowercase();
    AttachmentMeta {
        id: part.part_id.clone(),
        name: if part.filename.is_empty() { format!("part-{}", part.part_id) } else { part.filename.clone() },
        content_type: Some(part.mime_type.clone()).filter(|m| !m.is_empty()),
        size: Some(part.body.size),
        is_inline: content_id.is_some() && !disposition.starts_with("attachment"),
        content_id,
    }
}

impl Google {
    async fn full(&self, message_id: &str) -> Result<Part> {
        let m: FullMessage = self.get(&format!("{GMAIL}/messages/{message_id}"), &[("format", "full".into())]).await?;
        m.payload.ok_or_else(|| Error::Other(format!("Gmail message {message_id} has no payload")))
    }

    async fn part_bytes(&self, message_id: &str, part: &Part) -> Result<Vec<u8>> {
        if let Some(data) = &part.body.data {
            return b64(data);
        }
        let Some(att) = &part.body.attachment_id else { return Ok(Vec::new()) };
        let d: AttachmentData = self.get(&format!("{GMAIL}/messages/{message_id}/attachments/{att}"), &[]).await?;
        b64(&d.data)
    }

    async fn part_text(&self, message_id: &str, part: Option<&Part>) -> Result<Option<String>> {
        let Some(part) = part else { return Ok(None) };
        let bytes = self.part_bytes(message_id, part).await?;
        Ok(Some(decode_text(&bytes, charset(part).as_deref())))
    }
}

impl BodyProvider for Google {
    async fn raw(&self, message_id: &str) -> Result<Vec<u8>> {
        #[derive(serde::Deserialize)]
        struct Raw {
            raw: String,
        }
        let r: Raw = self.get(&format!("{GMAIL}/messages/{message_id}"), &[("format", "raw".to_string())]).await?;
        // base64url, padded or not.
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(r.raw.trim_end_matches('='))
            .map_err(|e| Error::Other(format!("unreadable message: {e}")))
    }

    async fn body(&self, message_id: &str) -> Result<MessageBody> {
        let root = self.full(message_id).await?;
        let mut w = Walk::default();
        walk(&root, &mut w);
        let html = self.part_text(message_id, w.html).await?;
        let text = self.part_text(message_id, w.text).await?;
        let invite = match self.part_text(message_id, w.calendar).await {
            Ok(ics) => ics.as_deref().and_then(pigeon_core::ics::parse_invite),
            Err(e) => {
                tracing::warn!(error = %e, "could not read invite part");
                None
            }
        };
        let attachments: Vec<AttachmentMeta> = w.attachments.iter().map(|p| meta(p)).collect();
        let mut inline_data = Vec::new();
        for (part, m) in w.attachments.iter().zip(&attachments) {
            if m.is_inline && part.body.size <= INLINE_EAGER_MAX {
                inline_data.push((m.id.clone(), self.part_bytes(message_id, part).await?));
            }
        }
        Ok(MessageBody {
            html,
            text,
            attachments,
            inline_data,
            in_reply_to: root.header("In-Reply-To").and_then(|v| message_ids(v).into_iter().next()),
            references: root.header("References").map(message_ids).unwrap_or_default(),
            invite,
            origin: Some(pigeon_core::identity::origin_from_headers(
                root.headers.iter().map(|h| (h.name.as_str(), h.value.as_str())),
            )),
        })
    }

    async fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let root = self.full(message_id).await?;
        let part = root
            .find(attachment_id)
            .ok_or_else(|| Error::Other(format!("attachment {attachment_id} not found in {message_id}")))?;
        self.part_bytes(message_id, part).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Part {
        serde_json::from_str(
            r#"{
              "partId": "", "mimeType": "multipart/mixed",
              "headers": [{"name": "References", "value": "<a@x> <b@x>"}, {"name": "In-Reply-To", "value": "<b@x>"}],
              "parts": [
                {"partId": "0", "mimeType": "multipart/related", "parts": [
                  {"partId": "0.0", "mimeType": "multipart/alternative", "parts": [
                    {"partId": "0.0.0", "mimeType": "text/plain",
                     "headers": [{"name": "Content-Type", "value": "text/plain; charset=\"windows-1255\""}],
                     "body": {"size": 4, "data": "-ezl7Q"}},
                    {"partId": "0.0.1", "mimeType": "text/html",
                     "headers": [{"name": "Content-Type", "value": "text/html; charset=UTF-8"}],
                     "body": {"size": 10, "data": "PHA-aGk8L3A-"}}
                  ]},
                  {"partId": "0.1", "mimeType": "image/png", "filename": "",
                   "headers": [{"name": "Content-ID", "value": "<logo@x>"}, {"name": "Content-Disposition", "value": "inline"}],
                   "body": {"size": 3, "attachmentId": "ANGjdJ"}}
                ]},
                {"partId": "1", "mimeType": "application/pdf", "filename": "deck.pdf",
                 "headers": [{"name": "Content-Disposition", "value": "attachment; filename=deck.pdf"}],
                 "body": {"size": 1234, "attachmentId": "ANGjdK"}}
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn walk_classifies_parts() {
        let root = fixture();
        let mut w = Walk::default();
        walk(&root, &mut w);
        assert_eq!(w.text.unwrap().part_id, "0.0.0");
        assert_eq!(w.html.unwrap().part_id, "0.0.1");
        let metas: Vec<_> = w.attachments.iter().map(|p| meta(p)).collect();
        assert_eq!(metas.len(), 2);
        assert_eq!((metas[0].content_id.as_deref(), metas[0].is_inline), (Some("logo@x"), true));
        assert_eq!((metas[1].name.as_str(), metas[1].is_inline, metas[1].size), ("deck.pdf", false, Some(1234)));
        assert_eq!(root.find("1").unwrap().filename, "deck.pdf");
        assert_eq!(message_ids(root.header("References").unwrap()), ["<a@x>", "<b@x>"]);
    }

    #[test]
    fn finds_inline_invite_part() {
        let root: Part = serde_json::from_str(
            r#"{"partId": "", "mimeType": "multipart/mixed", "parts": [
                {"partId": "0", "mimeType": "multipart/alternative", "parts": [
                  {"partId": "0.0", "mimeType": "text/plain", "body": {"size": 1, "data": "aGk"}},
                  {"partId": "0.1", "mimeType": "text/calendar",
                   "headers": [{"name": "Content-Type", "value": "text/calendar; charset=\"UTF-8\"; method=REQUEST"}],
                   "body": {"size": 10, "data": "QkVHSU4"}}
                ]},
                {"partId": "1", "mimeType": "application/ics", "filename": "invite.ics", "body": {"size": 10, "attachmentId": "A"}}
            ]}"#,
        )
        .unwrap();
        let mut w = Walk::default();
        walk(&root, &mut w);
        assert_eq!(w.calendar.unwrap().part_id, "0.1");
        assert_eq!(w.text.unwrap().part_id, "0.0");
        assert_eq!(w.attachments.len(), 1, "invite.ics still listed as an attachment");
    }

    #[test]
    fn decodes_legacy_hebrew_charset() {
        let root = fixture();
        let text = root.find("0.0.0").unwrap();
        let bytes = b64(text.body.data.as_deref().unwrap()).unwrap();
        assert_eq!(decode_text(&bytes, charset(text).as_deref()), "שלום");
        let html = root.find("0.0.1").unwrap();
        assert_eq!(decode_text(&b64(html.body.data.as_deref().unwrap()).unwrap(), None), "<p>hi</p>");
    }
}
