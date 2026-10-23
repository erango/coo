//! Traits each backend (Microsoft Graph, Google) implements. The sync driver
//! in `sync.rs` is written once against these.

use std::future::Future;

use crate::Result;
use crate::model::{
    Address, Calendar, CalendarChange, Event, EventDraft, Folder, FreeBusy, MailChange, MailOp, MessageBody, Response,
};

/// One page of mail changes for a sync scope.
#[derive(Debug, Default)]
pub struct MailPage {
    pub changes: Vec<MailChange>,
    /// Opaque cursor to resume from; persisted with the changes.
    pub cursor: String,
    /// More pages are available right now.
    pub more: bool,
}

pub trait MailProvider: Sync {
    fn folders(&self) -> impl Future<Output = Result<Vec<Folder>>> + Send;

    /// Independent sync scopes (e.g. one per Exchange folder; one for all of
    /// Gmail). Cursors are stored per scope.
    fn mail_scopes(&self, folders: &[Folder]) -> Vec<String>;

    /// Fetch the next page for `scope`, starting fresh when `cursor` is None.
    /// Returns `Error::CursorExpired` when the cursor is no longer valid.
    fn mail_page(&self, scope: &str, cursor: Option<&str>) -> impl Future<Output = Result<MailPage>> + Send;
}

/// One page of calendar changes.
#[derive(Debug, Default)]
pub struct EventPage {
    pub changes: Vec<CalendarChange>,
    /// Fetch the next page now with this cursor.
    pub next: Option<String>,
    /// On the last page: cursor to persist for the next incremental sync.
    /// `None` means the provider can't sync this calendar incrementally;
    /// it is refetched in full (within the window) every time.
    pub sync_cursor: Option<String>,
}

pub trait CalendarProvider: Sync {
    fn calendars(&self) -> impl Future<Output = Result<Vec<Calendar>>> + Send;

    /// Next page of changes for `calendar`. With no cursor, returns a full
    /// snapshot of expanded instances overlapping `[from, to)` (epoch ms);
    /// with one, the changes since. Returns `Error::CursorExpired` when the
    /// cursor is no longer accepted.
    fn event_page(
        &self,
        calendar: &Calendar,
        from: i64,
        to: i64,
        cursor: Option<&str>,
    ) -> impl Future<Output = Result<EventPage>> + Send;
}

pub trait BodyProvider: Sync {
    /// Full body, attachment list, threading headers, and inline part bytes.
    fn body(&self, message_id: &str) -> impl Future<Output = Result<MessageBody>> + Send;

    fn attachment(&self, message_id: &str, attachment_id: &str) -> impl Future<Output = Result<Vec<u8>>> + Send;
}

pub trait MailActions: Sync {
    /// Perform `op` on the server. Must be idempotent (it may be retried) and
    /// treat already-gone messages as success.
    fn apply(&self, op: &MailOp) -> impl Future<Output = Result<()>> + Send;
}

pub trait MailSender: Sync {
    /// Extra verified addresses this account may send as (aliases). The
    /// account's own address is implied.
    fn send_as(&self) -> impl Future<Output = Result<Vec<Address>>> + Send;

    /// Send a complete RFC 5322 message. `thread_id` files it into an
    /// existing provider thread when known.
    fn send_mime(&self, mime: &[u8], thread_id: Option<&str>) -> impl Future<Output = Result<()>> + Send;
}

/// Calendar writes go straight to the server; the returned event is then
/// stored locally.
pub trait CalendarWriter: Sync {
    fn create_event(&self, draft: &EventDraft) -> impl Future<Output = Result<Event>> + Send;

    fn update_event(&self, event_id: &str, draft: &EventDraft) -> impl Future<Output = Result<Event>> + Send;

    /// Already-gone events count as deleted.
    fn delete_event(&self, calendar_id: &str, event_id: &str) -> impl Future<Output = Result<()>> + Send;

    /// RSVP to an invitation (`Accepted` / `Tentative` / `Declined`),
    /// notifying the organizer.
    fn respond(
        &self,
        calendar_id: &str,
        event_id: &str,
        response: Response,
        comment: Option<&str>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Busy intervals for each address over `[from, to)`.
    fn free_busy(&self, emails: &[String], from: i64, to: i64) -> impl Future<Output = Result<FreeBusy>> + Send;
}
