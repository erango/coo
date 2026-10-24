mod agent_setup;
mod avatar;
pub mod calendar;
pub mod compose;
pub mod compose_window;
pub mod event_editor;
mod icloud;
mod invite;
mod list;
mod list_motion;
pub mod mailframe;
mod move_picker;
mod onboarding;
mod palette;
mod reader;
mod recipients;
pub mod search;
pub mod sender_card;
mod settings;
mod settings_sections;
mod sidebar;
mod toasts;
pub mod vlist;
pub mod zoom;

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ListItem};

use crate::bridge::{self, Msg};
use crate::send_cmd;
use crate::state::{self, AppState, Mode, Theme};

pub(crate) const ICONS: &str = include_str!("../../../../design/icons.svg");
/// App-only icons the design sprite lacks. `i-gear`: the design's
/// `i-settings` is a circle with rays and reads as a sun next to the theme
/// toggle. Gear path from Lucide (ISC license).
pub(crate) const APP_ICONS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><symbol id="i-gear" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></symbol></defs></svg>"#;
const DAY_MS: i64 = 86_400_000;

#[component]
pub fn Icon(name: &'static str, #[props(default)] class: &'static str) -> Element {
    rsx! {
        svg { class: "icon {class}", "aria-hidden": "true",
            r#use { href: "#i-{name}" }
        }
    }
}

#[component]
pub fn App() -> Element {
    let s = use_context_provider(AppState::new);
    use_context_provider(|| s.main_slot());
    // Closing the main window either hides it into the menu bar (Settings ›
    // General) or quits; quitting closes compose windows first, since they
    // read the main window's state. Launching Coo again while it's hidden
    // (Finder, Dock, Spotlight) brings the window back.
    dioxus::desktop::use_wry_event_handler(move |event, _| {
        use dioxus::desktop::tao::event::{Event, WindowEvent};
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if crate::menubar::before_close() {
                    // Dioxus hides the window right after this handler.
                    spawn(async {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        crate::menubar::did_hide(compose_window::any_open());
                    });
                } else {
                    compose_window::close_all();
                }
            }
            Event::Reopen { .. } => {
                crate::menubar::show_main();
                crate::updater::check_background(false);
            }
            // Coming back to the foreground: a silent update check, at most hourly.
            Event::WindowEvent { event: WindowEvent::Focused(true), .. } => crate::updater::check_background(false),
            _ => {}
        }
    });

    use_hook(move || {
        // Sparkle (release builds only): starts its background check schedule.
        crate::updater::start();
        crate::menubar::install(dioxus::desktop::window());
        if crate::prefs::get().relaunch_hidden {
            crate::prefs::update(|p| p.relaunch_hidden = false);
            crate::menubar::start_hidden();
        }
        spawn(update_loop(s));
        spawn(menubar_loop(s));
        spawn(state::pump(s));
        spawn(bridge_loop(s));
        spawn(reminder_loop(s));
        spawn(click_loop(s));
        // Debug builds: open a surface on launch for screenshots/testing,
        // e.g. PIGEON_DEBUG_OPEN=palette:inv, search:invoice, event-new:a@x.com,b@y.com
        if cfg!(debug_assertions)
            && let Ok(v) = std::env::var("PIGEON_DEBUG_OPEN")
        {
            let (what, arg) = v.split_once(':').unwrap_or((v.as_str(), ""));
            match what {
                // compose-docked[:<to list>] — a new draft docked in the reading pane
                "compose-docked" => {
                    crate::prefs::update(|p| p.compose_mode = "docked".into());
                    send_cmd(Cmd::NewDraft);
                }
                "palette" => {
                    let mut p = s.palette_open;
                    p.set(true);
                    let arg = arg.to_string();
                    document::eval(&format!(
                        "setTimeout(()=>{{const i=document.getElementById('palette-input'); if(i){{i.value={arg:?}; i.dispatchEvent(new Event('input',{{bubbles:true}}));}}}},800)"
                    ));
                }
                // compose:<to list>[|<partial>] — new draft, chips + autocomplete
                "compose" => send_cmd(Cmd::NewDraft),
                "search" => search::open(s, arg),
                // find:<query>|<account_id>|<thread_id> — search, then open a thread from it
                "find" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [q, account_id, thread_id] = p[..] {
                        search::open(s, q);
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                    }
                }
                // open:<account_id>|<thread_id>
                "open" => {
                    if let Some((account_id, thread_id)) = arg.split_once('|') {
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                    }
                }
                // card:<account_id>|<thread_id>|<message_id>|<name>|<email> — open a
                // thread and pin its sender card open (screenshots)
                "card" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [account_id, thread_id, message_id, name, email] = p[..] {
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                        send_cmd(Cmd::SenderSeen { account_id: account_id.into(), email: email.into() });
                        let mut c = s.sender_card;
                        c.set(Some(sender_card::CardState {
                            account_id: account_id.into(),
                            message_id: Some(message_id.into()),
                            addr: pigeon_core::model::Address {
                                name: (!name.is_empty()).then(|| name.to_string()),
                                email: email.into(),
                            },
                            sender: true,
                            x: 770.0,
                            y: 200.0,
                        }));
                    }
                }
                // reply:<account_id>|<thread_id>|<message_id> — open a thread and start a reply
                "reply" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [account_id, thread_id, message_id] = p[..] {
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                        let (account_id, message_id) = (account_id.to_string(), message_id.to_string());
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                            send_cmd(Cmd::StartReply {
                                account_id,
                                message_id,
                                kind: pigeon_core::compose::ReplyKind::Reply,
                            });
                        });
                    }
                }
                // calendar[:day|week|month]
                "calendar" => {
                    let mut m = s.mode;
                    m.set(Mode::Calendar);
                    let mut v = s.cal_view;
                    match arg {
                        "day" => v.set(crate::state::CalView::Day),
                        "month" => v.set(crate::state::CalView::Month),
                        _ => {}
                    }
                }
                // anim-demo — take the 2nd row out and put it back every 1.6 s,
                // so screenshots can catch the collapse / slide-in mid-way.
                "anim-demo" => {
                    let mut list = s.list;
                    spawn(async move {
                        let mut held: Option<(usize, pigeon_engine::service::ListItem)> = None;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
                            let mut l = list.write();
                            match held.take() {
                                Some((i, item)) => {
                                    let at = i.min(l.len());
                                    l.insert(at, item);
                                }
                                None if l.len() > 2 => held = Some((1, l.remove(1))),
                                None => {}
                            }
                        }
                    });
                }
                // scroll:<row> — scroll the message list to roughly that row.
                "scroll" => {
                    let y = arg.parse::<f64>().unwrap_or(0.0) * vlist::ROW_H;
                    // Retry until the list exists and is tall enough (pages load as it scrolls).
                    document::eval(&format!(
                        "let n=0; const t=setInterval(()=>{{const l=document.querySelector('.pane--list .list[role=listbox]'); \
                         if(l){{l.scrollTop={y}; if(Math.abs(l.scrollTop-{y})<2||++n>40) clearInterval(t);}}}},250)"
                    ));
                }
                // move:<account_id>|<thread_id>[|label] — open the thread, then the picker
                "move" => {
                    let parts: Vec<String> = arg.split('|').map(String::from).collect();
                    if parts.len() >= 2 {
                        let label = parts.get(2).is_some_and(|p| p == "label");
                        let mut sel = s.selected;
                        sel.set(Some(format!("{}|{}", parts[0], parts[1])));
                        send_cmd(Cmd::OpenThread { account_id: parts[0].clone(), thread_id: parts[1].clone() });
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                            move_picker::open(s, label);
                        });
                    }
                }
                // settings[:<section title>] — open, optionally scrolled to a section
                "settings" => {
                    let mut o = s.settings_open;
                    o.set(true);
                    if !arg.is_empty() {
                        let js = format!(
                            "setTimeout(() => {{ const h = [...document.querySelectorAll('.settings__title')].find(e => e.textContent.trim() === {arg:?}); h && h.scrollIntoView(); }}, 400);"
                        );
                        document::eval(&js);
                    }
                }
                // menubar-hint — hide into the menu bar and show the one-time hint
                "menubar-hint" => {
                    spawn(async {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        crate::menubar::hide_main_for_demo();
                    });
                }
                // Ask Sparkle to check now (update tests; release builds only).
                "check-updates" => crate::updater::check_now(),
                // update-ready:<version>: pretend an update downloaded (the pill, menus)
                "update-ready" => crate::updater::fake_ready(if arg.is_empty() { "9.9.9" } else { arg }),
                // install-update: run the immediate-install path once an update is ready
                // install-update[:<secs>]: once an update is ready, wait <secs>, then install
                "install-update" => {
                    let wait = arg.parse::<u64>().unwrap_or(0);
                    spawn(async move {
                        for _ in 0..180 {
                            if matches!(crate::updater::status(), crate::updater::Status::Ready(_)) {
                                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                                restart_to_update();
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                        tracing::warn!("install-update: no update became ready");
                    });
                }
                "about" => crate::about::show(),
                "agent-setup" => {
                    let mut o = s.agent_setup_open;
                    o.set(true);
                }
                // theme:<palette>:<system|light|dark>
                "theme" => {
                    let (palette, mode) = arg.split_once(':').unwrap_or((arg, "system"));
                    let (mut p, mut t) = (s.palette, s.theme);
                    if crate::themes::is_known(palette) {
                        p.set(palette.to_string());
                    }
                    t.set(Theme::from_pref(mode));
                }
                "event-new" => {
                    let guests = arg.to_string();
                    let mut m = s.mode;
                    m.set(Mode::Calendar);
                    // Wait for the calendar list so the picker has a default.
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                        event_editor::open_new_now(s);
                        if !guests.is_empty() {
                            let mut e = s.editor;
                            if let Some(f) = e.write().as_mut() {
                                f.title = "Roadmap review".into();
                                f.guests = guests;
                            }
                        }
                    });
                }
                _ => {}
            }
        }
    });

    // Native menu items (see menu.rs for which keys each side owns). The app
    // menu and the status item's menu share one muda event channel, and with
    // a status item Dioxus delivers both as tray-menu events, so listen to
    // both kinds and dispatch on the item id.
    dioxus::desktop::use_muda_event_handler(move |e| on_menu(s, e.id().0.as_str()));
    dioxus::desktop::use_tray_menu_event_handler(move |e| on_menu(s, e.id().0.as_str()));

    // Apply and remember light/dark/system and the color theme.
    use_effect(move || {
        let theme = *s.theme.read();
        let palette = s.palette.read().clone();
        bridge::set_theme(theme.as_pref());
        bridge::set_palette(&palette);
        let saved = crate::prefs::get();
        if saved.theme_mode != theme.as_pref() || saved.palette != palette {
            crate::prefs::update(|p| {
                p.theme_mode = theme.as_pref().to_string();
                p.palette = palette;
            });
        }
    });

    // Dock badge: unread across every account's inbox.
    use_effect(move || {
        let unread: u32 = s
            .accounts
            .read()
            .iter()
            .flat_map(|a| a.folders.iter())
            .filter(|f| f.folder.role == Some(pigeon_core::model::FolderRole::Inbox))
            .map(|f| f.unread)
            .sum();
        crate::dock::set_badge(unread);
        crate::menubar::set_unread(unread);
    });

    // Tell the engine what to list whenever the view changes.
    use_effect(move || {
        let mailbox = s.mailbox.read().clone();
        let threaded = *s.threaded.read();
        crate::perf::mark_show_sent();
        send_cmd(Cmd::ShowMailbox { mailbox, threaded });
    });

    // Timing: log when a new list has rendered (debug level).
    use_effect(move || {
        let rows = s.list.read().len();
        crate::perf::mark_list_rendered(rows);
    });

    use_effect(move || {
        if *s.mode.read() == Mode::Calendar {
            let (from, to) = calendar::view_range(*s.cal_view.read(), *s.cal_day.read(), *s.first_weekday.read());
            send_cmd(Cmd::ShowCalendar { from, to });
        }
    });

    let body = if !*s.loaded.read() {
        rsx! { div { class: "app" } }
    } else if s.accounts.read().is_empty() || *s.first_run.read() {
        // Onboarding stays up while accounts are added, until "Continue".
        rsx! { onboarding::Onboarding {} }
    } else {
        rsx! {
            div { class: "app",
                sidebar::Sidebar {}
                div { class: "splitter", "data-pane": "sidebar", role: "separator", "aria-orientation": "vertical",
                    title: "Drag to resize · double-click to reset" }
                if *s.settings_open.read() {
                    settings::Settings {}
                } else {
                    match *s.mode.read() {
                        Mode::Mail => rsx! {
                            list::MailList {}
                            div { class: "splitter", role: "separator", "aria-orientation": "vertical",
                                title: "Drag to resize · double-click to reset" }
                            reader::Reader {}
                        },
                        Mode::Calendar => rsx! { calendar::CalendarView {} },
                    }
                }
            }
            compose::ComposerOverlay {}
            sender_card::SenderCard {}
            if s.editor.read().is_some() { event_editor::EventEditor {} }
            if *s.palette_open.read() { palette::Palette {} }
            if s.move_picker.read().is_some() { move_picker::MovePickerView {} }
        }
    };

    rsx! {
        div { style: "display:none", dangerous_inner_html: ICONS }
        div { style: "display:none", dangerous_inner_html: APP_ICONS }
        {body}
        if *s.agent_setup_open.read() { agent_setup::AgentSetup {} }
    }
}

/// Event reminders from the next-up agenda, once per event occurrence.
async fn reminder_loop(s: AppState) {
    let mut reminded = std::collections::HashSet::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let events = s.next_up.peek().clone();
        let now = pigeon_core::now_ms();
        let prefs = crate::prefs::get();
        let due = crate::notify::due_reminders(&events, now, &prefs, &reminded);
        if !due.is_empty() {
            crate::sound::play(&prefs.sound_theme, crate::sound::Kind::Meeting, prefs.sound_volume);
        }
        for e in due {
            crate::notify::post(&crate::notify::event_note(e, now));
            reminded.insert(crate::notify::reminder_key(e));
        }
    }
}

/// Route notification clicks: open the thread/event, or join the meeting.
/// Menu bar and status item menu clicks, by item id.
fn on_menu(s: AppState, id: &str) {
    match id {
        crate::menu::SETTINGS => {
            crate::menubar::show_main();
            let mut o = s.settings_open;
            o.set(true);
        }
        crate::menubar::OPEN => crate::menubar::show_main(),
        crate::menubar::NEW_MESSAGE => {
            crate::menubar::show_main();
            on_key(s, "meta+n");
        }
        crate::menubar::SYNC => send_cmd(Cmd::SyncNow),
        crate::menubar::SETTINGS => open_settings_at(s, "General"),
        crate::menu::AGENT_SETUP => {
            let mut o = s.agent_setup_open;
            o.set(true);
        }
        crate::menu::ABOUT => crate::about::show(),
        crate::menu::RESTART_TO_UPDATE => restart_to_update(),
        crate::menu::CHECK_UPDATES => {
            if crate::updater::available() {
                crate::updater::check_now();
            } else {
                s.local_toast("Updates are off in development builds", false);
            }
        }
        crate::menu::ZOOM_IN => zoom::change(s, zoom::Step::Bigger),
        crate::menu::ZOOM_OUT => zoom::change(s, zoom::Step::Smaller),
        crate::menu::ZOOM_RESET => zoom::change(s, zoom::Step::Reset),
        // Message menu items run the same handler as their keyboard shortcut.
        id => {
            if let Some(key) = crate::menu::key_for(id) {
                on_key(s, key);
            }
        }
    }
}

/// Show the window on Settings, scrolled to a section.
pub fn open_settings_at(s: AppState, section: &str) {
    crate::menubar::show_main();
    let mut o = s.settings_open;
    o.set(true);
    document::eval(&format!(
        "setTimeout(() => {{ const h = [...document.querySelectorAll('.settings__title')].find(e => e.textContent.trim() === {section:?}); h && h.scrollIntoView(); }}, 300);"
    ));
}

/// Install the downloaded update and relaunch into it.
pub fn restart_to_update() {
    if !crate::updater::install_now() {
        tracing::warn!("restart to update: no update is ready");
    }
}

/// Composing, or a send still inside its undo window: never restart then.
fn update_would_interrupt(s: AppState) -> bool {
    compose_window::any_open()
        || s.draft.peek().is_some()
        || s.outgoing.peek().iter().any(|o| matches!(o.state, crate::state::OutState::Sending))
}

/// Updater status to the "Update ready" pill, menus and About; and the
/// silent install while Coo sits in the menu bar.
async fn update_loop(s: AppState) {
    let Some(mut rx) = crate::updater::take_events() else { return };
    let mut ready_since: Option<std::time::SystemTime> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        tokio::select! {
            next = rx.recv() => {
                let Some(next) = next else { return };
                let ready = match &next {
                    crate::updater::Status::Ready(v) => Some(v.clone()),
                    _ => None,
                };
                if ready.is_some() && ready_since.is_none() {
                    ready_since = Some(std::time::SystemTime::now());
                }
                crate::menu::set_restart(ready.as_deref());
                crate::menubar::set_restart(ready.as_deref());
                let mut r = s.update_ready;
                r.set(ready);
                crate::about::refresh();
            }
            _ = tick.tick() => {
                let Some(since) = ready_since else { continue };
                let hidden = crate::menubar::main_hidden();
                if crate::updater::silent_install_due(since, std::time::SystemTime::now(), hidden, update_would_interrupt(s)) {
                    tracing::info!("installing the ready update silently (Coo is in the menu bar)");
                    crate::prefs::update(|p| p.relaunch_hidden = true);
                    if !crate::updater::install_now() {
                        crate::prefs::update(|p| p.relaunch_hidden = false);
                    }
                }
            }
        }
    }
}

/// The status item's popover buttons.
async fn menubar_loop(s: AppState) {
    let Some(mut rx) = crate::menubar::take_actions() else { return };
    while let Some(action) = rx.recv().await {
        match action {
            crate::menubar::Action::OpenSettings => open_settings_at(s, "General"),
        }
    }
}

async fn click_loop(s: AppState) {
    use crate::notify::{Click, Target};
    let Some(mut rx) = crate::notify::take_clicks() else { return };
    while let Some(click) = rx.recv().await {
        if !matches!(click, Click::Join(_)) {
            crate::menubar::show_main();
        }
        let (mut mode, mut settings, mut search) = (s.mode, s.settings_open, s.search);
        match click {
            Click::Join(url) => {
                let _ = open::that_detached(&url);
            }
            Click::Open(Target::Thread { account, thread }) => {
                mode.set(Mode::Mail);
                settings.set(false);
                search.set(None);
                let mut sel = s.selected;
                sel.set(Some(format!("{account}|{thread}")));
                send_cmd(Cmd::OpenThread { account_id: account, thread_id: thread });
            }
            Click::Open(Target::Inbox) => {
                mode.set(Mode::Mail);
                settings.set(false);
                search.set(None);
                let mut mb = s.mailbox;
                mb.set(pigeon_engine::service::Mailbox::Unified(pigeon_core::model::FolderRole::Inbox));
            }
            Click::Open(Target::Event { start_at, .. }) => {
                settings.set(false);
                mode.set(Mode::Calendar);
                if let Some(t) = crate::format::local(start_at) {
                    calendar::go_to(s, t.date_naive());
                }
            }
        }
    }
}

/// What double-clicking a title bar does (System Settings › Desktop & Dock ›
/// "Double-click a window's title bar to").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleDoubleClick {
    Zoom,
    Minimize,
    Nothing,
}

/// Parse `defaults read -g AppleActionOnDoubleClick` (unset means Zoom).
pub fn title_double_click(pref: Option<&str>) -> TitleDoubleClick {
    match pref.map(str::trim) {
        Some("Minimize") => TitleDoubleClick::Minimize,
        Some("None") => TitleDoubleClick::Nothing,
        _ => TitleDoubleClick::Zoom,
    }
}

/// Native window actions from the title strips: drag, or the user's
/// double-click preference.
fn window_action(action: &str) {
    let w = dioxus::desktop::window();
    match action {
        "drag" => w.drag(),
        "zoom" => {
            static PREF: std::sync::OnceLock<TitleDoubleClick> = std::sync::OnceLock::new();
            let pref = *PREF.get_or_init(|| {
                let out =
                    std::process::Command::new("defaults").args(["read", "-g", "AppleActionOnDoubleClick"]).output();
                title_double_click(
                    out.ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .as_deref(),
                )
            });
            match pref {
                TitleDoubleClick::Zoom => w.set_maximized(!w.is_maximized()),
                TitleDoubleClick::Minimize => w.set_minimized(true),
                TitleDoubleClick::Nothing => {}
            }
        }
        _ => {}
    }
}

/// Test/screenshot builds can ignore all keyboard actions (PIGEON_IGNORE_INPUT=1).
fn input_ignored() -> bool {
    static IGNORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IGNORE.get_or_init(|| std::env::var("PIGEON_IGNORE_INPUT").is_ok_and(|v| v == "1"))
}

/// Message list width bounds (px).
pub const LIST_W_MIN: u32 = 300;
pub const LIST_W_MAX: u32 = 760;

/// Head CSS for a saved list width (empty = design default).
pub fn list_width_css(px: u32) -> String {
    if px == 0 { String::new() } else { format!(":root{{--list-w:{}px}}", px.clamp(LIST_W_MIN, LIST_W_MAX)) }
}

/// Sidebar width bounds (px); the design default is `--sidebar-w`.
pub const SIDEBAR_W_MIN: u32 = 200;
pub const SIDEBAR_W_MAX: u32 = 360;

/// Head CSS for a saved sidebar width (empty = design default).
pub fn sidebar_width_css(px: u32) -> String {
    if px == 0 { String::new() } else { format!(":root{{--sidebar-w:{}px}}", px.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX)) }
}

async fn bridge_loop(s: AppState) {
    let mut eval = document::eval(bridge::JS);
    while let Ok(msg) = eval.recv::<Msg>().await {
        match msg.k.as_str() {
            "os" => {
                let mut os = s.os_dark;
                os.set(msg.v == "dark");
            }
            "win" => window_action(&msg.v),
            "sidebar-w" => {
                let px: u32 = msg.v.parse().unwrap_or(0);
                crate::prefs::update(|p| {
                    p.sidebar_width = if px == 0 { 0 } else { px.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX) }
                });
            }
            "list-w" => {
                let px: u32 = msg.v.parse().unwrap_or(0);
                crate::prefs::update(|p| p.list_width = if px == 0 { 0 } else { px.clamp(LIST_W_MIN, LIST_W_MAX) });
            }
            "link" => {
                let v = msg.v.to_ascii_lowercase();
                if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("mailto:") {
                    let _ = open::that_detached(&msg.v);
                }
            }
            // Screenshot runs (PIGEON_IGNORE_INPUT=1) open frontmost; never let a
            // stray keystroke meant for another app act on the demo mailbox.
            "key" if input_ignored() => {}
            "key" => on_key(s, &msg.v),
            "draft-input" => compose::autosave(s.main_slot(), &msg.v),
            "sig-input" => {
                if let Ok((email, html)) = serde_json::from_str::<(String, String)>(&msg.v) {
                    let clean = pigeon_core::html::sanitize_signature(&html);
                    let empty = pigeon_core::text::html_to_text(&clean).trim().is_empty() && !clean.contains("<img");
                    crate::prefs::update(move |p| {
                        if empty {
                            p.signatures.remove(&email);
                        } else {
                            p.signatures.insert(email, clean);
                        }
                    });
                }
            }
            "cal-create" | "cal-move" | "cal-resize" => calendar::on_drag(s, &msg.k, &msg.v),
            "focus" => {
                let mut f = s.window_focused;
                f.set(msg.v == "1");
            }
            _ => {}
        }
    }
}

/// The selected list item, if any.
pub fn selected_item(s: &AppState) -> Option<ListItem> {
    let key = s.selected.read().clone()?;
    s.visible_items().into_iter().find(|i| AppState::key(i) == key)
}

/// Select a list item and open it in the reader.
pub fn select(s: AppState, item: &ListItem) {
    compose::leave_docked(s);
    let mut sel = s.selected;
    sel.set(Some(AppState::key(item)));
    match item {
        ListItem::Thread(t) => {
            send_cmd(Cmd::OpenThread { account_id: t.account_id.clone(), thread_id: t.thread_id.clone() })
        }
        ListItem::Message(m) => {
            send_cmd(Cmd::OpenMessage { account_id: m.account_id.clone(), message_id: m.header.id.clone() })
        }
    }
}

fn step(s: AppState, delta: isize) {
    let list = s.visible_items();
    if list.is_empty() {
        return;
    }
    let current = s.selected.read().clone();
    let idx = current.and_then(|k| list.iter().position(|i| AppState::key(i) == k));
    let next = match idx {
        Some(i) => (i as isize + delta).clamp(0, list.len() as isize - 1) as usize,
        None => 0,
    };
    select(s, &list[next]);
}

/// Run a mail action on a list item; advances the selection when the item
/// leaves the list.
pub fn act(s: AppState, item: &ListItem, action: MailAction) {
    let header = item.header();
    let leaves = matches!(action, MailAction::Archive | MailAction::Trash | MailAction::MoveTo(_));
    if leaves && s.selected.read().as_deref() == Some(AppState::key(item).as_str()) {
        let list = s.visible_items();
        let pos = list.iter().position(|i| AppState::key(i) == AppState::key(item));
        let next = pos.and_then(|p| list.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| list.get(q)))).cloned();
        match next {
            Some(n) => select(s, &n),
            None => {
                let (mut sel, mut reader) = (s.selected, s.reader);
                sel.set(None);
                reader.set(None);
            }
        }
    }
    let subject = if header.subject.is_empty() { "(no subject)".to_string() } else { header.subject.clone() };
    let label = match &action {
        MailAction::Archive => format!("Archived “{subject}”"),
        MailAction::Trash => format!("Deleted “{subject}”"),
        MailAction::Flag => format!("Flagged “{subject}”"),
        MailAction::Unflag => format!("Unflagged “{subject}”"),
        MailAction::MoveTo(_) => format!("Moved “{subject}”"),
        MailAction::AddLabel(_) => format!("Labeled “{subject}”"),
        _ => String::new(),
    };
    let thread_id = match item {
        ListItem::Thread(t) => Some(t.thread_id.clone()),
        ListItem::Message(_) => None,
    };
    send_cmd(Cmd::Act {
        account_id: item.account_id().to_string(),
        ids: vec![header.id.clone()],
        thread_id,
        action,
        label,
    });
}

pub fn reply(item: &ListItem, kind: ReplyKind) {
    send_cmd(Cmd::StartReply { account_id: item.account_id().to_string(), message_id: item.header().id.clone(), kind });
}

/// The meeting joinable right now, if any: (title, url).
pub fn joinable_now(s: &AppState) -> Option<(String, String)> {
    let events = s.next_up.read();
    pigeon_core::meeting::joinable(events.iter().map(|r| &r.event), pigeon_core::now_ms())
        .map(|(e, l)| (e.title.clone(), l.url))
}

pub fn join_now(s: AppState) {
    match joinable_now(&s) {
        Some((_, url)) => crate::meeting::join(&url),
        None => s.local_toast("No meeting to join right now", false),
    }
}

fn on_key(s: AppState, key: &str) {
    let (mut mode, draft) = (s.mode, s.draft);
    match key {
        "meta+1" => return mode.set(Mode::Mail),
        "meta+2" => return mode.set(Mode::Calendar),
        // ⇧⌘N, as in Mail ("Get All New Mail"); ⌘R is Reply.
        "meta+shift+n" => return send_cmd(Cmd::SyncNow),
        // ⇧J: join the meeting that's on now (or starting soon), from anywhere.
        "J" => return join_now(s),
        "zoom-in" => return zoom::change(s, zoom::Step::Bigger),
        "zoom-out" => return zoom::change(s, zoom::Step::Smaller),
        "zoom-reset" => return zoom::change(s, zoom::Step::Reset),
        "c" if *s.mode.read() == Mode::Calendar => return event_editor::open_new_now(s),
        "meta+n" | "c" => return send_cmd(Cmd::NewDraft),
        "meta+k" => {
            let mut p = s.palette_open;
            let v = *p.read();
            return p.set(!v);
        }
        // Open (not toggle): the menu's ⌘, accelerator may fire as well.
        "meta+," => {
            let mut o = s.settings_open;
            return o.set(true);
        }
        "Escape" => {
            // A docked draft set aside behind the "Draft:" chip isn't being edited.
            let composing = draft.read().is_some() && !*s.docked_hidden.read();
            if s.move_picker.read().is_some() {
                let mut p = s.move_picker;
                return p.set(None);
            }
            if *s.agent_setup_open.read() {
                let mut o = s.agent_setup_open;
                return o.set(false);
            }
            if *s.palette_open.read() {
                let mut p = s.palette_open;
                return p.set(false);
            }
            if s.editor.read().is_some() {
                let mut e = s.editor;
                return e.set(None);
            }
            if !composing && *s.settings_open.read() {
                let mut o = s.settings_open;
                return o.set(false);
            }
            if !composing && s.search.read().is_some() {
                return search::close(s);
            }
            if composing {
                compose::save_and_close(s.main_slot());
            }
            return;
        }
        "meta+Enter" => {
            if !*s.docked_hidden.read() {
                compose::send_current(s, s.main_slot());
            }
            return;
        }
        "z" | "meta+z" => {
            let undo = s.toasts.read().iter().rev().find_map(|t| t.undo.clone());
            if let Some((account_id, handle)) = undo {
                let mut toasts = s.toasts;
                toasts.write().retain(|t| t.undo.as_ref().map(|u| u.1) != Some(handle));
                send_cmd(Cmd::Undo { account_id, handle });
            }
            return;
        }
        _ => {}
    }
    if *s.mode.read() != Mode::Mail {
        return;
    }
    match key {
        "/" | "meta+f" => search::open(s, ""),
        "v" => move_picker::open(s, false),
        "l" => move_picker::open(s, true),
        "j" | "ArrowDown" => step(s, 1),
        "k" | "ArrowUp" => step(s, -1),
        _ => {
            let Some(item) = selected_item(&s) else { return };
            let h = item.header();
            let unread = match &item {
                ListItem::Thread(t) => t.unread_count > 0,
                ListItem::Message(m) => !m.header.is_read,
            };
            match key {
                "e" | "ctrl+meta+a" => act(s, &item, MailAction::Archive),
                "meta+Backspace" | "#" | "x" => act(s, &item, MailAction::Trash),
                "s" | "meta+shift+l" => act(s, &item, if h.is_flagged { MailAction::Unflag } else { MailAction::Flag }),
                "u" | "meta+shift+u" => {
                    act(s, &item, if unread { MailAction::MarkRead } else { MailAction::MarkUnread })
                }
                "r" | "meta+r" => reply(&item, ReplyKind::Reply),
                "a" | "meta+shift+r" => reply(&item, ReplyKind::ReplyAll),
                "f" | "meta+shift+f" => reply(&item, ReplyKind::Forward),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    #[test]
    fn title_double_click_follows_the_macos_setting() {
        assert_eq!(title_double_click(None), TitleDoubleClick::Zoom);
        assert_eq!(title_double_click(Some("Maximize\n")), TitleDoubleClick::Zoom);
        assert_eq!(title_double_click(Some("Minimize\n")), TitleDoubleClick::Minimize);
        assert_eq!(title_double_click(Some("None")), TitleDoubleClick::Nothing);
    }

    #[test]
    fn saved_pane_widths_are_clamped() {
        assert_eq!(sidebar_width_css(0), "");
        assert_eq!(sidebar_width_css(120), ":root{--sidebar-w:200px}");
        assert_eq!(sidebar_width_css(280), ":root{--sidebar-w:280px}");
        assert_eq!(sidebar_width_css(999), ":root{--sidebar-w:360px}");
        assert_eq!(list_width_css(999), ":root{--list-w:760px}");
    }
}
