//! Sender photos from Google contacts: each Google account that allowed
//! contacts access keeps an index of its contacts' photo URLs (refreshed
//! about daily), and a sender's photo comes from the first account that
//! knows them. Only addresses (hashed) and photo URLs are kept.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use pigeon_core::auth::{GOOGLE_CONTACTS_SCOPE, GOOGLE_DIRECTORY_SCOPE, GOOGLE_OTHER_CONTACTS_SCOPE};
use pigeon_core::drive::has_scope;
use pigeon_core::model::{Account, ProviderKind};
use pigeon_core::{Result, now_ms};
use pigeon_google::people::{People, Source};

use crate::Pigeon;

/// How often an account's contact photos are read again.
pub const REFRESH_MS: i64 = 24 * 3600 * 1000;
/// How long after a failed read the next try waits.
const RETRY_MS: i64 = 3600 * 1000;

/// When each account's last read failed.
static FAILED: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(Default::default);

/// Whether an index synced at `synced_at` is due for a refresh at `now`.
pub fn due(synced_at: Option<i64>, now: i64) -> bool {
    synced_at.is_none_or(|t| now - t >= REFRESH_MS || t > now)
}

impl Pigeon {
    /// Whether the account's sign-in may read its contacts' photos.
    pub async fn google_contacts_ready(&self, account_id: &str) -> Result<bool> {
        self.google_scope_ready(account_id, GOOGLE_OTHER_CONTACTS_SCOPE).await
    }

    /// Ask Google (in the browser) for contacts access on an existing Google
    /// account, then read its contacts' photos.
    pub async fn allow_google_contacts(&self, account_id: &str, on_url: impl FnOnce(&str)) -> Result<()> {
        let denied = "Coo wasn't allowed to see your Google contacts. Try again and allow contacts access.";
        self.add_google_scope(account_id, GOOGLE_OTHER_CONTACTS_SCOPE, "show its contacts' photos", denied, on_url)
            .await?;
        let account = self.account(account_id)?;
        self.refresh_contact_photos(&account).await
    }

    /// Google accounts whose sign-in allows contact photos, in account order
    /// (without asking Google about accounts with unrecorded scopes).
    pub fn contact_photo_accounts(&self) -> Result<Vec<String>> {
        let store = self.store();
        let mut out = Vec::new();
        for a in store.accounts()? {
            if a.provider == ProviderKind::Google
                && store.account_scopes(&a.id)?.is_some_and(|s| has_scope(&s, GOOGLE_OTHER_CONTACTS_SCOPE))
            {
                out.push(a.id);
            }
        }
        Ok(out)
    }

    /// Read the account's contact photos again: people it mailed with, plus
    /// its own contacts and Workspace directory where those are allowed.
    pub async fn refresh_contact_photos(&self, account: &Account) -> Result<()> {
        let scopes = self.store().account_scopes(&account.id)?.unwrap_or_default();
        if !has_scope(&scopes, GOOGLE_OTHER_CONTACTS_SCOPE) {
            return Ok(());
        }
        let people = People::new(self.token_source(account)?);
        let mut photos = people.photos(Source::OtherContacts).await?;
        for (source, scope) in [(Source::Contacts, GOOGLE_CONTACTS_SCOPE), (Source::Directory, GOOGLE_DIRECTORY_SCOPE)]
        {
            if !has_scope(&scopes, scope) {
                continue;
            }
            match people.photos(source).await {
                Ok(more) => photos.extend(more),
                Err(e) => tracing::debug!(error = %e, ?source, "contact photos: a source failed"),
            }
        }
        // Saved contacts and the directory win over "other contacts".
        photos.reverse();
        let mut seen = std::collections::HashSet::new();
        photos.retain(|(email, _)| seen.insert(email.clone()));
        tracing::debug!(count = photos.len(), "contact photos read");
        self.store().replace_contact_photos(&account.id, &photos, now_ms())
    }

    /// Refresh every allowed account whose index is a day old (or missing);
    /// true when any was read. One run at a time: the sync tick asks every
    /// 30 s.
    pub async fn refresh_contact_photos_due(&self) -> bool {
        static RUNNING: AtomicBool = AtomicBool::new(false);
        if RUNNING.swap(true, Ordering::AcqRel) {
            return false;
        }
        let read = self.refresh_due_accounts().await;
        RUNNING.store(false, Ordering::Release);
        read
    }

    async fn refresh_due_accounts(&self) -> bool {
        let Ok(ids) = self.contact_photo_accounts() else { return false };
        let mut read = false;
        for id in ids {
            let synced = self.store().contact_photos_synced_at(&id).ok().flatten();
            if !due(synced, now_ms()) {
                continue;
            }
            // After a failure (API off, offline…), wait before trying again.
            let failed_at = FAILED.lock().unwrap_or_else(|p| p.into_inner()).get(&id).copied();
            if failed_at.is_some_and(|t| now_ms() - t < RETRY_MS) {
                continue;
            }
            let Ok(account) = self.account(&id) else { continue };
            match self.refresh_contact_photos(&account).await {
                Ok(()) => {
                    read = true;
                    FAILED.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "could not read Google contact photos");
                    FAILED.lock().unwrap_or_else(|p| p.into_inner()).insert(id, now_ms());
                }
            }
        }
        read
    }

    /// The sender's photo from a Google account's contacts, or None.
    pub async fn contact_photo(&self, email: &str) -> Result<Option<Vec<u8>>> {
        let accounts = self.contact_photo_accounts()?;
        if accounts.is_empty() {
            return Ok(None);
        }
        let Some((account_id, url)) = self.store().contact_photo(email, &accounts)? else { return Ok(None) };
        let account = self.account(&account_id)?;
        let token = self.token_source(&account)?.access_token().await.ok();
        pigeon_core::photos::contact_photo(&url, token.as_deref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_about_daily() {
        assert!(due(None, 1_000));
        assert!(!due(Some(1_000), 1_000 + REFRESH_MS - 1));
        assert!(due(Some(1_000), 1_000 + REFRESH_MS));
        assert!(due(Some(5_000), 1_000), "a clock that went back refreshes");
    }
}
