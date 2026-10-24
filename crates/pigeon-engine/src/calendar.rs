//! Calendar writes, RSVP and free/busy. Writes hit the server directly; the
//! store is updated from what the server returns.

use pigeon_core::model::{Account, Conferencing, Event, EventDraft, FreeBusy, Response, ZoomMeetingLink};
use pigeon_core::provider::CalendarWriter;
use pigeon_core::{Error, Result};

use crate::zoom::{meeting_times, rollback_on_error};
use crate::{Backend, Pigeon};

impl Pigeon {
    /// Create an event. With Zoom conferencing the meeting is made first and
    /// its link goes into the event's location and description; if the
    /// calendar then refuses the event, the meeting is deleted again.
    pub async fn create_event(&self, account: &Account, draft: &EventDraft) -> Result<Event> {
        check_conferencing(account, draft)?;
        let (draft, meeting) = self.attach_zoom(draft, true).await?;
        let result = self.provider_create(account, &draft).await;
        let (event, meeting) =
            rollback_on_error(meeting, result, async |m: (pigeon_zoom::Zoom, pigeon_zoom::Meeting)| {
                m.0.delete(&m.1.id).await
            })
            .await?;
        let store = self.store();
        store.upsert_event(&account.id, &event)?;
        if let Some((_, m)) = meeting {
            store.set_zoom_meeting(
                &account.id,
                &event.id,
                &ZoomMeetingLink { meeting_id: m.id, join_url: m.join_url },
            )?;
        }
        Ok(event)
    }

    /// Update an event; a Zoom meeting Coo made for it follows the new time
    /// and title (best effort: the event change itself already succeeded).
    pub async fn update_event(&self, account: &Account, event_id: &str, draft: &EventDraft) -> Result<Event> {
        check_conferencing(account, draft)?;
        let existing = self.store().zoom_meeting(&account.id, event_id)?;
        // Adding Zoom to an event that doesn't have one yet.
        let add = existing.is_none() && draft.conferencing == Conferencing::Zoom;
        let (draft, meeting) = if add { self.attach_zoom(draft, false).await? } else { (no_conferencing(draft), None) };
        let result = self.provider_update(account, event_id, &draft).await;
        let (event, meeting) =
            rollback_on_error(meeting, result, async |m: (pigeon_zoom::Zoom, pigeon_zoom::Meeting)| {
                m.0.delete(&m.1.id).await
            })
            .await?;
        self.store().upsert_event(&account.id, &event)?;
        if let Some((_, m)) = meeting {
            self.store().set_zoom_meeting(
                &account.id,
                &event.id,
                &ZoomMeetingLink { meeting_id: m.id, join_url: m.join_url },
            )?;
        } else if let Some(link) = existing {
            match self.zoom() {
                Ok(zoom) => {
                    if let Err(e) = zoom.update(&link.meeting_id, &meeting_times(&draft)).await {
                        tracing::warn!(error = %e, "couldn't reschedule the Zoom meeting");
                    }
                }
                Err(e) => tracing::info!(error = %e, "Zoom not connected; meeting not rescheduled"),
            }
        }
        Ok(event)
    }

    /// Delete an event, and the Zoom meeting Coo made for it.
    pub async fn delete_event(&self, account: &Account, calendar_id: &str, event_id: &str) -> Result<()> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.delete_event(calendar_id, event_id).await?,
            Backend::Google(p) => p.delete_event(calendar_id, event_id).await?,
            Backend::ICloud(p) => p.cal.delete_event(calendar_id, event_id).await?,
        }
        let link = self.store().zoom_meeting(&account.id, event_id)?;
        if let Some(link) = link {
            match self.zoom() {
                Ok(zoom) => {
                    if let Err(e) = zoom.delete(&link.meeting_id).await {
                        tracing::warn!(error = %e, "couldn't delete the Zoom meeting");
                    }
                }
                Err(e) => tracing::info!(error = %e, "Zoom not connected; meeting left in place"),
            }
            self.store().delete_zoom_meeting(&account.id, event_id)?;
        }
        self.store().delete_event(&account.id, calendar_id, event_id)
    }

    /// For Zoom conferencing: create the meeting and write its link into the
    /// draft (location always; description only when the draft sets one, since
    /// an edit's empty description means "keep the server's").
    async fn attach_zoom(
        &self,
        draft: &EventDraft,
        is_new: bool,
    ) -> Result<(EventDraft, Option<(pigeon_zoom::Zoom, pigeon_zoom::Meeting)>)> {
        if draft.conferencing != Conferencing::Zoom {
            return Ok((no_conferencing(draft), None));
        }
        let zoom = self.zoom()?;
        let meeting = zoom.create(&meeting_times(draft)).await?;
        let mut d = no_conferencing(draft);
        let (location, description) =
            pigeon_zoom::add_to_event(draft.location.as_deref(), draft.description.as_deref(), &meeting);
        d.location = Some(location);
        if is_new || draft.description.is_some() {
            d.description = Some(description);
        }
        Ok((d, Some((zoom, meeting))))
    }

    async fn provider_create(&self, account: &Account, draft: &EventDraft) -> Result<Event> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.create_event(draft).await,
            Backend::Google(p) => p.create_event(draft).await,
            Backend::ICloud(p) => p.cal.create_event(draft).await,
        }
    }

    async fn provider_update(&self, account: &Account, event_id: &str, draft: &EventDraft) -> Result<Event> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.update_event(event_id, draft).await,
            Backend::Google(p) => p.update_event(event_id, draft).await,
            Backend::ICloud(p) => p.cal.update_event(event_id, draft).await,
        }
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
        Conferencing::Zoom if draft.all_day => {
            Err(pigeon_core::Error::Other("Zoom meetings need a start and end time, not an all-day event".into()))
        }
        _ => Ok(()),
    }
}

/// The draft as the calendar provider sees it: Zoom is handled by Coo (the
/// link is already in the location/description), so providers only ever
/// get Google Meet requests.
fn no_conferencing(d: &EventDraft) -> EventDraft {
    let mut d = d.clone();
    if d.conferencing == Conferencing::Zoom {
        d.conferencing = Conferencing::None;
    }
    d
}
