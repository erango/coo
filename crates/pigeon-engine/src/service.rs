//! The UI-facing engine loop: the UI sends [`Cmd`]s, the engine answers with
//! [`Delta`]s rendered from the local store. Runs on its own Tokio runtime;
//! the UI thread never touches SQL or the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use pigeon_core::compose::{Draft, ReplyKind, Sender};
use pigeon_core::model::*;
use pigeon_core::store::{FolderSel, ListQuery};
use pigeon_core::{Error, Result, now_ms};
use tokio::sync::{Mutex, mpsc};

use crate::{MailAction, Pigeon};

const SYNC_EVERY: Duration = Duration::from_secs(30);
/// First screenful: what's visible plus a little, so a cold launch touches
/// few pages. The UI asks for more (`Cmd::LoadMore`) as you scroll.
const LIST_FIRST_PAGE: u32 = 60;
const LIST_PAGE: u32 = 200;
const LIST_MAX: u32 = 20_000;
const NEXT_UP_HOURS: i64 = 12;
const SEARCH_LIMIT: u32 = 200;
const SYNC_PROGRESS_EVERY: Duration = Duration::from_millis(1500);

/// A password that never shows up in `Debug` output (commands get logged).
#[derive(Clone)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Which list the user is looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mailbox {
    /// A role across every account (unified inbox, sent, ...).
    Unified(FolderRole),
    Folder {
        account_id: String,
        folder_id: String,
    },
}

impl Mailbox {
    fn query(&self, limit: u32) -> ListQuery {
        match self {
            Mailbox::Unified(role) => ListQuery::unified(*role, limit),
            Mailbox::Folder { account_id, folder_id } => ListQuery {
                account_id: Some(account_id.clone()),
                folder: FolderSel::Id(folder_id.clone()),
                limit,
                before: None,
            },
        }
    }

    /// The concrete folder a message was listed from (Gmail moves need it).
    fn folder_for(&self, store_role: impl Fn(&str, FolderRole) -> Option<String>, account_id: &str) -> Option<String> {
        match self {
            Mailbox::Unified(role) => store_role(account_id, *role),
            Mailbox::Folder { folder_id, .. } => Some(folder_id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Cmd {
    /// Send `draft` at `at` (ms). `undo_window`: the undo-send grace period
    /// rather than a chosen time.
    Schedule {
        draft: Draft,
        at: i64,
        undo_window: bool,
    },
    /// Cancel a scheduled send; the draft reopens in the composer.
    CancelScheduled {
        draft_id: String,
    },
    /// The user scrolled near the end of the list; `have` is how many rows
    /// the UI currently shows.
    LoadMore {
        have: u32,
    },
    /// Recipient autocomplete; `token` lets the UI drop stale results.
    SuggestContacts {
        query: String,
        token: u64,
        limit: u32,
    },
    /// Local search; `token` lets the UI drop stale results.
    Search {
        query: String,
        token: u64,
        limit: u32,
    },
    /// Then ask the servers for older matches and re-run locally.
    SearchServers {
        query: String,
        token: u64,
    },
    ShowMailbox {
        mailbox: Mailbox,
        threaded: bool,
    },
    OpenThread {
        account_id: String,
        thread_id: String,
    },
    OpenMessage {
        account_id: String,
        message_id: String,
    },
    /// Act on messages; with `thread_id`, on that thread's messages in the
    /// current mailbox (so archiving a thread leaves your Sent copies alone).
    Act {
        account_id: String,
        ids: Vec<String>,
        thread_id: Option<String>,
        action: MailAction,
        label: String,
    },
    Undo {
        account_id: String,
        handle: i64,
    },
    SyncNow,
    Login(ProviderKind),
    /// iCloud-style sign-in with an app-specific password.
    LoginPassword {
        provider: ProviderKind,
        email: String,
        password: Secret,
    },
    /// Sign out and delete the account's local data.
    Logout {
        account_id: String,
    },
    LoadSenders,
    /// How many messages from `email` this account already has (sender
    /// card's "first time" check). Answered with `Delta::SenderSeen`.
    SenderSeen {
        account_id: String,
        email: String,
    },
    /// A sender photo from the on-disk cache or Gravatar (the UI only asks
    /// when the user turned Gravatar photos on). Answered with `Delta::Photo`.
    Photo {
        email: String,
    },
    StartReply {
        account_id: String,
        message_id: String,
        kind: ReplyKind,
    },
    NewDraft,
    /// Save locally now; push to the server after a short pause.
    SaveDraft(Draft),
    /// Delete a draft here and on the server.
    DiscardDraft {
        draft_id: String,
    },
    /// Open a synced server draft in the composer.
    EditDraft {
        account_id: String,
        message_id: String,
    },
    Send(Draft),
    OpenAttachment {
        account_id: String,
        message_id: String,
        attachment_id: String,
    },
    ShowCalendar {
        from: i64,
        to: i64,
    },
    Rsvp {
        account_id: String,
        calendar_id: String,
        event_id: String,
        response: Response,
    },
    CreateEvent {
        account_id: String,
        draft: EventDraft,
    },
    UpdateEvent {
        account_id: String,
        event_id: String,
        draft: EventDraft,
    },
    /// Busy intervals for guests; answered with `Delta::FreeBusy`.
    FreeBusy {
        account_id: String,
        emails: Vec<String>,
        from: i64,
        to: i64,
        token: u64,
    },
    DeleteEvent {
        account_id: String,
        calendar_id: String,
        event_id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountNav {
    pub account: Account,
    /// Account color slot, 1..=8.
    pub hue: u8,
    pub folders: Vec<FolderInfo>,
    /// At least one mail page synced; false during the cold first sync.
    pub first_sync_done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListItem {
    Thread(ThreadRow),
    Message(MessageRow),
}

impl ListItem {
    pub fn account_id(&self) -> &str {
        match self {
            ListItem::Thread(t) => &t.account_id,
            ListItem::Message(m) => &m.account_id,
        }
    }

    pub fn header(&self) -> &MessageHeader {
        match self {
            ListItem::Thread(t) => &t.latest,
            ListItem::Message(m) => &m.header,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReaderView {
    pub account_id: String,
    pub thread_id: String,
    /// Oldest first.
    pub messages: Vec<MessageHeader>,
    /// The message to expand (newest unless a single message was opened).
    pub focus: String,
    /// Opened as a single message from the unthreaded list.
    pub single: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub error: bool,
    pub undo: Option<(String, i64)>,
    /// Draft id whose scheduled send this toast can cancel ("Sending… Undo").
    pub cancel_send: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncState {
    pub syncing: bool,
    pub last_ok: Option<i64>,
    pub error: Option<String>,
    /// Accounts whose sign-in expired.
    pub needs_sign_in: Vec<String>,
    /// Every failure of the last sync was a network error.
    pub offline: bool,
}

#[derive(Debug, Clone)]
pub enum Delta {
    /// An OAuth sign-in failed (the UI decides whether it's admin consent).
    LoginFailed {
        provider: ProviderKind,
        error: String,
    },
    /// Unread inbox mail that arrived since the last sync (never on first
    /// sync), newest first. The UI decides whether to notify.
    NewMail(Vec<MessageRow>),
    /// Drafts waiting for their scheduled send (send later / undo window).
    Scheduled(Vec<Draft>),
    Unscheduled {
        draft_id: String,
    },
    Accounts(Vec<AccountNav>),
    List {
        mailbox: Mailbox,
        threaded: bool,
        items: Vec<ListItem>,
    },
    Reader(ReaderView),
    /// Body with inline images resolved to data URLs.
    Body {
        account_id: String,
        message_id: String,
        body: StoredBody,
        /// Set when the message is a meeting invite.
        invite: Option<Box<InviteView>>,
    },
    Toast(Toast),
    Sync(SyncState),
    NextUp(Vec<EventRow>),
    Events {
        from: i64,
        to: i64,
        events: Vec<EventRow>,
        calendars: Vec<(String, Calendar)>,
    },
    Senders(Vec<Sender>),
    SenderSeen {
        account_id: String,
        email: String,
        count: u32,
    },
    /// A sender photo as a `data:` URI, or None (use initials).
    Photo {
        email: String,
        uri: Option<String>,
    },
    Draft(Draft),
    Sent {
        draft_id: String,
    },
    SendFailed {
        draft_id: String,
        error: String,
    },
    AttachmentReady(std::path::PathBuf),
    SearchResults {
        token: u64,
        items: Vec<MessageRow>,
        server: ServerSearch,
    },
    FreeBusy {
        token: u64,
        result: FreeBusy,
    },
    Contacts {
        token: u64,
        items: Vec<Address>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerSearch {
    Idle,
    Loading,
    /// Done; this many messages came back from the servers.
    Done(usize),
}

fn local_when(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%a %-d %b, %H:%M").to_string())
        .unwrap_or_default()
}

/// Stable 1..=8 color slot for an account address.
pub fn hue_for(email: &str) -> u8 {
    // FNV-1a: stable across runs and machines.
    let mut h: u32 = 0x811c9dc5;
    for b in email.to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x01000193);
    }
    (h % 8) as u8 + 1
}

/// An invite resolved against the local calendars.
#[derive(Debug, Clone, PartialEq)]
pub struct InviteView {
    /// The synced event it refers to (RSVP target), if already in a calendar.
    pub event: Option<EventRow>,
    /// Your other events around the invite's time, for the conflict view.
    pub busy: Vec<EventRow>,
}

const HOUR_MS: i64 = 3_600_000;

fn invite_view(store: &pigeon_core::store::Store, account_id: &str, inv: &Invite) -> Result<InviteView> {
    let event = store.invite_event(account_id, inv)?;
    let (from, to) = (inv.start_at - HOUR_MS, inv.end_at.max(inv.start_at) + HOUR_MS);
    let busy = store
        .events_between(from, to)?
        .into_iter()
        .filter(|e| !e.event.all_day)
        .filter(|e| event.as_ref().is_none_or(|m| !(m.account_id == e.account_id && m.event.id == e.event.id)))
        .collect();
    Ok(InviteView { event, busy })
}

/// Replace `cid:` references with data URLs from the cached inline parts.
fn inline_images(mut body: StoredBody) -> StoredBody {
    let Some(html) = body.html.as_mut() else { return body };
    for a in &body.attachments {
        let (Some(cid), Some(path)) = (&a.meta.content_id, &a.local_path) else { continue };
        let needle = format!("cid:{cid}");
        if !html.contains(&needle) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let ct = a.meta.content_type.as_deref().unwrap_or("image/png");
        let url = format!("data:{ct};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
        *html = html.replace(&needle, &url);
    }
    body
}

struct View {
    mailbox: Mailbox,
    threaded: bool,
    calendar: Option<(i64, i64)>,
    /// Rows to list; grows with `Cmd::LoadMore`, resets on a new mailbox.
    limit: u32,
}

struct Service {
    app: Arc<Pigeon>,
    tx: mpsc::UnboundedSender<Delta>,
    view: Mutex<View>,
    syncing: AtomicBool,
    sync_state: Mutex<SyncState>,
    toast_seq: std::sync::atomic::AtomicU64,
    my_addresses: Mutex<Option<Vec<String>>>,
    /// Save generation per draft: a server sync only runs if no newer save
    /// arrived during the debounce.
    draft_gen: std::sync::Mutex<std::collections::HashMap<String, u64>>,
    /// Drafts being sent; their server copy must not be re-created.
    draft_sending: std::sync::Mutex<std::collections::HashSet<String>>,
    /// New-mail watermarks per account (see `notify.rs`).
    notify_marks: std::sync::Mutex<crate::notify::Marks>,
    /// Scheduled drafts currently being sent (guards the 1 s due-check).
    in_flight: Mutex<std::collections::HashSet<String>>,
}

/// How many newest inbox rows per account are checked for new mail.
const NEW_MAIL_WINDOW: u32 = 50;

/// Pause after the last save before a draft goes to the server.
const DRAFT_SYNC_DELAY: Duration = Duration::from_secs(2);

impl Service {
    /// Compare each account's inbox against its watermark and announce
    /// what's new.
    async fn check_new_mail(&self) {
        let mine = self.my_addresses().await;
        let accounts = self.app.accounts().unwrap_or_default();
        let mut fresh = Vec::new();
        for a in &accounts {
            let q = ListQuery {
                account_id: Some(a.id.clone()),
                folder: FolderSel::Role(FolderRole::Inbox),
                limit: NEW_MAIL_WINDOW,
                before: None,
            };
            let rows = match self.app.store().list_messages(&q) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "new-mail check failed");
                    continue;
                }
            };
            fresh.extend(self.notify_marks.lock().unwrap_or_else(|p| p.into_inner()).fresh(&a.id, &rows, &mine));
        }
        if !fresh.is_empty() {
            fresh.sort_by_key(|r| std::cmp::Reverse(r.header.received_at));
            self.send(Delta::NewMail(fresh));
        }
    }

    /// Debounced server save for a draft.
    fn schedule_draft_sync(self: &Arc<Self>, draft_id: String) {
        let generation = {
            let mut g = self.draft_gen.lock().unwrap_or_else(|p| p.into_inner());
            let n = g.entry(draft_id.clone()).or_insert(0);
            *n += 1;
            *n
        };
        let svc = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(DRAFT_SYNC_DELAY).await;
            let current = svc.draft_gen.lock().unwrap_or_else(|p| p.into_inner()).get(&draft_id).copied();
            let sending = svc.draft_sending.lock().unwrap_or_else(|p| p.into_inner()).contains(&draft_id);
            if current != Some(generation) || sending {
                return;
            }
            match svc.app.sync_draft(&draft_id).await {
                Ok(_) => {}
                // Drafts stay local when the server can't take them.
                Err(e) => tracing::warn!(error = %e, "draft not saved to the server"),
            }
        });
    }

    fn send(&self, d: Delta) {
        let _ = self.tx.send(d);
    }

    fn toast(&self, message: impl Into<String>, error: bool, undo: Option<(String, i64)>) {
        let id = self.toast_seq.fetch_add(1, Ordering::Relaxed);
        self.send(Delta::Toast(Toast { id, message: message.into(), error, undo, cancel_send: None }));
    }

    fn fail(&self, context: &str, e: &Error) {
        tracing::error!(error = %e, "{context}");
        self.toast(format!("{context}: {e}"), true, None);
    }

    /// Re-render everything derived from the store.
    async fn publish(&self) {
        if let Err(e) = self.publish_inner().await {
            self.fail("Could not load mail", &e);
        }
    }

    async fn publish_inner(&self) -> Result<()> {
        let started = std::time::Instant::now();
        let r = self.publish_snapshot().await;
        tracing::debug!(ms = started.elapsed().as_secs_f64() * 1000.0, "publish");
        r
    }

    async fn publish_snapshot(&self) -> Result<()> {
        let (mailbox, threaded, calendar, limit) = {
            let v = self.view.lock().await;
            (v.mailbox.clone(), v.threaded, v.calendar, v.limit)
        };
        let now = now_ms();
        let (accounts, items, next_up, cal) = {
            let t = std::time::Instant::now();
            let store = self.app.store();
            let lock_ms = t.elapsed().as_secs_f64() * 1000.0;
            let mut accounts = Vec::new();
            for a in store.accounts()? {
                accounts.push(AccountNav {
                    hue: hue_for(&a.email),
                    folders: store.folders(&a.id)?,
                    first_sync_done: store.has_mail_cursor(&a.id)?,
                    account: a,
                });
            }
            let accounts_ms = t.elapsed().as_secs_f64() * 1000.0 - lock_ms;
            let q = mailbox.query(limit);
            let items = if threaded {
                store.list_threads(&q)?.into_iter().map(ListItem::Thread).collect()
            } else {
                store.list_messages(&q)?.into_iter().map(ListItem::Message).collect()
            };
            let list_ms = t.elapsed().as_secs_f64() * 1000.0 - lock_ms - accounts_ms;
            tracing::debug!(lock_ms, accounts_ms, list_ms, "publish steps");
            let next_up = store.events_between(now, now + NEXT_UP_HOURS * 3_600_000)?;
            let cal = match calendar {
                Some((from, to)) => {
                    let events = store.events_between(from, to)?;
                    let mut cals = Vec::new();
                    for a in &accounts {
                        cals.extend(store.calendars(&a.account.id)?.into_iter().map(|c| (a.account.id.clone(), c)));
                    }
                    Some((from, to, events, cals))
                }
                None => None,
            };
            (accounts, items, next_up, cal)
        };
        let scheduled = self.app.store().drafts()?.into_iter().filter(|d| d.scheduled_at.is_some()).collect();
        self.send(Delta::Scheduled(scheduled));
        self.send(Delta::Accounts(accounts));
        self.send(Delta::List { mailbox, threaded, items });
        self.send(Delta::NextUp(next_up.into_iter().filter(|e| !e.event.all_day).take(3).collect()));
        if let Some((from, to, events, calendars)) = cal {
            self.send(Delta::Events { from, to, events, calendars });
        }
        Ok(())
    }

    /// Every address the user sends from (accounts + aliases), cached after
    /// the first lookup; falls back to account addresses if aliases fail.
    async fn my_addresses(&self) -> Vec<String> {
        let mut cache = self.my_addresses.lock().await;
        if let Some(m) = cache.as_ref() {
            return m.clone();
        }
        let mut mine: Vec<String> = self.app.accounts().unwrap_or_default().into_iter().map(|a| a.email).collect();
        if let Ok(senders) = self.app.senders().await {
            mine.extend(senders.into_iter().map(|s| s.address.email));
            *cache = Some(mine.clone());
        }
        mine
    }

    async fn set_sync(&self, f: impl FnOnce(&mut SyncState)) {
        let mut s = self.sync_state.lock().await;
        f(&mut s);
        self.send(Delta::Sync(s.clone()));
    }

    async fn sync(self: Arc<Self>) {
        if self.syncing.swap(true, Ordering::SeqCst) {
            return;
        }
        self.set_sync(|s| s.syncing = true).await;
        // Show mail as it lands during a long (first) sync instead of only
        // at the end; a publish costs ~10 ms even at 100k messages.
        let progress = {
            let svc = self.clone();
            tokio::spawn(async move {
                let mut every = tokio::time::interval(SYNC_PROGRESS_EVERY);
                every.tick().await;
                loop {
                    every.tick().await;
                    svc.publish().await;
                }
            })
        };
        let reports = self.app.sync_all().await;
        progress.abort();
        let mut errors = Vec::new();
        let (mut failures, mut network_failures) = (0, 0);
        let mut needs_sign_in = Vec::new();
        match &reports {
            Ok(reports) => {
                for r in reports {
                    for e in [r.mail.as_ref().err(), r.calendar.as_ref().err()].into_iter().flatten() {
                        failures += 1;
                        if e.is_network() {
                            network_failures += 1;
                        }
                        if matches!(e, Error::SignInRequired(_)) {
                            needs_sign_in.push(r.account.id.clone());
                        } else {
                            errors.push(format!("{}: {e}", r.account.email));
                        }
                    }
                }
            }
            Err(e) => {
                failures += 1;
                if e.is_network() {
                    network_failures += 1;
                }
                errors.push(e.to_string())
            }
        }
        let offline = failures > 0 && failures == network_failures;
        needs_sign_in.dedup();
        for e in &errors {
            tracing::error!(error = %e, "sync failed");
        }
        self.set_sync(|s| {
            s.syncing = false;
            if errors.is_empty() {
                s.last_ok = Some(now_ms());
            }
            s.error = errors.first().cloned();
            s.needs_sign_in = needs_sign_in;
            s.offline = offline;
        })
        .await;
        self.syncing.store(false, Ordering::SeqCst);
        self.publish().await;
        self.check_new_mail().await;
    }

    async fn open_bodies(self: Arc<Self>, account: Account, ids: Vec<String>) {
        for id in ids {
            match self.app.body(&account, &id).await {
                Ok(body) => {
                    let invite = match &body.invite {
                        Some(inv) => invite_view(&self.app.store(), &account.id, inv).ok().map(Box::new),
                        None => None,
                    };
                    self.send(Delta::Body {
                        account_id: account.id.clone(),
                        message_id: id,
                        body: inline_images(body),
                        invite,
                    })
                }
                Err(e) => self.fail("Could not load message", &e),
            }
        }
    }

    async fn handle(self: Arc<Self>, cmd: Cmd) {
        if let Err(e) = self.clone().handle_inner(cmd).await {
            self.fail("Something went wrong", &e);
        }
    }

    async fn handle_inner(self: Arc<Self>, cmd: Cmd) -> Result<()> {
        match cmd {
            Cmd::ShowMailbox { mailbox, threaded } => {
                {
                    let mut v = self.view.lock().await;
                    if v.mailbox != mailbox || v.threaded != threaded {
                        v.limit = LIST_FIRST_PAGE;
                    }
                    v.mailbox = mailbox;
                    v.threaded = threaded;
                }
                self.publish().await;
            }
            Cmd::LoadMore { have } => {
                {
                    let mut v = self.view.lock().await;
                    // Ignore stale requests (already grown past what the UI saw).
                    if have < v.limit {
                        return Ok(());
                    }
                    v.limit = (v.limit + LIST_PAGE).min(LIST_MAX);
                }
                self.publish().await;
            }
            Cmd::OpenThread { account_id, thread_id } => {
                let account = self.app.account(&account_id)?;
                let messages = self.app.store().thread_messages(&account_id, &thread_id)?;
                let Some(last) = messages.last() else { return Ok(()) };
                let focus = last.id.clone();
                self.open(account, thread_id, messages, focus, false).await?;
            }
            Cmd::OpenMessage { account_id, message_id } => {
                let account = self.app.account(&account_id)?;
                let Some(h) = self.app.store().message(&account_id, &message_id)? else { return Ok(()) };
                let thread_id = h.thread_id.clone();
                self.open(account, thread_id, vec![h], message_id, true).await?;
            }
            Cmd::Act { account_id, mut ids, thread_id, action, label } => {
                let account = self.app.account(&account_id)?;
                let from = {
                    let v = self.view.lock().await;
                    let store = self.app.store();
                    v.mailbox.folder_for(|a, r| store.role_folder(a, r).ok().flatten(), &account_id)
                };
                if let Some(thread_id) = thread_id {
                    let store = self.app.store();
                    let in_view = |m: &MessageHeader| {
                        let full = store.message(&account_id, &m.id).ok().flatten();
                        from.as_ref().is_none_or(|f| full.is_some_and(|h| h.folder_ids.contains(f)))
                    };
                    let messages = store.thread_messages(&account_id, &thread_id)?;
                    ids = match action {
                        // Read state applies to the whole conversation.
                        MailAction::MarkRead | MailAction::MarkUnread => {
                            messages.iter().map(|m| m.id.clone()).collect()
                        }
                        // Flag the newest message, like Gmail/Outlook.
                        MailAction::Flag | MailAction::Unflag => {
                            messages.iter().rev().find(|m| in_view(m)).map(|m| vec![m.id.clone()]).unwrap_or_default()
                        }
                        _ => messages.iter().filter(|m| in_view(m)).map(|m| m.id.clone()).collect(),
                    };
                }
                if ids.is_empty() {
                    return Ok(());
                }
                let handle = self.app.act(&account, &ids, from.as_deref(), &action)?;
                let undoable = !matches!(action, MailAction::MarkRead | MailAction::MarkUnread);
                if undoable {
                    self.toast(label, false, Some((account_id, handle)));
                }
                self.publish().await;
                self.flush(account).await;
            }
            Cmd::Undo { account_id, handle } => {
                let account = self.app.account(&account_id)?;
                self.app.undo(&account, handle)?;
                self.publish().await;
                self.flush(account).await;
            }
            Cmd::SyncNow => tokio::spawn(self.clone().sync()).await.map_err(|e| Error::Other(e.to_string()))?,
            Cmd::Login(provider) => {
                let account = match self.app.login(provider, |url| tracing::info!(url, "sign-in URL")).await {
                    Ok(a) => a,
                    Err(e) => {
                        self.send(Delta::LoginFailed { provider, error: e.to_string() });
                        if pigeon_core::auth::admin_consent_code(&e.to_string()).is_some() {
                            self.toast("Your organization's admin has to approve Coo first", true, None);
                            return Ok(());
                        }
                        return Err(e);
                    }
                };
                self.toast(format!("Signed in as {}", account.email), false, None);
                self.publish().await;
                tokio::spawn(self.clone().sync());
            }
            Cmd::LoginPassword { provider, email, password } => {
                let account = self.app.login_password(provider, &email, &password.0).await?;
                self.toast(format!("Signed in as {}", account.email), false, None);
                self.publish().await;
                tokio::spawn(self.clone().sync());
            }
            Cmd::Logout { account_id } => {
                let account = self.app.account(&account_id)?;
                self.app.logout(&account_id)?;
                *self.my_addresses.lock().await = None;
                self.notify_marks.lock().unwrap_or_else(|p| p.into_inner()).forget(&account_id);
                self.toast(format!("Signed out of {}", account.email), false, None);
                self.publish().await;
            }
            Cmd::LoadSenders => self.send(Delta::Senders(self.app.senders().await?)),
            Cmd::SenderSeen { account_id, email } => {
                let count = self.app.store().messages_from(&account_id, &email)?;
                self.send(Delta::SenderSeen { account_id, email, count });
            }
            Cmd::Photo { email } => {
                let svc = self.clone();
                tokio::spawn(async move {
                    use pigeon_core::photos::{self, Cached};
                    let dir = svc.app.data_dir.join("photos");
                    let bytes = match photos::cached(&dir, &email, std::time::SystemTime::now()) {
                        Cached::Photo(b) => Some(b),
                        Cached::None => None,
                        Cached::Miss => match photos::gravatar(&email).await {
                            Ok(found) => {
                                if let Err(e) = photos::store(&dir, &email, found.as_deref()) {
                                    tracing::warn!(error = %e, "could not cache a sender photo");
                                }
                                found
                            }
                            // Offline / server trouble: initials now, retry another time.
                            Err(e) => {
                                tracing::debug!(error = %e, "photo lookup failed");
                                None
                            }
                        },
                    };
                    svc.send(Delta::Photo { email, uri: bytes.as_deref().map(photos::data_uri) });
                });
            }
            Cmd::StartReply { account_id, message_id, kind } => {
                let account = self.app.account(&account_id)?;
                let draft = self.app.reply(&account, &message_id, kind, None).await?;
                self.send(Delta::Draft(draft));
            }
            Cmd::NewDraft => {
                let senders = self.app.senders().await?;
                let from = senders.into_iter().next().ok_or_else(|| Error::Other("add an account first".into()))?;
                let mut draft = Draft::new(from, now_ms());
                self.app.save_draft(&mut draft)?;
                self.send(Delta::Draft(draft));
            }
            Cmd::SaveDraft(mut draft) => {
                self.app.save_draft(&mut draft)?;
                self.schedule_draft_sync(draft.id);
            }
            Cmd::DiscardDraft { draft_id } => {
                self.draft_gen.lock().unwrap_or_else(|p| p.into_inner()).remove(&draft_id);
                self.app.discard_draft(&draft_id).await?;
                self.toast("Draft discarded", false, None);
            }
            Cmd::EditDraft { account_id, message_id } => {
                let account = self.app.account(&account_id)?;
                let draft = self.app.edit_draft(&account, &message_id).await?;
                self.send(Delta::Draft(draft));
            }
            Cmd::Send(mut draft) => {
                draft.scheduled_at = None;
                draft.undo_window = false;
                self.app.save_draft(&mut draft)?;
                self.send_now(draft).await;
            }
            Cmd::Schedule { mut draft, at, undo_window } => {
                draft.scheduled_at = Some(at);
                draft.undo_window = undo_window;
                self.app.save_draft(&mut draft)?;
                let id = self.toast_seq.fetch_add(1, Ordering::Relaxed);
                let message =
                    if undo_window { "Sending…".to_string() } else { format!("Scheduled for {}", local_when(at)) };
                self.send(Delta::Toast(Toast {
                    id,
                    message,
                    cancel_send: Some(draft.id.clone()),
                    ..Default::default()
                }));
                self.publish().await;
            }
            Cmd::CancelScheduled { draft_id } => {
                let busy = self.in_flight.lock().await.contains(&draft_id);
                let draft = self.app.store().draft(&draft_id)?;
                match draft {
                    Some(mut d) if !busy && d.scheduled_at.is_some() => {
                        d.scheduled_at = None;
                        d.undo_window = false;
                        self.app.save_draft(&mut d)?;
                        self.send(Delta::Unscheduled { draft_id: d.id.clone() });
                        self.send(Delta::Draft(d));
                        self.toast("Not sent — back in the composer", false, None);
                    }
                    _ => self.toast("Too late to undo: it's already on its way", true, None),
                }
                self.publish().await;
            }
            Cmd::OpenAttachment { account_id, message_id, attachment_id } => {
                let account = self.app.account(&account_id)?;
                let path = self.app.attachment(&account, &message_id, &attachment_id).await?;
                self.send(Delta::AttachmentReady(path));
            }
            Cmd::ShowCalendar { from, to } => {
                self.view.lock().await.calendar = Some((from, to));
                self.publish().await;
            }
            Cmd::SuggestContacts { query, token, limit } => {
                let mine = self.my_addresses().await;
                let items = self.app.store().suggest_contacts(&query, &mine, limit as usize, now_ms())?;
                self.send(Delta::Contacts { token, items });
            }
            Cmd::Search { query, token, limit } => {
                let q = pigeon_core::search::parse(&query);
                let items = self.app.store().search_query(&q, limit)?;
                self.send(Delta::SearchResults { token, items, server: ServerSearch::Idle });
            }
            Cmd::SearchServers { query, token } => {
                let q = pigeon_core::search::parse(&query);
                let local = self.app.store().search_query(&q, SEARCH_LIMIT)?;
                self.send(Delta::SearchResults { token, items: local, server: ServerSearch::Loading });
                let found = self.app.search_servers(&q, 50).await?;
                let items = self.app.store().search_query(&q, SEARCH_LIMIT)?;
                self.send(Delta::SearchResults { token, items, server: ServerSearch::Done(found) });
            }
            Cmd::Rsvp { account_id, calendar_id, event_id, response } => {
                let account = self.app.account(&account_id)?;
                self.app.rsvp(&account, &calendar_id, &event_id, response, None).await?;
                self.toast(
                    match response {
                        Response::Accepted => "Accepted",
                        Response::Tentative => "Replied maybe",
                        _ => "Declined",
                    },
                    false,
                    None,
                );
                self.publish().await;
            }
            Cmd::CreateEvent { account_id, mut draft } => {
                let account = self.app.account(&account_id)?;
                if draft.calendar_id.is_empty() {
                    draft.calendar_id = self.app.primary_calendar(&account)?;
                }
                let event = self.app.create_event(&account, &draft).await?;
                self.toast(format!("Created “{}”", event.title), false, None);
                self.publish().await;
            }
            Cmd::UpdateEvent { account_id, event_id, draft } => {
                let account = self.app.account(&account_id)?;
                let event = match self.app.update_event(&account, &event_id, &draft).await {
                    Ok(e) => e,
                    Err(e) => {
                        // The UI may have moved it optimistically (drag); put it back.
                        self.publish().await;
                        return Err(e);
                    }
                };
                self.toast(format!("Updated “{}”", event.title), false, None);
                self.publish().await;
            }
            Cmd::FreeBusy { account_id, emails, from, to, token } => {
                let account = self.app.account(&account_id)?;
                let result = self.app.free_busy(&account, &emails, from, to).await?;
                self.send(Delta::FreeBusy { token, result });
            }
            Cmd::DeleteEvent { account_id, calendar_id, event_id } => {
                let account = self.app.account(&account_id)?;
                self.app.delete_event(&account, &calendar_id, &event_id).await?;
                self.toast("Event deleted", false, None);
                self.publish().await;
            }
        }
        Ok(())
    }

    /// Send a saved draft now; reports Sent / SendFailed.
    async fn send_now(self: Arc<Self>, draft: Draft) {
        // While sending, the draft's server copy must not be re-created.
        self.draft_sending.lock().unwrap_or_else(|p| p.into_inner()).insert(draft.id.clone());
        let sent = self.app.send(&draft).await;
        self.draft_sending.lock().unwrap_or_else(|p| p.into_inner()).remove(&draft.id);
        match sent {
            Ok(_) => {
                self.send(Delta::Sent { draft_id: draft.id.clone() });
                self.toast(format!("Sent from {}", draft.from.address.email), false, None);
                tokio::spawn(self.clone().sync());
            }
            Err(e) => {
                self.send(Delta::SendFailed { draft_id: draft.id.clone(), error: e.to_string() });
                self.fail("Send failed, saved to Drafts", &e);
            }
        }
    }

    /// Send every scheduled draft whose time has come.
    async fn send_due(self: Arc<Self>) {
        let due = match self.app.store().drafts() {
            Ok(d) => pigeon_core::compose::due_drafts(d, now_ms()),
            Err(_) => return,
        };
        for mut draft in due {
            if !self.in_flight.lock().await.insert(draft.id.clone()) {
                continue;
            }
            draft.scheduled_at = None;
            draft.undo_window = false;
            if self.app.save_draft(&mut draft).is_ok() {
                self.clone().send_now(draft.clone()).await;
            }
            self.in_flight.lock().await.remove(&draft.id);
            self.publish().await;
        }
    }

    async fn open(
        self: Arc<Self>,
        account: Account,
        thread_id: String,
        messages: Vec<MessageHeader>,
        focus: String,
        single: bool,
    ) -> Result<()> {
        let unread: Vec<String> = messages.iter().filter(|m| !m.is_read).map(|m| m.id.clone()).collect();
        let ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
        self.send(Delta::Reader(ReaderView { account_id: account.id.clone(), thread_id, messages, focus, single }));
        if !unread.is_empty() {
            self.app.act(&account, &unread, None, &MailAction::MarkRead)?;
            self.publish().await;
            tokio::spawn(self.clone().flush(account.clone()));
        }
        // Newest first: the expanded message is what the user reads.
        tokio::spawn(self.clone().open_bodies(account, ids.into_iter().rev().collect()));
        Ok(())
    }

    async fn flush(self: Arc<Self>, account: Account) {
        match self.app.flush(&account).await {
            Ok(r) => {
                for (_, err) in &r.rejected {
                    self.toast(format!("Change undone: the server refused it ({err})"), true, None);
                }
                if !r.rejected.is_empty() {
                    self.publish().await;
                }
                if r.needs_sign_in {
                    self.set_sync(|s| {
                        if !s.needs_sign_in.contains(&account.id) {
                            s.needs_sign_in.push(account.id.clone());
                        }
                    })
                    .await;
                }
            }
            Err(e) => tracing::warn!(error = %e, "flush failed"),
        }
    }
}

/// Run the engine until the command channel closes.
pub async fn run(app: Arc<Pigeon>, mut rx: mpsc::UnboundedReceiver<Cmd>, tx: mpsc::UnboundedSender<Delta>) {
    // Share sign-ins with the `coo` helper (AI agents) while the app runs.
    crate::broker::start(app.clone());
    let svc = Arc::new(Service {
        app,
        tx,
        view: Mutex::new(View {
            mailbox: Mailbox::Unified(FolderRole::Inbox),
            threaded: true,
            calendar: None,
            limit: LIST_FIRST_PAGE,
        }),
        syncing: AtomicBool::new(false),
        sync_state: Mutex::new(SyncState::default()),
        toast_seq: Default::default(),
        my_addresses: Mutex::new(None),
        draft_gen: Default::default(),
        draft_sending: Default::default(),
        notify_marks: Default::default(),
        in_flight: Mutex::new(Default::default()),
    });
    // Watermark existing mail so only what arrives from now on notifies.
    svc.check_new_mail().await;
    svc.publish().await;
    let mut tick = tokio::time::interval(SYNC_EVERY);
    // Scheduled sends (send later, undo window); also catches ones that came
    // due while the app was closed.
    let mut due = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(cmd) => { tokio::spawn(svc.clone().handle(cmd)); }
                None => break,
            },
            _ = tick.tick() => { tokio::spawn(svc.clone().sync()); }
            _ = due.tick() => { tokio::spawn(svc.clone().send_due()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_in_debug() {
        let c = Cmd::LoginPassword {
            provider: ProviderKind::ICloud,
            email: "a@icloud.com".into(),
            password: Secret("hunter2".into()),
        };
        assert!(!format!("{c:?}").contains("hunter2"));
    }

    #[test]
    fn hues_are_stable_and_in_range() {
        let h = hue_for("Me@Work.com");
        assert_eq!(h, hue_for("me@work.com"));
        assert!((1..=8).contains(&h));
        let distinct: std::collections::HashSet<u8> =
            ["a@x.com", "b@x.com", "c@x.com", "d@x.com", "e@x.com", "f@x.com", "g@x.com"]
                .iter()
                .map(|e| hue_for(e))
                .collect();
        assert!(distinct.len() >= 4, "spread across slots");
    }

    #[test]
    fn cid_images_become_data_urls() {
        let dir = std::env::temp_dir().join(format!("pigeon-cid-{}", pigeon_core::auth::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("logo.png");
        std::fs::write(&path, b"PNG").unwrap();
        let body = StoredBody {
            html: Some(r#"<img src="cid:logo@x"><img src="cid:missing">"#.into()),
            attachments: vec![StoredAttachment {
                meta: AttachmentMeta {
                    id: "1".into(),
                    name: "logo.png".into(),
                    content_type: Some("image/png".into()),
                    content_id: Some("logo@x".into()),
                    is_inline: true,
                    ..Default::default()
                },
                local_path: Some(path.to_string_lossy().into_owned()),
            }],
            ..Default::default()
        };
        let html = inline_images(body).html.unwrap();
        assert!(html.contains("data:image/png;base64,UE5H"));
        assert!(html.contains("cid:missing"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
