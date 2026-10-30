//! Sender photos from Microsoft 365: a Microsoft account that allowed it
//! (Settings › Appearance; never asked at sign-in) keeps an index of its
//! Outlook contacts (hashed address → contact id, in the same table as
//! Google's, refreshed about daily by `contact_photos`), and, where its
//! organization allows `User.ReadBasic.All`, a coworker's profile photo is
//! fetched on demand. Only hashed addresses and contact ids are kept.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use pigeon_core::auth::{self, MS_CONTACTS_SCOPE, MS_PEOPLE_SCOPE, OAuthApp, ms_has_scope};
use pigeon_core::model::{Account, ProviderKind};
use pigeon_core::photos::address_hash;
use pigeon_core::{Error, Result, now_ms};
use pigeon_graph::Graph;

use crate::Pigeon;

/// A Microsoft contact's entry in the contact photo index (its "URL").
const CONTACT: &str = "graph-contact:";
/// How long an organization's domains, and a coworker without a photo, are
/// trusted before asking again.
const ORG_TTL_MS: i64 = 24 * 3600 * 1000;

/// An account's organization domains, with when they were read.
type Domains = (Vec<String>, i64);

/// Each account's organization domains.
static ORG_DOMAINS: LazyLock<Mutex<HashMap<String, Domains>>> = LazyLock::new(Default::default);
/// Coworkers (address hash) found without a photo, with when.
static NO_PHOTO: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(Default::default);

/// Whether a failed consent looks like the organization withholding
/// `User.ReadBasic.All` (an admin must approve), so contacts alone are
/// worth asking for.
fn needs_admin(message: &str) -> bool {
    ["AADSTS90094", "AADSTS65001", "AADSTS65004", "admin", "consent_required", "access_denied"]
        .iter()
        .any(|m| message.contains(m))
}

/// The address's domain, lowercased.
fn domain(email: &str) -> Option<String> {
    email.rsplit_once('@').map(|(_, d)| d.trim().to_lowercase()).filter(|d| !d.is_empty())
}

impl Pigeon {
    /// Microsoft accounts whose sign-in allows sender photos (contacts or
    /// coworkers), in account order.
    pub fn microsoft_photo_accounts(&self) -> Result<Vec<String>> {
        let store = self.store();
        let mut out = Vec::new();
        for a in store.accounts()? {
            if a.provider == ProviderKind::Microsoft
                && store
                    .account_scopes(&a.id)?
                    .is_some_and(|s| ms_has_scope(&s, MS_CONTACTS_SCOPE) || ms_has_scope(&s, MS_PEOPLE_SCOPE))
            {
                out.push(a.id);
            }
        }
        Ok(out)
    }

    /// Microsoft accounts allowed to read their Outlook contacts.
    pub(crate) fn microsoft_contact_accounts(&self) -> Result<Vec<String>> {
        let store = self.store();
        self.microsoft_photo_accounts_with(&store, MS_CONTACTS_SCOPE)
    }

    fn microsoft_photo_accounts_with(&self, store: &pigeon_core::store::Store, scope: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for a in store.accounts()? {
            if a.provider == ProviderKind::Microsoft
                && store.account_scopes(&a.id)?.is_some_and(|s| ms_has_scope(&s, scope))
            {
                out.push(a.id);
            }
        }
        Ok(out)
    }

    /// Ask Microsoft (in the browser) to let an existing account show sender
    /// photos: its contacts and its coworkers. Where the organization needs
    /// an admin to approve coworkers' profiles, ask again for contacts only.
    pub async fn allow_microsoft_photos(&self, account_id: &str, on_url: impl Fn(&str)) -> Result<()> {
        let account = self.account(account_id)?;
        if account.provider != ProviderKind::Microsoft {
            return Err(Error::Other("Microsoft 365 photos need a Microsoft account".into()));
        }
        let app = OAuthApp::from_env(ProviderKind::Microsoft)?;
        let token = match auth::authorize(&app.clone().microsoft_add_photo_scopes(&account.email, false), &on_url).await
        {
            Ok(t) => t,
            Err(e) if needs_admin(&e.to_string()) => {
                tracing::info!("coworkers' photos need an admin; asking for contacts only");
                auth::authorize(&app.microsoft_add_photo_scopes(&account.email, true), &on_url).await?
            }
            Err(e) => return Err(e),
        };
        let (email, _) = pigeon_graph::whoami(&token.access_token).await?;
        if !email.eq_ignore_ascii_case(&account.email) {
            return Err(Error::Auth(format!(
                "That was a different Microsoft account. Choose {} to show its contacts' photos.",
                account.email
            )));
        }
        let scopes = token.scope.clone().unwrap_or_default();
        if !ms_has_scope(&scopes, MS_CONTACTS_SCOPE) && !ms_has_scope(&scopes, MS_PEOPLE_SCOPE) {
            return Err(Error::Auth("Coo wasn't allowed to see your contacts. Try again and accept.".into()));
        }
        let refresh =
            token.refresh_token.as_deref().ok_or_else(|| Error::Auth("Microsoft returned no refresh token".into()))?;
        self.tokens.set(&account.id, refresh)?;
        self.store().set_account_scopes(&account.id, &scopes)?;
        // Fresh token source + backend, refreshing with the new scopes.
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        ORG_DOMAINS.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        NO_PHOTO.lock().unwrap_or_else(|p| p.into_inner()).clear();
        self.refresh_microsoft_contacts(&account).await
    }

    fn graph(&self, account: &Account) -> Result<Graph> {
        Ok(Graph::new(self.token_source(account)?))
    }

    /// Read the account's Outlook contacts again (address → contact id).
    pub(crate) async fn refresh_microsoft_contacts(&self, account: &Account) -> Result<()> {
        let scopes = self.store().account_scopes(&account.id)?.unwrap_or_default();
        if !ms_has_scope(&scopes, MS_CONTACTS_SCOPE) {
            return Ok(());
        }
        let index = self.graph(account)?.contact_index().await?;
        tracing::debug!(count = index.len(), "Outlook contacts read");
        let rows: Vec<(String, String)> =
            index.into_iter().map(|(email, id)| (email, format!("{CONTACT}{id}"))).collect();
        self.store().replace_contact_photos(&account.id, &rows, now_ms())
    }

    /// The sender's photo from Microsoft 365: an Outlook contact's, else a
    /// coworker's profile photo. None when there's none.
    pub async fn microsoft_photo(&self, email: &str) -> Result<Option<Vec<u8>>> {
        let contacts = self.microsoft_contact_accounts()?;
        if !contacts.is_empty() {
            let found = self.store().contact_photo(email, &contacts)?;
            if let Some((account_id, entry)) = found
                && let Some(id) = entry.strip_prefix(CONTACT)
            {
                let account = self.account(&account_id)?;
                if let Some(photo) = self.graph(&account)?.contact_photo(id).await? {
                    return Ok(Some(photo));
                }
            }
        }
        self.coworker_photo(email).await
    }

    /// A coworker's profile photo, from the first account in their
    /// organization that may see it.
    async fn coworker_photo(&self, email: &str) -> Result<Option<Vec<u8>>> {
        let Some(their_domain) = domain(email) else { return Ok(None) };
        let hash = address_hash(email);
        let now = now_ms();
        if NO_PHOTO.lock().unwrap_or_else(|p| p.into_inner()).get(&hash).is_some_and(|t| now - t < ORG_TTL_MS) {
            return Ok(None);
        }
        let people = {
            let store = self.store();
            self.microsoft_photo_accounts_with(&store, MS_PEOPLE_SCOPE)?
        };
        let mut asked = false;
        for id in people {
            let account = self.account(&id)?;
            if !self.org_domains(&account).await.contains(&their_domain) {
                continue;
            }
            asked = true;
            if let Some(photo) = self.graph(&account)?.user_photo(email).await? {
                return Ok(Some(photo));
            }
        }
        if asked {
            NO_PHOTO.lock().unwrap_or_else(|p| p.into_inner()).insert(hash, now);
        }
        Ok(None)
    }

    /// The account's organization domains (its own domain when they can't
    /// be read), cached for a day.
    async fn org_domains(&self, account: &Account) -> Vec<String> {
        let now = now_ms();
        if let Some((domains, at)) = ORG_DOMAINS.lock().unwrap_or_else(|p| p.into_inner()).get(&account.id)
            && now - at < ORG_TTL_MS
        {
            return domains.clone();
        }
        let read = match self.graph(account) {
            Ok(g) => g.org_domains().await,
            Err(e) => Err(e),
        };
        let mut domains = read.unwrap_or_else(|e| {
            tracing::debug!(error = %e, "organization domains unavailable");
            Vec::new()
        });
        domains.extend(domain(&account.email));
        domains.sort();
        domains.dedup();
        ORG_DOMAINS.lock().unwrap_or_else(|p| p.into_inner()).insert(account.id.clone(), (domains.clone(), now));
        domains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_only_consent_falls_back_to_contacts() {
        assert!(needs_admin("sign-in failed: access_denied: AADSTS65001: The user or administrator has not consented"));
        assert!(needs_admin("AADSTS90094: Admin consent is required"));
        assert!(!needs_admin("timed out waiting for the browser sign-in"));
    }

    #[test]
    fn domains() {
        assert_eq!(domain("Ava@Halyard.CO").as_deref(), Some("halyard.co"));
        assert_eq!(domain("nobody"), None);
    }
}
