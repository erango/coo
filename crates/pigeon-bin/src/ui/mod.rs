pub mod account_menu;
pub mod add_account;
mod agent_setup;
mod attachments;
mod avatar;
pub mod bird;
mod bug_report;
pub mod calendar;
pub mod compose;
pub mod compose_window;
mod drive;
pub mod event_editor;
mod format_bar;
mod icloud;
mod invite;
mod list;
mod list_motion;
pub(crate) mod mail_drag;
pub mod mailframe;
mod move_picker;
mod multi;
mod onboarding;
mod palette;
mod reader;
pub mod reader_window;
mod recipients;
pub(crate) mod row_actions;
pub mod row_menu;
pub mod search;
pub mod sender_card;
mod settings;
mod settings_sections;
mod sidebar;
mod spam;
mod toasts;
mod update_dialog;
pub mod vlist;
pub mod zoom;

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_core::model::FolderRole;
use pigeon_engine::MailAction;
use pigeon_engine::service::{AccountNav, Cmd, ListItem, Mailbox};

use crate::bridge::{self, Msg};
use crate::send_cmd;
use crate::shortcuts::{self, Do};
use crate::state::{self, AppState, Mode, Theme};

pub(crate) const ICONS: &str = include_str!("../../../../design/icons.svg");
/// App-only icons the design sprite lacks. `i-gear`: the design's
/// `i-settings` is a circle with rays and reads as a sun next to the theme
/// toggle. `i-maximize` / `i-minimize`: the inline composer's grow and
/// restore. Paths from Lucide (ISC license). `i-arrow-left`: the design's
/// `i-arrow-right`, mirrored (Settings › Back). The agent icons (terminal,
/// code-window, chat, plug) are for "Connect an AI agent". The composer's
/// formatting toolbar: `fmt-*`, lists, indents, alignment, code, emoji (Lucide), `redo`
/// (the design's `undo`, mirrored); `fmt-color` / `fmt-highlight` draw
/// their bar in `--fmt-swatch`, the color under the caret.
pub(crate) const APP_ICONS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><symbol id="i-gear" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></symbol><symbol id="i-maximize" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></symbol><symbol id="i-minimize" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7"/></symbol><symbol id="i-arrow-left" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5"/><path d="M11 6l-6 6 6 6"/></symbol><symbol id="i-terminal" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15H17"/></symbol><symbol id="i-code-window" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M3 8.5h18M6 6.3h.01M8.2 6.3h.01M9.5 12l-2 2 2 2M14.5 12l2 2-2 2"/></symbol><symbol id="i-chat" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7a2.5 2.5 0 0 1-2.5 2.5H10l-4.5 4v-4A1.5 1.5 0 0 1 4 14.5z"/><path d="M8.5 9h7M8.5 12h4.5"/></symbol><symbol id="i-plug" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3v5M15 3v5M6.5 8h11v3a5.5 5.5 0 0 1-11 0zM12 16.5V21"/></symbol><symbol id="i-fmt-bold" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 5h6a3.5 3.5 0 0 1 0 7H7zM7 12h7a3.5 3.5 0 0 1 0 7H7z"/></symbol><symbol id="i-fmt-italic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M19 4h-9M14 20H5M15 4 9 20"/></symbol><symbol id="i-fmt-underline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4v6a6 6 0 0 0 12 0V4M4 20h16"/></symbol><symbol id="i-fmt-strike" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M16 4H9a3 3 0 0 0-2.83 4M14 12a4 4 0 0 1 0 8H6M4 12h16"/></symbol><symbol id="i-fmt-size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 14h-5M16 16v-3.5a2.5 2.5 0 0 1 5 0V16M4.5 13h6M3 16l4.5-9 4.5 9"/></symbol><symbol id="i-fmt-color" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M6.5 16 12 3l5.5 13M8.6 11h6.8"/><path d="M4 21h16" style="stroke:var(--fmt-swatch,currentColor);stroke-width:3"/></symbol><symbol id="i-fmt-highlight" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m9 13-4 4v1h6l2-2M19 9l-4.2 4.2a1.6 1.6 0 0 1-2.2 0L9.8 10.4a1.6 1.6 0 0 1 0-2.2L14 4"/><path d="M4 21h16" style="stroke:var(--fmt-swatch,currentColor);stroke-width:3"/></symbol><symbol id="i-list-ordered" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 6h11M10 12h11M10 18h11M4 6h1v4M4 10h2M6 18H4c0-1 2-2 2-3s-1-1.5-2-1"/></symbol><symbol id="i-indent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 5H11M21 12H11M21 19H11M3 8l4 4-4 4"/></symbol><symbol id="i-outdent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 5H11M21 12H11M21 19H11M7 8l-4 4 4 4"/></symbol><symbol id="i-align-left" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 5H3M15 12H3M17 19H3"/></symbol><symbol id="i-align-center" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 5H3M17 12H7M19 19H5"/></symbol><symbol id="i-align-right" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 5H3M21 12H9M21 19H7"/></symbol><symbol id="i-fmt-clear" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7V4h16v3M5 20h6M13 4 8 20M15 15l5 5M20 15l-5 5"/></symbol><symbol id="i-redo" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/></symbol><symbol id="i-code" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/></symbol><symbol id="i-code-block" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="m10 9-3 3 3 3M14 15l3-3-3-3"/></symbol><symbol id="i-emoji" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M8.5 14s1.3 1.8 3.5 1.8 3.5-1.8 3.5-1.8M9 9.5h.01M15 9.5h.01"/></symbol><symbol id="i-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 2.8v5.4c0 4.4-2.9 8-7 9.8-4.1-1.8-7-5.4-7-9.8V5.8z"/><path d="M12 8.5v4M12 15.8h.01"/></symbol><symbol id="i-pushpin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3.5h6l-1 6.5 3.5 3v1.5h-11V13L10 10z"/><path d="M12 14.5V21"/></symbol></defs></svg>"#;
const DAY_MS: i64 = 86_400_000;
/// The Settings swap: fade-out and fade-in lengths (match app.css).
const SWAP_OUT_MS: u64 = 110;
const SWAP_IN_MS: u64 = 190;

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
    crate::keymon::use_native_keys();
    zoom::use_live_zoom();
    zoom::use_wheel_zoom(s);
    crate::dragout::use_main_js();
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
                        crate::menubar::did_hide(compose_window::any_open() || reader_window::any_open());
                    });
                } else {
                    // A message being written here is kept in Drafts.
                    compose::save_now(s.main_slot());
                    compose_window::close_all();
                    reader_window::close_all();
                }
            }
            Event::Reopen { .. } => {
                crate::menubar::show_main();
                crate::updater::check_background(false);
            }
            // Coming back to the foreground: a silent update check, at most every 10 minutes.
            Event::WindowEvent { event: WindowEvent::Focused(true), .. } => crate::updater::check_background(false),
            _ => {}
        }
    });

    use_hook(move || {
        // Sparkle (release builds only): starts its background check schedule.
        crate::updater::start();
        let window = dioxus::desktop::window();
        // Dioxus shows the window after this first render, so it opens
        // where it was.
        remember_frame(&window);
        crate::menubar::install(window.clone());
        if crate::prefs::get().relaunch_hidden {
            crate::prefs::update(|p| p.relaunch_hidden = false);
            crate::menubar::start_hidden();
        }
        spawn(update_loop(s));
        spawn(menubar_loop(s));
        // The menu bar is installed with the window; line up its shortcuts
        // once it's there.
        spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            crate::menu::align_shortcuts();
        });
        spawn(save_image_loop());
        spawn(state::pump(s));
        spawn(bridge_loop(s));
        send_read_delay();
        crate::keymon::start();
        spawn(reminder_loop(s));
        spawn(click_loop(s));
        // Debug builds: open a surface on launch for screenshots/testing,
        // e.g. PIGEON_DEBUG_OPEN=palette:inv, search:invoice, event-new:a@x.com,b@y.com
        if cfg!(debug_assertions)
            && let Ok(v) = std::env::var("PIGEON_DEBUG_OPEN")
        {
            let (what, arg) = v.split_once(':').unwrap_or((v.as_str(), ""));
            match what {
                // compose-docked[:<line>/<line>...] — a new draft docked in the
                // reading pane, with these lines typed in
                "compose-docked" => {
                    crate::prefs::update(|p| p.compose_mode = "docked".into());
                    new_message(s);
                    if !arg.is_empty() {
                        let lines: Vec<String> =
                            arg.split('/').map(|l| format!("<div dir=\"auto\">{l}</div>")).collect();
                        let html = serde_json::to_string(&lines.concat()).unwrap_or_default();
                        document::eval(&format!(
                            "setTimeout(()=>{{const e=document.querySelector('.composer__editor');if(e)e.innerHTML={html};}},1200)"
                        ));
                    }
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
                "compose" => new_message(s),
                // add-account[:<email> | choose|<email> | icloud|<email> | waiting|google|<email>]
                "add-account" => {
                    let mut slot = s.add_account;
                    slot.set(Some(add_account::debug_opening(arg)));
                }
                // search:<query>, with an optional `|` marking the caret
                "search" if arg.contains('|') => search::open_at(s, arg),
                "search" => search::open(s, arg, search::here(&s)),
                // find:<query>|<account_id>|<thread_id> — search, then open a thread from it
                "find" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [q, account_id, thread_id] = p[..] {
                        search::open(s, q, pigeon_engine::service::SearchScope::All);
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                    }
                }
                // sending:<account_id>|<to>|<subject> — a sending row in the list
                // (screenshots; nothing is sent)
                "sending" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [account_id, to, subject] = p[..] {
                        let addr = |e: &str| pigeon_core::model::Address { name: None, email: e.into() };
                        let from = pigeon_core::compose::Sender {
                            account_id: account_id.into(),
                            address: addr("me@example.com"),
                        };
                        let mut d = pigeon_core::compose::Draft::new(from, pigeon_core::now_ms());
                        d.to = to.split(',').map(addr).collect();
                        d.subject = subject.into();
                        s.track_outgoing(d, crate::state::OutState::Sending);
                    }
                }
                // toast:<message> — an Archive toast with Undo (screenshots; Undo of a
                // made-up handle undoes nothing)
                "toast" => s.push_toast(pigeon_engine::service::Toast {
                    id: 1,
                    message: arg.into(),
                    error: false,
                    undo: Some(("debug".into(), 0)),
                    cancel_send: None,
                    action: Some(pigeon_engine::MailAction::Archive),
                    reveal: None,
                }),
                // saved-toast — a "Saved …" toast with Show in Finder (screenshots)
                "saved-toast" => s.push_toast(pigeon_engine::service::Toast {
                    id: 2,
                    message: "Saved Q4-roadmap.pdf".into(),
                    reveal: Some(std::env::temp_dir()),
                    ..Default::default()
                }),
                // open:<account_id>|<thread_id>
                // menu-shot:<Message|View|…>: pop up a menu-bar menu (screenshots)
                "menu-shot" => {
                    let name = arg.to_string();
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        crate::menu::debug_pop(&name);
                    });
                }
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
                            window: None,
                        }));
                    }
                }
                // reply:<account_id>|<thread_id>|<message_id>[|forward][|quote] —
                // open a thread and start a reply (or forward); `quote` unfolds
                // the quoted original and types into it
                "reply" => {
                    let p: Vec<&str> = arg.split('|').collect();
                    if let [account_id, thread_id, message_id, ref rest @ ..] = p[..] {
                        let kind = match rest.first() {
                            Some(&"forward") => ReplyKind::Forward,
                            Some(&"reuse") => ReplyKind::Reuse,
                            _ => ReplyKind::Reply,
                        };
                        let mut sel = s.selected;
                        sel.set(Some(format!("{account_id}|{thread_id}")));
                        send_cmd(Cmd::OpenThread { account_id: account_id.into(), thread_id: thread_id.into() });
                        let (account_id, message_id) = (account_id.to_string(), message_id.to_string());
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                            send_cmd(Cmd::StartReply { account_id, message_id, kind });
                        });
                        if rest.contains(&"quote") {
                            document::eval(
                                "setTimeout(()=>{document.querySelector('.inline-reply__quoted')?.click();\
                                 setTimeout(()=>{const f=document.querySelector('iframe.mail-frame--edit');\
                                 const b=f&&f.contentDocument.body;if(!b)return;\
                                 b.focus();const d=f.contentDocument;const r=d.createRange();r.setStart(b,0);r.collapse(true);\
                                 d.getSelection().removeAllRanges();d.getSelection().addRange(r);\
                                 d.execCommand('insertText',false,'EDITED QUOTE');},800)},1800)",
                            );
                        }
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
                // rowmenu:<account_id>|<thread_id>[|<folder_id>] — show that folder
                // (else the unified inbox), open the thread and its right-click menu
                "rowmenu" => {
                    let p: Vec<String> = arg.split('|').map(String::from).collect();
                    if let [account_id, thread_id, rest @ ..] = &p[..] {
                        if let Some(folder_id) = rest.first() {
                            let mut mb = s.mailbox;
                            mb.set(pigeon_engine::service::Mailbox::Folder {
                                account_id: account_id.clone(),
                                folder_id: folder_id.clone(),
                            });
                        }
                        let key = format!("{account_id}|{thread_id}");
                        let mut sel = s.selected;
                        sel.set(Some(key.clone()));
                        send_cmd(Cmd::OpenThread { account_id: account_id.clone(), thread_id: thread_id.clone() });
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                            if let Some(item) = s.list.peek().iter().find(|i| AppState::key(i) == key).cloned() {
                                row_menu::open(s, &item, 300.0, 140.0);
                            }
                        });
                    }
                }
                // accountmenu[:<n>] — the right-click menu of the n-th account
                // header in the sidebar (default 0), once accounts have loaded
                "accountmenu" => {
                    let n: usize = arg.parse().unwrap_or(0);
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        let id = s.accounts.peek().get(n).map(|a| a.account.id.clone());
                        if let Some(id) = id {
                            account_menu::open(s, &id, 60.0, 300.0);
                        }
                    });
                }
                // calmenu:<n>[|<k>] — the calendar, with the right-click menu of
                // the n-th account header in its sidebar, or of its k-th calendar
                "calmenu" => {
                    let mut m = s.mode;
                    m.set(Mode::Calendar);
                    let (n, k) = arg.split_once('|').map_or((arg, None), |(n, k)| (n, k.parse::<usize>().ok()));
                    let n: usize = n.parse().unwrap_or(0);
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                        let Some(id) = s.accounts.peek().get(n).map(|a| a.account.id.clone()) else { return };
                        let cal = k.and_then(|k| {
                            s.calendar
                                .peek()
                                .calendars
                                .iter()
                                .filter(|(a, _)| *a == id)
                                .nth(k)
                                .map(|(_, c)| c.id.clone())
                        });
                        let target = cal.map_or(account_menu::Target::CalAccount, account_menu::Target::Calendar);
                        account_menu::open_for(s, &id, target, 120.0, 330.0);
                    });
                }
                // settings-account:<n> — Settings › Accounts at the n-th account
                "settings-account" => {
                    let n: usize = arg.parse().unwrap_or(0);
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        let id = s.accounts.peek().get(n).map(|a| a.account.id.clone());
                        if let Some(id) = id {
                            settings::show_account(s, &id);
                        }
                    });
                }
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
                        settings::show_page(s, arg);
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
                // bug-report: the Report a Bug form (screenshots)
                "bug-report" => report_bug(s),
                // update-ready:<version>: pretend an update downloaded (the pill, menus)
                "update-ready" => crate::updater::fake_ready(if arg.is_empty() { "9.9.9" } else { arg }),
                // update-dialog:<checking|uptodate|downloading|ready|failed>: the Check
                // for Updates dialog in that state (screenshots)
                "update-dialog" => {
                    use crate::updater::Status;
                    let st = match arg {
                        "uptodate" => Status::UpToDate,
                        "downloading" => Status::Downloading("0.1.8".into()),
                        "ready" => Status::Ready("0.1.8".into()),
                        "failed" => Status::Failed,
                        _ => Status::Checking,
                    };
                    let mut status = s.update_status;
                    status.set(st);
                    let mut open = s.update_dialog;
                    open.set(true);
                }
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

    // Dock badge: unread across the inboxes of accounts that aren't muted.
    use_effect(move || {
        let _ = s.accounts.read();
        refresh_badge(s);
    });

    // Tell the engine what to list whenever the view changes. Moving to
    // another mailbox closes the message that was open in the last one.
    let mut last_mailbox = use_signal(|| None::<pigeon_engine::service::Mailbox>);
    use_effect(move || {
        let mailbox = s.mailbox.read().clone();
        let threaded = *s.threaded.read();
        if last_mailbox.peek().as_ref().is_some_and(|m| *m != mailbox) {
            let (mut sel, mut reader, mut multi) = (s.selected, s.reader, s.multi);
            sel.set(None);
            reader.set(None);
            multi.set(Vec::new());
        }
        last_mailbox.set(Some(mailbox.clone()));
        let key = state::mailbox_key(&mailbox);
        if crate::prefs::get().last_mailbox != key {
            crate::prefs::update(|p| p.last_mailbox = key);
        }
        crate::perf::mark_show_sent();
        send_cmd(Cmd::ShowMailbox { mailbox, threaded });
    });

    // The mailbox restored from last time is gone (account removed, folder
    // deleted): fall back to the unified inbox once accounts have loaded.
    use_effect(move || {
        let accounts = s.accounts.read();
        if *s.loaded.read() && !accounts.is_empty() && !state::mailbox_exists(&s.mailbox.peek(), &accounts) {
            let mut mb = s.mailbox;
            mb.set(pigeon_engine::service::Mailbox::Unified(pigeon_core::model::FolderRole::Inbox));
        }
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

    // Opening or leaving Settings: the current view fades out, the swap
    // happens while it's gone, then the new view fades in, nudged in the
    // direction of travel (app.css, `data-swap`). A toggle mid-way
    // restarts from where things are.
    let mut swap = use_signal(|| None::<&'static str>);
    let mut generation = use_signal(|| 0u64);
    use_effect(move || {
        let open = *s.settings_open.read();
        let mut shown = s.settings_shown;
        if *shown.peek() == open {
            return;
        }
        let g = *generation.peek() + 1;
        generation.set(g);
        let current = move || *generation.peek() == g;
        swap.set(Some(if open { "leave-fwd" } else { "leave-back" }));
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(SWAP_OUT_MS)).await;
            if !current() {
                return;
            }
            shown.set(open);
            swap.set(Some(if open { "enter-fwd" } else { "enter-back" }));
            tokio::time::sleep(std::time::Duration::from_millis(SWAP_IN_MS + 40)).await;
            if current() {
                swap.set(None);
            }
        });
    });

    let body = if !*s.loaded.read() {
        rsx! { div { class: "app" } }
    } else if s.accounts.read().is_empty() || *s.first_run.read() {
        // Onboarding stays up while accounts are added, until "Continue".
        rsx! { onboarding::Onboarding {} }
    } else {
        rsx! {
            div { class: "app", "data-swap": *swap.read(),
                sidebar::Sidebar {}
                div { class: "splitter", "data-pane": "sidebar", role: "separator", "aria-orientation": "vertical",
                    title: "Drag to resize · double-click to reset" }
                // Everything but the sidebar sits on one sheet, raised off
                // the window's ground (app.css, "Paper on stone").
                div { class: "sheet",
                    if *s.settings_shown.read() {
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
            }
            compose::ComposerOverlay {}
            sender_card::SenderCard {}
            if s.editor.read().is_some() { event_editor::EventEditor {} }
            if *s.palette_open.read() { palette::Palette {} }
            if s.move_picker.read().is_some() { move_picker::MovePickerView {} }
            if s.row_menu.read().is_some() { row_menu::RowMenuView {} }
            if s.account_menu.read().is_some() { account_menu::AccountMenuView {} }
        }
    };

    rsx! {
        div { style: "display:none", dangerous_inner_html: ICONS }
        div { style: "display:none", dangerous_inner_html: APP_ICONS }
        {body}
        if *s.agent_setup_open.read() { agent_setup::AgentSetup {} }
        if *s.update_dialog.read() { update_dialog::UpdateDialog {} }
        if *s.bug_report_open.read() { bug_report::BugReportDialog {} }
        if s.add_account.read().is_some() { add_account::AddAccountSheet {} }
        // Window-wide, bottom right, over panes and sheets alike.
        if *s.loaded.read() { toasts::Toasts {} }
    }
}

/// Event reminders from the next-up agenda, once per event occurrence.
async fn reminder_loop(s: AppState) {
    let mut reminded = std::collections::HashSet::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let events = s.next_up_shown_peek();
        let now = pigeon_core::now_ms();
        let prefs = crate::prefs::get();
        let due = crate::notify::due_reminders(&events, now, &prefs, &reminded);
        let mut sound = !due.is_empty() && prefs.meeting_sound && crate::sound::take_alert(crate::sound::Kind::Meeting);
        for e in due {
            let mut note = crate::notify::event_note(e, now);
            note.sound = std::mem::take(&mut sound).then_some(crate::sound::Kind::Meeting);
            crate::notify::post(&note);
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
            run(s, Do::Compose);
        }
        crate::menubar::SYNC => sync_now(s),
        crate::menubar::SETTINGS => open_settings_at(s, "General"),
        crate::menu::AGENT_SETUP => {
            let mut o = s.agent_setup_open;
            o.set(true);
        }
        crate::menu::REPORT_BUG | crate::menu::REPORT_BUG_APP => report_bug(s),
        crate::menu::ABOUT => crate::about::show(),
        crate::menu::RESTART_TO_UPDATE => restart_to_update(),
        crate::menu::CHECK_UPDATES => check_for_updates(s),
        crate::menu::ZOOM_IN => zoom::change(s, zoom::Step::Bigger),
        crate::menu::ZOOM_OUT => zoom::change(s, zoom::Step::Smaller),
        crate::menu::ZOOM_RESET => zoom::change(s, zoom::Step::Reset),
        crate::menu::REPORT_PHISHING => spam::report_phishing(s),
        // The selection, else the open message.
        crate::menu::SAVE_AS => {
            let many = multi::items(&s);
            save_eml(if many.is_empty() { selected_item(&s).into_iter().collect() } else { many });
        }
        crate::menu::OPEN_WINDOW => {
            let key = s.selected.peek().clone();
            let item = key.and_then(|k| s.list.peek().iter().find(|i| AppState::key(i) == k).cloned());
            if let Some(item) = item {
                reader_window::open(s, item);
            }
        }
        crate::menu::NEW_EVENT => {
            crate::menubar::show_main();
            let mut mode = s.mode;
            mode.set(Mode::Calendar);
            event_editor::open_new_now(s);
        }
        // View, Message and Calendar items run the same handler as their
        // keyboard shortcut.
        id => {
            if let Some(what) = crate::menu::action_for(id) {
                run(s, what);
            }
        }
    }
}

/// Show the window on Settings, scrolled to a section.
pub fn open_settings_at(s: AppState, section: &str) {
    crate::menubar::show_main();
    let mut o = s.settings_open;
    o.set(true);
    settings::show_page(s, section);
}

/// Check for Updates (menu, Settings): check now and show the outcome in
/// Coo's update dialog, which follows the check live.
pub fn check_for_updates(s: AppState) {
    if crate::updater::available() && !crate::updater::automatic() {
        // Manual updates: Sparkle's own windows walk through download and install.
        return crate::updater::check_now();
    }
    crate::menubar::show_main();
    crate::updater::check_user();
    let mut status = s.update_status;
    status.set(crate::updater::status());
    let mut open = s.update_dialog;
    open.set(true);
}

/// Install the downloaded update and relaunch into it.
pub fn restart_to_update() {
    if !crate::updater::install_now() {
        tracing::warn!("restart to update: no update is ready");
    }
}

/// Restore the main window's last position and size, and keep saving it
/// as it moves (AppKit's frame autosave, in Coo's defaults), so relaunches
/// and updates open it where it was.
#[cfg(target_os = "macos")]
fn remember_frame(w: &dioxus::desktop::DesktopContext) {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    let name = objc2_foundation::NSString::from_str("Coo main window");
    // SAFETY: tao's NSWindow for a live window, used on the main thread.
    if let Some(ns) = unsafe { (w.window.ns_window() as *const objc2_app_kit::NSWindow).as_ref() } {
        // Kept on a screen that's still attached.
        ns.setFrameUsingName(&name);
        ns.setFrameAutosaveName(&name);
    }
}

#[cfg(not(target_os = "macos"))]
fn remember_frame(_: &dioxus::desktop::DesktopContext) {}

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
                let mut status = s.update_status;
                status.set(next.clone());
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

/// Right-click › Save Image…: ask where, then the engine fetches and writes.
async fn save_image_loop() {
    let Some(mut rx) = crate::ctxmenu::take_actions() else { return };
    while let Some(url) = rx.recv().await {
        let name = pigeon_core::images::image_file_name(&url);
        let dialog = rfd::AsyncFileDialog::new().set_title("Save Image").set_file_name(&name);
        let dialog = match dirs_downloads() {
            Some(d) => dialog.set_directory(d),
            None => dialog,
        };
        if let Some(file) = dialog.save_file().await {
            send_cmd(Cmd::SaveImage { url, path: file.path().to_path_buf() });
        }
    }
}

/// ~/Downloads, where the save dialog starts.
pub(crate) fn dirs_downloads() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads")).filter(|p| p.is_dir())
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
    use crate::notify::{Click, MailNoteAction, Target};
    let Some(mut rx) = crate::notify::take_clicks() else { return };
    while let Some(click) = rx.recv().await {
        // Join opens the browser; Archive / Delete / Mark as Read stay in
        // the background.
        let background = match &click {
            Click::Join(_) => true,
            Click::Mail { action, .. } => *action != MailNoteAction::Reply,
            Click::Open(_) => false,
        };
        if !background {
            crate::menubar::show_main();
        }
        let (mut mode, mut settings, mut search) = (s.mode, s.settings_open, s.search);
        match click {
            Click::Join(url) => {
                let _ = open::that_detached(&url);
            }
            Click::Mail { account, thread, message, action: MailNoteAction::Reply } => {
                mode.set(Mode::Mail);
                settings.set(false);
                search.set(None);
                let mut sel = s.selected;
                sel.set(Some(format!("{account}|{thread}")));
                send_cmd(Cmd::OpenThread { account_id: account.clone(), thread_id: thread });
                send_cmd(Cmd::StartReply { account_id: account, message_id: message, kind: ReplyKind::Reply });
            }
            Click::Mail { account, message, action, .. } => {
                let (mail_action, label) = match action {
                    MailNoteAction::Archive => (MailAction::Archive, "Archived from the notification"),
                    MailNoteAction::Delete => (MailAction::Trash, "Deleted from the notification"),
                    _ => (MailAction::MarkRead, ""),
                };
                send_cmd(Cmd::Act {
                    account_id: account,
                    ids: vec![message],
                    thread_id: None,
                    action: mail_action,
                    label: label.into(),
                });
            }
            Click::Open(Target::Thread { account, thread, .. }) => {
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
    let mut eval = document::eval(&bridge::script());
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
            "ctx-img" => crate::ctxmenu::set_image(&msg.v),
            "attach-drag" => crate::dragout::start(&msg.v),
            "list-band" => multi::on_band(s, &msg.v),
            "shortcut-rec" => settings_sections::on_shortcut_recorded(&msg.v),
            "pane" => *LIST_FOCUS.write() = msg.v == "list",
            // A row pressed and moved: drag it (or the selection it's in).
            "mail-drag" => {
                if let Some(item) = s.visible_items().into_iter().find(|i| AppState::key(i) == msg.v) {
                    mail_drag::start(s, &item);
                    let files = s
                        .mail_drag
                        .peek()
                        .iter()
                        .flatten()
                        .map(|i| crate::dragout::MailFile {
                            account_id: i.account_id().to_string(),
                            message_id: i.header().id.clone(),
                            subject: i.header().subject.clone(),
                        })
                        .collect();
                    crate::dragout::start_mail(files);
                }
            }
            // Any drop inside Coo has taken what it needs by now.
            "mail-drag-end" => {
                let mut d = s.mail_drag;
                if d.peek().is_some() {
                    d.set(None);
                }
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
            "key" | "key-synth" => on_key(s, &msg.v),
            "search-caret" => search::on_caret(s, &msg.v),
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
    let (mut sel, mut multi, mut anchor) = (s.selected, s.multi, s.anchor);
    multi.set(Vec::new());
    anchor.set(Some(AppState::key(item)));
    sel.set(Some(AppState::key(item)));
    // A draft opens straight in the editor (docked or in its own window,
    // per Settings › Composing), not in the reader.
    if is_draft_row(&s, item) {
        let mut reader = s.reader;
        reader.set(None);
        send_cmd(Cmd::EditDraft { account_id: item.account_id().to_string(), message_id: item.header().id.clone() });
        return;
    }
    match item {
        ListItem::Thread(t) => {
            send_cmd(Cmd::OpenThread { account_id: t.account_id.clone(), thread_id: t.thread_id.clone() })
        }
        ListItem::Message(m) => {
            send_cmd(Cmd::OpenMessage { account_id: m.account_id.clone(), message_id: m.header.id.clone() })
        }
    }
}

/// A draft row: a draft message, or a conversation shown in Drafts (its
/// latest message is the draft). Elsewhere a conversation with a reply
/// draft opens in the reader, with the draft inline.
fn is_draft_row(s: &AppState, item: &ListItem) -> bool {
    match item {
        ListItem::Message(m) => m.header.is_draft,
        ListItem::Thread(t) => {
            t.latest.is_draft
                && s.accounts
                    .read()
                    .iter()
                    .find(|a| a.account.id == t.account_id)
                    .is_some_and(|a| viewing(&s.mailbox.read(), a, FolderRole::Drafts))
        }
    }
}

fn step(s: AppState, delta: isize) {
    let list = s.visible_items();
    if list.is_empty() {
        return;
    }
    // From a multi-selection, move on from the row clicked last.
    let current = s.selected.read().clone().or_else(|| s.anchor.read().clone().filter(|_| !s.multi.read().is_empty()));
    let idx = current.and_then(|k| list.iter().position(|i| AppState::key(i) == k));
    let next = match idx {
        Some(i) => (i as isize + delta).clamp(0, list.len() as isize - 1) as usize,
        None => 0,
    };
    select(s, &list[next]);
}

/// Run a mail action on a list item; advances the selection when the item
/// leaves the list.
/// `mailbox` is this account's `role` folder (or the unified one).
pub fn viewing(mailbox: &Mailbox, account: &AccountNav, role: FolderRole) -> bool {
    match mailbox {
        Mailbox::Unified(r) => *r == role,
        Mailbox::Folder { account_id, folder_id } => {
            *account_id == account.account.id
                && account.folders.iter().any(|f| f.folder.id == *folder_id && f.folder.role == Some(role))
        }
    }
}

/// Save messages as .eml files (the original message, as sent): one
/// asks where to save it; several, which folder. A conversation saves its
/// newest message.
pub fn save_eml(items: Vec<ListItem>) {
    let messages: Vec<(String, String, String)> =
        items.iter().map(|i| (i.account_id().to_string(), i.header().id.clone(), i.header().subject.clone())).collect();
    if messages.is_empty() {
        return;
    }
    // Outlives the right-click menu that asked (it closes right away).
    dioxus::core::spawn_forever(async move {
        let dialog = rfd::AsyncFileDialog::new().set_parent(&*dioxus::desktop::window().window);
        let dialog = match dirs_downloads() {
            Some(d) => dialog.set_directory(d),
            None => dialog,
        };
        let (dest, into_folder) = if let [(_, _, subject)] = messages.as_slice() {
            let picked = dialog
                .set_title("Save Message")
                .set_file_name(pigeon_engine::service::eml_name(subject))
                .add_filter("Email message", &["eml"])
                .save_file()
                .await;
            (picked, false)
        } else {
            (dialog.set_title("Save Messages").pick_folder().await, true)
        };
        if let Some(dest) = dest {
            send_cmd(Cmd::SaveMessages { messages, dest: dest.path().to_path_buf(), into_folder, done: None });
        }
    });
}

/// Viewing a Trash folder (one account's, or the unified one): its
/// accounts. Mail there only gets restored, moved or deleted for good.
pub fn trash_view(s: &AppState) -> Option<Vec<String>> {
    let accounts = s.accounts.read();
    match &*s.mailbox.read() {
        Mailbox::Unified(FolderRole::Trash) => Some(accounts.iter().map(|a| a.account.id.clone()).collect()),
        mb @ Mailbox::Folder { account_id, .. } => accounts
            .iter()
            .find(|a| &a.account.id == account_id)
            .filter(|a| viewing(mb, a, FolderRole::Trash))
            .map(|a| vec![a.account.id.clone()]),
        _ => None,
    }
}

/// Ask, then delete everything in these accounts' Trash for good.
pub fn empty_trash(account_ids: Vec<String>) {
    // Not tied to the right-click menu that may have asked (it closes).
    dioxus::core::spawn_forever(async move {
        let empty = "Empty Trash".to_string();
        let answer = rfd::AsyncMessageDialog::new()
            // A sheet on Coo's window: without a parent, macOS shows a system alert
            // that can open apart from (or behind) Coo.
            .set_parent(&*dioxus::desktop::window().window)
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Empty Trash?")
            .set_description("Everything in Trash is deleted for good. You can undo for a few seconds.")
            .set_buttons(rfd::MessageButtons::OkCancelCustom(empty.clone(), "Cancel".into()))
            .show()
            .await;
        if answer == rfd::MessageDialogResult::Custom(empty) {
            // Viewing that Trash: its rows sweep down into the bin first
            // (app.css `.list[data-sweep]`), then go for real.
            document::eval(SWEEP_JS);
            tokio::time::sleep(std::time::Duration::from_millis(SWEEP_MS)).await;
            send_cmd(Cmd::EmptyTrash { account_ids });
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            document::eval(
                "const l=document.querySelector('.pane--list .list[role=listbox]'); if(l) delete l.dataset.sweep;",
            );
        }
    });
}

/// How long the Empty Trash sweep runs before the rows go.
const SWEEP_MS: u64 = 560;
/// Stagger the Trash rows on screen and start their sweep (only while the
/// list shows a Trash: its Empty button is there).
const SWEEP_JS: &str = "(() => { const l = document.querySelector('.pane--list .list[role=listbox]'); \
    if (!l || !document.querySelector('.list-empty-trash')) return; \
    [...l.querySelectorAll('.row')].forEach((r, i) => { r.style.animationDelay = Math.min(i, 12) * 40 + 'ms'; }); \
    l.dataset.sweep = '1'; })()";

/// Whether Delete here deletes for good (viewing these accounts' Trash)
/// rather than moving to Trash.
pub fn delete_in_trash(s: &AppState, account_ids: &[&str]) -> bool {
    let accounts = s.accounts.read();
    let mailbox = s.mailbox.read();
    !account_ids.is_empty()
        && account_ids.iter().all(|id| {
            accounts.iter().find(|a| a.account.id == *id).is_some_and(|a| viewing(&mailbox, a, FolderRole::Trash))
        })
}

pub fn act(s: AppState, item: &ListItem, action: MailAction) {
    let header = item.header();
    let in_trash = action == MailAction::Trash && delete_in_trash(&s, &[item.account_id()]);
    if action.leaves() && s.selected.read().as_deref() == Some(AppState::key(item).as_str()) {
        let list = s.visible_items();
        let pos = list.iter().position(|i| AppState::key(i) == AppState::key(item));
        let go_next = crate::prefs::get().after_remove != "close";
        let next = pos
            .filter(|_| go_next)
            .and_then(|p| list.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| list.get(q))))
            .cloned();
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
        MailAction::Trash if in_trash => format!("Deleted “{subject}” for good"),
        MailAction::Trash => format!("Deleted “{subject}”"),
        MailAction::Flag => format!("Flagged “{subject}”"),
        MailAction::Unflag => format!("Unflagged “{subject}”"),
        MailAction::MoveTo(_) => format!("Moved “{subject}”"),
        MailAction::AddLabel(_) => format!("Labeled “{subject}”"),
        MailAction::Spam | MailAction::NotSpam | MailAction::Phishing => {
            spam::label_for(&s, std::slice::from_ref(item), &action, &format!("“{subject}”")).unwrap_or_default()
        }
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

/// A new message from the default account (Settings › Accounts; else the
/// top account in the sidebar).
/// Open the Report a Bug form.
pub fn report_bug(s: AppState) {
    let mut o = s.bug_report_open;
    o.set(true);
}

/// A new message, from the account whose folder is open (any of its
/// folders); from the all-accounts mailboxes (or Calendar), from the
/// default set in Settings.
pub fn new_message(s: AppState) {
    send_cmd(Cmd::NewDraft { from_account: contextual_from(&s) });
}

fn contextual_from(s: &AppState) -> Option<String> {
    let here = match &*s.mailbox.read() {
        Mailbox::Folder { account_id, .. } if *s.mode.read() == Mode::Mail => Some(account_id.clone()),
        _ => None,
    };
    here.filter(|id| s.accounts.read().iter().any(|a| &a.account.id == id)).or_else(|| s.default_from())
}

pub fn reply(item: &ListItem, kind: ReplyKind) {
    send_cmd(Cmd::StartReply { account_id: item.account_id().to_string(), message_id: item.header().id.clone(), kind });
}

/// The meeting joinable right now, if any: (title, url).
pub fn joinable_now(s: &AppState) -> Option<(String, String)> {
    let events = s.next_up_shown();
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
    // An open right-click menu takes every key, like a native one.
    if s.row_menu.read().is_some() {
        return row_menu::on_key(s, key);
    }
    if s.account_menu.read().is_some() {
        return account_menu::on_key(s, key);
    }
    // The update dialog's primary button looks focused; Enter presses it.
    if key == "Enter" {
        if *s.update_dialog.read() {
            update_dialog::confirm(s);
        }
        return;
    }
    let calendar = *s.mode.read() == Mode::Calendar;
    if let Some(what) = shortcuts::action(*shortcuts::STYLE.peek(), key, calendar) {
        run(s, what);
    }
}

/// Unread in the inboxes of `accounts`, all of them or only those whose
/// notifications aren't muted.
fn inbox_unread(s: &AppState, skip_muted: bool) -> u32 {
    let muted = if skip_muted { crate::prefs::get().notify_accounts_muted } else { Vec::new() };
    s.accounts
        .peek()
        .iter()
        .filter(|a| !muted.contains(&a.account.id))
        .flat_map(|a| a.folders.iter())
        .filter(|f| f.folder.role == Some(pigeon_core::model::FolderRole::Inbox))
        .map(|f| f.unread)
        .sum()
}

/// Set the Dock badge (muted accounts left out) and the menu bar count.
pub fn refresh_badge(s: AppState) {
    let badge = if crate::prefs::get().dock_badge { inbox_unread(&s, true) } else { 0 };
    crate::dock::set_badge(badge);
    crate::menubar::set_unread(inbox_unread(&s, false));
}

/// Whether the message list is the pane last clicked (bridge "pane"): ⌘F
/// then searches the folder even with a message open.
static LIST_FOCUS: GlobalSignal<bool> = Signal::global(|| false);

/// Undo the latest action that can still be undone (its notice may be
/// long gone).
pub fn undo_last(s: AppState) {
    let mut stack = s.undo_stack;
    let Some((account_id, handle)) = stack.write().pop() else {
        return s.local_toast("Nothing to undo", false);
    };
    let mut toasts = s.toasts;
    toasts.write().retain(|t| t.undo.as_ref().map(|u| u.1) != Some(handle));
    send_cmd(Cmd::Undo { account_id, handle });
}

/// Tell the engine how long an opened conversation waits to be marked read.
pub fn send_read_delay() {
    send_cmd(Cmd::SetReadDelay { ms: u64::from(crate::prefs::get().read_delay_secs) * 1000 });
}

/// A sync the user asked for: the footer says "Syncing…" until it's done.
pub fn sync_now(s: AppState) {
    let mut asked = s.sync_asked;
    asked.set(true);
    send_cmd(Cmd::SyncNow);
}

/// Run a shortcut's action (from its key or its menu item).
pub fn run(s: AppState, what: Do) {
    let (mut mode, draft, mut settings) = (s.mode, s.draft, s.settings_open);
    match what {
        // ⌘1 / ⌘2 also leave Settings, straight to Mail or Calendar.
        Do::Mail => {
            settings.set(false);
            return mode.set(Mode::Mail);
        }
        Do::Calendar => {
            settings.set(false);
            return mode.set(Mode::Calendar);
        }
        Do::Sync => return sync_now(s),
        // Join the meeting that's on now (or starting soon), from anywhere.
        Do::Join => return join_now(s),
        Do::Bigger => return zoom::change(s, zoom::Step::Bigger),
        Do::Smaller => return zoom::change(s, zoom::Step::Smaller),
        Do::ActualSize => return zoom::change(s, zoom::Step::Reset),
        Do::NewEvent => {
            mode.set(Mode::Calendar);
            return event_editor::open_new_now(s);
        }
        Do::Compose => return new_message(s),
        Do::Palette => {
            let mut p = s.palette_open;
            let v = *p.read();
            return p.set(!v);
        }
        // Open (not toggle): the menu's ⌘, accelerator may fire as well.
        Do::Settings => return settings.set(true),
        Do::Close => {
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
            if *s.update_dialog.read() {
                let mut o = s.update_dialog;
                return o.set(false);
            }
            if s.add_account.read().is_some() {
                let (mut o, mut si) = (s.add_account, s.signing_in);
                si.set(false);
                return o.set(None);
            }
            if *s.palette_open.read() {
                let mut p = s.palette_open;
                return p.set(false);
            }
            if s.editor.read().is_some() {
                let mut e = s.editor;
                return e.set(None);
            }
            if s.new_label.read().is_some() {
                let mut n = s.new_label;
                return n.set(None);
            }
            if s.find.read().is_some() {
                let mut f = s.find;
                return f.set(None);
            }
            if !composing && !s.multi.read().is_empty() {
                return multi::clear(s);
            }
            if !composing && *s.settings_open.read() {
                return settings.set(false);
            }
            if !composing && s.search.read().is_some() {
                return search::close(s);
            }
            if composing {
                compose::save_and_close(s.main_slot());
            }
            return;
        }
        Do::Send => {
            if !*s.docked_hidden.read() {
                compose::send_current(s, s.main_slot());
            }
            return;
        }
        Do::Undo => return undo_last(s),
        _ => {}
    }
    if *s.mode.read() != Mode::Mail {
        return;
    }
    match what {
        // A conversation open (and the list not the pane last clicked):
        // find in it; otherwise search the folder.
        Do::Find
            if s.reader.read().is_some()
                && !*LIST_FOCUS.peek()
                && multi::items(&s).is_empty()
                && !*s.settings_open.read() =>
        {
            let mut f = s.find;
            if f.read().is_none() {
                f.set(Some(String::new()));
            }
            document::eval(
                "(()=>{let n=0;const go=()=>{const e=document.getElementById('find-input');\
                 if(e){e.focus();e.select();}else if(n++<40)setTimeout(go,25);};go();})()",
            );
        }
        Do::Find | Do::Search => search::open_here(s),
        Do::SearchAll => search::focus_in(s, pigeon_engine::service::SearchScope::All),
        Do::Move => move_picker::open(s, false),
        Do::Label => move_picker::open(s, true),
        // "Not spam" in the spam folder.
        Do::Spam => spam::toggle(s),
        Do::Phishing => spam::report_phishing(s),
        // The selection, else the open message.
        Do::SaveEml => {
            let many = multi::items(&s);
            save_eml(if many.is_empty() { selected_item(&s).into_iter().collect() } else { many });
        }
        Do::OpenWindow => {
            if let Some(item) = selected_item(&s) {
                reader_window::open(s, item);
            }
        }
        Do::SelectAll => multi::select_all(s),
        Do::Next => step(s, 1),
        Do::Prev => step(s, -1),
        _ if !multi::items(&s).is_empty() => {
            let items = multi::items(&s);
            match what {
                Do::Archive => multi::act_many(s, &items, MailAction::Archive),
                Do::Delete => multi::act_many(s, &items, MailAction::Trash),
                Do::Flag => multi::act_many(s, &items, multi::toggle_flag(&items)),
                Do::MarkRead => multi::act_many(s, &items, MailAction::MarkRead),
                Do::MarkUnread => multi::act_many(s, &items, MailAction::MarkUnread),
                _ => {}
            }
        }
        _ => {
            let Some(item) = selected_item(&s) else { return };
            let h = item.header();
            match what {
                Do::Archive => act(s, &item, MailAction::Archive),
                Do::Delete => act(s, &item, MailAction::Trash),
                Do::Flag => act(s, &item, if h.is_flagged { MailAction::Unflag } else { MailAction::Flag }),
                Do::MarkRead => act(s, &item, MailAction::MarkRead),
                Do::MarkUnread => act(s, &item, MailAction::MarkUnread),
                Do::Reply => reply(&item, ReplyKind::Reply),
                Do::ReplyAll => reply(&item, ReplyKind::ReplyAll),
                Do::Forward => reply(&item, ReplyKind::Forward),
                Do::Reuse => reply(&item, ReplyKind::Reuse),
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
