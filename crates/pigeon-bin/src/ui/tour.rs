//! The welcome tour: a few questions that set what's otherwise easy to miss
//! on a first run (theme, list, threads, photos, keys, alerts, reading),
//! then a sheet of the bigger extras. It opens by itself once, after the
//! first accounts are added (onboarding's Continue), and again from the Coo
//! menu, Settings › General and ⌘K. Every choice applies at once, so the
//! app (and this sheet) wears it while you decide.
//!
//! The pigeon stands on the step track and hops to the next dot (leaning
//! back on Back, never turning round). Finishing every step, rather than
//! skipping, ends with it flying over the app to deliver the first letter
//! (`WelcomeIn`).

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::Icon;
use super::bird::{Bird, Pose};
use crate::prefs;
use crate::send_cmd;
use crate::state::{AppState, ListStyle, Theme};

/// Question steps (the track's dots); 0 is the welcome, 8 the finish.
const STEPS: u8 = 7;
/// How long the welcome-in fly-over runs (app.css `.welcome-in`).
const WELCOME_MS: u64 = 2800;

/// Show the tour (and remember it was seen).
pub fn open(s: AppState) {
    crate::menubar::show_main();
    let (mut o, mut settings) = (s.tour_open, s.settings_open);
    settings.set(false);
    o.set(true);
    prefs::update(|p| p.tour_seen = true);
}

/// The first run: once the first accounts are in, unless it was seen.
pub fn open_first_time(s: AppState) {
    if !prefs::get().tour_seen {
        open(s);
    }
}

#[component]
pub fn Tour() -> Element {
    let s = use_context::<AppState>();
    let mut step = use_signal(|| 0u8);
    let mut hops = use_signal(|| 0u32);
    let mut backward = use_signal(|| false);
    let mut finished = use_signal(|| false);
    // The control that just changed and a counter, so its motion replays.
    let mut fx = use_signal(|| ("", 0u32));
    // Prefs as of the last change here (re-read after each one).
    let mut p = use_signal(prefs::get);
    let banner_demo = use_signal(|| 0u32);
    let menubar_demo = use_signal(|| 0u32);

    let mut go = move |to: u8| {
        let from = *step.peek();
        backward.set(to < from);
        *hops.write() += 1;
        step.set(to);
    };
    let mut touched = move |key: &'static str| {
        let n = fx.peek().1 + 1;
        fx.set((key, n));
        p.set(prefs::get());
    };
    // A short demo (a banner, the menu bar) that clears itself.
    let flash = move |mut sig: Signal<u32>, ms: u64| {
        let id = *sig.peek() + 1;
        sig.set(id);
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            if *sig.peek() == id {
                sig.set(0);
            }
        });
    };
    let close = move |welcome: bool| {
        let mut o = s.tour_open;
        o.set(false);
        if welcome {
            let mut w = s.welcome_in;
            w.set(true);
            spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(WELCOME_MS)).await;
                w.set(false);
            });
        }
    };

    let st = *step.read();
    let pr = p.read().clone();
    let (fx_key, fx_n) = *fx.read();
    let ab = if fx_n % 2 == 1 { "a" } else { "b" };
    // An icon's pop (on) or dip (off) when its own control just changed.
    let ic = move |key: &str, on: bool| {
        if fx_key == key {
            format!("tour-ic tour-ic--{}-{ab}", if on { "pop" } else { "dip" })
        } else {
            "tour-ic".to_string()
        }
    };
    let q = st.clamp(1, STEPS);
    let pct = f32::from(q - 1) / f32::from(STEPS - 1) * 100.0;
    let hop = format!(
        "tour-bird__hop tour-bird__hop--{}-{}",
        if *backward.read() { "back" } else { "fwd" },
        *hops.read() % 2
    );
    let dark = s.dark();
    let newest = if pr.thread_newest_first { "top" } else { "bottom" };
    let after = if pr.after_remove == "close" { "close" } else { "next" };

    rsx! {
        div { class: "tour-scrim",
            div { class: "tour", role: "dialog", "aria-modal": "true", "aria-label": "Welcome tour",
                onkeydown: move |e| if e.key() == Key::Escape { close(false) },
                if *banner_demo.read() > 0 {
                    div { class: "tour-demo-banner", "aria-hidden": "true",
                        span { class: "tour-demo-banner__app", "C" }
                        div { class: "tour-demo-banner__text",
                            b { "Northwind Travel" }
                            span { "Your trip to Lisbon is confirmed" }
                        }
                        span { class: "tour-demo-banner__btn", "Archive" }
                    }
                }
                if *menubar_demo.read() > 0 {
                    div { class: "tour-demo-menubar", "aria-hidden": "true",
                        Bird { pose: Pose::Awake, style: "width:18px;height:16px" }
                        span { "3" }
                    }
                }
                if (1..=STEPS).contains(&st) {
                    div { class: "tour__head",
                        span { class: "tour__count", "{q} of {STEPS}" }
                        div { class: "tour-track", style: "--pct:{pct}%",
                            span { class: "tour-track__line" }
                            span { class: "tour-track__fill" }
                            for i in 1..=STEPS {
                                span { class: "tour-track__dot", key: "{i}", "data-on": (i <= q).then_some("true"),
                                    style: "left:{f32::from(i - 1) / f32::from(STEPS - 1) * 100.0}%" }
                            }
                            div { class: "tour-bird", "aria-hidden": "true",
                                div { class: "{hop}",
                                    // Flipped to face the way it walks; only this
                                    // static wrapper flips, never an animated one.
                                    div { class: "tour-bird__face", Bird { pose: Pose::Awake } }
                                }
                            }
                        }
                        button { class: "tour__skip", r#type: "button", onclick: move |_| go(8), "Skip setup" }
                    }
                }
                div { class: "tour__body", key: "{st}",
                    match st {
                        0 => rsx! {
                            div { class: "tour-hello",
                                div { class: "tour-hello__bird", div { class: "tour-bird__face", Bird { pose: Pose::Awake } } }
                                h1 { "Welcome to Coo" }
                                p { class: "tour__lede", "A few quick choices make Coo feel like yours: how it looks, how your mail is laid out, the keys you already know. About a minute; everything can change later in Settings." }
                                div { class: "tour__row",
                                    button { class: "btn btn--primary tour__btn", r#type: "button", onmounted: focus, onclick: move |_| go(1), "Let’s go" }
                                    button { class: "btn btn--secondary tour__btn", r#type: "button", onclick: move |_| go(8), "Use the defaults" }
                                }
                            }
                        },
                        1 => rsx! {
                            Heading { title: "How should Coo look?", sub: "Every theme has a light and a dark side. Coo wears your pick as you choose." }
                            Seg { label: "Appearance",
                                opts: vec![("system", "Match macOS"), ("light", "Light"), ("dark", "Dark")],
                                cur: s.theme.read().as_pref().to_string(),
                                on_pick: move |v: &'static str| { let mut t = s.theme; t.set(Theme::from_pref(v)); touched("mode"); } }
                            div { class: "tour-hc",
                                div { class: "tour-toggle__text", b { "High contrast" } span { class: "tour__hint", "Strongest text and edges, in any theme." } }
                                super::settings::ContrastSwitch {}
                            }
                            div { class: "theme-grid tour-themes", role: "radiogroup", "aria-label": "Color theme",
                                for (id, name) in crate::themes::choices() {
                                    super::settings::ThemeCard { key: "{id}", id, name, checked: *s.palette.read() == id, dark, hc: *s.high_contrast.read(),
                                        on_pick: move |id: &'static str| { let mut pal = s.palette; pal.set(id.to_string()); touched("theme"); } }
                                }
                            }
                        },
                        2 => rsx! {
                            Heading { title: "How should your inbox look?", sub: "Three ways to draw the list. Pick the one your eyes like; you can switch any time." }
                            div { class: "theme-grid list-style-grid", role: "radiogroup", "aria-label": "Message list style",
                                for ls in ListStyle::ALL {
                                    button { class: "theme-card", r#type: "button", role: "radio", key: "{ls.as_pref()}",
                                        "aria-checked": "{*s.list_style.read() == ls}",
                                        onclick: move |_| {
                                            let mut l = s.list_style;
                                            l.set(ls);
                                            prefs::update(move |p| p.list_style = ls.as_pref().to_string());
                                            touched("list");
                                        },
                                        super::settings::ListStylePreview { style: ls, avatars: *s.list_avatars.read(), lines: *s.list_lines.read() }
                                        div { class: "theme-card__name", "{ls.label()}" }
                                        div { class: "theme-card__variants", "{ls.blurb()}" }
                                    }
                                }
                            }
                            Choice { label: "Lines per message", hint: "Two puts the preview after the subject, so more mail fits.",
                                Seg { label: "Lines per message", opts: vec![("3", "3 lines"), ("2", "2 lines")],
                                    cur: s.list_lines.read().to_string(),
                                    on_pick: move |v: &'static str| {
                                        let n: u8 = if v == "2" { 2 } else { 3 };
                                        let mut l = s.list_lines;
                                        l.set(n);
                                        prefs::update(move |p| p.list_lines = n);
                                        touched("lines");
                                    } }
                            }
                        },
                        3 => rsx! {
                            Heading { title: "Conversations or single messages?", sub: "Threads keep a back-and-forth together in one row; Messages lists every email on its own. The switch above the list does the same." }
                            div { class: "tour-pair", role: "radiogroup", "aria-label": "Grouping",
                                for (on, label, sub) in [(true, "Threads", "A conversation is one row, with how many messages it holds"), (false, "Messages", "Every email is its own row")] {
                                    button { class: "tour-opt", r#type: "button", role: "radio", key: "{label}",
                                        "aria-checked": "{*s.threaded.read() == on}",
                                        onclick: move |_| {
                                            let mut t = s.threaded;
                                            t.set(on);
                                            prefs::update(move |p| p.threaded = on);
                                            touched("threads");
                                        },
                                        div { class: "tour-pv tour-pv--rows", "data-threads": on.then_some("true"), "aria-hidden": "true",
                                            for (i, n) in (if on { vec![4, 0, 2] } else { vec![0; 5] }).into_iter().enumerate() {
                                                div { class: "tour-pv__row", key: "{i}",
                                                    span { class: "tour-pv__bar", style: "width:{50 + (i % 3) * 12}px" }
                                                    if n > 0 { span { class: "tour-pv__count", "{n}" } }
                                                }
                                            }
                                        }
                                        b { "{label}" }
                                        span { class: "tour__hint", "{sub}" }
                                    }
                                }
                            }
                            Choice { label: "In a conversation, show the newest message", hint: "On top, or at the bottom like a chat.",
                                Seg { label: "Newest message", opts: vec![("top", "On top"), ("bottom", "At the bottom")],
                                    cur: newest.to_string(),
                                    on_pick: move |v: &'static str| { prefs::update(move |p| p.thread_newest_first = v == "top"); touched("newest"); } }
                            }
                        },
                        4 => rsx! {
                            Heading { title: "Faces next to names", sub: "Photos help you spot people at a glance. Each source is your choice; without one, Coo draws initials in the sender’s colour." }
                            div { class: "tour-split",
                                div { class: "tour-list",
                                    if has(&s, ProviderKind::Google) {
                                        Toggle { icon: "at", label: "Google contacts", hint: "Photos of people in your Google contacts. Coo asks Google first.",
                                            on: pr.photos_google, class: ic("google", pr.photos_google),
                                            on_flip: move |_| { photos_google(s, !prefs::get().photos_google); touched("google"); } }
                                    }
                                    if has(&s, ProviderKind::Microsoft) {
                                        Toggle { icon: "inbox", label: "Microsoft 365", hint: "Your Outlook contacts and coworkers.",
                                            on: pr.photos_microsoft, class: ic("microsoft", pr.photos_microsoft),
                                            on_flip: move |_| { photos_microsoft(s, !prefs::get().photos_microsoft); touched("microsoft"); } }
                                    }
                                    Toggle { icon: "globe", label: "Gravatar and company logos", hint: "Public photos and brand logos. Sends a scrambled form of each address, never the address.",
                                        on: pr.photos_gravatar, class: ic("gravatar", pr.photos_gravatar),
                                        on_flip: move |_| {
                                            let now = !prefs::get().photos_gravatar;
                                            prefs::update(move |x| x.photos_gravatar = now);
                                            let mut photos = s.photos;
                                            photos.write().retain(|_, uri| uri.is_some());
                                            touched("gravatar");
                                        } }
                                }
                                div { class: "tour-faces", "aria-hidden": "true",
                                    for (i, (initials, on)) in [("MC", pr.photos_google || pr.photos_microsoft), ("NT", pr.photos_gravatar), ("DO", pr.photos_microsoft || pr.photos_google), ("FW", pr.photos_gravatar)].into_iter().enumerate() {
                                        div { class: "tour-face", key: "{i}", "data-hue": "{i + 1}", "data-photo": on.then_some("true"), style: "--d:{i * 60}ms",
                                            span { class: "tour-face__av",
                                                span { class: "tour-face__initials", "{initials}" }
                                                svg { class: "tour-face__photo", view_box: "0 0 32 32",
                                                    circle { cx: "16", cy: "12.5", r: "5.2" }
                                                    path { d: "M5.5 29c1.6-5.6 5.6-8.6 10.5-8.6s8.9 3 10.5 8.6" }
                                                }
                                            }
                                            span { class: "tour-face__lines", span {} span {} }
                                        }
                                    }
                                }
                            }
                        },
                        5 => rsx! {
                            Heading { title: "Which keys do your fingers know?", sub: "Coo speaks both. You can change any single key later, and Coo warns you about clashes." }
                            div { class: "tour-pair", role: "radiogroup", "aria-label": "Shortcut style",
                                for (style, id, label, sub, keys) in [
                                    (crate::shortcuts::Style::Gmail, "gmail", "Single keys", "Like Gmail and most mail web apps.",
                                        [("R", "Reply"), ("A", "Reply all"), ("F", "Forward"), ("E", "Archive"), ("#", "Delete"), ("C", "New message")]),
                                    (crate::shortcuts::Style::Outlook, "outlook", "⌘ keys", "Like Outlook and Apple Mail on the Mac.",
                                        [("⌘R", "Reply"), ("⇧⌘R", "Reply all"), ("⌘J", "Forward"), ("⌃⌘A", "Archive"), ("⌘⌫", "Delete"), ("⌘N", "New message")]),
                                ] {
                                    button { class: "tour-opt", r#type: "button", role: "radio", key: "{id}",
                                        "aria-checked": "{pr.shortcut_style == id}",
                                        onclick: move |_| { crate::shortcuts::set_style(style); touched("keys"); },
                                        b { class: "tour-opt__big", "{label}" }
                                        span { class: "tour__hint", "{sub}" }
                                        div { class: if fx_key == "keys" && pr.shortcut_style == id { format!("tour-keys tour-keys--press-{ab}") } else { "tour-keys".to_string() },
                                            for (j, (k, what)) in keys.into_iter().enumerate() {
                                                div { class: "tour-keys__row", key: "{j}",
                                                    kbd { class: "kbd", style: "animation-delay:{j * 70}ms", "{k}" }
                                                    span { "{what}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        6 => rsx! {
                            Heading { title: "When new mail arrives", sub: "Coo can stay quietly in your menu bar, so mail keeps arriving with the window closed." }
                            div { class: "tour-list",
                                Toggle { icon: "bell", label: "Banners for new mail", hint: "With Reply and Archive right on the banner.",
                                    on: pr.notify_new_mail, class: ic("banners", pr.notify_new_mail),
                                    on_flip: move |_| {
                                        let now = !prefs::get().notify_new_mail;
                                        prefs::update(move |x| x.notify_new_mail = now);
                                        touched("banners");
                                        if now { flash(banner_demo, 2600); }
                                    } }
                                Toggle { icon: "menubar", label: "Keep Coo in the menu bar", hint: "Closing the window keeps Coo running; ⌘Q quits.",
                                    on: pr.close_to_menu_bar, class: ic("menubar", pr.close_to_menu_bar),
                                    on_flip: move |_| {
                                        let now = !prefs::get().close_to_menu_bar;
                                        prefs::update(move |x| x.close_to_menu_bar = now);
                                        touched("menubar");
                                        if now { flash(menubar_demo, 2400); }
                                    } }
                                div { class: "tour-toggle",
                                    span { class: ic("sound", pr.sound_theme != crate::sound::NONE) + if fx_key == "sound" && pr.sound_theme != crate::sound::NONE { " tour-ic--wave" } else { "" },
                                        svg { class: "icon", view_box: "0 0 24 24", "aria-hidden": "true", fill: "none", stroke: "currentColor", stroke_width: "1.7", stroke_linecap: "round", stroke_linejoin: "round",
                                            path { d: "M4 9v6h4l5 4V5L8 9H4z" }
                                            path { class: "tour-wave", d: "M16.5 9.5a3.5 3.5 0 0 1 0 5" }
                                            path { class: "tour-wave tour-wave--2", d: "M19 7a7 7 0 0 1 0 10" }
                                        }
                                    }
                                    div { class: "tour-toggle__text", b { "Sound" } span { class: "tour__hint", "Click one to hear it." } }
                                }
                                div { class: "chips tour-sounds", role: "radiogroup", "aria-label": "Sound",
                                    {
                                        let choices = crate::sound::choices().into_iter().chain(std::iter::once((crate::sound::NONE, "None")));
                                        rsx! {
                                            for (id, label) in choices {
                                                button { class: "chip", role: "radio", key: "{id}", r#type: "button",
                                                    "aria-checked": "{pr.sound_theme == id}",
                                                    onclick: move |_| {
                                                        prefs::update(move |x| x.sound_theme = id.to_string());
                                                        crate::sound::play(id, crate::sound::Kind::Mail);
                                                        touched("sound");
                                                    },
                                                    if id == crate::sound::MACOS { "macOS" } else { "{label}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        7 => rsx! {
                            Heading { title: "Reading and tidying up", sub: "How earlier replies show inside a message, and where you land after you archive or delete." }
                            div { class: "tour-pair", role: "radiogroup", "aria-label": "Quoted history",
                                for (id, label, sub) in [("cards", "Stacked cards", "Each earlier reply a shade deeper, with who said it"), ("rail", "Soft rail", "A thin line per level, closer to classic mail")] {
                                    button { class: "tour-opt", r#type: "button", role: "radio", key: "{id}",
                                        "aria-checked": "{pr.quote_style == id}",
                                        onclick: move |_| { prefs::update(move |p| p.quote_style = id.to_string()); touched("quotes"); },
                                        div { class: "tour-pv tour-pv--quote", "data-style": id, "aria-hidden": "true",
                                            span { class: "tour-pv__text" }
                                            div { class: if fx_key == "quotes" && pr.quote_style == id { format!("tour-q tour-q--in-{ab}") } else { "tour-q".to_string() },
                                                span {} span { class: "short" }
                                                div { class: "tour-q", span {} span { class: "short" } }
                                            }
                                        }
                                        b { "{label}" }
                                        span { class: "tour__hint", "{sub}" }
                                    }
                                }
                            }
                            Choice { label: "After archiving or deleting", hint: "Keep going through your mail, or step back to the list.",
                                Seg { label: "After archiving", opts: vec![("next", "Open the next one"), ("close", "Back to the list")],
                                    cur: after.to_string(),
                                    on_pick: move |v: &'static str| { prefs::update(move |p| p.after_remove = v.to_string()); touched("after"); } }
                            }
                        },
                        _ => rsx! {
                            div { class: "tour-done",
                                div { class: "tour-hello__bird tour-hello__bird--party", div { class: "tour-bird__face", Bird { pose: Pose::Awake } } }
                                h2 { if *finished.read() { "All set. Coo is yours." } else { "Coo is ready" } }
                                p { class: "tour__lede", "A few more things you can set up whenever you like. Coo is ready without them." }
                                div { class: "tour-extras",
                                    Extra { icon: "video", label: "Zoom", hint: "Add Zoom links to events you create, in one click.", cta: "Connect",
                                        on_go: move |_| { close(false); super::open_settings_at(s, "Calendar"); } }
                                    Extra { icon: "link", label: "AI agents", hint: "Let Claude and other assistants read, sort and draft your mail, with your say on what they may do.", cta: "Set up",
                                        on_go: move |_| { close(false); let mut a = s.agent_setup_open; a.set(true); } }
                                    Extra { icon: "edit", label: "Signatures", hint: "One per account or address, picked as you write.", cta: "Add",
                                        on_go: move |_| { close(false); super::open_settings_at(s, "Writing"); } }
                                    Extra { icon: "list", label: "Hover buttons", hint: "Choose the buttons a message shows under your pointer.", cta: "Choose",
                                        on_go: move |_| { close(false); super::open_settings_at(s, "Appearance"); } }
                                    Extra { icon: "keyboard", label: "Shortcuts", hint: "Change any key, with clash checks against macOS.", cta: "Customize",
                                        on_go: move |_| { close(false); super::open_settings_at(s, "Keyboard shortcuts"); } }
                                }
                                span { class: "tour__hint", "Want this tour again? Coo menu › Welcome Tour, or Settings › General." }
                            }
                        },
                    }
                }
                div { class: "tour__foot",
                    if (1..=STEPS).contains(&st) {
                        button { class: "tour__back", r#type: "button", onclick: move |_| go(st - 1), "Back" }
                    }
                    span { class: "spacer" }
                    if (1..=STEPS).contains(&st) {
                        button { class: "btn btn--primary tour__btn", r#type: "button",
                            onclick: move |_| {
                                if st == STEPS { finished.set(true); }
                                go(st + 1);
                            },
                            if st == STEPS { "Finish" } else { "Continue" }
                        }
                    }
                    if st > STEPS {
                        button { class: "btn btn--primary tour__btn", r#type: "button", onmounted: focus,
                            onclick: move |_| close(*finished.peek()), "Start using Coo" }
                    }
                }
            }
        }
    }
}

/// The welcome-in after a finished tour: the pigeon flies over the app and
/// drops a letter into the Inbox, whose count pops (app.css `.welcome-in`;
/// the list's rows drop in under `.app[data-welcome]`).
#[component]
pub fn WelcomeIn() -> Element {
    rsx! {
        div { class: "welcome-in", "aria-hidden": "true",
            onmounted: move |_| {
                // Aim the letter at the Inbox in the sidebar.
                document::eval(
                    "requestAnimationFrame(()=>{const t=document.querySelector('.sidebar .nav-item[aria-current=\"page\"]')||document.querySelector('.sidebar .nav-item');const l=document.querySelector('.welcome-in__letter');if(t&&l){const r=t.getBoundingClientRect();l.style.left=(r.right-44)+'px';l.style.top=(r.top+r.height/2-10)+'px';}})",
                );
            },
            div { class: "welcome-in__fly",
                div { class: "tour-bird__face", Bird { pose: Pose::Awake } }
            }
            div { class: "welcome-in__letter",
                svg { view_box: "0 0 26 20", width: "26", height: "20",
                    rect { x: "1", y: "1", width: "24", height: "18", rx: "3" }
                    path { d: "M2 3l11 8 11-8" }
                }
            }
        }
    }
}

async fn focus(e: MountedEvent) {
    let _ = e.set_focus(true).await;
}

fn has(s: &AppState, kind: ProviderKind) -> bool {
    s.accounts.read().iter().any(|a| a.account.provider == kind)
}

/// Settings › Appearance's Google photos switch: on asks the first Google
/// account that hasn't allowed contacts yet.
fn photos_google(s: AppState, on: bool) {
    prefs::update(move |p| p.photos_google = on);
    let mut photos = s.photos;
    photos.write().retain(|_, uri| uri.is_some());
    if on {
        let allowed = s.google_contacts.peek().clone();
        let first = s
            .accounts
            .peek()
            .iter()
            .find(|a| a.account.provider == ProviderKind::Google && !allowed.contains(&a.account.id))
            .map(|a| a.account.id.clone());
        if let Some(id) = first {
            send_cmd(Cmd::AllowGoogleContacts { account_id: id });
        }
    }
}

/// The same for Microsoft 365.
fn photos_microsoft(s: AppState, on: bool) {
    prefs::update(move |p| p.photos_microsoft = on);
    let mut photos = s.photos;
    photos.write().retain(|_, uri| uri.is_some());
    if on {
        let allowed = s.microsoft_photos.peek().clone();
        let first = s
            .accounts
            .peek()
            .iter()
            .find(|a| a.account.provider == ProviderKind::Microsoft && !allowed.contains(&a.account.id))
            .map(|a| a.account.id.clone());
        if let Some(id) = first {
            send_cmd(Cmd::AllowMicrosoftPhotos { account_id: id });
        }
    }
}

#[component]
fn Heading(title: &'static str, sub: &'static str) -> Element {
    rsx! {
        div { class: "tour__heading",
            h2 { "{title}" }
            p { "{sub}" }
        }
    }
}

/// A labelled row with its control on the right.
#[component]
fn Choice(label: &'static str, hint: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "tour-choice",
            div { class: "tour-toggle__text", b { "{label}" } span { class: "tour__hint", "{hint}" } }
            {children}
        }
    }
}

/// A segmented control whose pill slides under the chosen option.
#[component]
fn Seg(
    label: &'static str,
    opts: Vec<(&'static str, &'static str)>,
    cur: String,
    on_pick: EventHandler<&'static str>,
) -> Element {
    let i = opts.iter().position(|(v, _)| *v == cur).unwrap_or(0);
    let n = opts.len();
    rsx! {
        div { class: "tour-seg", role: "radiogroup", "aria-label": "{label}", style: "--n:{n};--i:{i}",
            span { class: "tour-seg__pill" }
            for (v, text) in opts {
                button { r#type: "button", role: "radio", key: "{v}", "aria-checked": "{v == cur}",
                    onclick: move |_| on_pick.call(v), "{text}" }
            }
        }
    }
}

#[component]
fn Toggle(
    icon: &'static str,
    label: &'static str,
    hint: &'static str,
    on: bool,
    class: String,
    on_flip: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "tour-toggle",
            span { class: "{class}", Icon { name: icon, class: "icon--sm" } }
            div { class: "tour-toggle__text", b { "{label}" } span { class: "tour__hint", "{hint}" } }
            input { class: "switch", r#type: "checkbox", checked: on, "aria-label": "{label}", onchange: move |_| on_flip.call(()) }
        }
    }
}

#[component]
fn Extra(
    icon: &'static str,
    label: &'static str,
    hint: &'static str,
    cta: &'static str,
    on_go: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "tour-toggle",
            span { class: "tour-ic", Icon { name: icon, class: "icon--sm" } }
            div { class: "tour-toggle__text", b { "{label}" } span { class: "tour__hint", "{hint}" } }
            button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| on_go.call(()), "{cta}" }
        }
    }
}
