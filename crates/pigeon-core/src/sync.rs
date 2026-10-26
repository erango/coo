//! Provider-agnostic sync drivers: pull pages from a provider and apply them
//! to the store, one transaction per page.

use std::sync::Mutex;

use futures::{StreamExt, stream};

use crate::model::Calendar;
use crate::provider::{CalendarProvider, MailProvider};
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MailStats {
    pub folders: usize,
    pub pages: usize,
    pub changes: usize,
}

pub async fn sync_mail<P: MailProvider>(p: &P, account_id: &str, store: &Mutex<Store>) -> Result<MailStats> {
    let folders = p.folders().await?;
    lock(store).replace_folders(account_id, &folders)?;
    let mut stats = MailStats { folders: folders.len(), ..Default::default() };
    tracing::info!(account_id, folders = folders.len(), "mail sync");

    // Scopes run concurrently up to the provider's limit; one failing
    // scope doesn't stop the others. The first error is returned at the end.
    let scopes = p.mail_scopes(&folders);
    // Owned items: closures over references trip the `Send` check when the
    // whole sync is spawned.
    let results: Vec<Result<(usize, usize)>> = stream::iter(scopes)
        .map(|scope| async move { sync_scope(p, account_id, &scope, store).await })
        .buffer_unordered(p.mail_concurrency().max(1))
        .collect()
        .await;
    let mut first_err = None;
    for r in results {
        match r {
            Ok((pages, changes)) => {
                stats.pages += pages;
                stats.changes += changes;
            }
            Err(e) => {
                tracing::warn!(account_id, error = %e, "mail scope failed");
                first_err.get_or_insert(e);
            }
        }
    }
    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(stats)
}

/// Page one scope to completion. Returns (pages, changes).
async fn sync_scope<P: MailProvider>(
    p: &P,
    account_id: &str,
    scope: &str,
    store: &Mutex<Store>,
) -> Result<(usize, usize)> {
    let (mut pages, mut changes) = (0, 0);
    let mut cursor = lock(store).cursor(account_id, scope)?;
    loop {
        let page = match p.mail_page(scope, cursor.as_deref()).await {
            Err(Error::CursorExpired) if cursor.is_some() => {
                tracing::warn!(account_id, scope, "sync cursor expired; resyncing scope");
                lock(store).clear_cursor(account_id, scope)?;
                cursor = None;
                continue;
            }
            page => page?,
        };
        pages += 1;
        changes += page.changes.len();
        tracing::info!(account_id, scope, changes = page.changes.len(), more = page.more, "mail page");
        lock(store).apply_mail(account_id, scope, &page.changes, &page.cursor)?;
        if !page.more {
            return Ok((pages, changes));
        }
        cursor = Some(page.cursor);
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CalendarStats {
    pub calendars: usize,
    pub events: usize,
}

/// Sync every calendar: incremental where the provider keeps a cursor,
/// otherwise a full snapshot of `[from, to)`.
pub async fn sync_calendars<P: CalendarProvider>(
    p: &P,
    account_id: &str,
    store: &Mutex<Store>,
    from: i64,
    to: i64,
) -> Result<CalendarStats> {
    let calendars = p.calendars().await?;
    lock(store).replace_calendars(account_id, &calendars)?;
    let mut stats = CalendarStats { calendars: calendars.len(), events: 0 };
    tracing::info!(account_id, calendars = calendars.len(), "calendar sync");
    let results: Vec<(String, Result<usize>)> = stream::iter(calendars.clone())
        .map(|cal| async move {
            tracing::info!(account_id, "syncing calendar");
            let r = sync_calendar(p, account_id, &cal, store, from, to).await;
            (cal.name, r)
        })
        .buffer_unordered(p.calendar_concurrency().max(1))
        .collect()
        .await;
    let mut first_err = None;
    for (name, r) in results {
        match r {
            Ok(n) => stats.events += n,
            // Shared calendars can be listed but not readable (free/busy only).
            Err(e) if matches!(e.status(), Some(403 | 404)) => {
                tracing::warn!(calendar = name, error = %e, "skipping unreadable calendar");
            }
            Err(e) => {
                tracing::warn!(calendar = name, error = %e, "calendar sync failed");
                first_err.get_or_insert(e);
            }
        }
    }
    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(stats)
}

async fn sync_calendar<P: CalendarProvider>(
    p: &P,
    account_id: &str,
    cal: &Calendar,
    store: &Mutex<Store>,
    from: i64,
    to: i64,
) -> Result<usize> {
    let scope = format!("cal:{}", cal.id);
    let mut saved = lock(store).cursor(account_id, &scope)?;
    let mut request = saved.clone();
    let mut first = true;
    let mut changes = 0;
    loop {
        let page = match p.event_page(cal, from, to, request.as_deref()).await {
            Err(Error::CursorExpired) if saved.is_some() => {
                tracing::warn!(account_id, scope, "calendar cursor expired; full resync");
                lock(store).clear_cursor(account_id, &scope)?;
                (saved, request, first) = (None, None, true);
                continue;
            }
            page => page?,
        };
        changes += page.changes.len();
        // A fresh snapshot replaces whatever we had for the calendar.
        let reset = first && saved.is_none();
        let cursor = if page.next.is_none() { page.sync_cursor.as_deref() } else { None };
        lock(store).apply_events(account_id, &cal.id, &scope, &page.changes, reset, cursor)?;
        match page.next {
            Some(next) => {
                request = Some(next);
                first = false;
            }
            None => return Ok(changes),
        }
    }
}

pub(crate) fn lock(store: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::*;
    use crate::provider::{EventPage, MailPage};
    use crate::store::ListQuery;

    /// Two pages, then an expired cursor on the next run.
    struct Fake {
        calls: AtomicUsize,
    }

    impl MailProvider for Fake {
        async fn folders(&self) -> Result<Vec<Folder>> {
            Ok(vec![Folder {
                id: "INBOX".into(),
                name: "Inbox".into(),
                parent_id: None,
                role: Some(FolderRole::Inbox),
                kind: FolderKind::Label,
            }])
        }

        fn mail_scopes(&self, _: &[Folder]) -> Vec<String> {
            vec!["mail".into()]
        }

        async fn mail_page(&self, _scope: &str, cursor: Option<&str>) -> Result<MailPage> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let upsert = |id: &str| {
                MailChange::Upsert(MessageHeader {
                    id: id.into(),
                    thread_id: id.into(),
                    received_at: 1,
                    folder_ids: vec!["INBOX".into()],
                    ..Default::default()
                })
            };
            match cursor {
                None => Ok(MailPage { changes: vec![upsert("a")], cursor: "p2".into(), more: true }),
                Some("p2") => Ok(MailPage { changes: vec![upsert("b")], cursor: "done".into(), more: false }),
                Some("done") => Err(Error::CursorExpired),
                Some(other) => panic!("unexpected cursor {other}"),
            }
        }
    }

    #[tokio::test]
    async fn pages_until_done_and_recovers_from_expired_cursor() {
        let store = Mutex::new(Store::open_in_memory().unwrap());
        let acct = Account {
            id: "google:a@b.c".into(),
            provider: ProviderKind::Google,
            email: "a@b.c".into(),
            display_name: String::new(),
        };
        store.lock().unwrap().upsert_account(&acct).unwrap();
        let fake = Fake { calls: AtomicUsize::new(0) };

        let stats = sync_mail(&fake, &acct.id, &store).await.unwrap();
        assert_eq!((stats.pages, stats.changes), (2, 2));
        assert_eq!(store.lock().unwrap().cursor(&acct.id, "mail").unwrap().as_deref(), Some("done"));

        // Next run: "done" has expired -> full resync from scratch.
        let stats = sync_mail(&fake, &acct.id, &store).await.unwrap();
        assert_eq!(stats.pages, 2);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 5);
        let rows = store.lock().unwrap().list_messages(&ListQuery::unified(FolderRole::Inbox, 10)).unwrap();
        assert_eq!(rows.len(), 2);
    }

    /// Snapshot in two pages, then incremental: one change, then expiry.
    struct FakeCal {
        calls: AtomicUsize,
    }

    fn ev(id: &str) -> Event {
        Event {
            id: id.into(),
            calendar_id: "c".into(),
            title: id.into(),
            start_at: 10,
            end_at: 20,
            ..Default::default()
        }
    }

    impl CalendarProvider for FakeCal {
        async fn calendars(&self) -> Result<Vec<Calendar>> {
            Ok(vec![Calendar { id: "c".into(), name: "C".into(), color: None, is_primary: true, can_edit: true }])
        }

        async fn event_page(&self, _: &Calendar, _: i64, _: i64, cursor: Option<&str>) -> Result<EventPage> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(match cursor {
                None => EventPage {
                    changes: vec![CalendarChange::Upsert(ev("a"))],
                    next: Some("p2".into()),
                    sync_cursor: None,
                },
                Some("p2") => EventPage {
                    changes: vec![CalendarChange::Upsert(ev("b"))],
                    next: None,
                    sync_cursor: Some("s1".into()),
                },
                Some("s1") => EventPage {
                    changes: vec![CalendarChange::Delete { id: "a".into() }],
                    next: None,
                    sync_cursor: Some("s2".into()),
                },
                Some("s2") => return Err(Error::CursorExpired),
                Some(other) => panic!("unexpected cursor {other}"),
            })
        }
    }

    fn event_ids(store: &Mutex<Store>) -> Vec<String> {
        store.lock().unwrap().events_between(0, 100).unwrap().into_iter().map(|e| e.event.id).collect()
    }

    #[tokio::test]
    async fn calendar_snapshot_then_incremental_then_resync() {
        let store = Mutex::new(Store::open_in_memory().unwrap());
        let acct = Account {
            id: "google:a@b.c".into(),
            provider: ProviderKind::Google,
            email: "a@b.c".into(),
            display_name: String::new(),
        };
        store.lock().unwrap().upsert_account(&acct).unwrap();
        let p = FakeCal { calls: AtomicUsize::new(0) };

        sync_calendars(&p, &acct.id, &store, 0, 100).await.unwrap();
        assert_eq!(event_ids(&store), ["a", "b"]);
        assert_eq!(store.lock().unwrap().cursor(&acct.id, "cal:c").unwrap().as_deref(), Some("s1"));

        sync_calendars(&p, &acct.id, &store, 0, 100).await.unwrap();
        assert_eq!(event_ids(&store), ["b"]);

        // s2 expired: full snapshot again.
        sync_calendars(&p, &acct.id, &store, 0, 100).await.unwrap();
        assert_eq!(event_ids(&store), ["a", "b"]);
        assert_eq!(p.calls.load(Ordering::SeqCst), 6);
    }
}
