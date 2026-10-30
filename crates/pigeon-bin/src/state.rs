//! UI state: one bundle of signals shared through context, fed by the
//! engine's deltas.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use pigeon_core::compose::{Draft, Sender};
use pigeon_core::model::{Calendar, EventRow, FolderRole, FreeBusy, MessageRow, ProviderKind, StoredBody};
use pigeon_engine::service::{
    AccountNav, Delta, Elsewhere, InviteView, ListItem, Mailbox, OpenHow, ReaderView, SearchScope, ServerSearch,
    SyncState, Toast,
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

/// How the message list draws its rows (Settings › Appearance).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListStyle {
    /// Rows separated by space alone (the original look).
    Classic,
    /// Each message in its own rounded card.
    Cards,
    /// Each day one panel, two-line rows divided by hairlines.
    Bands,
}

impl ListStyle {
    pub const ALL: [ListStyle; 3] = [ListStyle::Cards, ListStyle::Bands, ListStyle::Classic];

    pub fn from_pref(s: &str) -> Self {
        match s {
            "classic" => ListStyle::Classic,
            "bands" => ListStyle::Bands,
            _ => ListStyle::Cards,
        }
    }

    pub fn as_pref(self) -> &'static str {
        match self {
            ListStyle::Classic => "classic",
            ListStyle::Cards => "cards",
            ListStyle::Bands => "bands",
        }
    }

    /// One line under the preview in Settings.
    pub fn blurb(self) -> &'static str {
        match self {
            ListStyle::Classic => "Rows separated by space",
            ListStyle::Cards => "Each message in its own box",
            ListStyle::Bands => "One panel per day, denser rows",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ListStyle::Classic => "Classic",
            ListStyle::Cards => "Cards",
            ListStyle::Bands => "Day bands",
        }
    }

    /// Slot heights; app.css (`.list[data-style]`) sizes rows to match.
    pub fn metrics(self) -> crate::ui::vlist::Metrics {
        use crate::ui::vlist::{CLASSIC, Metrics};
        match self {
            ListStyle::Classic => CLASSIC,
            ListStyle::Cards => Metrics { row_h: 78.0, group_h: 32.0 },
            ListStyle::Bands => Metrics { row_h: 60.0, group_h: 34.0 },
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
    /// Where it looks (the scope chip); operators narrow further.
    pub scope: SearchScope,
    /// Local matches the scope leaves out.
    pub elsewhere: Elsewhere,
    /// Last caret the search box reported: (its text then, byte offset).
    /// Only meaningful while that text is still the query.
    pub caret: Option<(String, usize)>,
}

impl SearchState {
    /// The caret as a byte offset into the query (its end when unknown).
    pub fn caret(&self) -> usize {
        match &self.caret {
            Some((text, at)) if *text == self.query => *at,
            _ => self.query.len(),
        }
    }
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
    /// Rows being dragged toward a sidebar folder (ui/mail_drag.rs).
    pub mail_drag: Signal<Option<Vec<ListItem>>>,
    /// A list row's right-click menu while open.
    pub row_menu: Signal<Option<crate::ui::row_menu::RowMenu>>,
    /// A reply started from a message window, as (account id, message id):
    /// its draft opens in a compose window, not inline in the main reader.
    pub reply_to_window: Signal<Option<(String, String)>>,
    /// The Coo window has keyboard focus.
    pub window_focused: Signal<bool>,
    /// Outgoing messages (sending / failed / scheduled), newest first.
    pub outgoing: Signal<Vec<Outgoing>>,
    /// The "Add account" sheet while open (ui/add_account.rs), with what
    /// it opened on.
    pub add_account: Signal<Option<crate::ui::add_account::Opening>>,
    /// Counts finished sign-ins (`Delta::SignedIn`); the sheet closes on a new one.
    pub signed_in: Signal<u64>,
    /// The latest `Delta::ProviderDetected`, as (token, result).
    pub detected: Signal<Option<(u64, pigeon_core::provider_detect::Detection)>>,
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
    /// Find in the open conversation (⌘F): the text, while the bar is open.
    pub find: Signal<Option<String>>,
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
    /// ⌘A: the multi-selection is "everything in this view" and grows as
    /// the rest of the folder loads.
    pub select_all: Signal<bool>,
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
    pub list_style: Signal<ListStyle>,
    pub list_avatars: Signal<bool>,
    /// Sidebar: accounts with the Folders group open; pinned folders
    /// (`account|folder_id`). Mirrors prefs.
    pub folders_open: Signal<Vec<String>>,
    pub pinned_folders: Signal<Vec<String>>,
    /// Calendars unchecked in the calendar sidebar (`account|calendar_id`). Mirrors prefs.
    pub hidden_calendars: Signal<Vec<String>>,
    /// Color theme family id (see `themes`).
    pub palette: Signal<String>,
    pub settings_open: Signal<bool>,
    /// What's on screen: follows `settings_open` once the old view has
    /// faded out (ui/mod.rs), so the swap can animate both ways.
    pub settings_shown: Signal<bool>,
    /// Settings subpage shown (a `settings::PAGES` title).
    pub settings_page: Signal<String>,
    /// The connected Zoom account (Zoom calls on events), if any.
    pub zoom: Signal<Option<pigeon_core::model::ConnectedService>>,
    /// Google accounts whose sign-in allows contact photos.
    pub google_contacts: Signal<Vec<String>>,
    /// Microsoft accounts whose sign-in allows sender photos.
    pub microsoft_photos: Signal<Vec<String>>,
    /// "Connect an AI agent" popup.
    pub agent_setup_open: Signal<bool>,
    /// Help › Report a Bug… (ui/bug_report.rs).
    pub bug_report_open: Signal<bool>,
    /// A downloaded update waiting for a restart (its version).
    pub update_ready: Signal<Option<String>>,
    /// The updater's latest status, and whether the Check for Updates dialog is open.
    pub update_status: Signal<crate::updater::Status>,
    pub update_dialog: Signal<bool>,
    pub os_dark: Signal<bool>,
    pub signing_in: Signal<bool>,
    /// Accounts open in the sidebar (prefs `accounts_open`).
    pub accounts_open: Signal<Vec<String>>,
    /// Account nicknames (Settings › Accounts), by account id.
    pub nicknames: Signal<std::collections::BTreeMap<String, String>>,
    /// The chosen default account for new messages (prefs
    /// `default_from_account`; empty: the top account). Read it through
    /// [`AppState::default_from`].
    pub default_account: Signal<String>,
    /// The chosen calendar for new events (prefs `default_calendar`,
    /// `account_id|calendar_id`; empty: automatic). Resolved by
    /// `event_editor::new_event_calendar`.
    pub default_calendar: Signal<String>,
    /// An account header's right-click menu while open.
    pub account_menu: Signal<Option<crate::ui::account_menu::AccountMenu>>,
    /// The account whose sidebar shows the "New label" field (Gmail).
    pub new_label: Signal<Option<String>>,
    /// Gmail labels being deleted ("account|label"): shown as "Deleting…".
    pub deleting_labels: Signal<Vec<String>>,
    /// The user asked for a sync (Sync now, ⇧⌘N): show "Syncing…" until it's
    /// done. The background ones only move the "Synced" time.
    pub sync_asked: Signal<bool>,
    /// Actions that can still be undone, oldest first: (account id, handle).
    /// ⌘Z takes the last, whether or not its notice is still up.
    pub undo_stack: Signal<Vec<(String, i64)>>,
    /// When each draft last reached the server's Drafts (auto-save).
    pub draft_saved: Signal<std::collections::HashMap<String, i64>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            move_picker: Signal::new(None),
            mail_drag: Signal::new(None),
            row_menu: Signal::new(None),
            reply_to_window: Signal::new(None),
            window_focused: Signal::new(true),
            outgoing: Signal::new(Vec::new()),
            add_account: Signal::new(None),
            signed_in: Signal::new(0),
            detected: Signal::new(None),
            login_error: Signal::new(None),
            first_run: Signal::new(false),
            reopening: Signal::new(None),
            editor: Signal::new(None),
            free_busy: Signal::new((0, Vec::new())),
            search: Signal::new(None),
            find: Signal::new(None),
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
            select_all: Signal::new(false),
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
            list_style: Signal::new(ListStyle::from_pref(&crate::prefs::get().list_style)),
            list_avatars: Signal::new(crate::prefs::get().list_avatars),
            folders_open: Signal::new(crate::prefs::get().folders_open),
            pinned_folders: Signal::new(crate::prefs::get().pinned_folders),
            hidden_calendars: Signal::new(crate::prefs::get().hidden_calendars),
            palette: Signal::new({
                let p = crate::prefs::get().palette;
                crate::themes::current(&p).to_string()
            }),
            settings_open: Signal::new(false),
            settings_shown: Signal::new(false),
            settings_page: Signal::new("General".into()),
            zoom: Signal::new(None),
            google_contacts: Signal::new(Vec::new()),
            microsoft_photos: Signal::new(Vec::new()),
            agent_setup_open: Signal::new(false),
            bug_report_open: Signal::new(false),
            update_ready: Signal::new(None),
            update_status: Signal::new(crate::updater::Status::Idle),
            update_dialog: Signal::new(false),
            os_dark: Signal::new(true),
            signing_in: Signal::new(false),
            accounts_open: Signal::new(crate::prefs::get().accounts_open),
            nicknames: Signal::new(crate::prefs::get().account_nicknames),
            default_account: Signal::new(crate::prefs::get().default_from_account),
            default_calendar: Signal::new(crate::prefs::get().default_calendar),
            account_menu: Signal::new(None),
            new_label: Signal::new(None),
            deleting_labels: Signal::new(Vec::new()),
            sync_asked: Signal::new(false),
            undo_stack: Signal::new(Vec::new()),
            draft_saved: Signal::new(Default::default()),
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

    /// Each calendar's own color, `account|calendar_id` → 1..=8: no two
    /// share one until all eight are used, in sidebar order (accounts, then
    /// their calendars).
    pub fn calendar_hues(&self) -> std::collections::HashMap<String, u8> {
        let accounts = self.accounts.read();
        let cal = self.calendar.read();
        let keys: Vec<String> = accounts
            .iter()
            .flat_map(|a| {
                cal.calendars.iter().filter(move |(id, _)| *id == a.account.id).map(|(id, c)| format!("{id}|{}", c.id))
            })
            .collect();
        keys.iter().cloned().zip(pigeon_engine::service::distinct_hues(&keys)).collect()
    }

    /// The color of `calendar_id`'s events (its account's while calendars
    /// haven't loaded).
    pub fn calendar_hue(
        &self,
        hues: &std::collections::HashMap<String, u8>,
        account_id: &str,
        calendar_id: &str,
    ) -> u8 {
        hues.get(&format!("{account_id}|{calendar_id}")).copied().unwrap_or_else(|| self.hue(account_id))
    }

    /// Next up without the calendars unchecked in the calendar sidebar, so
    /// they neither show in Next up nor remind.
    pub fn next_up_shown(&self) -> Vec<EventRow> {
        without_hidden(&self.next_up.read(), &self.hidden_calendars.read())
    }

    /// As `next_up_shown`, without subscribing (for background loops).
    pub fn next_up_shown_peek(&self) -> Vec<EventRow> {
        without_hidden(&self.next_up.peek(), &self.hidden_calendars.peek())
    }

    pub fn dark(&self) -> bool {
        match *self.theme.read() {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => *self.os_dark.read(),
        }
    }

    /// How an account is named in the UI: its nickname, else its address,
    /// plus the provider when another account has the same address (an
    /// iCloud Apple ID that's a Gmail address next to that Gmail account).
    /// The account's own address stands for its main address when one is
    /// picked.
    pub fn account_label(&self, account_id: &str, email: &str) -> String {
        let nickname = self.nicknames.read().get(account_id).cloned();
        let accounts = self.accounts.read();
        let nav = accounts.iter().find(|a| a.account.id == account_id);
        let email = match nav {
            Some(a) if a.account.email.eq_ignore_ascii_case(email) => main_address(a),
            _ => email,
        };
        let shared = accounts.iter().any(|a| a.account.id != account_id && main_address(a).eq_ignore_ascii_case(email));
        account_label(nickname.as_deref(), email, nav.map(|a| a.account.provider).filter(|_| shared))
    }

    /// A short second label telling accounts apart (send-from pickers): the
    /// nickname, else the provider. None with a single account.
    pub fn account_tag(&self, account_id: &str) -> Option<String> {
        let accounts = self.accounts.read();
        if accounts.len() < 2 {
            return None;
        }
        let a = accounts.iter().find(|a| a.account.id == account_id)?;
        let nickname = self.nicknames.read().get(account_id).cloned();
        Some(account_tag(nickname.as_deref(), a.account.provider, &a.account.email))
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

    /// Make a calendar (account id, calendar id) the default for new
    /// events; `None`: automatic.
    pub fn set_default_calendar(&self, calendar: Option<(&str, &str)>) {
        let v = calendar.map(|(a, c)| format!("{a}|{c}")).unwrap_or_default();
        let mut d = self.default_calendar;
        d.set(v.clone());
        crate::prefs::update(move |p| p.default_calendar = v);
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
        self.push_toast(Toast { id, message: message.into(), error, ..Default::default() });
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

/// How many undoable actions ⌘Z can walk back through.
const UNDO_KEEP: usize = 50;

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
                let banner = prefs.mail_alert != "background" || !crate::notify::app_is_active();
                let notes = crate::notify::mail_notes(&rows, &prefs);
                // One soft sound per burst (all accounts' batches), even with
                // the window in front.
                let mut sound =
                    !notes.is_empty() && prefs.mail_sound && crate::sound::take_alert(crate::sound::Kind::Mail);
                for mut note in notes {
                    note.sound = std::mem::take(&mut sound).then_some(crate::sound::Kind::Mail);
                    note.banner = banner;
                    if banner || note.sound.is_some() {
                        crate::notify::post(&note);
                    }
                }
            }
            Delta::LoginFailed { provider, error } => {
                s.signing_in.set(false);
                s.login_error.set(Some((provider, error)));
            }
            Delta::Zoom(z) => s.zoom.set(z),
            Delta::GoogleContacts(ids) => {
                if *s.google_contacts.peek() != ids {
                    s.google_contacts.set(ids);
                }
            }
            Delta::MicrosoftPhotos(ids) => {
                if *s.microsoft_photos.peek() != ids {
                    s.microsoft_photos.set(ids);
                }
            }
            // Avatars without a photo ask again (found ones are cached on disk).
            Delta::ContactPhotosChanged => s.photos.write().retain(|_, uri| uri.is_some()),
            Delta::SignedIn { .. } => {
                let n = *s.signed_in.peek();
                s.signed_in.set(n + 1);
            }
            Delta::ProviderDetected { token, result } => s.detected.set(Some((token, result))),
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
                    if *s.select_all.peek() {
                        if s.multi.peek().is_empty() {
                            s.select_all.set(false);
                        } else {
                            s.multi.set(items.iter().map(AppState::key).collect());
                        }
                    }
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
                if let Some(u) = t.undo.clone() {
                    let mut stack = s.undo_stack.write();
                    stack.push(u);
                    let extra = stack.len().saturating_sub(UNDO_KEEP);
                    stack.drain(..extra);
                }
                s.push_toast(t)
            }
            Delta::Sync(st) => {
                // A finished sync has picked up what we sent; drop those rows.
                if !st.syncing {
                    let done = st.last_ok.unwrap_or(0);
                    s.outgoing.write().retain(|o| !matches!(o.state, OutState::Sent(at) if at <= done));
                    if *s.sync_asked.peek() {
                        s.sync_asked.set(false);
                    }
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
                    // Inline in the message window it was started from, or
                    // in a compose window (Settings › Composing › Replies).
                    if crate::ui::compose::reply_window_mode() || !crate::ui::reader_window::take_reply(&d) {
                        crate::ui::compose_window::open(*s, d);
                    }
                    return;
                }
                // A new draft replaces a set-aside docked one (already saved).
                s.docked_hidden.set(false);
                s.compose_notice.set(None);
                s.draft.set(Some(d))
            }
            Delta::DraftSaved { draft_id } => {
                s.draft_saved.write().insert(draft_id, pigeon_core::now_ms());
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
                crate::ui::milestone::record(|m, now| m.sent(now));
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
            Delta::SearchResults { token, items, server, elsewhere } => {
                let current = s.search.peek().as_ref().map(|st| st.token);
                if current == Some(token) {
                    // Matches in other folders are offered instead.
                    let nothing_local = items.is_empty() && elsewhere.total == 0 && server == ServerSearch::Idle;
                    if let Some(st) = s.search.write().as_mut() {
                        st.items = items;
                        st.server = server;
                        st.elsewhere = elsewhere;
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
            Delta::AttachmentReady(path, how) => match how {
                OpenHow::Open => {
                    // No path in the log: it carries the file's name.
                    if let Err(e) = open::that_detached(&path) {
                        tracing::warn!(error = %e, "could not open attachment");
                    }
                }
                OpenHow::Preview => crate::quicklook::preview(&path),
                OpenHow::Reveal => crate::quicklook::reveal(&path),
            },
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

/// How a provider is named next to an account: "Gmail" for gmail.com
/// addresses, "Google Workspace" for other Google accounts.
pub fn provider_name(provider: ProviderKind, email: &str) -> &'static str {
    match provider {
        ProviderKind::Microsoft => "Microsoft 365",
        ProviderKind::Google if email.to_lowercase().ends_with("@gmail.com") => "Gmail",
        ProviderKind::Google => "Google Workspace",
        ProviderKind::ICloud => "iCloud",
    }
}

/// The address an account goes by: the alias picked as its main address,
/// else its own.
pub fn main_address(nav: &AccountNav) -> &str {
    nav.main_address.as_deref().unwrap_or(&nav.account.email)
}

/// An account's name: its nickname, else its address; `ambiguous` (the
/// provider, when another account shares the address) is appended to a
/// bare address.
pub fn account_label(nickname: Option<&str>, email: &str, ambiguous: Option<ProviderKind>) -> String {
    match (nickname.map(str::trim).filter(|n| !n.is_empty()), ambiguous) {
        (Some(n), _) => n.to_string(),
        (None, Some(p)) => format!("{email} · {}", provider_name(p, email)),
        (None, None) => email.to_string(),
    }
}

/// Second label for an account: its nickname, else its provider.
pub fn account_tag(nickname: Option<&str>, provider: ProviderKind, email: &str) -> String {
    nickname
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| provider_name(provider, email).to_string(), str::to_string)
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

/// `events` minus those of the `account|calendar_id` keys in `hidden`.
pub fn without_hidden(events: &[EventRow], hidden: &[String]) -> Vec<EventRow> {
    events
        .iter()
        .filter(|e| !hidden.iter().any(|k| *k == format!("{}|{}", e.account_id, e.event.calendar_id)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod hidden_calendar_tests {
    use super::*;
    use pigeon_core::model::Event;

    #[test]
    fn unchecked_calendars_drop_out() {
        let ev = |account: &str, cal: &str| EventRow {
            account_id: account.into(),
            event: Event { calendar_id: cal.into(), ..Default::default() },
        };
        let events = vec![ev("a", "work"), ev("a", "home"), ev("b", "work")];
        let kept = without_hidden(&events, &["a|work".to_string()]);
        let keys: Vec<_> = kept.iter().map(|e| format!("{}|{}", e.account_id, e.event.calendar_id)).collect();
        assert_eq!(keys, ["a|home", "b|work"]);
        assert_eq!(without_hidden(&events, &[]).len(), 3);
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

    #[test]
    fn accounts_sharing_an_address_are_told_apart() {
        let g = "me@gmail.com";
        assert_eq!(account_label(None, g, None), g);
        assert_eq!(account_label(None, g, Some(ProviderKind::ICloud)), "me@gmail.com · iCloud");
        assert_eq!(account_label(None, g, Some(ProviderKind::Google)), "me@gmail.com · Gmail");
        assert_eq!(account_label(Some(" Family "), g, Some(ProviderKind::ICloud)), "Family", "a nickname is enough");
        assert_eq!(account_label(Some("  "), g, None), g);
        assert_eq!(account_tag(None, ProviderKind::ICloud, g), "iCloud");
        assert_eq!(account_tag(None, ProviderKind::Google, "me@work.com"), "Google Workspace");
        assert_eq!(account_tag(Some("Work"), ProviderKind::Microsoft, "me@work.com"), "Work");
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
