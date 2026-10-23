//! Stable JSON shapes for `--json` output and MCP tool results. Field names
//! are part of the agent-facing contract: add fields, don't rename them.
//! Times are RFC 3339 in the local zone; all-day dates are `YYYY-MM-DD`.

use chrono::{Local, TimeZone};
use pigeon_core::model::*;
use serde::Serialize;

pub fn iso(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn iso_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

#[derive(Debug, Serialize)]
pub struct AddrOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

impl From<&Address> for AddrOut {
    fn from(a: &Address) -> Self {
        AddrOut { name: a.name.clone(), email: a.email.clone() }
    }
}

fn addrs(v: &[Address]) -> Vec<AddrOut> {
    v.iter().map(AddrOut::from).collect()
}

#[derive(Debug, Serialize)]
pub struct AccountOut {
    pub id: String,
    pub provider: &'static str,
    pub email: String,
    pub name: String,
}

impl From<&Account> for AccountOut {
    fn from(a: &Account) -> Self {
        AccountOut {
            id: a.id.clone(),
            provider: a.provider.as_str(),
            email: a.email.clone(),
            name: a.display_name.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FolderOut {
    pub account: String,
    pub id: String,
    pub name: String,
    pub role: Option<&'static str>,
    pub kind: &'static str,
    pub parent_id: Option<String>,
    pub unread: u32,
    pub total: u32,
}

impl FolderOut {
    pub fn new(account: &str, f: &FolderInfo) -> Self {
        FolderOut {
            account: account.to_string(),
            id: f.folder.id.clone(),
            name: f.folder.name.clone(),
            role: f.folder.role.map(FolderRole::as_str),
            kind: f.folder.kind.as_str(),
            parent_id: f.folder.parent_id.clone(),
            unread: f.unread,
            total: f.total,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MessageOut {
    pub account: String,
    pub id: String,
    pub thread_id: String,
    pub subject: String,
    pub from: Option<AddrOut>,
    pub to: Vec<AddrOut>,
    pub cc: Vec<AddrOut>,
    pub snippet: String,
    pub received_at: String,
    pub unread: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub draft: bool,
}

impl MessageOut {
    pub fn new(account: &str, h: &MessageHeader) -> Self {
        MessageOut {
            account: account.to_string(),
            id: h.id.clone(),
            thread_id: h.thread_id.clone(),
            subject: h.subject.clone(),
            from: h.from.as_ref().map(AddrOut::from),
            to: addrs(&h.to),
            cc: addrs(&h.cc),
            snippet: h.snippet.clone(),
            received_at: iso(h.received_at),
            unread: !h.is_read,
            flagged: h.is_flagged,
            has_attachments: h.has_attachments,
            draft: h.is_draft,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ThreadOut {
    pub account: String,
    pub thread_id: String,
    pub subject: String,
    pub participants: Vec<String>,
    pub message_count: u32,
    pub unread_count: u32,
    pub latest: MessageOut,
}

impl From<&ThreadRow> for ThreadOut {
    fn from(t: &ThreadRow) -> Self {
        ThreadOut {
            account: t.account_id.clone(),
            thread_id: t.thread_id.clone(),
            subject: t.latest.subject.clone(),
            participants: t.participants.clone(),
            message_count: t.message_count,
            unread_count: t.unread_count,
            latest: MessageOut::new(&t.account_id, &t.latest),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AttachmentOut {
    pub id: String,
    pub name: String,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    pub inline: bool,
}

#[derive(Debug, Serialize)]
pub struct FullMessageOut {
    #[serde(flatten)]
    pub message: MessageOut,
    /// Plain text (from the text part, or converted from HTML).
    pub text: String,
    /// True if `text` was cut at `max_chars`.
    pub truncated: bool,
    pub attachments: Vec<AttachmentOut>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AttendeeOut {
    pub email: String,
    pub name: Option<String>,
    pub response: &'static str,
    pub optional: bool,
}

#[derive(Debug, Serialize)]
pub struct EventOut {
    pub account: String,
    pub calendar_id: String,
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub location: Option<String>,
    pub organizer: Option<AddrOut>,
    pub attendees: Vec<AttendeeOut>,
    pub my_response: &'static str,
    pub online_url: Option<String>,
    pub cancelled: bool,
}

impl From<&EventRow> for EventOut {
    fn from(r: &EventRow) -> Self {
        let e = &r.event;
        let t = |ms| if e.all_day { iso_date(ms) } else { iso(ms) };
        EventOut {
            account: r.account_id.clone(),
            calendar_id: e.calendar_id.clone(),
            id: e.id.clone(),
            title: e.title.clone(),
            start: t(e.start_at),
            // All-day ends are exclusive midnights; report the last day.
            end: if e.all_day { iso_date(e.end_at - 1) } else { iso(e.end_at) },
            all_day: e.all_day,
            location: e.location.clone(),
            organizer: e.organizer.as_ref().map(AddrOut::from),
            attendees: e
                .attendees
                .iter()
                .map(|a| AttendeeOut {
                    email: a.address.email.clone(),
                    name: a.address.name.clone(),
                    response: a.response.as_str(),
                    optional: a.optional,
                })
                .collect(),
            my_response: e.my_response.as_str(),
            online_url: e.online_url.clone(),
            cancelled: e.cancelled,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SpanOut {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Serialize)]
pub struct BusyOut {
    pub email: String,
    pub busy: Vec<SpanOut>,
}

#[derive(Debug, Serialize)]
pub struct SyncPartOut {
    pub ok: bool,
    pub error: Option<String>,
    #[serde(flatten)]
    pub stats: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct SyncOut {
    pub account: String,
    pub mail: SyncPartOut,
    pub calendar: SyncPartOut,
}

#[derive(Debug, Serialize)]
pub struct ActOut {
    /// Pass to `undo` to revert.
    pub undo_handle: i64,
    pub sent: usize,
    pub deferred: usize,
    pub rejected: Vec<String>,
    pub needs_sign_in: bool,
}

#[derive(Debug, Serialize)]
pub struct DraftOut {
    pub draft_id: String,
    pub from: AddrOut,
    pub from_account: String,
    pub to: Vec<AddrOut>,
    pub cc: Vec<AddrOut>,
    pub subject: String,
    pub crosses_accounts: bool,
    /// Set once sent.
    pub message_id: Option<String>,
}

impl DraftOut {
    pub fn new(d: &pigeon_core::compose::Draft, message_id: Option<String>) -> Self {
        DraftOut {
            draft_id: d.id.clone(),
            from: AddrOut::from(&d.from.address),
            from_account: d.from.account_id.clone(),
            to: addrs(&d.to),
            cc: addrs(&d.cc),
            subject: d.subject.clone(),
            crosses_accounts: d.crosses_accounts(),
            message_id,
        }
    }
}
