//! Wires providers to the core: sign-in, account registry, and sync across
//! all accounts concurrently. Phase 1 grows the Cmd/Delta command loop here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod actions;
mod calendar;
pub mod service;
pub mod telemetry;

pub use actions::MailAction;
use chrono::{Duration, Utc};
use futures::future::join_all;
use pigeon_core::actions::{FlushReport, flush_outbox};
use pigeon_core::auth::{self, OAuthApp, TokenSource};
use pigeon_core::bodies::{download_attachment, load_body};
use pigeon_core::compose::{AttachmentSource, Draft, ReplyKind, Sender, build_mime, default_sender, reply_draft};
use pigeon_core::model::{Account, Address, FolderRole, ProviderKind, StoredBody};
use pigeon_core::provider::MailSender;
use pigeon_core::store::{Store, undo_ops};
use pigeon_core::sync::{CalendarStats, MailStats, sync_calendars, sync_mail};
use pigeon_core::tokens::TokenStore;
use pigeon_core::{Error, Result, paths};
use pigeon_google::Google;
use pigeon_graph::Graph;

/// Calendar window kept locally, relative to now.
const CALENDAR_PAST_DAYS: i64 = 30;
const CALENDAR_FUTURE_DAYS: i64 = 365;
/// Sent ops stay undoable this long.
const UNDO_WINDOW_MS: i64 = 10 * 60_000;

pub struct Pigeon {
    pub store: Arc<Mutex<Store>>,
    tokens: TokenStore,
    pub data_dir: PathBuf,
    /// One backend (and so one token cache) per account.
    backends: Mutex<HashMap<String, Arc<Backend>>>,
}

#[derive(Debug)]
pub struct SyncReport {
    pub account: Account,
    pub mail: Result<MailStats>,
    pub calendar: Result<CalendarStats>,
}

enum Backend {
    Microsoft(Graph),
    Google(Google),
}

impl Pigeon {
    pub fn open() -> Result<Self> {
        let data_dir = paths::data_dir()?;
        let store = Store::open(data_dir.join("pigeon.db"))?;
        Ok(Pigeon {
            store: Arc::new(Mutex::new(store)),
            tokens: TokenStore::from_env(&data_dir),
            data_dir,
            backends: Mutex::default(),
        })
    }

    pub fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.store().accounts()
    }

    /// Interactive browser sign-in; adds (or refreshes) the account.
    pub async fn login(&self, provider: ProviderKind, on_url: impl FnOnce(&str)) -> Result<Account> {
        let app = OAuthApp::from_env(provider)?;
        let token = auth::authorize(&app, on_url).await?;
        let refresh =
            token.refresh_token.as_deref().ok_or_else(|| Error::Auth("provider returned no refresh token".into()))?;
        let (email, display_name) = match provider {
            ProviderKind::Microsoft => pigeon_graph::whoami(&token.access_token).await?,
            ProviderKind::Google => pigeon_google::whoami(&token.access_token).await?,
        };
        let account = Account { id: Account::make_id(provider, &email), provider, email, display_name };
        self.tokens.set(&account.id, refresh)?;
        self.store().upsert_account(&account)?;
        Ok(account)
    }

    pub fn logout(&self, account_id: &str) -> Result<()> {
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(account_id);
        self.tokens.delete(account_id)?;
        self.store().delete_account(account_id)
    }

    fn backend(&self, account: &Account) -> Result<Arc<Backend>> {
        let mut backends = self.backends.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(b) = backends.get(&account.id) {
            return Ok(b.clone());
        }
        let app = OAuthApp::from_env(account.provider)?;
        let tokens = Arc::new(TokenSource::new(app, &account.id, self.tokens.clone()));
        let backend = Arc::new(match account.provider {
            ProviderKind::Microsoft => Backend::Microsoft(Graph::new(tokens)),
            ProviderKind::Google => Backend::Google(Google::new(tokens)),
        });
        backends.insert(account.id.clone(), backend.clone());
        Ok(backend)
    }

    /// Message body, fetched and sanitized on first open, then served locally.
    pub async fn body(&self, account: &Account, message_id: &str) -> Result<StoredBody> {
        // Cached bodies open offline and without a valid sign-in.
        if let Some(body) = self.store().body(&account.id, message_id)? {
            return Ok(body);
        }
        let (id, store, cache) = (account.id.as_str(), &*self.store, self.data_dir.as_path());
        match &*self.backend(account)? {
            Backend::Microsoft(p) => load_body(p, id, message_id, store, cache).await,
            Backend::Google(p) => load_body(p, id, message_id, store, cache).await,
        }
    }

    /// Local path of an attachment, downloading on first use.
    pub async fn attachment(&self, account: &Account, message_id: &str, attachment_id: &str) -> Result<PathBuf> {
        let (id, store, cache) = (account.id.as_str(), &*self.store, self.data_dir.as_path());
        match &*self.backend(account)? {
            Backend::Microsoft(p) => download_attachment(p, id, message_id, attachment_id, store, cache).await,
            Backend::Google(p) => download_attachment(p, id, message_id, attachment_id, store, cache).await,
        }
    }

    /// Apply a user action locally right away and queue it for the server.
    /// Returns the undo handle. Call [`Pigeon::flush`] to send it.
    pub fn act(
        &self,
        account: &Account,
        ids: &[String],
        from_folder: Option<&str>,
        action: &MailAction,
    ) -> Result<i64> {
        let mut store = self.store();
        let op = actions::resolve(account.provider, action, ids.to_vec(), from_folder, |role| {
            store.role_folder(&account.id, role).ok().flatten()
        })?;
        store.enqueue(&account.id, &op)
    }

    /// Send queued actions for an account.
    pub async fn flush(&self, account: &Account) -> Result<FlushReport> {
        let backend = self.backend(account)?;
        self.flush_backend(account, &backend).await
    }

    async fn flush_backend(&self, account: &Account, backend: &Backend) -> Result<FlushReport> {
        let (id, store) = (account.id.as_str(), &*self.store);
        let report = match backend {
            Backend::Microsoft(p) => flush_outbox(p, id, store).await?,
            Backend::Google(p) => flush_outbox(p, id, store).await?,
        };
        self.store().prune_done_ops(pigeon_core::now_ms() - UNDO_WINDOW_MS)?;
        Ok(report)
    }

    /// Undo an action. Unsent: dropped and restored locally. Sent: inverse
    /// ops are queued (call [`Pigeon::flush`]).
    pub fn undo(&self, account: &Account, handle: i64) -> Result<()> {
        let mut store = self.store();
        if store.cancel_pending_op(handle)? {
            return Ok(());
        }
        let entry = store
            .outbox_entry(handle)?
            .filter(|e| e.account_id == account.id)
            .ok_or_else(|| Error::Other("nothing to undo (expired or unknown)".into()))?;
        let ids: Vec<String> = entry.snapshot.iter().map(|s| s.id.clone()).collect();
        let current = store.message_states(&account.id, &ids)?;
        let trash = store.role_folder(&account.id, FolderRole::Trash)?;
        let labels = account.provider == ProviderKind::Google;
        for op in undo_ops(labels, trash.as_deref(), &entry.snapshot, &current) {
            store.enqueue(&account.id, &op)?;
        }
        Ok(())
    }

    pub fn account(&self, account_id: &str) -> Result<Account> {
        self.accounts()?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or_else(|| Error::Other(format!("unknown account {account_id}")))
    }

    /// Every address the user can send from, across all accounts: each
    /// account's own address plus its verified aliases.
    pub async fn senders(&self) -> Result<Vec<Sender>> {
        let accounts = self.accounts()?;
        let per_account = join_all(accounts.iter().map(|a| async move {
            let mut out =
                vec![Sender { account_id: a.id.clone(), address: Address::new(Some(&a.display_name), &a.email) }];
            let aliases = match self.backend(a) {
                Ok(b) => match &*b {
                    Backend::Microsoft(p) => p.send_as().await,
                    Backend::Google(p) => p.send_as().await,
                },
                Err(e) => Err(e),
            };
            match aliases {
                Ok(list) => out.extend(
                    list.into_iter()
                        .filter(|x| !x.email.eq_ignore_ascii_case(&a.email))
                        .map(|address| Sender { account_id: a.id.clone(), address }),
                ),
                Err(e) => tracing::warn!(account = a.email, error = %e, "could not list send-as aliases"),
            }
            out
        }))
        .await;
        Ok(per_account.into_iter().flatten().collect())
    }

    pub fn save_draft(&self, draft: &mut Draft) -> Result<()> {
        draft.updated_at = pigeon_core::now_ms();
        self.store().save_draft(draft)
    }

    /// Start a reply/reply-all/forward to a stored message. `from` overrides
    /// the default sender (which is the address the original was sent to).
    pub async fn reply(
        &self,
        account: &Account,
        message_id: &str,
        kind: ReplyKind,
        from: Option<Sender>,
    ) -> Result<Draft> {
        let original = self
            .store()
            .message(&account.id, message_id)?
            .ok_or_else(|| Error::Other(format!("message {message_id} is not synced")))?;
        let body = self.body(account, message_id).await?;
        let senders = self.senders().await?;
        let from = match from {
            Some(f) => f,
            None => default_sender(&account.id, &original, &senders)
                .cloned()
                .ok_or_else(|| Error::Other("no sender for this account".into()))?,
        };
        let mine: Vec<String> = senders.iter().map(|s| s.address.email.clone()).collect();
        let mut draft = reply_draft(kind, &account.id, &original, &body, from, &mine, pigeon_core::now_ms());
        self.save_draft(&mut draft)?;
        Ok(draft)
    }

    /// Send a draft from whichever account its From belongs to, then drop
    /// it. Returns the Message-ID.
    pub async fn send(&self, draft: &Draft) -> Result<String> {
        let mut bytes = Vec::with_capacity(draft.attachments.len());
        for a in &draft.attachments {
            let path = match &a.source {
                AttachmentSource::File { path } => PathBuf::from(path),
                AttachmentSource::Message { account_id, message_id, attachment_id } => {
                    self.attachment(&self.account(account_id)?, message_id, attachment_id).await?
                }
            };
            bytes.push(std::fs::read(&path)?);
        }
        let (message_id, mime) = build_mime(draft, &bytes, Utc::now().timestamp())?;
        let account = self.account(&draft.from.account_id)?;
        match &*self.backend(&account)? {
            Backend::Microsoft(p) => p.send_mime(&mime, draft.thread_hint()).await?,
            Backend::Google(p) => p.send_mime(&mime, draft.thread_hint()).await?,
        }
        self.store().delete_draft(&draft.id)?;
        Ok(message_id)
    }

    /// Sync mail + calendar for one account. Mail and calendar run
    /// concurrently and fail independently.
    pub async fn sync_account(&self, account: &Account) -> SyncReport {
        let backend = match self.backend(account) {
            Ok(b) => b,
            Err(e) => {
                let msg = e.to_string();
                return SyncReport { account: account.clone(), mail: Err(e), calendar: Err(Error::Other(msg)) };
            }
        };
        // Push local changes first so the sync reads them back.
        if let Err(e) = self.flush_backend(account, &backend).await {
            tracing::warn!(account = account.email, error = %e, "outbox flush failed");
        }
        let now = Utc::now();
        let from = (now - Duration::days(CALENDAR_PAST_DAYS)).timestamp_millis();
        let to = (now + Duration::days(CALENDAR_FUTURE_DAYS)).timestamp_millis();
        let id = account.id.as_str();
        let store = &*self.store;
        let (mail, calendar) = match &*backend {
            Backend::Microsoft(p) => {
                futures::join!(sync_mail(p, id, store), sync_calendars(p, id, store, from, to))
            }
            Backend::Google(p) => futures::join!(sync_mail(p, id, store), sync_calendars(p, id, store, from, to)),
        };
        SyncReport { account: account.clone(), mail, calendar }
    }

    /// Sync every account concurrently; one slow or broken account never
    /// blocks the rest.
    pub async fn sync_all(&self) -> Result<Vec<SyncReport>> {
        let accounts = self.accounts()?;
        Ok(join_all(accounts.iter().map(|a| self.sync_account(a))).await)
    }

    /// Resolve an account by id, email, or unique prefix of either.
    pub fn find_account(&self, needle: &str) -> Result<Account> {
        let needle = needle.to_lowercase();
        let accounts = self.accounts()?;
        if let Some(a) = accounts.iter().find(|a| a.id == needle || a.email.to_lowercase() == needle) {
            return Ok(a.clone());
        }
        let hits: Vec<&Account> = accounts
            .iter()
            .filter(|a| a.id.starts_with(&needle) || a.email.to_lowercase().starts_with(&needle))
            .collect();
        match hits.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(Error::Other(format!("no account matches {needle:?}"))),
            _ => Err(Error::Other(format!("{needle:?} matches several accounts"))),
        }
    }
}
