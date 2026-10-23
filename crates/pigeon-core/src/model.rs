//! Provider-agnostic data model. Microsoft Graph and Google payloads are
//! normalized into these types before they reach the store or the UI.
//!
//! Timestamps are Unix epoch milliseconds (UTC) throughout.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Microsoft,
    Google,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Microsoft => "microsoft",
            ProviderKind::Google => "google",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "microsoft" | "ms" | "m365" | "outlook" | "exchange" => Some(ProviderKind::Microsoft),
            "google" | "gmail" | "gsuite" | "workspace" => Some(ProviderKind::Google),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: ProviderKind,
    pub email: String,
    pub display_name: String,
}

impl Account {
    pub fn make_id(provider: ProviderKind, email: &str) -> String {
        format!("{}:{}", provider.as_str(), email.to_lowercase())
    }
}

/// Well-known folder semantics, shared by Exchange folders and Gmail labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Spam,
    Archive,
    Starred,
    Important,
}

impl FolderRole {
    pub const ALL: [FolderRole; 8] = [
        FolderRole::Inbox,
        FolderRole::Sent,
        FolderRole::Drafts,
        FolderRole::Trash,
        FolderRole::Spam,
        FolderRole::Archive,
        FolderRole::Starred,
        FolderRole::Important,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FolderRole::Inbox => "inbox",
            FolderRole::Sent => "sent",
            FolderRole::Drafts => "drafts",
            FolderRole::Trash => "trash",
            FolderRole::Spam => "spam",
            FolderRole::Archive => "archive",
            FolderRole::Starred => "starred",
            FolderRole::Important => "important",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str().eq_ignore_ascii_case(s))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderKind {
    /// Exchange folder: a message lives in exactly one.
    Folder,
    /// Gmail label: a message can carry many.
    Label,
}

impl FolderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FolderKind::Folder => "folder",
            FolderKind::Label => "label",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "label" { FolderKind::Label } else { FolderKind::Folder }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub role: Option<FolderRole>,
    pub kind: FolderKind,
}

/// A folder plus counts computed from the local store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    pub folder: Folder,
    pub total: u32,
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Address {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

impl Address {
    pub fn new(name: Option<&str>, email: &str) -> Self {
        let name = name.map(str::trim).filter(|n| !n.is_empty() && *n != email);
        Address { name: name.map(String::from), email: email.trim().to_string() }
    }

    /// Name if known, else the address.
    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.email)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageHeader {
    pub id: String,
    /// Gmail `threadId` / Graph `conversationId`.
    pub thread_id: String,
    pub internet_message_id: Option<String>,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub snippet: String,
    pub received_at: i64,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachments: bool,
    pub is_draft: bool,
    /// Folder/label membership. Authoritative on `MailChange::Upsert`;
    /// left empty on rows read back from list queries.
    pub folder_ids: Vec<String>,
}

// Upserts dominate; boxing them would just add an allocation per message.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailChange {
    /// Insert or replace a message, including its full folder membership.
    Upsert(MessageHeader),
    /// The message left this folder (Graph per-folder delta). Messages left in
    /// no folder are deleted.
    Unlink { id: String, folder_id: String },
    /// The message is gone.
    Delete { id: String },
}

/// A message as listed in the UI, tagged with its account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub account_id: String,
    pub header: MessageHeader,
}

/// One row of the threaded (conversation) list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRow {
    pub account_id: String,
    pub thread_id: String,
    /// Newest message of the thread within the listed folder.
    pub latest: MessageHeader,
    /// Messages in the whole thread (all folders).
    pub message_count: u32,
    /// Unread messages of the thread within the listed folder.
    pub unread_count: u32,
    /// Distinct sender display names within the listed folder.
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    /// `#rrggbb` when the provider supplies one.
    pub color: Option<String>,
    pub is_primary: bool,
    pub can_edit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Response {
    #[default]
    None,
    Organizer,
    Accepted,
    Tentative,
    Declined,
}

impl Response {
    pub fn as_str(self) -> &'static str {
        match self {
            Response::None => "none",
            Response::Organizer => "organizer",
            Response::Accepted => "accepted",
            Response::Tentative => "tentative",
            Response::Declined => "declined",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "organizer" => Response::Organizer,
            "accepted" => Response::Accepted,
            "tentative" => Response::Tentative,
            "declined" => Response::Declined,
            _ => Response::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub address: Address,
    pub response: Response,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub calendar_id: String,
    /// Recurring series this instance belongs to.
    pub series_id: Option<String>,
    pub title: String,
    pub start_at: i64,
    /// Exclusive.
    pub end_at: i64,
    /// All-day events start/end at UTC midnight of their (floating) dates.
    pub all_day: bool,
    /// IANA or Windows zone the event was authored in, when known.
    pub timezone: Option<String>,
    pub location: Option<String>,
    pub organizer: Option<Address>,
    pub attendees: Vec<Attendee>,
    pub my_response: Response,
    pub online_url: Option<String>,
    pub cancelled: bool,
    pub updated_at: i64,
    /// RFC 5545 UID (Google `iCalUID`, Graph `iCalUId`); links invites to events.
    #[serde(default)]
    pub ical_uid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub account_id: String,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AttachmentMeta {
    /// Provider-scoped id, stable for this message (Graph attachment id,
    /// Gmail part id).
    pub id: String,
    pub name: String,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    /// `Content-ID` without angle brackets; referenced as `cid:` in HTML.
    pub content_id: Option<String>,
    pub is_inline: bool,
}

/// Body as fetched from a provider, before sanitizing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageBody {
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
    /// Bytes of inline parts (cid images), fetched eagerly so the body
    /// renders complete: (attachment id, bytes).
    pub inline_data: Vec<(String, Vec<u8>)>,
    pub in_reply_to: Option<String>,
    /// `References` message ids, oldest first.
    pub references: Vec<String>,
    /// The meeting this message invites to / updates / cancels.
    pub invite: Option<Invite>,
}

/// Body as kept in the store and handed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredBody {
    /// Sanitized HTML, safe to drop into the sandboxed body frame.
    pub html: Option<String>,
    pub text: Option<String>,
    /// The HTML references remote images/CSS (blocked until allowed).
    pub has_remote_content: bool,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<StoredAttachment>,
    pub fetched_at: i64,
    pub invite: Option<Invite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredAttachment {
    pub meta: AttachmentMeta,
    /// Downloaded copy under the cache dir.
    pub local_path: Option<String>,
}

/// A server-side mail mutation, applied optimistically to the store first
/// and replayed to the provider through the outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MailOp {
    SetRead {
        ids: Vec<String>,
        read: bool,
    },
    SetFlagged {
        ids: Vec<String>,
        flagged: bool,
    },
    /// Label providers (Gmail).
    Relabel {
        ids: Vec<String>,
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// Folder providers (Exchange): the message ends up in exactly this folder.
    MoveTo {
        ids: Vec<String>,
        folder_id: String,
    },
    /// Gmail trash/untrash (Exchange trashing is a `MoveTo`).
    Trash {
        ids: Vec<String>,
    },
    Untrash {
        ids: Vec<String>,
    },
}

impl MailOp {
    pub fn ids(&self) -> &[String] {
        match self {
            MailOp::SetRead { ids, .. }
            | MailOp::SetFlagged { ids, .. }
            | MailOp::Relabel { ids, .. }
            | MailOp::MoveTo { ids, .. }
            | MailOp::Trash { ids }
            | MailOp::Untrash { ids } => ids,
        }
    }
}

/// The mutable bits of a message, captured before an op for undo/rollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageState {
    pub id: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub folder_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    Pending,
    /// Sent; kept briefly so it can still be undone.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub id: i64,
    pub account_id: String,
    pub op: MailOp,
    pub snapshot: Vec<MessageState>,
    pub attempts: u32,
    pub state: OutboxState,
    pub last_error: Option<String>,
}

/// One incremental calendar change.
// Upserts dominate; boxing them would just add an allocation per event.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarChange {
    Upsert(Event),
    /// Gone (deleted or cancelled). Also removes instances of a series with this id.
    Delete {
        id: String,
    },
}

/// Online meeting to attach when creating an event (phase 2 adds providers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conferencing {
    #[default]
    None,
}

/// What the user edits in the event composer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventDraft {
    pub calendar_id: String,
    pub title: String,
    pub start_at: i64,
    /// Exclusive. For all-day events: UTC midnight after the last day.
    pub end_at: i64,
    pub all_day: bool,
    /// Zone to author the event in; timed events are sent in UTC when unset.
    pub timezone: Option<String>,
    pub location: Option<String>,
    /// Plain text.
    pub description: Option<String>,
    /// Invitees; `response` is ignored.
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub conferencing: Conferencing,
}

/// Busy intervals `[start, end)` per attendee email.
pub type FreeBusy = Vec<(String, Vec<(i64, i64)>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InviteMethod {
    /// New invitation or update.
    #[default]
    Request,
    Cancel,
    /// Someone's RSVP (accept/decline notification).
    Reply,
}

/// A meeting invite carried by an email, as parsed from its iCalendar part
/// (Gmail) or the linked event (Graph).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Invite {
    pub method: InviteMethod,
    /// RFC 5545 UID.
    pub uid: Option<String>,
    /// Provider event id when the provider links it directly (Graph).
    pub event_id: Option<String>,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub all_day: bool,
    pub location: Option<String>,
    pub organizer: Option<Address>,
    pub online_url: Option<String>,
}
