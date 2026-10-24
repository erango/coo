//! Wires providers to the core: sign-in, account registry, and sync across
//! all accounts concurrently. Phase 1 grows the Cmd/Delta command loop here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod actions;
pub mod agent_setup;
pub mod broker;
mod calendar;
pub mod drive;
pub mod notify;
pub mod service;
pub mod telemetry;
mod zoom;

pub use actions::MailAction;
use chrono::{Duration, Utc};
use futures::future::join_all;
use pigeon_core::actions::{FlushReport, flush_outbox};
use pigeon_core::auth::{self, OAuthApp, TokenSource};
use pigeon_core::bodies::{download_attachment, load_body};
use pigeon_core::compose::{
    AttachmentSource, Draft, ReplyContext, ReplyKind, Sender, ServerDraft, build_mime, default_sender, reply_draft,
};
use pigeon_core::model::{Account, Address, FolderRole, ProviderKind, StoredBody};
use pigeon_core::provider::{DraftSync, MailSearch, MailSender};
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
    /// The OAuth token source behind each backend (the broker hands out its tokens).
    sources: Mutex<HashMap<String, Arc<TokenSource>>>,
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
    ICloud(Box<ICloud>),
}

/// iCloud: mail over IMAP/SMTP, calendars over CalDAV.
struct ICloud {
    mail: pigeon_imap::Imap,
    cal: pigeon_caldav::CalDav,
}

impl Pigeon {
    /// The app: it owns the sign-ins (Keychain or the dev file).
    pub fn open() -> Result<Self> {
        let data_dir = paths::data_dir()?;
        let tokens = TokenStore::from_env(&data_dir);
        Self::open_with(data_dir, tokens)
    }

    /// The `coo` helper (CLI / MCP server): sign-ins come from the running
    /// app through the broker. `interactive` (a person at a terminal) may
    /// fall back to reading the Keychain directly when the app is away.
    pub fn open_helper(interactive: bool) -> Result<Self> {
        let data_dir = paths::data_dir()?;
        let tokens = TokenStore::for_helper(&data_dir, interactive);
        Self::open_with(data_dir, tokens)
    }

    fn open_with(data_dir: PathBuf, tokens: TokenStore) -> Result<Self> {
        let store = Store::open(data_dir.join("pigeon.db"))?;
        Ok(Pigeon {
            store: Arc::new(Mutex::new(store)),
            tokens,
            data_dir,
            backends: Mutex::default(),
            sources: Mutex::default(),
        })
    }

    /// Whether sign-ins come from the app through the broker.
    pub fn uses_broker(&self) -> bool {
        self.tokens.broker().is_some()
    }

    /// The OAuth token source for an account (creating its backend).
    pub(crate) fn token_source(&self, account: &Account) -> Result<Arc<TokenSource>> {
        self.backend(account)?;
        self.sources
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&account.id)
            .cloned()
            .ok_or_else(|| Error::Other(format!("{} has no OAuth sign-in", account.email)))
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
            ProviderKind::ICloud => return Err(Error::Auth("iCloud signs in with an app-specific password".into())),
        };
        let account = Account { id: Account::make_id(provider, &email), provider, email, display_name };
        self.tokens.set(&account.id, refresh)?;
        self.store().upsert_account(&account)?;
        if let Some(scopes) = &token.scope {
            self.store().set_account_scopes(&account.id, scopes)?;
        }
        Ok(account)
    }

    /// `keychain` or `file`: where sign-ins live (agents must use the same).
    pub fn token_store_kind(&self) -> &'static str {
        self.tokens.kind()
    }

    /// Whether this process can read the account's stored sign-in. From a
    /// terminal this is where a Keychain "allow access" prompt appears.
    pub fn has_sign_in(&self, account: &Account) -> Result<bool> {
        self.tokens.has(&account.id)
    }

    /// Add an iCloud (app-specific password) account. Verifies both IMAP and
    /// CalDAV before saving anything.
    pub async fn login_password(&self, provider: ProviderKind, email: &str, password: &str) -> Result<Account> {
        if provider != ProviderKind::ICloud {
            return Err(Error::Auth(format!("{} signs in through the browser", provider.as_str())));
        }
        let email = email.trim().to_lowercase();
        if !email.contains('@') {
            return Err(Error::Auth("enter your full iCloud address (name@icloud.com)".into()));
        }
        let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();
        let cfg = pigeon_imap::ServerConfig::ICLOUD;
        let (mail, cal) = futures::join!(
            pigeon_imap::verify(&cfg, &email, &password),
            pigeon_caldav::verify(pigeon_caldav::ICLOUD_URL, &email, &password)
        );
        mail?;
        if let Err(e) = cal {
            // Mail works; calendars may be off for this Apple Account.
            tracing::warn!(error = %e, "iCloud calendar check failed");
        }
        let account = Account {
            id: Account::make_id(provider, &email),
            provider,
            display_name: email.split('@').next().unwrap_or_default().to_string(),
            email,
        };
        self.tokens.set(&account.id, &password)?;
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        self.store().upsert_account(&account)?;
        Ok(account)
    }

    pub fn logout(&self, account_id: &str) -> Result<()> {
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(account_id);
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).remove(account_id);
        self.tokens.delete(account_id)?;
        self.store().delete_account(account_id)
    }

    fn backend(&self, account: &Account) -> Result<Arc<Backend>> {
        let mut backends = self.backends.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(b) = backends.get(&account.id) {
            return Ok(b.clone());
        }
        let backend = Arc::new(match account.provider {
            ProviderKind::ICloud => {
                let password =
                    self.tokens.get(&account.id)?.ok_or_else(|| Error::SignInRequired(account.id.clone()))?;
                Backend::ICloud(Box::new(ICloud {
                    mail: pigeon_imap::Imap::new(pigeon_imap::ServerConfig::ICLOUD, &account.email, &password),
                    cal: pigeon_caldav::CalDav::new(pigeon_caldav::ICLOUD_URL, &account.email, &password)?,
                }))
            }
            provider => {
                let app = OAuthApp::from_env(provider)?;
                let tokens = Arc::new(TokenSource::new(app, &account.id, self.tokens.clone()));
                self.sources.lock().unwrap_or_else(|p| p.into_inner()).insert(account.id.clone(), tokens.clone());
                match provider {
                    ProviderKind::Microsoft => Backend::Microsoft(Graph::new(tokens)),
                    _ => Backend::Google(Google::new(tokens)),
                }
            }
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
            Backend::ICloud(p) => load_body(&p.mail, id, message_id, store, cache).await,
        }
    }

    /// Local path of an attachment, downloading on first use.
    pub async fn attachment(&self, account: &Account, message_id: &str, attachment_id: &str) -> Result<PathBuf> {
        let (id, store, cache) = (account.id.as_str(), &*self.store, self.data_dir.as_path());
        match &*self.backend(account)? {
            Backend::Microsoft(p) => download_attachment(p, id, message_id, attachment_id, store, cache).await,
            Backend::Google(p) => download_attachment(p, id, message_id, attachment_id, store, cache).await,
            Backend::ICloud(p) => download_attachment(&p.mail, id, message_id, attachment_id, store, cache).await,
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
            Backend::ICloud(p) => flush_outbox(&p.mail, id, store).await?,
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
                    Backend::ICloud(p) => p.mail.send_as().await,
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

    /// Save locally. The UI's copy may predate the last server sync, so a
    /// missing server reference is taken from what's stored.
    pub fn save_draft(&self, draft: &mut Draft) -> Result<()> {
        draft.updated_at = pigeon_core::now_ms();
        let store = self.store();
        if draft.server.is_none()
            && let Some(stored) = store.draft(&draft.id)?
        {
            draft.server = stored.server;
        }
        store.save_draft(draft)
    }

    /// Push the stored draft to its From account's server (create or
    /// replace). If From moved to another account, the old account's copy is
    /// deleted first. Returns the new server reference.
    pub async fn sync_draft(&self, draft_id: &str) -> Result<Option<ServerDraft>> {
        let Some(mut draft) = self.store().draft(draft_id)? else { return Ok(None) };
        let account = self.account(&draft.from.account_id)?;
        let mut existing = None;
        if let Some(old) = draft.server.take() {
            if old.account_id == account.id {
                existing = Some(old.id);
            } else if let Ok(old_account) = self.account(&old.account_id)
                && let Err(e) = self.delete_server_draft(&old_account, &old.id).await
            {
                tracing::warn!(error = %e, "could not remove the draft from the previous account");
            }
        }
        let (_, mime) = self.draft_mime(&server_copy(&draft)).await?;
        let thread = draft.thread_hint().map(String::from);
        let id = match &*self.backend(&account)? {
            Backend::Microsoft(p) => p.put_draft(&mime, existing.as_deref(), thread.as_deref()).await?,
            Backend::Google(p) => p.put_draft(&mime, existing.as_deref(), thread.as_deref()).await?,
            Backend::ICloud(p) => p.mail.put_draft(&mime, existing.as_deref(), thread.as_deref()).await?,
        };
        let server = ServerDraft { account_id: account.id.clone(), id };
        // Re-read: the user may have kept typing while this was in flight.
        let latest = self.store().draft(draft_id)?;
        match latest {
            Some(mut latest) => {
                latest.server = Some(server.clone());
                self.store().save_draft(&latest)?;
                Ok(Some(server))
            }
            // Sent or discarded while this save was in flight: don't leave
            // an orphan copy in Drafts.
            None => {
                self.delete_server_draft(&account, &server.id).await?;
                Ok(None)
            }
        }
    }

    async fn delete_server_draft(&self, account: &Account, id: &str) -> Result<()> {
        match &*self.backend(account)? {
            Backend::Microsoft(p) => p.delete_draft(id).await,
            Backend::Google(p) => p.delete_draft(id).await,
            Backend::ICloud(p) => p.mail.delete_draft(id).await,
        }
    }

    /// Throw a draft away, here and on the server.
    pub async fn discard_draft(&self, draft_id: &str) -> Result<()> {
        let stored = self.store().draft(draft_id)?;
        if let Some(server) = stored.and_then(|d| d.server)
            && let Ok(account) = self.account(&server.account_id)
        {
            self.delete_server_draft(&account, &server.id).await?;
        }
        self.store().delete_draft(draft_id)
    }

    /// Open a draft that synced in from the server (e.g. written on the
    /// phone) as an editable local draft linked to its server copy.
    pub async fn edit_draft(&self, account: &Account, message_id: &str) -> Result<Draft> {
        let h = self
            .store()
            .message(&account.id, message_id)?
            .ok_or_else(|| Error::Other(format!("draft {message_id} is not synced")))?;
        let body = self.body(account, message_id).await?;
        let senders = self.senders().await?;
        let from = h
            .from
            .as_ref()
            .and_then(|f| {
                senders.iter().find(|s| s.account_id == account.id && s.address.email.eq_ignore_ascii_case(&f.email))
            })
            .or_else(|| senders.iter().find(|s| s.account_id == account.id))
            .cloned()
            .ok_or_else(|| Error::Other("no sender for this account".into()))?;
        let server_id = match &*self.backend(account)? {
            Backend::Microsoft(p) => p.draft_for_message(message_id).await?,
            Backend::Google(p) => p.draft_for_message(message_id).await?,
            Backend::ICloud(p) => p.mail.draft_for_message(message_id).await?,
        };
        let mut d = edit_from_message(&h, &body, from, pigeon_core::now_ms());
        d.server = server_id.map(|id| ServerDraft { account_id: account.id.clone(), id });
        self.save_draft(&mut d)?;
        Ok(d)
    }

    async fn draft_mime(&self, draft: &Draft) -> Result<(String, Vec<u8>)> {
        let mut bytes = Vec::with_capacity(draft.attachments.len());
        for a in &draft.attachments {
            let path = match &a.source {
                AttachmentSource::File { path } => PathBuf::from(path),
                AttachmentSource::Message { account_id, message_id, attachment_id } => {
                    self.attachment(&self.account(account_id)?, message_id, attachment_id).await?
                }
                // Streamed from Drive into the message; never written to disk.
                AttachmentSource::Drive { account_id, file_id, export_mime, .. } => {
                    bytes.push(self.drive_bytes(account_id, file_id, export_mime.as_deref()).await?);
                    continue;
                }
            };
            bytes.push(std::fs::read(&path)?);
        }
        build_mime(draft, &bytes, Utc::now().timestamp())
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
    /// it here and on the server. Returns the Message-ID.
    ///
    /// Always sends the MIME built right here from `draft`, never the
    /// server's draft copy: a background draft sync that read an older
    /// snapshot could update that copy between our update and the
    /// provider's "send draft" call, and the stale (empty) version went out
    /// (Gmail re-serialized it, so the recipient got a blank message).
    /// Threading is kept by passing the thread to the send call.
    pub async fn send(&self, draft: &Draft) -> Result<String> {
        let (message_id, mime) = self.draft_mime(draft).await?;
        let account = self.account(&draft.from.account_id)?;
        let stored_server = self.store().draft(&draft.id)?.and_then(|d| d.server);
        let server = draft.server.clone().or(stored_server);
        let thread = draft.thread_hint();
        match &*self.backend(&account)? {
            Backend::Microsoft(p) => p.send_mime(&mime, thread).await?,
            Backend::Google(p) => p.send_mime(&mime, thread).await?,
            Backend::ICloud(p) => p.mail.send_mime(&mime, thread).await?,
        }
        // Gone locally first, so an in-flight draft sync removes whatever it
        // writes (it re-reads the store and finds nothing).
        self.store().delete_draft(&draft.id)?;
        if let Some(sd) = &server
            && let Ok(server_account) = self.account(&sd.account_id)
            && let Err(e) = self.delete_server_draft(&server_account, &sd.id).await
        {
            tracing::warn!(error = %e, "sent, but could not remove the server copy of the draft");
        }
        Ok(message_id)
    }

    /// Ask every account's server for older matches, store them, and return
    /// how many came back. Accounts fail independently (logged).
    pub async fn search_servers(&self, q: &pigeon_core::search::SearchQuery, per_account: u32) -> Result<usize> {
        let accounts: Vec<Account> = self
            .accounts()?
            .into_iter()
            .filter(|a| q.account.as_ref().is_none_or(|n| a.id.contains(n.as_str())))
            .collect();
        let results = join_all(accounts.iter().map(|a| async move {
            let found = match &*self.backend(a)? {
                Backend::Microsoft(p) => p.search(q, per_account).await?,
                Backend::Google(p) => p.search(q, per_account).await?,
                Backend::ICloud(p) => p.mail.search(q, per_account).await?,
            };
            self.store().apply_found(&a.id, &found)?;
            Ok::<usize, Error>(found.len())
        }))
        .await;
        let mut total = 0;
        for (a, r) in accounts.iter().zip(results) {
            match r {
                Ok(n) => total += n,
                Err(e) => tracing::warn!(account = a.email, error = %e, "server search failed"),
            }
        }
        Ok(total)
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
            Backend::ICloud(p) => {
                futures::join!(sync_mail(&p.mail, id, store), sync_calendars(&p.cal, id, store, from, to))
            }
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

/// What a draft's server copy holds. It leaves out Google Drive
/// attachments: fetching them from Drive on every autosave would be slow
/// and spend quota. The local draft keeps them, and they go out with the
/// message.
pub fn server_copy(draft: &Draft) -> Draft {
    let mut copy = draft.clone();
    copy.attachments.retain(|a| !a.source.is_drive());
    copy
}

/// A local draft rebuilt from a server draft message: recipients, subject,
/// body (into `body_html`, the editable part) and threading headers, so a
/// reply drafted elsewhere still threads when sent from here.
pub fn edit_from_message(h: &pigeon_core::model::MessageHeader, body: &StoredBody, from: Sender, now: i64) -> Draft {
    let mut d = Draft::new(from.clone(), now);
    d.to = h.to.clone();
    d.cc = h.cc.clone();
    d.subject = h.subject.clone();
    d.body_html =
        body.html.clone().or_else(|| body.text.as_deref().map(pigeon_core::text::text_to_html)).unwrap_or_default();
    if body.in_reply_to.is_some() || !body.references.is_empty() {
        d.reply = Some(ReplyContext {
            kind: ReplyKind::Reply,
            account_id: from.account_id,
            message_id: String::new(),
            thread_id: h.thread_id.clone(),
            internet_message_id: body.in_reply_to.clone(),
            references: body.references.clone(),
        });
    }
    d
}

#[cfg(test)]
mod draft_tests {
    use pigeon_core::model::{Address, MessageHeader};

    use super::*;

    #[test]
    fn server_draft_copies_leave_out_drive_attachments() {
        use pigeon_core::compose::DraftAttachment;
        let from = Sender { account_id: "graph:me@work.com".into(), address: Address::new(None, "me@work.com") };
        let mut d = Draft::new(from, 0);
        let att = |name: &str, source| DraftAttachment { name: name.into(), content_type: None, source };
        d.attachments = vec![
            att("local.pdf", AttachmentSource::File { path: "/tmp/local.pdf".into() }),
            att(
                "Budget.xlsx",
                AttachmentSource::Drive {
                    account_id: "google:me@gmail.com".into(),
                    file_id: "f1".into(),
                    export_mime: Some("application/pdf".into()),
                    size: None,
                },
            ),
        ];
        let copy = server_copy(&d);
        assert_eq!(copy.attachments.len(), 1);
        assert_eq!(copy.attachments[0].name, "local.pdf");
        assert_eq!(d.attachments.len(), 2, "the local draft keeps them");
    }

    #[test]
    fn edit_from_message_keeps_recipients_body_and_threading() {
        let h = MessageHeader {
            id: "m".into(),
            thread_id: "t9".into(),
            subject: "Re: שלום".into(),
            to: vec![Address::new(None, "c@cust.com")],
            ..Default::default()
        };
        let body = StoredBody {
            text: Some("draft from phone".into()),
            in_reply_to: Some("<p@x>".into()),
            references: vec!["<r@x>".into(), "<p@x>".into()],
            ..Default::default()
        };
        let from = Sender { account_id: "google:a@b.c".into(), address: Address::new(None, "a@b.c") };
        let d = edit_from_message(&h, &body, from, 1);
        assert_eq!(d.subject, "Re: שלום");
        assert_eq!(d.to[0].email, "c@cust.com");
        assert!(d.body_html.contains("draft from phone"));
        assert!(d.html.is_empty(), "no quote appended twice");
        let r = d.reply.unwrap();
        assert_eq!((r.thread_id.as_str(), r.internet_message_id.as_deref()), ("t9", Some("<p@x>")));
        // Same account: the provider thread id applies on send.
        let d2 = edit_from_message(
            &h,
            &body,
            Sender { account_id: "google:a@b.c".into(), address: Address::new(None, "a@b.c") },
            1,
        );
        assert_eq!(d2.thread_hint(), Some("t9"));
    }
}
