//! UI state: one bundle of signals shared through context, fed by the
//! engine's deltas.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, Sender};
use pigeon_core::model::{Calendar, EventRow, FolderRole, FreeBusy, MessageRow, StoredBody};
use pigeon_engine::service::{
    AccountNav, Delta, InviteView, ListItem, Mailbox, ReaderView, ServerSearch, SyncState, Toast,
};

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

impl Theme {
    pub fn from_pref(s: &str) -> Self {
        match s {
            "dark" => Theme::Dark,
            "light" => Theme::Light,
            _ => Theme::System,
        }
    }

    pub fn as_pref(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }
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

/// The Move/Label picker while open.
#[derive(Clone, PartialEq)]
pub struct MovePicker {
    pub item: ListItem,
    /// Gmail: add a label instead of moving.
    pub label: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalView {
    Day,
    Week,
    Month,
}

/// A message on its way out, shown as a row until the next sync lists it.
#[derive(Clone, PartialEq, Debug)]
pub enum OutState {
    Sending,
    /// Sent at (ms); dropped once a later sync has run.
    Sent(i64),
    Failed(String),
    /// Scheduled to go out at (ms) by send later.
    Queued(i64),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Outgoing {
    pub draft: Draft,
    pub state: OutState,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub move_picker: Signal<Option<MovePicker>>,
    /// The Coo window has keyboard focus.
    pub window_focused: Signal<bool>,
    /// Outgoing messages (sending / failed / scheduled), newest first.
    pub outgoing: Signal<Vec<Outgoing>>,
    /// Settings should open the iCloud password form (re-sign-in).
    pub icloud_form: Signal<bool>,
    /// Last OAuth sign-in failure (provider, message), shown by onboarding.
    pub login_error: Signal<Option<(pigeon_core::model::ProviderKind, String)>>,
    /// This session started with no accounts: show first-sync progress
    /// after the first sign-in instead of an empty inbox.
    pub first_run: Signal<bool>,
    /// Draft id whose send was just cancelled (its Draft delta reopens it).
    pub reopening: Signal<Option<String>>,
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
    /// Invites in loaded bodies, resolved to local events.
    pub invites: Signal<HashMap<(String, String), InviteView>>,
    /// RSVPs sent this session (`account|event_id`), shown before the next sync.
    pub rsvp_sent: Signal<HashMap<String, pigeon_core::model::Response>>,
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
    /// Day / Week / Month.
    pub cal_view: Signal<CalView>,
    /// First day of the week in effect (setting or macOS).
    pub first_weekday: Signal<chrono::Weekday>,
    /// The day the calendar is focused on (Day view shows it; Month shows its month).
    pub cal_day: Signal<chrono::NaiveDate>,
    pub theme: Signal<Theme>,
    /// Color theme family id (see `themes`).
    pub palette: Signal<String>,
    pub settings_open: Signal<bool>,
    /// "Connect an AI agent" popup.
    pub agent_setup_open: Signal<bool>,
    pub os_dark: Signal<bool>,
    pub signing_in: Signal<bool>,
    pub collapsed_accounts: Signal<HashSet<String>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            move_picker: Signal::new(None),
            window_focused: Signal::new(true),
            outgoing: Signal::new(Vec::new()),
            icloud_form: Signal::new(false),
            login_error: Signal::new(None),
            first_run: Signal::new(false),
            reopening: Signal::new(None),
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
            invites: Signal::new(HashMap::new()),
            rsvp_sent: Signal::new(HashMap::new()),
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
            week_start: Signal::new(crate::format::week_start(chrono::Local::now(), crate::week::first_day())),
            first_weekday: Signal::new(crate::week::first_day()),
            cal_view: Signal::new(CalView::Week),
            cal_day: Signal::new(chrono::Local::now().date_naive()),
            theme: Signal::new(Theme::from_pref(&crate::prefs::get().theme_mode)),
            palette: Signal::new({
                let p = crate::prefs::get().palette;
                if crate::themes::is_known(&p) { p } else { crate::themes::DEFAULT.to_string() }
            }),
            settings_open: Signal::new(false),
            agent_setup_open: Signal::new(false),
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

    /// Track a draft as outgoing (replacing an earlier entry for it).
    pub fn track_outgoing(&self, draft: Draft, state: OutState) {
        let mut out = self.outgoing;
        let mut w = out.write();
        w.retain(|o| o.draft.id != draft.id);
        w.insert(0, Outgoing { draft, state });
    }

    /// A toast raised by the UI itself (no engine round trip).
    pub fn local_toast(&self, message: impl Into<String>, error: bool) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.push_toast(Toast { id, message: message.into(), error, undo: None, cancel_send: None });
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

/// Debug builds: fake list states for screenshots, e.g.
/// PIGEON_DEBUG_OPEN=auth-expired | offline | cold | outgoing.
fn debug_patch(s: &AppState, delta: Delta) -> Delta {
    if !cfg!(debug_assertions) {
        return delta;
    }
    let Ok(want) = std::env::var("PIGEON_DEBUG_OPEN") else { return delta };
    match (want.as_str(), delta) {
        ("auth-expired", Delta::Sync(mut st)) => {
            st.needs_sign_in = s.accounts.peek().iter().take(1).map(|a| a.account.id.clone()).collect();
            st.error = None;
            Delta::Sync(st)
        }
        ("offline", Delta::Sync(mut st)) => {
            st.offline = true;
            st.error = Some("network unreachable".into());
            Delta::Sync(st)
        }
        ("consent", Delta::Accounts(_)) => {
            let mut e = s.login_error;
            e.set(Some((
                pigeon_core::model::ProviderKind::Microsoft,
                "access_denied: AADSTS65001: The user or administrator has not consented to use the application".into(),
            )));
            Delta::Accounts(Vec::new())
        }
        ("first-sync", Delta::Accounts(mut a)) => {
            let mut f = s.first_run;
            f.set(true);
            for (i, n) in a.iter_mut().enumerate() {
                n.first_sync_done = i == 0;
            }
            Delta::Accounts(a)
        }
        ("cold", Delta::Accounts(mut a)) => {
            a.iter_mut().for_each(|n| n.first_sync_done = false);
            Delta::Accounts(a)
        }
        ("cold", Delta::List { mailbox, threaded, .. }) => Delta::List { mailbox, threaded, items: Vec::new() },
        ("outgoing", Delta::Accounts(a)) => {
            if s.outgoing.peek().is_empty()
                && let Some(n) = a.first()
            {
                let from = pigeon_core::compose::Sender {
                    account_id: n.account.id.clone(),
                    address: pigeon_core::model::Address::new(None, &n.account.email),
                };
                let mk = |id: &str, subject: &str, to: &str| {
                    let mut d = Draft::new(from.clone(), 0);
                    d.id = id.into();
                    d.subject = subject.into();
                    d.to = vec![pigeon_core::model::Address::new(Some(to), "x@example.com")];
                    d
                };
                s.track_outgoing(mk("d1", "Re: Contractor invoice", "Jonah Weiss"), OutState::Sending);
                s.track_outgoing(
                    mk("d2", "Re: Venue options for the offsite", "Lena Fischer"),
                    OutState::Failed("connection reset".into()),
                );
            }
            Delta::Accounts(a)
        }
        (_, d) => d,
    }
}

/// Apply engine deltas to the signals, forever.
pub async fn pump(mut s: AppState) {
    let Some(mut rx) = RX.lock().unwrap().take() else { return };
    // Coalesce bursts: apply everything already queued without awaiting in
    // between, so Dioxus re-renders once per batch, not once per delta.
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while batch.len() < 64
            && let Ok(more) = rx.try_recv()
        {
            batch.push(more);
        }
        for delta in batch {
            apply(&mut s, delta);
        }
    }
}

fn apply(s: &mut AppState, delta: Delta) {
    {
        let delta = debug_patch(s, delta);
        match delta {
            Delta::NewMail(rows) => {
                let prefs = crate::prefs::get();
                // One soft sound per batch, even with the window in front.
                if prefs.notify_new_mail && rows.iter().any(|r| !prefs.notify_accounts_muted.contains(&r.account_id)) {
                    crate::sound::play(&prefs.sound_theme, crate::sound::Kind::Mail, prefs.sound_volume);
                }
                // Someone looking at the window sees it arrive.
                if !*s.window_focused.peek() {
                    for note in crate::notify::mail_notes(&rows, &prefs) {
                        crate::notify::post(&note);
                    }
                }
            }
            Delta::LoginFailed { provider, error } => {
                s.signing_in.set(false);
                s.login_error.set(Some((provider, error)));
            }
            Delta::Accounts(a) => {
                if a.is_empty() && !*s.loaded.peek() {
                    s.first_run.set(true);
                }
                let order = crate::prefs::get().account_order;
                s.accounts.set(order_by(a, &order, |n| n.account.id.as_str()));
                s.loaded.set(true);
                s.signing_in.set(false);
            }
            Delta::List { mailbox, threaded, items } => {
                if mailbox == *s.mailbox.peek() && threaded == *s.threaded.peek() {
                    crate::perf::mark_list_arrived();
                    s.list.set(items);
                }
            }
            Delta::Reader(r) => s.reader.set(Some(r)),
            Delta::Body { account_id, message_id, body, invite } => {
                let key = (account_id, message_id);
                match invite {
                    Some(v) => s.invites.write().insert(key.clone(), *v),
                    None => s.invites.write().remove(&key),
                };
                s.bodies.write().insert(key, body);
            }
            Delta::Toast(t) => {
                if t.error {
                    s.signing_in.set(false);
                }
                s.push_toast(t)
            }
            Delta::Sync(st) => {
                // A finished sync has picked up what we sent; drop those rows.
                if !st.syncing {
                    let done = st.last_ok.unwrap_or(0);
                    s.outgoing.write().retain(|o| !matches!(o.state, OutState::Sent(at) if at <= done));
                }
                s.sync.set(st)
            }
            Delta::NextUp(e) => s.next_up.set(e),
            Delta::Events { from, to, events, calendars } => {
                s.calendar.set(CalendarData { from, to, events, calendars })
            }
            Delta::Senders(v) => s.senders.set(v),
            Delta::Draft(mut d) => {
                // An undone send comes back with its signature in body_html;
                // drop it (the composer shows it separately and re-adds it).
                if s.reopening.peek().as_deref() == Some(d.id.as_str()) {
                    s.reopening.set(None);
                    d.body_html = crate::ui::compose::strip_signature(&d.body_html);
                }
                s.draft.set(Some(d))
            }
            Delta::Scheduled(drafts) => {
                let mut out = s.outgoing.write();
                // Scheduled entries come from the engine; drop ones it no longer has.
                out.retain(|o| !matches!(o.state, OutState::Queued(_)) || drafts.iter().any(|d| d.id == o.draft.id));
                for d in drafts {
                    let state = match d.scheduled_at {
                        Some(_) if d.undo_window => OutState::Sending,
                        Some(at) => OutState::Queued(at),
                        None => continue,
                    };
                    match out.iter_mut().find(|o| o.draft.id == d.id) {
                        Some(o) => o.state = state,
                        None => out.insert(0, Outgoing { draft: d, state }),
                    }
                }
            }
            Delta::Unscheduled { draft_id } => {
                s.outgoing.write().retain(|o| o.draft.id != draft_id);
                s.reopening.set(Some(draft_id));
                s.sending.set(false);
            }
            Delta::Sent { draft_id } => {
                if let Some(o) = s.outgoing.write().iter_mut().find(|o| o.draft.id == draft_id) {
                    o.state = OutState::Sent(pigeon_core::now_ms());
                }
                s.sending.set(false);
                if s.draft.peek().as_ref().is_some_and(|d| d.id == draft_id) {
                    s.draft.set(None);
                }
            }
            Delta::SendFailed { draft_id, error } => {
                if let Some(o) = s.outgoing.write().iter_mut().find(|o| o.draft.id == draft_id) {
                    o.state = OutState::Failed(error);
                }
                s.sending.set(false)
            }
            Delta::Contacts { token, items } => s.contacts.set((token, items)),
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
                if let Err(e) = open::that_detached(&path) {
                    tracing::warn!(error = %e, ?path, "could not open attachment");
                }
            }
        }
    }
}

/// Order `items` by saved ids: known ids first in saved order, then anything
/// new in its original order. Stale saved ids are ignored.
pub fn order_by<T>(items: Vec<T>, order: &[String], id: impl Fn(&T) -> &str) -> Vec<T> {
    let rank = |t: &T| order.iter().position(|o| o == id(t)).unwrap_or(usize::MAX);
    let mut indexed: Vec<(usize, usize, T)> = items.into_iter().enumerate().map(|(i, t)| (rank(&t), i, t)).collect();
    indexed.sort_by_key(|(r, i, _)| (*r, *i));
    indexed.into_iter().map(|(_, _, t)| t).collect()
}

/// Move the item at `from` so it ends up at index `to` (clamped).
pub fn move_item<T>(v: &mut Vec<T>, from: usize, to: usize) {
    if from >= v.len() {
        return;
    }
    let item = v.remove(from);
    let to = to.min(v.len());
    v.insert(to, item);
}

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn orders_by_saved_ids_and_appends_new() {
        let order = vec!["c".to_string(), "stale".to_string(), "a".to_string()];
        let got = order_by(vec!["a", "b", "c", "d"], &order, |s| s);
        assert_eq!(got, ["c", "a", "b", "d"]);
        assert_eq!(order_by(vec!["x", "y"], &[], |s| s), ["x", "y"]);
    }

    #[test]
    fn moves_items() {
        let mut v = vec![1, 2, 3, 4];
        move_item(&mut v, 0, 2);
        assert_eq!(v, [2, 3, 1, 4]);
        move_item(&mut v, 3, 0);
        assert_eq!(v, [4, 2, 3, 1]);
        move_item(&mut v, 1, 99);
        assert_eq!(v, [4, 3, 1, 2]);
        move_item(&mut v, 9, 0);
        assert_eq!(v, [4, 3, 1, 2]);
    }
}
