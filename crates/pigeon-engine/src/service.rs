//! The UI-facing engine loop: the UI sends [`Cmd`]s, the engine answers with
//! [`Delta`]s rendered from the local store. Runs on its own Tokio runtime;
//! the UI thread never touches SQL or the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use pigeon_core::compose::{Draft, ReplyKind, Sender};
use pigeon_core::model::*;
pub use pigeon_core::search::{Elsewhere, SearchScope};
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

    /// Searching "here": this view's folder(s).
    pub fn search_scope(&self) -> SearchScope {
        match self {
            Mailbox::Unified(role) => SearchScope::Role(*role),
            Mailbox::Folder { account_id, folder_id } => {
                SearchScope::Folder { account_id: account_id.clone(), folder_id: folder_id.clone() }
            }
        }
    }
}

/// One list item of an `ActMany`: a message, or a thread (see `Cmd::Act`).
#[derive(Debug, Clone, PartialEq)]
pub struct ActItem {
    pub account_id: String,
    pub ids: Vec<String>,
    pub thread_id: Option<String>,
}

/// Undo "account" of a bulk action that touched several accounts.
pub const BULK_UNDO: &str = "*";

#[derive(Debug, Clone)]
pub enum Cmd {
    /// Send a bug report the user wrote (the engine adds which providers
    /// are connected and the macOS version).
    ReportBug(crate::telemetry::BugReport),
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
    /// Select all (⌘A): list the whole view, up to the cap.
    LoadAll,
    /// Recipient autocomplete; `token` lets the UI drop stale results.
    SuggestContacts {
        query: String,
        token: u64,
        limit: u32,
    },
    /// Local search within `scope`; `token` lets the UI drop stale
    /// results. A narrower scope also reports what it leaves out.
    Search {
        query: String,
        scope: SearchScope,
        token: u64,
        limit: u32,
    },
    /// Then ask the servers (the scope's account, or all) for older
    /// matches and re-run locally.
    SearchServers {
        query: String,
        scope: SearchScope,
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
    /// `Act` on several list items at once (a multi-selection), possibly
    /// across accounts; one toast, one undo for all of them.
    ActMany {
        items: Vec<ActItem>,
        action: MailAction,
        label: String,
    },
    /// `account_id` is `BULK_UNDO` for an `ActMany` spanning accounts.
    Undo {
        account_id: String,
        handle: i64,
    },
    SyncNow,
    /// Browser (OAuth) sign-in; `login_hint` pre-fills the provider's page
    /// with the address typed in "Add account".
    Login {
        provider: ProviderKind,
        login_hint: Option<String>,
    },
    /// "Add account": who handles mail for `email`. Answered with
    /// `Delta::ProviderDetected` carrying the same `token`.
    DetectProvider {
        email: String,
        token: u64,
    },
    /// Connect a Zoom account in the browser (for Zoom calls on events).
    ZoomConnect,
    ZoomDisconnect,
    /// Give a Google account Drive access (browser consent for the
    /// `drive.file` scope: only files the user picks).
    ConnectDrive {
        account_id: String,
    },
    /// Open the Google Drive picker in the browser for a draft, connecting
    /// Drive first if the account needs it. Answered with
    /// `Delta::DrivePicked`.
    DrivePick {
        draft_id: String,
        account_id: String,
        /// The account's nickname, shown on the picker page with its address.
        nickname: Option<String>,
    },
    /// Right-click › Save Image…: fetch (or decode, for `data:`) an image
    /// from a message and write it to `path`.
    SaveImage {
        url: String,
        path: std::path::PathBuf,
    },
    /// "Anyone with the link can view" for Drive files linked in a message.
    DriveShare {
        account_id: String,
        file_ids: Vec<String>,
    },
    /// iCloud-style sign-in with an app-specific password.
    LoginPassword {
        provider: ProviderKind,
        email: String,
        password: Secret,
        /// iCloud Mail address to send from, when the Apple ID is an
        /// address at another provider.
        mail_address: Option<String>,
    },
    /// Sign out and delete the account's local data.
    Logout {
        account_id: String,
    },
    LoadSenders,
    /// Offer (`offered`) or stop offering an iCloud send-from address.
    /// Answered with `Delta::Senders`.
    SetSendAddress {
        account_id: String,
        email: String,
        offered: bool,
    },
    /// Pick the address an account goes by (None: its own). Answered with
    /// `Delta::Senders` and a fresh `Delta::Accounts`.
    SetMainAddress {
        account_id: String,
        email: Option<String>,
    },
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
    /// A blank draft from `from_account`'s own address (the UI passes the
    /// default account for new messages); `None` or an account that's gone:
    /// the first sending address.
    NewDraft {
        from_account: Option<String>,
    },
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
    /// Right-click › Save As… (`dest` is the file) or Save All Attachments…
    /// (`into_folder`: `dest` is a folder; names that are taken get " 2").
    /// `files`: (attachment id, file name).
    SaveAttachments {
        account_id: String,
        message_id: String,
        files: Vec<(String, String)>,
        dest: std::path::PathBuf,
        into_folder: bool,
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
    /// The alias the account goes by (sidebar, new mail), when one is picked.
    pub main_address: Option<String>,
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
    /// The mail action an undo toast confirms (picks its icon).
    pub action: Option<MailAction>,
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
    /// A `Cmd::Login` / `Cmd::LoginPassword` finished: the account is
    /// saved (new, or signed in again).
    SignedIn {
        account_id: String,
    },
    /// The answer to `Cmd::DetectProvider`.
    ProviderDetected {
        token: u64,
        result: pigeon_core::provider_detect::Detection,
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
    /// The connected Zoom account, if any.
    Zoom(Option<pigeon_core::model::ConnectedService>),
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
    /// Files picked from Google Drive for a draft; empty when the picker
    /// was cancelled or failed (the UI stops waiting either way).
    DrivePicked {
        draft_id: String,
        account_id: String,
        files: Vec<pigeon_core::drive::DriveFile>,
    },
    SearchResults {
        token: u64,
        items: Vec<MessageRow>,
        server: ServerSearch,
        /// Local matches outside the scope (empty for all mail).
        elsewhere: Elsewhere,
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
    /// Undo handles of bulk actions that spanned accounts, by bulk handle.
    bulk_undo: std::sync::Mutex<std::collections::HashMap<i64, Vec<(String, i64)>>>,
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
        self.send(Delta::Toast(Toast { id, message: message.into(), error, undo, ..Default::default() }));
    }

    /// "Archived …" with Undo, after `action`.
    fn action_toast(&self, message: impl Into<String>, undo: Option<(String, i64)>, action: &MailAction) {
        let id = self.toast_seq.fetch_add(1, Ordering::Relaxed);
        let action = Some(action.clone());
        self.send(Delta::Toast(Toast { id, message: message.into(), undo, action, ..Default::default() }));
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
                    main_address: store.main_address(&a.id)?,
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
        self.send(Delta::Zoom(self.app.zoom_service().unwrap_or(None)));
        self.send(Delta::List { mailbox, threaded, items });
        self.send(Delta::NextUp(next_up.into_iter().filter(|e| !e.event.all_day).take(3).collect()));
        if let Some((from, to, events, calendars)) = cal {
            self.send(Delta::Events { from, to, events, calendars });
        }
        Ok(())
    }

    /// Matches within the scope, and the count of those outside it (local
    /// index only).
    fn search_local(
        &self,
        q: &pigeon_core::search::SearchQuery,
        scope: &SearchScope,
        limit: u32,
    ) -> Result<(Vec<MessageRow>, Elsewhere)> {
        let store = self.app.store();
        Ok((store.search_scoped(q, scope, limit)?, store.search_elsewhere(q, scope)?))
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
                    let parts = [("mail", r.mail.as_ref().err()), ("calendar", r.calendar.as_ref().err())];
                    for (part, e) in parts.into_iter().filter_map(|(p, e)| e.map(|e| (p, e))) {
                        failures += 1;
                        let network = e.is_network();
                        if network {
                            network_failures += 1;
                        }
                        if matches!(e, Error::SignInRequired(_)) {
                            needs_sign_in.push(r.account.id.clone());
                            continue;
                        }
                        errors.push(format!("{}: {e}", r.account.email));
                        // A dropped connection (sleep, a network change) is
                        // expected: a breadcrumb, not a report. The provider
                        // and part survive scrubbing, unlike the message.
                        let provider = r.account.provider.as_str();
                        if network {
                            tracing::warn!(error = %e, provider, part, "sync failed (network)");
                        } else {
                            tracing::error!(error = %e, provider, part, "sync failed");
                        }
                    }
                }
            }
            Err(e) => {
                failures += 1;
                if e.is_network() {
                    network_failures += 1;
                    tracing::warn!(error = %e, "sync failed (network)");
                } else {
                    tracing::error!(error = %e, "sync failed");
                }
                errors.push(e.to_string())
            }
        }
        let offline = failures > 0 && failures == network_failures;
        needs_sign_in.dedup();
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
            Cmd::ReportBug(mut report) => {
                let mut kinds: Vec<&str> =
                    self.app.accounts().unwrap_or_default().iter().map(|a| a.provider.as_str()).collect();
                kinds.sort_unstable();
                report.details.push(("accounts".into(), kinds.join(", ")));
                let release = concat!("coo-app@", env!("CARGO_PKG_VERSION"));
                let sent = tokio::task::spawn_blocking(move || {
                    if let Ok(out) = std::process::Command::new("sw_vers").arg("-productVersion").output() {
                        report.details.push(("macos".into(), String::from_utf8_lossy(&out.stdout).trim().to_string()));
                    }
                    crate::telemetry::send_report(release, &report)
                })
                .await
                .unwrap_or_else(|_| Err("Couldn't send the report.".into()));
                match sent {
                    Ok(()) => self.toast("Thanks! Your report was sent.", false, None),
                    Err(e) => self.toast(e, true, None),
                }
            }
            Cmd::LoadAll => {
                {
                    let mut v = self.view.lock().await;
                    if v.limit >= LIST_MAX {
                        return Ok(());
                    }
                    v.limit = LIST_MAX;
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
            Cmd::Act { account_id, ids, thread_id, action, label } => {
                let account = self.app.account(&account_id)?;
                let ids = self.act_ids(&account_id, ids, thread_id, &action).await?;
                if ids.is_empty() {
                    return Ok(());
                }
                let from = self.act_folder(&account_id).await;
                let handle = self.app.act(&account, &ids, from.as_deref(), &action)?;
                let undoable = !matches!(action, MailAction::MarkRead | MailAction::MarkUnread);
                if undoable {
                    self.action_toast(label, Some((account_id, handle)), &action);
                }
                self.publish().await;
                self.flush(account).await;
            }
            Cmd::ActMany { items, action, label } => {
                // One act per account, so each account's change undoes as one.
                let mut by_account: Vec<(String, Vec<String>)> = Vec::new();
                for item in items {
                    let ids = self.act_ids(&item.account_id, item.ids, item.thread_id, &action).await?;
                    match by_account.iter_mut().find(|(a, _)| *a == item.account_id) {
                        Some((_, all)) => all.extend(ids),
                        None => by_account.push((item.account_id, ids)),
                    }
                }
                let mut handles = Vec::new();
                let mut touched = Vec::new();
                for (account_id, ids) in by_account.into_iter().filter(|(_, ids)| !ids.is_empty()) {
                    let account = self.app.account(&account_id)?;
                    let from = self.act_folder(&account_id).await;
                    handles.push((account_id, self.app.act(&account, &ids, from.as_deref(), &action)?));
                    touched.push(account);
                }
                let undoable = !matches!(action, MailAction::MarkRead | MailAction::MarkUnread);
                if undoable && !handles.is_empty() {
                    let undo = if handles.len() == 1 {
                        handles.pop()
                    } else {
                        let bulk = handles[0].1;
                        self.bulk_undo.lock().unwrap().insert(bulk, handles);
                        Some((BULK_UNDO.to_string(), bulk))
                    };
                    self.action_toast(label, undo, &action);
                }
                self.publish().await;
                for account in touched {
                    self.clone().flush(account).await;
                }
            }
            Cmd::Undo { account_id, handle } if account_id == BULK_UNDO => {
                let handles = self.bulk_undo.lock().unwrap().remove(&handle).unwrap_or_default();
                let mut touched = Vec::new();
                for (account_id, handle) in handles {
                    let account = self.app.account(&account_id)?;
                    self.app.undo(&account, handle)?;
                    touched.push(account);
                }
                self.publish().await;
                for account in touched {
                    self.clone().flush(account).await;
                }
            }
            Cmd::Undo { account_id, handle } => {
                let account = self.app.account(&account_id)?;
                self.app.undo(&account, handle)?;
                self.publish().await;
                self.flush(account).await;
            }
            Cmd::SyncNow => tokio::spawn(self.clone().sync()).await.map_err(|e| Error::Other(e.to_string()))?,
            Cmd::DetectProvider { email, token } => {
                let result = crate::detect::detect(&email).await;
                self.send(Delta::ProviderDetected { token, result });
            }
            Cmd::Login { provider, login_hint } => {
                // The URL can carry the hint (an address): telemetry scrubs
                // both query strings and addresses from breadcrumbs.
                let login = self.app.login(provider, login_hint.as_deref(), |url| tracing::info!(url, "sign-in URL"));
                let account = match login.await {
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
                self.send(Delta::SignedIn { account_id: account.id.clone() });
                tokio::spawn(self.clone().sync());
            }
            Cmd::ZoomConnect => match self.app.zoom_connect(|url| tracing::info!(url, "Zoom sign-in URL")).await {
                Ok(z) => {
                    self.toast(format!("Zoom connected as {}", z.email), false, None);
                    self.send(Delta::Zoom(Some(z)));
                }
                Err(e) => {
                    self.toast(e.to_string(), true, None);
                    self.send(Delta::Zoom(self.app.zoom_service().unwrap_or(None)));
                }
            },
            Cmd::ZoomDisconnect => {
                self.app.zoom_disconnect().await?;
                self.toast("Zoom disconnected. Meetings you already created keep working.", false, None);
                self.send(Delta::Zoom(None));
            }
            Cmd::ConnectDrive { account_id } => match self.connect_drive(&account_id).await {
                Ok(()) => self.toast("Google Drive connected", false, None),
                Err(e) => self.toast(e.to_string(), true, None),
            },
            Cmd::DrivePick { draft_id, account_id, nickname } => {
                let files = match self.drive_pick(&account_id, nickname.as_deref()).await {
                    Ok(files) => files,
                    Err(e) => {
                        // Not `fail`: messages can name the account.
                        tracing::warn!(error = %e, "Google Drive pick failed");
                        self.toast(e.to_string(), true, None);
                        Vec::new()
                    }
                };
                self.send(Delta::DrivePicked { draft_id, account_id, files });
            }
            Cmd::SaveImage { url, path } => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match save_image(&url, &path).await {
                    Ok(()) => self.toast(format!("Saved {name}"), false, None),
                    Err(e) => {
                        // No URL in the log: it can carry tracking tokens.
                        tracing::warn!(error = %e, "could not save an image");
                        self.toast(format!("Couldn't save the image: {e}"), true, None);
                    }
                }
            }
            Cmd::DriveShare { account_id, file_ids } => {
                if let Err(e) = self.app.drive_share(&account_id, &file_ids).await {
                    tracing::warn!(error = %e, "could not share Drive files");
                    self.toast(format!("Couldn't let anyone with the link view: {e}"), true, None);
                }
            }
            Cmd::LoginPassword { provider, email, password, mail_address } => {
                let account = self.app.login_password(provider, &email, &password.0).await?;
                if let Some(addr) = mail_address.as_deref().map(str::trim).filter(|a| !a.is_empty())
                    && let Err(e) = self.app.set_send_address(&account.id, addr, true)
                {
                    tracing::warn!(error = %e, "could not save the iCloud Mail address");
                }
                *self.my_addresses.lock().await = None;
                self.toast(format!("Signed in as {}", account.email), false, None);
                self.publish().await;
                self.send(Delta::SignedIn { account_id: account.id.clone() });
                // The From pickers learn the new account's addresses.
                if let Ok(senders) = self.app.senders().await {
                    self.send(Delta::Senders(senders));
                }
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
            Cmd::SetSendAddress { account_id, email, offered } => {
                let r = self.app.set_send_address(&account_id, &email, offered);
                *self.my_addresses.lock().await = None;
                self.send(Delta::Senders(self.app.senders().await?));
                self.publish().await;
                r?;
            }
            Cmd::SetMainAddress { account_id, email } => {
                let r = self.app.set_main_address(&account_id, email.as_deref()).await;
                self.send(Delta::Senders(self.app.senders().await?));
                self.publish().await;
                r?;
            }
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
            Cmd::NewDraft { from_account } => {
                let from = match from_account.and_then(|id| self.app.account(&id).ok()) {
                    Some(account) => self.app.primary_sender(&account)?,
                    None => self
                        .app
                        .senders()
                        .await?
                        .into_iter()
                        .next()
                        .ok_or_else(|| Error::Other("add an account first".into()))?,
                };
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
            Cmd::SaveAttachments { account_id, message_id, files, dest, into_folder } => {
                let account = self.app.account(&account_id)?;
                let mut saved = Vec::new();
                for (attachment_id, name) in &files {
                    let to = if into_folder { free_path(&dest, &safe_file_name(name)) } else { dest.clone() };
                    let copied = match self.app.attachment(&account, &message_id, attachment_id).await {
                        Ok(from) => tokio::fs::copy(&from, &to).await.map_err(|e| Error::Other(e.to_string())),
                        Err(e) => Err(e),
                    };
                    match copied {
                        Ok(_) => saved.push(to),
                        Err(e) => {
                            // No file names in the log.
                            tracing::warn!(error = %e, "could not save an attachment");
                            self.toast(format!("Couldn't save {name}: {e}"), true, None);
                        }
                    }
                }
                let msg = match saved.as_slice() {
                    [] => None,
                    [one] => one.file_name().map(|n| format!("Saved {}", n.to_string_lossy())),
                    many => {
                        let folder = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        Some(format!("Saved {} attachments to {folder}", many.len()))
                    }
                };
                if let Some(msg) = msg {
                    self.toast(msg, false, None);
                }
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
            Cmd::Search { query, scope, token, limit } => {
                let q = pigeon_core::search::parse(&query);
                let (items, elsewhere) = self.search_local(&q, &scope, limit)?;
                self.send(Delta::SearchResults { token, items, server: ServerSearch::Idle, elsewhere });
            }
            Cmd::SearchServers { query, scope, token } => {
                let q = pigeon_core::search::parse(&query);
                let (items, elsewhere) = self.search_local(&q, &scope, SEARCH_LIMIT)?;
                self.send(Delta::SearchResults { token, items, server: ServerSearch::Loading, elsewhere });
                // Only the scope's account needs asking.
                let mut server_q = q.clone();
                if let Some(a) = scope.account_id() {
                    server_q.account = Some(a.to_string());
                }
                let found = self.app.search_servers(&server_q, 50).await?;
                let (items, elsewhere) = self.search_local(&q, &scope, SEARCH_LIMIT)?;
                self.send(Delta::SearchResults { token, items, server: ServerSearch::Done(found), elsewhere });
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

    /// Browser consent for Drive on an existing Google account.
    async fn connect_drive(&self, account_id: &str) -> Result<()> {
        self.app.connect_drive(account_id, |url| tracing::debug!(url, "Drive consent URL")).await
    }

    /// Connect Drive if needed, then run the picker. Empty when cancelled.
    async fn drive_pick(&self, account_id: &str, nickname: Option<&str>) -> Result<Vec<pigeon_core::drive::DriveFile>> {
        if !crate::drive::drive_available() {
            return Err(Error::Other(crate::drive::NOT_IN_BUILD.into()));
        }
        if !self.app.drive_ready(account_id).await? {
            self.toast("Coo needs permission to open the Drive files you pick. Continue in your browser.", false, None);
            self.connect_drive(account_id).await?;
        }
        let picked = self.app.drive_pick(account_id, nickname, |url| tracing::debug!(url, "Drive picker URL")).await;
        match picked {
            Ok(pigeon_google::picker::PickOutcome::Picked(files)) => Ok(files),
            Ok(pigeon_google::picker::PickOutcome::Cancelled) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
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

    /// The folder the current mailbox lists for `account_id`.
    async fn act_folder(&self, account_id: &str) -> Option<String> {
        let v = self.view.lock().await;
        let store = self.app.store();
        v.mailbox.folder_for(|a, r| store.role_folder(a, r).ok().flatten(), account_id)
    }

    /// The message ids an action applies to: `ids`, or for a thread its
    /// messages in the current mailbox (all of them for read state, the
    /// newest for flags, like Gmail/Outlook).
    async fn act_ids(
        &self,
        account_id: &str,
        ids: Vec<String>,
        thread_id: Option<String>,
        action: &MailAction,
    ) -> Result<Vec<String>> {
        let Some(thread_id) = thread_id else { return Ok(ids) };
        let from = self.act_folder(account_id).await;
        let store = self.app.store();
        let in_view = |m: &MessageHeader| {
            let full = store.message(account_id, &m.id).ok().flatten();
            from.as_ref().is_none_or(|f| full.is_some_and(|h| h.folder_ids.contains(f)))
        };
        let messages = store.thread_messages(account_id, &thread_id)?;
        Ok(match action {
            MailAction::MarkRead | MailAction::MarkUnread => messages.iter().map(|m| m.id.clone()).collect(),
            MailAction::Flag | MailAction::Unflag => {
                messages.iter().rev().find(|m| in_view(m)).map(|m| vec![m.id.clone()]).unwrap_or_default()
            }
            _ => messages.iter().filter(|m| in_view(m)).map(|m| m.id.clone()).collect(),
        })
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
        bulk_undo: Default::default(),
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

/// An attachment's name made safe to save under: no folders (`/`), no
/// Finder-hidden leading dot, no colons (Finder shows them as `/`).
fn safe_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() { "Attachment".into() } else { cleaned.into() }
}

/// `dir/name`, or `dir/name 2.ext`, `name 3.ext`… when that's taken.
fn free_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = std::path::Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.into());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| dir.join(format!("{stem} {n}{ext}"))).find(|c| !c.exists()).expect("some number is free")
}

/// The image at `url` (inline `data:` or http/https) written to `path`.
async fn save_image(url: &str, path: &std::path::Path) -> Result<()> {
    let bytes = if url.starts_with("data:") {
        pigeon_core::images::decode_data_url(url).ok_or_else(|| Error::Other("it isn't a readable image".into()))?.1
    } else if url.starts_with("https://") || url.starts_with("http://") {
        let resp = pigeon_core::http::client()
            .get(url)
            .send()
            .await
            .map_err(|_| Error::Other("it couldn't be downloaded".into()))?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("the server answered {}", resp.status().as_u16())));
        }
        resp.bytes().await.map_err(|_| Error::Other("the download was cut off".into()))?.to_vec()
    } else {
        return Err(Error::Other("it isn't a web or inline image".into()));
    };
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::write(path, bytes))
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .map_err(|e| Error::Other(format!("the file couldn't be written ({e})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saves_inline_images_and_refuses_other_schemes() {
        let dir = std::env::temp_dir().join(format!("coo-save-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.png");
        save_image("data:image/png;base64,iVBORw==", &path).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0x89, b'P', b'N', b'G']);
        assert!(save_image("cid:logo@x", &dir.join("x.png")).await.is_err());
        assert!(save_image("file:///etc/passwd", &dir.join("y.png")).await.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saved_attachments_get_safe_free_names() {
        assert_eq!(safe_file_name("../etc/passwd"), "-etc-passwd");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        assert_eq!(safe_file_name("a:b.pdf"), "a-b.pdf");
        assert_eq!(safe_file_name("  "), "Attachment");
        assert_eq!(safe_file_name("דוח רבעוני.pdf"), "דוח רבעוני.pdf");
        let dir = std::env::temp_dir().join(format!("coo-free-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "r.pdf"), dir.join("r.pdf"));
        std::fs::write(dir.join("r.pdf"), b"").unwrap();
        std::fs::write(dir.join("r 2.pdf"), b"").unwrap();
        assert_eq!(free_path(&dir, "r.pdf"), dir.join("r 3.pdf"));
        std::fs::write(dir.join("notes"), b"").unwrap();
        assert_eq!(free_path(&dir, "notes"), dir.join("notes 2"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn secrets_are_redacted_in_debug() {
        let c = Cmd::LoginPassword {
            provider: ProviderKind::ICloud,
            email: "a@icloud.com".into(),
            password: Secret("hunter2".into()),
            mail_address: None,
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
