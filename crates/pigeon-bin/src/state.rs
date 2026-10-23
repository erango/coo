//! UI state: one bundle of signals shared through context, fed by the
//! engine's deltas.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, Sender};
use pigeon_core::model::{Calendar, EventRow, FolderRole, FreeBusy, MessageRow, StoredBody};
use pigeon_engine::service::{AccountNav, Delta, ListItem, Mailbox, ReaderView, ServerSearch, SyncState, Toast};

use crate::RX;

/// Monotonic ids for search requests (search pane and palette share it).
pub fn next_token() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Mail,
    Calendar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Clone, PartialEq, Default)]
pub struct CalendarData {
    pub from: i64,
    pub to: i64,
    pub events: Vec<EventRow>,
    pub calendars: Vec<(String, Calendar)>,
}

#[derive(Clone, PartialEq)]
pub struct SearchState {
    pub query: String,
    pub token: u64,
    pub items: Vec<MessageRow>,
    pub server: ServerSearch,
}

#[derive(Clone, Copy)]
pub struct AppState {
    /// Event editor form while open.
    pub editor: Signal<Option<crate::ui::event_editor::EventForm>>,
    /// Latest free/busy answer: (request token, result).
    pub free_busy: Signal<(u64, FreeBusy)>,
    /// Search replaces the list while `Some`.
    pub search: Signal<Option<SearchState>>,
    pub palette_open: Signal<bool>,
    pub palette_token: Signal<u64>,
    pub palette_hits: Signal<Vec<MessageRow>>,
    /// Accounts delta received at least once (distinguishes "none" from "loading").
    pub loaded: Signal<bool>,
    /// Latest recipient suggestions: (request token, people).
    pub contacts: Signal<(u64, Vec<pigeon_core::model::Address>)>,
    pub accounts: Signal<Vec<AccountNav>>,
    pub mailbox: Signal<Mailbox>,
    pub threaded: Signal<bool>,
    pub list: Signal<Vec<ListItem>>,
    /// `account|id` of the selected row (thread id or message id).
    pub selected: Signal<Option<String>>,
    pub reader: Signal<Option<ReaderView>>,
    pub bodies: Signal<HashMap<(String, String), StoredBody>>,
    /// Messages whose remote images the user allowed.
    pub remote_ok: Signal<HashSet<String>>,
    /// Messages the user flipped between adapted/paper rendering.
    pub render_flip: Signal<HashSet<String>>,
    pub toasts: Signal<Vec<Toast>>,
    pub sync: Signal<SyncState>,
    pub next_up: Signal<Vec<EventRow>>,
    pub senders: Signal<Vec<Sender>>,
    pub draft: Signal<Option<Draft>>,
    pub sending: Signal<bool>,
    pub mode: Signal<Mode>,
    pub calendar: Signal<CalendarData>,
    /// Monday 00:00 local of the shown week, epoch ms.
    pub week_start: Signal<i64>,
    pub theme: Signal<Theme>,
    pub os_dark: Signal<bool>,
    pub signing_in: Signal<bool>,
    pub collapsed_accounts: Signal<HashSet<String>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            editor: Signal::new(None),
            free_busy: Signal::new((0, Vec::new())),
            search: Signal::new(None),
            palette_open: Signal::new(false),
            palette_token: Signal::new(0),
            palette_hits: Signal::new(Vec::new()),
            loaded: Signal::new(false),
            accounts: Signal::new(Vec::new()),
            mailbox: Signal::new(Mailbox::Unified(FolderRole::Inbox)),
            threaded: Signal::new(true),
            list: Signal::new(Vec::new()),
            selected: Signal::new(None),
            contacts: Signal::new((0, Vec::new())),
            reader: Signal::new(None),
            bodies: Signal::new(HashMap::new()),
            remote_ok: Signal::new(HashSet::new()),
            render_flip: Signal::new(HashSet::new()),
            toasts: Signal::new(Vec::new()),
            sync: Signal::new(SyncState::default()),
            next_up: Signal::new(Vec::new()),
            senders: Signal::new(Vec::new()),
            draft: Signal::new(None),
            sending: Signal::new(false),
            mode: Signal::new(Mode::Mail),
            calendar: Signal::new(CalendarData::default()),
            week_start: Signal::new(crate::format::week_start(chrono::Local::now())),
            theme: Signal::new(Theme::System),
            os_dark: Signal::new(true),
            signing_in: Signal::new(false),
            collapsed_accounts: Signal::new(HashSet::new()),
        }
    }

    pub fn hue(&self, account_id: &str) -> u8 {
        self.accounts.read().iter().find(|a| a.account.id == account_id).map(|a| a.hue).unwrap_or(1)
    }

    pub fn dark(&self) -> bool {
        match *self.theme.read() {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => *self.os_dark.read(),
        }
    }

    /// Key for a list item, matching `selected`.
    pub fn key(item: &ListItem) -> String {
        match item {
            ListItem::Thread(t) => format!("{}|{}", t.account_id, t.thread_id),
            ListItem::Message(m) => format!("{}|{}", m.account_id, m.header.id),
        }
    }

    /// Rows the keyboard moves through: search results while searching.
    pub fn visible_items(&self) -> Vec<ListItem> {
        match self.search.read().as_ref() {
            Some(st) => st.items.iter().cloned().map(ListItem::Message).collect(),
            None => self.list.read().clone(),
        }
    }

    pub fn push_toast(&self, t: Toast) {
        let mut toasts = self.toasts;
        let id = t.id;
        toasts.write().push(t);
        let mut list = toasts;
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(7)).await;
            list.write().retain(|x| x.id != id);
        });
    }
}

/// Apply engine deltas to the signals, forever.
pub async fn pump(mut s: AppState) {
    let Some(mut rx) = RX.lock().unwrap().take() else { return };
    while let Some(delta) = rx.recv().await {
        match delta {
            Delta::Accounts(a) => {
                s.accounts.set(a);
                s.loaded.set(true);
                s.signing_in.set(false);
            }
            Delta::List { mailbox, threaded, items } => {
                if mailbox == *s.mailbox.peek() && threaded == *s.threaded.peek() {
                    s.list.set(items);
                }
            }
            Delta::Reader(r) => s.reader.set(Some(r)),
            Delta::Body { account_id, message_id, body } => {
                s.bodies.write().insert((account_id, message_id), body);
            }
            Delta::Toast(t) => {
                if t.error {
                    s.signing_in.set(false);
                }
                s.push_toast(t)
            }
            Delta::Sync(st) => s.sync.set(st),
            Delta::NextUp(e) => s.next_up.set(e),
            Delta::Events { from, to, events, calendars } => {
                s.calendar.set(CalendarData { from, to, events, calendars })
            }
            Delta::Senders(v) => s.senders.set(v),
            Delta::Draft(d) => s.draft.set(Some(d)),
            Delta::Sent { draft_id } => {
                s.sending.set(false);
                if s.draft.peek().as_ref().is_some_and(|d| d.id == draft_id) {
                    s.draft.set(None);
                }
            }
            Delta::SendFailed { .. } => s.sending.set(false),
            Delta::SearchResults { token, items, server } => {
                let current = s.search.peek().as_ref().map(|st| st.token);
                if current == Some(token) {
                    if let Some(st) = s.search.write().as_mut() {
                        st.items = items;
                        st.server = server;
                    }
                } else if *s.palette_token.peek() == token {
                    s.palette_hits.set(items);
                }
            }
            Delta::FreeBusy { token, result } => s.free_busy.set((token, result)),
            Delta::AttachmentReady(path) => {
            Delta::Contacts { token, items } => s.contacts.set((token, items)),
                if let Err(e) = open::that_detached(&path) {
                    tracing::warn!(error = %e, ?path, "could not open attachment");
                }
            }
        }
    }
}
