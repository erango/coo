//! The daily hello on screen (greeting.rs picks the scene and words). With
//! nothing open in the reader it takes the place of "Pick a conversation"
//! until a message opens; with a message open, a small card sits at the top
//! of the reader for a few seconds. No sound, no notification.

use chrono::NaiveDate;
use dioxus::prelude::*;

use super::Icon;
use crate::greeting::{self, Facts, Greeting, Scene};
use crate::state::{AppState, Mode};

#[derive(Clone, PartialEq)]
struct Shown {
    key: u64,
    greeting: Greeting,
    /// A message was open: the card, not the empty reader.
    card: bool,
}

static SHOWN: GlobalSignal<Option<Shown>> = Signal::global(|| None);
static LEAVING: GlobalSignal<bool> = Signal::global(|| false);

/// How long the card stays.
const CARD_MS: u64 = 6_000;
/// Its exit (app.css `greet-out`).
const LEAVE_MS: u64 = 240;

fn day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Something else has the user's attention (writing, Settings, the
/// palette, the tour, the calendar): wait for the next time Coo comes forward.
fn busy(s: &AppState) -> bool {
    s.draft.peek().is_some()
        || super::compose_window::any_open()
        || *s.settings_open.peek()
        || *s.palette_open.peek()
        || *s.tour_open.peek()
        || *s.mode.peek() == Mode::Calendar
}

/// Unread in the Inboxes, and the next meeting today.
fn facts(s: &AppState) -> Facts {
    let unread = s
        .accounts
        .peek()
        .iter()
        .flat_map(|a| a.folders.iter())
        .filter(|f| f.folder.role == Some(pigeon_core::model::FolderRole::Inbox))
        .map(|f| f.unread)
        .sum();
    let now = pigeon_core::now_ms();
    let next_meeting = s
        .next_up_shown_peek()
        .iter()
        .filter(|e| !e.event.all_day && !e.event.cancelled)
        .filter(|e| e.event.start_at > now && crate::format::same_day(e.event.start_at, now))
        .min_by_key(|e| e.event.start_at)
        .map(|e| crate::format::hhmm(e.event.start_at));
    Facts { unread, next_meeting }
}

/// Watch for Coo coming to the front (launch included) and greet the
/// first time each day. Only the moment it comes forward counts, so the
/// day turning over while you're using Coo doesn't interrupt.
pub async fn watch(s: AppState) {
    let mut was_active = false;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let active = crate::notify::app_is_active() && !crate::menubar::main_hidden();
        if active && !was_active {
            activated(s);
        }
        was_active = active;
    }
}

/// Coo came to the front: greet, once a day.
fn activated(s: AppState) {
    let p = crate::prefs::get();
    let now = chrono::Local::now().naive_local();
    if !p.greeting || !greeting::due(day(&p.greeted_day), now) || busy(&s) {
        return;
    }
    let today = greeting::day_of(now);
    let stamp = today.format("%Y-%m-%d").to_string();
    // A milestone card today goes first; the greeting skips the day.
    let milestone_today = super::milestone::showing()
        || crate::format::local(p.milestones.last_shown)
            .is_some_and(|t| p.milestones.last_shown > 0 && greeting::day_of(t.naive_local()) == today);
    let last_active = day(&p.active_day).filter(|d| *d < today);
    crate::prefs::update(move |p| {
        p.greeted_day = stamp.clone();
        p.active_day = stamp;
    });
    if milestone_today {
        return;
    }
    let c = greeting::Context {
        now,
        weekend: crate::week::system().weekend,
        last_active,
        since: day(&p.milestones.since_day),
        south: greeting::local_zone().is_some_and(|z| greeting::southern_zone(&z)),
    };
    show(&s, greeting::words(greeting::pick(&c), &c, &facts(&s)));
}

fn show(s: &AppState, g: Greeting) {
    let key = SHOWN.peek().as_ref().map(|x| x.key).unwrap_or(0) + 1;
    let card = s.reader.peek().is_some();
    *LEAVING.write() = false;
    *SHOWN.write() = Some(Shown { key, greeting: g, card });
    if card {
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(CARD_MS)).await;
            leave(key).await;
        });
    }
}

fn shown_key() -> Option<u64> {
    SHOWN.peek().as_ref().map(|x| x.key)
}

async fn leave(key: u64) {
    if shown_key() != Some(key) {
        return;
    }
    *LEAVING.write() = true;
    tokio::time::sleep(std::time::Duration::from_millis(LEAVE_MS)).await;
    if shown_key() == Some(key) {
        *SHOWN.write() = None;
        *LEAVING.write() = false;
    }
}

fn dismiss() {
    if let Some(key) = shown_key() {
        spawn(leave(key));
    }
}

/// A message opened (or another one): the greeting is done for the day.
pub fn reader_changed() {
    if SHOWN.peek().is_some() {
        *SHOWN.write() = None;
        *LEAVING.write() = false;
    }
}

/// A key while the card shows: any shortcut sends it away; Esc does only
/// that (returns true: the key is used up).
pub fn on_key(key: &str) -> bool {
    if !SHOWN.peek().as_ref().is_some_and(|x| x.card) {
        return false;
    }
    dismiss();
    key == "Escape"
}

/// The empty reader shows the greeting instead of its usual note.
pub fn in_empty_reader() -> bool {
    SHOWN.read().as_ref().is_some_and(|x| !x.card)
}

/// Debug builds: `greeting:<scene>[|<account_id>|<thread_id>]` shows that
/// scene a moment after launch, as a card over that thread when one is
/// given (screenshots). Nothing is remembered.
pub fn debug(s: AppState, arg: &str) {
    let mut parts = arg.split('|');
    let Some(scene) = parts.next().and_then(Scene::from_id) else { return };
    // With an update waiting too (PIGEON_DEBUG_UPDATE=<version>).
    if let Ok(v) = std::env::var("PIGEON_DEBUG_UPDATE") {
        crate::updater::fake_ready(&v);
    }
    if let (Some(account_id), Some(thread_id)) = (parts.next(), parts.next()) {
        let mut sel = s.selected;
        sel.set(Some(format!("{account_id}|{thread_id}")));
        crate::send_cmd(pigeon_engine::service::Cmd::OpenThread {
            account_id: account_id.into(),
            thread_id: thread_id.into(),
        });
    }
    spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        let p = crate::prefs::get();
        let c = greeting::Context {
            now: chrono::Local::now().naive_local(),
            weekend: crate::week::system().weekend,
            last_active: Some(chrono::Local::now().date_naive() - chrono::Duration::days(9)),
            since: day(&p.milestones.since_day),
            south: false,
        };
        show(&s, greeting::words(scene, &c, &facts(&s)));
    });
}

#[component]
fn Art(scene: Scene) -> Element {
    let (label, body) = super::greeting_art::art(scene);
    rsx! {
        svg { class: "greet-art", view_box: "0 0 200 125", role: "img", "aria-label": label,
            dangerous_inner_html: body,
        }
    }
}

/// An update waiting for a restart, offered with the day's hello.
#[component]
fn UpdateNote() -> Element {
    let s = use_context::<AppState>();
    let Some(version) = s.update_ready.read().clone() else { return rsx! {} };
    rsx! {
        div { class: "greet-update",
            Icon { name: "sync", class: "icon--sm" }
            span { class: "greet-update__text", "Coo {version} is ready" }
            button { class: "btn btn--secondary btn--sm", r#type: "button",
                onclick: move |e| { e.stop_propagation(); super::restart_to_update(); },
                "Restart to update"
            }
        }
    }
}

/// The greeting in the empty reader.
#[component]
pub fn GreetingHello() -> Element {
    let Some(x) = SHOWN.read().clone().filter(|x| !x.card) else { return rsx! {} };
    let g = x.greeting;
    rsx! {
        div { class: "greet", key: "{x.key}", "data-scene": g.scene.id(),
            div { class: "greet__scene", Art { scene: g.scene } }
            div { class: "greet__title", dir: "auto", "{g.title}" }
            div { class: "greet__line", dir: "auto", "{g.line}" }
            UpdateNote {}
        }
    }
}

/// The greeting over an open message.
#[component]
pub fn GreetingCard() -> Element {
    let Some(x) = SHOWN.read().clone().filter(|x| x.card) else { return rsx! {} };
    let g = x.greeting;
    rsx! {
        div { class: "greet-card", role: "status", key: "{x.key}", "data-scene": g.scene.id(),
            "data-leaving": LEAVING.read().then_some("true"),
            onclick: move |_| dismiss(),
            div { class: "greet-card__scene", Art { scene: g.scene } }
            div { class: "greet-card__text",
                div { class: "greet-card__title", dir: "auto", "{g.title}" }
                div { class: "greet-card__line", dir: "auto", "{g.line}" }
                UpdateNote {}
            }
            button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Dismiss", title: "Dismiss",
                onclick: move |e| { e.stop_propagation(); dismiss(); },
                Icon { name: "x", class: "icon--xs" }
            }
        }
    }
}
