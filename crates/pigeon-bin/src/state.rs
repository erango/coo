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

/// Files just picked from Google Drive for a draft, waiting for the user
/// to choose attach or link per file.
#[derive(Clone, PartialEq, Debug)]
pub struct DrivePicked {
    /// Fresh per pick, so a new pick resets the sheet's choices.
    pub token: u64,
    pub draft_id: String,
    pub account_id: String,
    pub files: Vec<pigeon_core::drive::DriveFile>,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub move_picker: Signal<Option<MovePicker>>,
    /// A list row's right-click menu while open.
    pub row_menu: Signal<Option<crate::ui::row_menu::RowMenu>>,
    /// A reply started from a message window, as (account id, message id):
    /// its draft opens in a compose window, not inline in the main reader.
    pub reply_to_window: Signal<Option<(String, String)>>,
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
    /// Keys of a multi-selection (two or more rows, list order); empty
    /// otherwise. See `ui/multi.rs`.
    pub multi: Signal<Vec<String>>,
    /// Row a ⇧-click range starts from.
    pub anchor: Signal<Option<String>>,
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
    /// Messages already received from (account id, lowercased address),
    /// for the sender card's "first time" warning.
    pub sender_seen: Signal<HashMap<(String, String), u32>>,
    /// Sender photos by lowercased address: None = asked / none (initials).
    pub photos: Signal<HashMap<String, Option<String>>>,
    /// Sender card open on hover: (anchor key, card data).
    pub sender_card: Signal<Option<crate::ui::sender_card::CardState>>,
    pub draft: Signal<Option<Draft>>,
    pub sending: Signal<bool>,
    /// Draft id whose Google Drive picker is open in the browser.
    pub drive_waiting: Signal<Option<String>>,
    /// Drive files picked for a draft, until added (or dismissed).
    pub drive_picked: Signal<Option<DrivePicked>>,
    /// Problem shown in the main window's composer (e.g. no recipients).
    pub compose_notice: Signal<Option<String>>,
    /// The docked draft is set aside behind the reader's "Draft:" chip.
    pub docked_hidden: Signal<bool>,
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
    /// Settings subpage shown (a `settings::PAGES` title).
    pub settings_page: Signal<String>,
    /// The connected Zoom account (Zoom calls on events), if any.
    pub zoom: Signal<Option<pigeon_core::model::ConnectedService>>,
    /// "Connect an AI agent" popup.
    pub agent_setup_open: Signal<bool>,
    /// A downloaded update waiting for a restart (its version).
    pub update_ready: Signal<Option<String>>,
    /// The updater's latest status, and whether the Check for Updates dialog is open.
    pub update_status: Signal<crate::updater::Status>,
    pub update_dialog: Signal<bool>,
    pub os_dark: Signal<bool>,
    pub signing_in: Signal<bool>,
    pub collapsed_accounts: Signal<HashSet<String>>,
    /// Account nicknames (Settings › Accounts), by account id.
    pub nicknames: Signal<std::collections::BTreeMap<String, String>>,
    /// The chosen default account for new messages (prefs
    /// `default_from_account`; empty: the top account). Read it through
    /// [`AppState::default_from`].
    pub default_account: Signal<String>,
    /// An account header's right-click menu while open.
    pub account_menu: Signal<Option<crate::ui::account_menu::AccountMenu>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            move_picker: Signal::new(None),
            row_menu: Signal::new(None),
            reply_to_window: Signal::new(None),
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
            mailbox: Signal::new(
                parse_mailbox(&crate::prefs::get().last_mailbox).unwrap_or(Mailbox::Unified(FolderRole::Inbox)),
            ),
            threaded: Signal::new(true),
            list: Signal::new(Vec::new()),
            selected: Signal::new(None),
            multi: Signal::new(Vec::new()),
            anchor: Signal::new(None),
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
            sender_seen: Signal::new(HashMap::new()),
            sender_card: Signal::new(None),
            photos: Signal::new(HashMap::new()),
            draft: Signal::new(None),
            sending: Signal::new(false),
            drive_waiting: Signal::new(None),
            drive_picked: Signal::new(None),
            compose_notice: Signal::new(None),
            docked_hidden: Signal::new(false),
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
            settings_page: Signal::new("General".into()),
            zoom: Signal::new(None),
            agent_setup_open: Signal::new(false),
            update_ready: Signal::new(None),
            update_status: Signal::new(crate::updater::Status::Idle),
            update_dialog: Signal::new(false),
            os_dark: Signal::new(true),
            signing_in: Signal::new(false),
            collapsed_accounts: Signal::new(HashSet::new()),
            nicknames: Signal::new(crate::prefs::get().account_nicknames),
            default_account: Signal::new(crate::prefs::get().default_from_account),
            account_menu: Signal::new(None),
        }
    }

    /// The main window's composer slot (inline replies, docked drafts).
    pub fn main_slot(&self) -> crate::ui::compose::ComposeSlot {
        crate::ui::compose::ComposeSlot {
            draft: self.draft,
            sending: self.sending,
            notice: self.compose_notice,
            windowed: false,
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
    /// How an account is named in the UI: its nickname, else its address.
    pub fn account_label(&self, account_id: &str, email: &str) -> String {
        self.nicknames
            .read()
            .get(account_id)
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| email.to_string())
    }

    /// The account new messages come from (see [`default_from_account`]).
    pub fn default_from(&self) -> Option<String> {
        let accounts = self.accounts.read();
        let pref = self.default_account.read();
        if !*self.loaded.read() {
            // Before the first accounts delta (a ⌘N right at launch): trust
            // the pref, else the saved order's top; the engine checks the
            // account still exists.
            if !pref.is_empty() {
                return Some(pref.clone());
            }
            return crate::prefs::get().account_order.into_iter().next();
        }
        default_from_account(&pref, accounts.iter().map(|a| a.account.id.as_str()))
    }

    /// The account the user picked as the default (not the automatic
    /// fallback), if it's still signed in.
    pub fn chosen_default(&self) -> Option<String> {
        let chosen = self.default_account.read().clone();
        self.accounts.read().iter().any(|a| a.account.id == chosen).then_some(chosen)
    }

    /// Make `account_id` the default for new messages; `None`: automatic
    /// (the top account in the sidebar).
    pub fn set_default_account(&self, account_id: Option<&str>) {
        let id = account_id.unwrap_or_default().to_string();
        let mut d = self.default_account;
        d.set(id.clone());
        crate::prefs::update(move |p| p.default_from_account = id);
    }

    /// Set (or, when blank, clear) an account's nickname.
    pub fn set_nickname(&self, account_id: &str, nickname: &str) {
        let (id, nick) = (account_id.to_string(), nickname.trim().to_string());
        let mut names = self.nicknames;
        if nick.is_empty() {
            names.write().remove(&id);
        } else {
            names.write().insert(id.clone(), nick.clone());
        }
        crate::prefs::update(move |p| {
            if nick.is_empty() {
                p.account_nicknames.remove(&id);
            } else {
                p.account_nicknames.insert(id, nick);
            }
        });
    }

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
                let show = match prefs.mail_alert.as_str() {
                    "sound" => false,
                    "background" => !crate::notify::app_is_active(),
                    _ => true,
                };
                if show {
                    for note in crate::notify::mail_notes(&rows, &prefs) {
                        crate::notify::post(&note);
                    }
                }
            }
            Delta::LoginFailed { provider, error } => {
                s.signing_in.set(false);
                s.login_error.set(Some((provider, error)));
            }
            Delta::Zoom(z) => s.zoom.set(z),
            Delta::Accounts(a) => {
                if a.is_empty() && !*s.loaded.peek() {
                    s.first_run.set(true);
                }
                // A new account means the sign-in in progress finished.
                if a.len() > s.accounts.peek().len() {
                    s.signing_in.set(false);
                }
                // The default account was signed out: forget it, so new
                // messages quietly fall back to the top account.
                let chosen = s.default_account.peek().clone();
                if forget_default(
                    &chosen,
                    s.accounts.peek().iter().map(|n| n.account.id.as_str()),
                    a.iter().map(|n| n.account.id.as_str()),
                ) {
                    s.default_account.set(String::new());
                    crate::prefs::update(|p| p.default_from_account.clear());
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
            Delta::SenderSeen { account_id, email, count } => {
                s.sender_seen.write().insert((account_id, email.to_lowercase()), count);
            }
            Delta::Photo { email, uri } => {
                s.photos.write().insert(email.to_lowercase(), uri);
            }
            Delta::Draft(mut d) => {
                // An undone send comes back with its signature in body_html;
                // drop it (the composer shows it separately and re-adds it).
                if s.reopening.peek().as_deref() == Some(d.id.as_str()) {
                    s.reopening.set(None);
                    d.body_html = crate::ui::compose::strip_signature(&d.body_html);
                }
                // Started from a message window: write it in its own window.
                if crate::ui::reader_window::wants_window(s.reply_to_window.peek().as_ref(), &d) {
                    s.reply_to_window.set(None);
                    crate::ui::compose_window::open(*s, d);
                    return;
                }
                // A new draft replaces a set-aside docked one (already saved).
                s.docked_hidden.set(false);
                s.compose_notice.set(None);
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
                    let nothing_local = items.is_empty() && server == ServerSearch::Idle;
                    if let Some(st) = s.search.write().as_mut() {
                        st.items = items;
                        st.server = server;
                    }
                    if nothing_local {
                        crate::ui::search::auto_server_search(*s, token);
                    }
                } else if *s.palette_token.peek() == token {
                    s.palette_hits.set(items);
                }
            }
            Delta::FreeBusy { token, result } => s.free_busy.set((token, result)),
            Delta::DrivePicked { draft_id, account_id, files } => {
                if s.drive_waiting.peek().as_deref() == Some(draft_id.as_str()) {
                    s.drive_waiting.set(None);
                }
                if !files.is_empty() {
                    s.drive_picked.set(Some(DrivePicked { token: next_token(), draft_id, account_id, files }));
                }
            }
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

/// The account new messages come from: the chosen default (`pref`, an
/// account id) while it's still signed in, else the first account in
/// sidebar order (`account_order` already applied), else none.
pub fn default_from_account<'a>(pref: &str, sidebar: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut first = None;
    for id in sidebar {
        if !pref.is_empty() && id == pref {
            return Some(id.to_string());
        }
        first = first.or(Some(id));
    }
    first.map(str::to_string)
}

/// The chosen default (`pref`) was in the previous account list and isn't in
/// the new one: it was signed out, so the pref should be cleared. Not on
/// first load (`before` empty), so a slow start never loses the choice.
pub fn forget_default<'a, 'b>(
    pref: &str,
    mut before: impl Iterator<Item = &'a str>,
    mut after: impl Iterator<Item = &'b str>,
) -> bool {
    !pref.is_empty() && before.any(|id| id == pref) && !after.any(|id| id == pref)
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

/// A mailbox as one string for prefs: `unified:<role>` or
/// `folder:<account_id>|<folder_id>`.
pub fn mailbox_key(m: &Mailbox) -> String {
    match m {
        Mailbox::Unified(role) => {
            let role = serde_json::to_value(role).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            format!("unified:{role}")
        }
        Mailbox::Folder { account_id, folder_id } => format!("folder:{account_id}|{folder_id}"),
    }
}

pub fn parse_mailbox(key: &str) -> Option<Mailbox> {
    if let Some(role) = key.strip_prefix("unified:") {
        return serde_json::from_value(serde_json::Value::String(role.to_string())).ok().map(Mailbox::Unified);
    }
    let (account_id, folder_id) = key.strip_prefix("folder:")?.split_once('|')?;
    Some(Mailbox::Folder { account_id: account_id.to_string(), folder_id: folder_id.to_string() })
}

/// The restored mailbox still exists: its account and folder are there.
pub fn mailbox_exists(m: &Mailbox, accounts: &[AccountNav]) -> bool {
    match m {
        Mailbox::Unified(_) => true,
        Mailbox::Folder { account_id, folder_id } => {
            accounts.iter().any(|a| a.account.id == *account_id && a.folders.iter().any(|f| f.folder.id == *folder_id))
        }
    }
}

#[cfg(test)]
mod mailbox_key_tests {
    use super::*;

    #[test]
    fn mailbox_keys_round_trip() {
        for m in [
            Mailbox::Unified(FolderRole::Inbox),
            Mailbox::Unified(FolderRole::Sent),
            Mailbox::Folder { account_id: "google:a@x.com".into(), folder_id: "Label_7|odd".into() },
        ] {
            assert_eq!(parse_mailbox(&mailbox_key(&m)), Some(m));
        }
        assert_eq!(parse_mailbox(""), None);
        assert_eq!(parse_mailbox("unified:nope"), None);
    }
}

#[cfg(test)]
mod default_from_tests {
    use super::*;

    #[test]
    fn chosen_default_wins_while_signed_in() {
        assert_eq!(default_from_account("b", ["a", "b", "c"]).as_deref(), Some("b"));
    }

    #[test]
    fn unset_or_gone_falls_back_to_the_top_account_in_sidebar_order() {
        let order = vec!["c".to_string(), "a".to_string()];
        let sidebar = order_by(vec!["a", "b", "c"], &order, |s| s);
        assert_eq!(default_from_account("", sidebar.iter().copied()).as_deref(), Some("c"));
        assert_eq!(default_from_account("gone", sidebar.iter().copied()).as_deref(), Some("c"));
        assert_eq!(default_from_account("", std::iter::empty()), None);
        assert_eq!(default_from_account("a", std::iter::empty()), None);
    }

    #[test]
    fn forgets_the_default_only_when_it_was_signed_out() {
        let ids = |v: &'static [&'static str]| v.iter().copied();
        assert!(forget_default("b", ids(&["a", "b"]), ids(&["a"])));
        assert!(!forget_default("b", ids(&["a", "b"]), ids(&["b", "a"])), "reordered, still there");
        assert!(!forget_default("b", ids(&[]), ids(&["a"])), "first load: keep the choice");
        assert!(!forget_default("", ids(&["a"]), ids(&[])));
    }
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
