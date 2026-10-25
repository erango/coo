//! Settings: Appearance (mode, color theme, text size) and Accounts (order,
//! sign out, add). Opens with ⌘, or from ⌘K; while open it takes over the
//! sidebar (page list, search, Back). Esc, Back or ⌘1/⌘2 leave.

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::{AccountNav, Cmd};

use super::Icon;
use super::zoom::{self, Step};
use crate::send_cmd;
use crate::state::{AppState, Mode, Theme};
use crate::{prefs, themes};

/// Settings subpages: (title, icon, search keywords).
pub const PAGES: &[(&str, &str, &str)] = &[
    ("General", "gear", "close window menu bar quit startup archive delete next message after"),
    ("Appearance", "sun", "theme dark light mode color colour text size font zoom"),
    (
        "Accounts",
        "at",
        "account sign in sign out add microsoft google gmail icloud zoom video calls order nickname name rename default from sender",
    ),
    ("Composing", "edit", "undo send later reply new message window docked"),
    ("Signatures", "text", "signature signatures sign-off footer html"),
    (
        "Calendar",
        "cal",
        "week start sunday monday weekend video call meet google zoom meeting link location default calendar new events",
    ),
    (
        "Notifications",
        "bell",
        "sound sounds alert banner reminder meeting focus do not disturb mute volume dock badge unread count icon",
    ),
    ("Privacy", "lock", "remote images tracking crash reports sentry photos gravatar"),
    ("Keyboard shortcuts", "dash", "keys shortcut hotkey"),
    ("AI agents", "link", "agent claude mcp assistant"),
    ("About & updates", "question", "version update check about"),
];

/// Pages whose title or keywords match `query` (all pages when empty).
pub fn matching_pages(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_lowercase();
    PAGES
        .iter()
        .filter(|(title, _, kw)| {
            q.is_empty()
                || title.to_lowercase().contains(&q)
                || kw.split(' ').any(|w| w.starts_with(&q))
                || kw.contains(&q)
        })
        .map(|(title, _, _)| *title)
        .collect()
}

/// Settings › Accounts with one account's row scrolled into view, briefly
/// highlighted, its nickname field focused (sidebar account menu).
pub fn show_account(s: AppState, account_id: &str) {
    super::open_settings_at(s, "Accounts");
    let id = serde_json::to_string(account_id).unwrap_or_default();
    // The page renders after this returns: retry for a moment.
    document::eval(&format!(
        "(function f(n){{const r=document.querySelector('[data-account-row='+CSS.escape({id})+']');\
           if(!r){{ if(n>0) setTimeout(()=>f(n-1),50); return; }}\
           r.scrollIntoView({{block:'center'}});\
           const i=r.querySelector('.account-row__nick'); if(i) i.focus({{preventScroll:true}});\
           r.setAttribute('data-flash',''); setTimeout(()=>r.removeAttribute('data-flash'),1200);\
         }})(20)"
    ));
}

/// Switch Settings to a page (unknown titles fall back to General).
pub fn show_page(s: AppState, title: &str) {
    let page = PAGES.iter().find(|(t, _, _)| t.eq_ignore_ascii_case(title)).map_or("General", |(t, _, _)| *t);
    let mut p = s.settings_page;
    p.set(page.to_string());
}

/// The sidebar while Settings is open: it takes over the mail/calendar
/// sidebar, so nothing on screen reads as live mail. Back (or Esc, ⌘1/⌘2)
/// leaves; the page list sits where the folders were.
#[component]
pub fn SettingsSidebar() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.settings_open;
    let mut page = s.settings_page;
    let mut query = use_signal(String::new);
    let current = page.read().clone();
    let shown = matching_pages(&query.read());
    let back = match *s.mode.read() {
        Mode::Mail => "Back to Mail",
        Mode::Calendar => "Back to Calendar",
    };
    rsx! {
        div { class: "sidebar__controls settings-side",
            button { class: "settings-back", r#type: "button", title: "{back} (Esc)", onclick: move |_| open.set(false),
                Icon { name: "arrow-left", class: "icon--sm" }
                span { class: "spacer", "{back}" }
                kbd { class: "kbd", "Esc" }
            }
            h1 { class: "settings-side__title", "Settings" }
            label { class: "settings-nav__search",
                Icon { name: "search", class: "icon--sm" }
                input { r#type: "search", placeholder: "Find a setting", "aria-label": "Find a setting", value: "{query}", autocomplete: "off",
                    oninput: move |e| {
                        let v = e.value();
                        // Jump to the first page that has it.
                        if let Some(first) = matching_pages(&v).first() { page.set((*first).to_string()); }
                        query.set(v);
                    }
                }
            }
        }
        nav { class: "sidebar__nav settings-nav", "aria-label": "Settings pages",
            for (title, icon, _) in PAGES.iter().filter(|(t, _, _)| shown.contains(t)) {
                a { class: "nav-item", href: "#", key: "{title}",
                    "aria-current": if current == *title { "page" } else { "false" },
                    onclick: move |e| { e.prevent_default(); page.set(title.to_string()); },
                    Icon { name: icon }
                    span { class: "nav-item__label", "{title}" }
                }
            }
            if shown.is_empty() {
                div { class: "settings__hint", style: "margin:8px 10px", "No settings match." }
            }
        }
    }
}

/// The open Settings page, across the full width beside the sidebar.
#[component]
pub fn Settings() -> Element {
    let s = use_context::<AppState>();
    let current = s.settings_page.read().clone();
    rsx! {
        main { class: "pane pane--reader",
            div { class: "settings",
                // Empty title strip: drags the window, hairline once scrolled.
                div { class: "settings__crumbs" }
                match current.as_str() {
                    "Appearance" => rsx! { Appearance {} },
                    "Accounts" => rsx! { Accounts {} },
                    "Composing" => rsx! { super::settings_sections::Composing {} },
                    "Signatures" => rsx! { super::settings_sections::Signatures {} },
                    "Calendar" => rsx! { super::settings_sections::CalendarSettings {} },
                    "Notifications" => rsx! { super::settings_sections::Notifications {} },
                    "Privacy" => rsx! { super::settings_sections::Privacy {} },
                    "Keyboard shortcuts" => rsx! { super::settings_sections::Shortcuts {} },
                    "About & updates" => rsx! { super::settings_sections::AboutUpdates {} },
                    "AI agents" => rsx! {
                        div { class: "settings__title", "AI agents" }
                        div { class: "settings__hint", "Let Claude or another agent search and read your mail and calendar through Coo." }
                        div {
                            button { class: "btn btn--secondary", r#type: "button",
                                onclick: move |_| { let mut o = s.agent_setup_open; o.set(true); },
                                Icon { name: "link", class: "icon--sm" }
                                "Connect an AI agent…"
                            }
                        }
                    },
                    _ => rsx! { super::settings_sections::General {} },
                }
            }
        }
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;

    #[test]
    fn search_finds_the_right_page() {
        assert_eq!(matching_pages("sound").first(), Some(&"Notifications"));
        assert_eq!(matching_pages("signature").first(), Some(&"Signatures"));
        assert_eq!(matching_pages("week").first(), Some(&"Calendar"));
        assert_eq!(matching_pages("dark").first(), Some(&"Appearance"));
        assert_eq!(matching_pages("default").first(), Some(&"Accounts"));
        assert_eq!(matching_pages("from").first(), Some(&"Accounts"));
        assert_eq!(matching_pages("").len(), PAGES.len());
        assert!(matching_pages("zzz").is_empty());
    }
}

#[component]
fn Appearance() -> Element {
    let s = use_context::<AppState>();
    let mut theme = s.theme;
    let mut palette = s.palette;
    let mut scale = use_signal(|| prefs::get().font_scale);
    let current = *theme.read();
    let selected = palette.read().clone();
    let modes = [(Theme::System, "System"), (Theme::Light, "Light"), (Theme::Dark, "Dark")];
    let mut step = move |st: Step| {
        zoom::change(s, st);
        scale.set(prefs::get().font_scale);
    };
    let pct = (*scale.read() * 100.0).round();

    rsx! {
        div { class: "settings__title", "Appearance" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Mode" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Appearance mode",
                    for (t, label) in modes {
                        button { role: "radio", "aria-selected": "{current == t}", "aria-checked": "{current == t}",
                            onclick: move |_| theme.set(t), "{label}" }
                    }
                }
                span { class: "settings-field__hint", "System follows macOS appearance." }
            }
            div { class: "settings-field settings-field--top",
                span { class: "settings-field__label", "Theme" }
                div { class: "theme-grid", role: "radiogroup", "aria-label": "Color theme",
                    for (id, name) in themes::choices() {
                        ThemeCard { key: "{id}", id, name, checked: selected == id,
                            on_pick: move |id: &'static str| palette.set(id.to_string()) }
                    }
                }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "Text size" }
                div { class: "text-size",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Smaller text (⌘-)", onclick: move |_| step(Step::Smaller), "−" }
                    span { class: "text-size__value", "{pct}%" }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Bigger text (⌘+)", onclick: move |_| step(Step::Bigger), "+" }
                    button { class: "btn btn--ghost btn--sm", r#type: "button", disabled: pct == 100.0,
                        onclick: move |_| step(Step::Reset), "Reset" kbd { class: "kbd", "⌘0" } }
                }
            }
        }
    }
}

#[component]
fn ThemeCard(id: &'static str, name: &'static str, checked: bool, on_pick: EventHandler<&'static str>) -> Element {
    let variants = themes::FAMILIES.iter().find(|f| f.id == id).map(|f| f.variants).unwrap_or(("Dark", "Light"));
    let half = |dark: bool| {
        let (bg, raised, accent, hues) = themes::preview(id, dark);
        rsx! {
            div { class: "theme-card__half", style: "background:{bg}",
                div { class: "theme-card__bar", style: "background:{raised}",
                    span { class: "theme-card__accent", style: "background:{accent}" }
                }
                div { class: "theme-card__dots",
                    for h in hues { span { style: "background:{h}" } }
                }
            }
        }
    };
    rsx! {
        button { class: "theme-card", r#type: "button", role: "radio", "aria-checked": "{checked}",
            title: "{name}: {variants.1} / {variants.0}",
            onclick: move |_| on_pick.call(id),
            div { class: "theme-card__preview", {half(false)} {half(true)} }
            div { class: "theme-card__name", "{name}" }
            div { class: "theme-card__variants", "{variants.1} · {variants.0}" }
        }
    }
}

#[component]
fn Accounts() -> Element {
    let s = use_context::<AppState>();
    let mut signing_in = s.signing_in;
    let login = move |p: ProviderKind| {
        signing_in.set(true);
        send_cmd(Cmd::Login(p));
    };
    let (mut l1, mut l2) = (login, login);
    let mut icloud = s.icloud_form;
    // Close the iCloud panel once its account shows up (sign-in succeeded);
    // debug builds can open it once accounts have loaded, like a real click.
    let mut seen = use_signal(|| None::<usize>);
    use_effect(move || {
        let n = s.accounts.read().len();
        let prev = *seen.peek();
        if prev.is_some_and(|p| n > p) && *icloud.peek() {
            icloud.set(false);
        }
        if prev.is_none() && n > 0 && super::icloud::debug_open("settings:icloud") {
            icloud.set(true);
        }
        if n > 0 || prev.is_some() {
            seen.set(Some(n));
        }
    });

    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Accounts" }
        div { class: "settings__hint", "Click an account's name to give it a nickname. Drag accounts in the sidebar to reorder them; this list follows the same order. New messages come from the default account; right-click an account in the sidebar for more." }
        AccountList {}
        div { class: "account-detail__actions", style: "margin-top:4px",
            button { class: "btn btn--secondary", r#type: "button", disabled: *signing_in.read(),
                onclick: move |_| l1(ProviderKind::Microsoft), Icon { name: "building", class: "icon--sm" } "Add Microsoft 365" }
            button { class: "btn btn--secondary", r#type: "button", disabled: *signing_in.read(),
                onclick: move |_| l2(ProviderKind::Google), Icon { name: "at", class: "icon--sm" } "Add Google" }
            button { class: "btn btn--secondary", r#type: "button", disabled: *signing_in.read(),
                onclick: move |_| icloud.set(true), Icon { name: "cloud", class: "icon--sm" } "Add iCloud" }
            if *signing_in.read() && !*icloud.read() { span { class: "settings__hint", style: "margin:0;align-self:center", "Finish signing in in your browser…" } }
        }
        ICloudPanel { open: icloud }
        if !s.accounts.read().is_empty() {
            div { class: "settings-group", style: "margin-top:16px", DefaultFromField {} }
        }
        VideoCalls {}
    }
}

/// "Send from": the default account for new messages, or
/// Automatic (the top account in the sidebar). Replies and forwards keep
/// coming from the account the mail came to; the composer's From picker
/// changes one message only. Also on Settings › Composing.
#[component]
pub fn DefaultFromField() -> Element {
    let s = use_context::<AppState>();
    let accounts = s.accounts.read().clone();
    let chosen = s.chosen_default().unwrap_or_default();
    let top = accounts.first().map(|a| s.account_label(&a.account.id, &a.account.email)).unwrap_or_default();
    rsx! {
        div { class: "settings-field",
            span { class: "settings-field__label", title: "The account new messages come from", "Send from" }
            select { class: "select default-from", "aria-label": "Default account for new messages",
                onchange: move |e| {
                    let v = e.value();
                    s.set_default_account((!v.is_empty()).then_some(v.as_str()));
                },
                option { value: "", selected: chosen.is_empty(), "Automatic: {top}" }
                for a in accounts.iter() {
                    option { key: "{a.account.id}", value: "{a.account.id}", selected: chosen == a.account.id,
                        "{s.account_label(&a.account.id, &a.account.email)}" }
                }
            }
            span { class: "settings-field__hint", "For new messages; Automatic is the top account in the sidebar. Replies come from the account the message was sent to." }
        }
    }
}

/// Settings › Accounts › Video calls: the connected Zoom account.
#[component]
fn VideoCalls() -> Element {
    let s = use_context::<AppState>();
    let zoom = s.zoom.read().clone();
    let mut connecting = use_signal(|| false);
    // A reply (connected or a failure toast) ends the wait.
    use_effect(move || {
        let _ = s.zoom.read();
        connecting.set(false);
    });
    let available = pigeon_core::auth::zoom_client_id().is_some();
    rsx! {
        div { class: "settings__title", style: "margin-top:20px", "Video calls" }
        div { class: "settings__hint", "Add Zoom meetings to events on any of your calendars." }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Zoom" }
                match zoom {
                    Some(z) => rsx! {
                        div { style: "display:flex;align-items:center;gap:10px;flex:1;min-width:0",
                            Icon { name: "video", class: "icon--sm" }
                            span { dir: "auto", style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap",
                                "{z.display_name} ", span { class: "settings-field__hint", style: "margin:0", "{z.email}" } }
                        }
                        button { class: "btn btn--ghost btn--sm", r#type: "button",
                            onclick: move |_| send_cmd(Cmd::ZoomDisconnect), "Disconnect" }
                    },
                    None => rsx! {
                        button { class: "btn btn--secondary btn--sm", r#type: "button",
                            disabled: *connecting.read() || !available,
                            onclick: move |_| { connecting.set(true); send_cmd(Cmd::ZoomConnect); },
                            Icon { name: "video", class: "icon--sm" } "Connect Zoom" }
                        span { class: "settings-field__hint", style: "margin:0",
                            if !available { "Zoom isn't set up in this build." }
                            else if *connecting.read() { "Finish signing in to Zoom in your browser…" }
                            else { "While Coo's Zoom app is in development, only accounts in its owner's Zoom organization can connect." }
                        }
                    },
                }
            }
        }
    }
}

/// The inline iCloud sign-in under the account list.
#[component]
fn ICloudPanel(open: Signal<bool>) -> Element {
    let mut open = open;
    if !*open.read() {
        return rsx! {};
    }
    rsx! {
        div { style: "margin:12px 0", super::icloud::ICloudForm { on_cancel: move |_| open.set(false) } }
    }
}

/// Its own component so the rows diff independently of the add-account
/// controls below (a list that fills in after mount next to a conditional
/// sibling otherwise lost its rows).
#[component]
fn AccountList() -> Element {
    let s = use_context::<AppState>();
    let accounts = s.accounts.read().clone();
    let mut confirm = use_signal(|| None::<String>);
    let needs_sign_in = s.sync.read().needs_sign_in.clone();
    rsx! {
        div { class: "account-list",
            for a in accounts.iter() {
                AccountRow {
                    key: "{a.account.id}",
                    nav: a.clone(),
                    auth: needs_sign_in.contains(&a.account.id),
                    confirming: confirm.read().as_deref() == Some(a.account.id.as_str()),
                    on_ask: move |id: String| confirm.set(Some(id)),
                    on_cancel: move |_| confirm.set(None),
                }
            }
        }
    }
}

#[component]
fn AccountRow(
    nav: AccountNav,
    auth: bool,
    confirming: bool,
    on_ask: EventHandler<String>,
    on_cancel: EventHandler<()>,
) -> Element {
    let a = nav.account.clone();
    let provider = match a.provider {
        ProviderKind::Microsoft => "Microsoft 365",
        ProviderKind::Google if a.email.to_lowercase().ends_with("@gmail.com") => "Gmail",
        ProviderKind::Google => "Google Workspace",
        ProviderKind::ICloud => "iCloud",
    };
    let s = use_context::<AppState>();
    let nickname = s.nicknames.read().get(&a.id).cloned().unwrap_or_default();
    let (id1, id2, id3, id4) = (a.id.clone(), a.id.clone(), a.id.clone(), a.id.clone());
    let is_default = s.default_from().as_deref() == Some(a.id.as_str());
    let automatic = s.chosen_default().is_none();
    rsx! {
        div { class: "account-row", "data-hue": "{nav.hue}", "aria-selected": "{confirming}", "data-account-row": "{a.id}",
            span { class: "account-row__grip", Icon { name: "grip", class: "icon--sm" } }
            span { class: "dot" }
            div { class: "account-row__text",
                input { class: "account-row__nick", r#type: "text", dir: "auto", value: "{nickname}",
                    placeholder: "Add a nickname", "aria-label": "Nickname for {a.email}", title: "Nickname shown in the sidebar and elsewhere",
                    spellcheck: "false", maxlength: "40",
                    onchange: move |e| s.set_nickname(&id3, &e.value()),
                }
                span { class: "account-row__addr", "{a.email} · {provider}" }
            }
            span { class: "account-row__status", "data-state": if auth { "auth" } else { "ok" },
                if auth { Icon { name: "lock", class: "icon--xs" } "Sign in again" } else { "Connected" }
            }
            span { class: "account-row__default",
                if is_default {
                    span { class: "tag",
                        title: if automatic { "New messages come from here: it's the top account and no default is chosen" } else { "New messages come from this account" },
                        "Default"
                    }
                } else {
                    button { class: "btn btn--ghost btn--sm", r#type: "button", title: "Send new messages from this account",
                        onclick: move |_| s.set_default_account(Some(&id4)), "Make default" }
                }
            }
            button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| on_ask.call(id1.clone()), "Sign out" }
        }
        if confirming {
            div { class: "account-detail",
                div { class: "account-detail__msg",
                    "Sign out of " b { "{a.email}" } "? Its mail, calendars and drafts are removed from this Mac. Nothing is deleted on the server."
                }
                div { class: "account-detail__actions",
                    button { class: "btn btn--danger", r#type: "button",
                        onclick: move |_| {
                            send_cmd(Cmd::Logout { account_id: id2.clone() });
                            on_cancel.call(());
                        },
                        "Sign out"
                    }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| on_cancel.call(()), "Cancel" }
                }
            }
        }
    }
}
