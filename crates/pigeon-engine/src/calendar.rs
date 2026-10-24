//! Calendar writes, RSVP and free/busy. Writes hit the server directly; the
//! store is updated from what the server returns.

use pigeon_core::model::{Account, Event, EventDraft, FreeBusy, Response};
use pigeon_core::provider::CalendarWriter;
use pigeon_core::{Error, Result};

use crate::{Backend, Pigeon};

impl Pigeon {
    pub async fn create_event(&self, account: &Account, draft: &EventDraft) -> Result<Event> {
        check_conferencing(account, draft)?;
        let event = match &*self.backend(account)? {
            Backend::Microsoft(p) => p.create_event(draft).await?,
            Backend::Google(p) => p.create_event(draft).await?,
            Backend::ICloud(p) => p.cal.create_event(draft).await?,
        };
        self.store().upsert_event(&account.id, &event)?;
        Ok(event)
    }

    pub async fn update_event(&self, account: &Account, event_id: &str, draft: &EventDraft) -> Result<Event> {
        check_conferencing(account, draft)?;
        let event = match &*self.backend(account)? {
            Backend::Microsoft(p) => p.update_event(event_id, draft).await?,
            Backend::Google(p) => p.update_event(event_id, draft).await?,
            Backend::ICloud(p) => p.cal.update_event(event_id, draft).await?,
        };
        self.store().upsert_event(&account.id, &event)?;
        Ok(event)
    }

    pub async fn delete_event(&self, account: &Account, calendar_id: &str, event_id: &str) -> Result<()> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.delete_event(calendar_id, event_id).await?,
            Backend::Google(p) => p.delete_event(calendar_id, event_id).await?,
            Backend::ICloud(p) => p.cal.delete_event(calendar_id, event_id).await?,
        }
        self.store().delete_event(&account.id, calendar_id, event_id)
    }

    /// Accept / tentatively accept / decline, notifying the organizer.
    pub async fn rsvp(
        &self,
        account: &Account,
        calendar_id: &str,
        event_id: &str,
        response: Response,
        comment: Option<&str>,
    ) -> Result<()> {
        if !matches!(response, Response::Accepted | Response::Tentative | Response::Declined) {
            return Err(Error::Other("RSVP must be accepted, tentative or declined".into()));
        }
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.respond(calendar_id, event_id, response, comment).await?,
            Backend::Google(p) => p.respond(calendar_id, event_id, response, comment).await?,
            Backend::ICloud(p) => p.cal.respond(calendar_id, event_id, response, comment).await?,
        }
        self.store().set_my_response(&account.id, calendar_id, event_id, response)
    }

    /// Busy intervals for `emails` over `[from, to)`, as seen by `account`
    /// (use the account the event will be created on).
    pub async fn free_busy(&self, account: &Account, emails: &[String], from: i64, to: i64) -> Result<FreeBusy> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.free_busy(emails, from, to).await,
            Backend::Google(p) => p.free_busy(emails, from, to).await,
            Backend::ICloud(p) => p.cal.free_busy(emails, from, to).await,
        }
    }

    /// The account's primary calendar id, from the local store.
    pub fn primary_calendar(&self, account: &Account) -> Result<String> {
        let cals = self.store().calendars(&account.id)?;
        cals.iter()
            .find(|c| c.is_primary)
            .or(cals.first())
            .map(|c| c.id.clone())
            .ok_or_else(|| Error::Other(format!("no calendars synced for {}", account.email)))
    }
}

/// A Meet link can only be created by a Google calendar (the provider attaches
/// it to its own event); say so rather than silently dropping the request.
fn check_conferencing(account: &Account, draft: &EventDraft) -> Result<()> {
    use pigeon_core::model::{Conferencing, ProviderKind};
    match draft.conferencing {
        Conferencing::GoogleMeet if account.provider != ProviderKind::Google => {
            Err(pigeon_core::Error::Other("Google Meet links can only be added to events on a Google calendar".into()))
        }
        _ => Ok(()),
    }
}
