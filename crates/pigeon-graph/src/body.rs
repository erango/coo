//! Graph bodies: message `body` + `internetMessageHeaders` (threading), the
//! attachment list, and inline attachments fetched eagerly for `cid:` images.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pigeon_core::http::{client, send};
use pigeon_core::model::{AttachmentMeta, Invite, InviteMethod, MessageBody};
use pigeon_core::provider::BodyProvider;
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{BASE, Graph};

const INLINE_EAGER_MAX: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyMessage {
    #[serde(rename = "@odata.type")]
    odata_type: Option<String>,
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

/// `eventMessage` with its linked event expanded.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventMessage {
    meeting_message_type: Option<String>,
    event: Option<crate::types::GEvent>,
}

/// Exchange marks calendar messages with this header; `@odata.type` is the
/// other signal (when the server includes it under `$select`).
fn is_event_message(m: &BodyMessage) -> bool {
    m.odata_type.as_deref().is_some_and(|t| t.contains("eventMessage"))
        || header(&m.internet_message_headers, "Content-Class").is_some_and(|v| v.contains("calendarmessage"))
}

fn invite_from(m: EventMessage) -> Option<Invite> {
    let method = match m.meeting_message_type.as_deref()? {
        "meetingRequest" => InviteMethod::Request,
        "meetingCancelled" => InviteMethod::Cancel,
        "meetingAccepted" | "meetingTenativelyAccepted" | "meetingTentativelyAccepted" | "meetingDeclined" => {
            InviteMethod::Reply
        }
        _ => return None,
    };
    let event = crate::calendar::to_event(m.event?, "")?;
    Some(Invite {
        method,
        uid: event.ical_uid.clone(),
        event_id: Some(event.id.clone()),
        title: event.title,
        start_at: event.start_at,
        end_at: event.end_at,
        all_day: event.all_day,
        location: event.location,
        organizer: event.organizer,
        online_url: event.online_url,
    })
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

        let invite = if is_event_message(&msg) {
            let url = format!("{BASE}/me/messages/{message_id}?$expand=microsoft.graph.eventMessage/event");
            match self.get::<EventMessage>(&url).await {
                Ok(m) => invite_from(m),
                Err(e) => {
                    tracing::warn!(error = %e, "could not load the invite's event");
                    None
                }
            }
        } else {
            None
        };
        let (html, text) = split_body(msg.body);
        let headers = &msg.internet_message_headers;
        Ok(MessageBody {
            html,
            text,
            attachments,
            inline_data,
            in_reply_to: header(headers, "In-Reply-To").and_then(|v| message_ids(v).into_iter().next()),
            references: header(headers, "References").map(message_ids).unwrap_or_default(),
            invite,
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
    fn event_message_to_invite() {
        let m: EventMessage = serde_json::from_str(
            r#"{"meetingMessageType": "meetingRequest", "event": {
                "id": "EV1", "iCalUId": "040000008200E0", "subject": "Hiring sync",
                "start": {"dateTime": "2026-10-26T09:30:00.0000000", "timeZone": "UTC"},
                "end": {"dateTime": "2026-10-26T10:00:00.0000000", "timeZone": "UTC"},
                "organizer": {"emailAddress": {"name": "Marcus Lee", "address": "marcus@halyard.co"}},
                "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup-join/x"}
            }}"#,
        )
        .unwrap();
        let inv = invite_from(m).unwrap();
        assert_eq!(inv.method, InviteMethod::Request);
        assert_eq!(inv.event_id.as_deref(), Some("EV1"));
        assert_eq!(inv.uid.as_deref(), Some("040000008200E0"));
        assert_eq!(inv.end_at - inv.start_at, 30 * 60_000);
        assert!(inv.online_url.is_some());
        let cancel: EventMessage =
            serde_json::from_str(r#"{"meetingMessageType": "meetingCancelled", "event": null}"#).unwrap();
        assert!(invite_from(cancel).is_none(), "no event to show");
        let m: BodyMessage = serde_json::from_str(
            r#"{"internetMessageHeaders": [{"name": "Content-Class", "value": "urn:content-classes:calendarmessage"}]}"#,
        )
        .unwrap();
        assert!(is_event_message(&m));
    }

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
