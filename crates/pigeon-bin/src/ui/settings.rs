//! Settings: Appearance (mode, color theme, text size) and Accounts (order,
//! sign out, add). Opens with ⌘, or from ⌘K; while open it takes over the
//! sidebar (page list, search, Back). Esc, Back or ⌘1/⌘2 leave.

use dioxus::prelude::*;
use pigeon_core::compose::{is_icloud_mail, is_plain_address};
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::{AccountNav, Cmd};

use super::Icon;
use super::zoom::{self, Step};
use crate::send_cmd;
use crate::state::{AppState, Mode, Theme};
use crate::{prefs, themes};

/// Settings subpages: (title, icon, search keywords).
pub const PAGES: &[(&str, &str, &str)] = &[
    (
        "General",
        "gear",
        "close window menu bar quit startup archive delete next message after attachments files top bottom",
    ),
    ("Appearance", "sun", "theme dark light mode color colour text size font zoom list cards bands avatars density"),
    (
        "Accounts",
        "at",
        "account sign in sign out add microsoft google gmail icloud zoom video calls order nickname name rename default from sender alias aliases address",
    ),
    ("Composing", "edit", "undo send later reply new message window docked draft save autosave ask"),
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
    ("Keyboard shortcuts", "dash", "keys shortcut hotkey gmail outlook style"),
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
    let (mut list_style, mut list_avatars) = (s.list_style, s.list_avatars);
    let style_now = *list_style.read();
    let avatars_on = *list_avatars.read();

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
            div { class: "settings-field settings-field--top",
                span { class: "settings-field__label", "Message list" }
                div { class: "theme-grid list-style-grid", role: "radiogroup", "aria-label": "Message list style",
                    for st in crate::state::ListStyle::ALL {
                        button { class: "theme-card", r#type: "button", role: "radio", key: "{st.as_pref()}",
                            "aria-checked": "{style_now == st}",
                            onclick: move |_| {
                                list_style.set(st);
                                prefs::update(move |p| p.list_style = st.as_pref().to_string());
                            },
                            ListStylePreview { style: st, avatars: avatars_on }
                            div { class: "theme-card__name", "{st.label()}" }
                            div { class: "theme-card__variants", "{st.blurb()}" }
                        }
                    }
                }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Sender avatars" }
                input { class: "switch", r#type: "checkbox", checked: avatars_on,
                    onchange: move |_| {
                        let on = !avatars_on;
                        list_avatars.set(on);
                        prefs::update(move |p| p.list_avatars = on);
                    }
                }
                span { class: "settings-field__hint", "Initials, or the sender's photo when Privacy › Sender photos is on." }
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

/// A miniature message list in `style`: three placeholder rows (with
/// avatar dots when `avatars`), drawn with the real list colors.
#[component]
fn ListStylePreview(style: crate::state::ListStyle, avatars: bool) -> Element {
    let rows = if style == crate::state::ListStyle::Bands { 4 } else { 3 };
    rsx! {
        div { class: "lsp", "data-style": style.as_pref(), "data-avatars": avatars.then_some("true"), "aria-hidden": "true",
            span { class: "lsp__group" }
            div { class: "lsp__rows",
                for i in 0..rows {
                    div { class: "lsp__row", key: "{i}", "data-unread": (i == 0).then_some("true"),
                        if avatars { span { class: "lsp__avatar", "data-hue": "{i % 4 + 1}" } }
                        span { class: "lsp__lines",
                            span { class: "lsp__who" }
                            span { class: "lsp__text" }
                            if style != crate::state::ListStyle::Bands { span { class: "lsp__text lsp__text--short" } }
                        }
                    }
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
    let signing_in = s.signing_in;
    let sheet_open = s.add_account.read().is_some();

    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Accounts" }
        div { class: "settings__hint", "Click an account's name to give it a nickname. Drag accounts in the sidebar to reorder them; this list follows the same order. New messages come from the default account; right-click an account in the sidebar for more." }
        AccountList {}
        div { class: "account-detail__actions", style: "margin-top:4px",
            button { class: "btn btn--outline", r#type: "button",
                onclick: move |_| super::add_account::open(s, ""), Icon { name: "plus", class: "icon--sm" } "Add account…" }
            if *signing_in.read() && !sheet_open { span { class: "settings__hint", style: "margin:0;align-self:center", "Finish signing in in your browser…" } }
        }
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
    let provider = crate::state::provider_name(a.provider, &a.email);
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
        if a.provider == ProviderKind::ICloud && !confirming {
            ICloudSendFrom { account_id: a.id.clone(), email: a.email.clone(), main: nav.main_address.clone() }
        }
        if a.provider != ProviderKind::ICloud && !confirming {
            AliasList { account_id: a.id.clone(), email: a.email.clone(), main: nav.main_address.clone() }
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

/// Hint under an iCloud account's send-from list.
pub const ICLOUD_SEND_HINT: &str =
    "iCloud sends only from your iCloud Mail addresses and aliases, not from an Apple ID at another provider.";

/// Settings › Accounts, a Google or Microsoft account with aliases: its
/// send-from addresses, to pick the one it goes by.
#[component]
fn AliasList(account_id: String, email: String, main: Option<String>) -> Element {
    let s = use_context::<AppState>();
    use_hook(|| {
        if s.senders.read().is_empty() {
            send_cmd(Cmd::LoadSenders);
        }
    });
    let own = email.to_lowercase();
    let mut listed: Vec<String> = s
        .senders
        .read()
        .iter()
        .filter(|x| x.account_id == account_id)
        .map(|x| x.address.email.to_lowercase())
        .collect();
    // The account's own address first, as the provider lists them.
    listed.sort_by_key(|e| *e != own);
    if listed.len() < 2 {
        return rsx! {};
    }
    rsx! {
        div { class: "account-detail account-detail--plain icloud-send",
            div { class: "icloud-send__head",
                span { class: "settings-field__label", "Addresses" }
                span { class: "settings-field__hint", "{MAIN_ADDRESS_HINT}" }
            }
            div { class: "icloud-send__list",
                for addr in listed {
                    div { class: "icloud-send__row", key: "{addr}",
                        span { class: "spacer", "{addr}" }
                        MainPick { account_id: account_id.clone(), email: addr.clone(), own: own.clone(), main: main.clone() }
                    }
                }
            }
        }
    }
}

/// What a main address does (Settings › Accounts).
pub const MAIN_ADDRESS_HINT: &str = "The main address is shown in the sidebar, and new messages come from it.";

/// A send-from row's "Main" tag, or the button that makes it the address
/// the account goes by.
#[component]
fn MainPick(account_id: String, email: String, own: String, main: Option<String>) -> Element {
    let is_main = match &main {
        Some(m) => m.eq_ignore_ascii_case(&email),
        None => email == own,
    };
    if is_main {
        return rsx! {
            span { class: "tag", title: "Shown in the sidebar; new messages come from it", "Main" }
        };
    }
    rsx! {
        button { class: "btn btn--ghost btn--sm", r#type: "button", title: "Show this address in the sidebar and send new messages from it",
            "aria-label": "Make {email} the main address",
            onclick: move |_| send_cmd(Cmd::SetMainAddress {
                account_id: account_id.clone(),
                email: (email != own).then(|| email.clone()),
            }),
            "Make main"
        }
    }
}

/// Settings › Accounts, an iCloud account: the addresses it sends from
/// (found in its mail, or added here) with remove buttons and an add box.
/// The Apple ID itself is listed only when it's an iCloud address.
#[component]
fn ICloudSendFrom(account_id: String, email: String, main: Option<String>) -> Element {
    let s = use_context::<AppState>();
    // Fresh each time: detection follows what has synced since.
    use_hook(|| send_cmd(Cmd::LoadSenders));
    let mut draft = use_signal(String::new);
    let mut touched = use_signal(|| false);
    let own = email.to_lowercase();
    let foreign = !is_icloud_mail(&own);
    let listed: Vec<String> = s
        .senders
        .read()
        .iter()
        .filter(|x| x.account_id == account_id)
        .map(|x| x.address.email.to_lowercase())
        .filter(|e| !(foreign && *e == own))
        .collect();
    let typed = draft.read().trim().to_string();
    let invalid = *touched.read() && !typed.is_empty() && !is_plain_address(&typed);
    let (id_key, id_click, id_rm) = (account_id.clone(), account_id.clone(), account_id.clone());
    let mut add = move |id: &str| {
        touched.set(true);
        let v = draft.read().trim().to_string();
        if !is_plain_address(&v) {
            return;
        }
        send_cmd(Cmd::SetSendAddress { account_id: id.to_string(), email: v, offered: true });
        draft.set(String::new());
        touched.set(false);
    };
    rsx! {
        div { class: "account-detail account-detail--plain icloud-send",
            div { class: "icloud-send__head",
                span { class: "settings-field__label", "Send from" }
                span { class: "settings-field__hint", "{ICLOUD_SEND_HINT} {MAIN_ADDRESS_HINT}" }
            }
            if listed.is_empty() {
                span { class: "settings-field__hint icloud-send__warn",
                    "No iCloud Mail address found yet. Until you add one, messages from this account go out as {email}, which iCloud may refuse."
                }
            }
            div { class: "icloud-send__list",
                for addr in listed {
                    {
                        let removable = addr != own;
                        let (id, a) = (id_rm.clone(), addr.clone());
                        rsx! {
                            div { class: "icloud-send__row", key: "{addr}",
                                span { class: "spacer", "{addr}" }
                                MainPick { account_id: id_rm.clone(), email: addr.clone(), own: own.clone(), main: main.clone() }
                                if removable {
                                    button { class: "btn btn--ghost btn--sm", r#type: "button", title: "Stop offering this address",
                                        "aria-label": "Remove {addr}",
                                        onclick: move |_| send_cmd(Cmd::SetSendAddress { account_id: id.clone(), email: a.clone(), offered: false }),
                                        "Remove"
                                    }
                                } else {
                                    span { class: "settings-field__hint", style: "margin:0", "Apple Account" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "icloud-send__add",
                input { class: "input", r#type: "email", placeholder: "name@icloud.com", "aria-label": "Add a send-from address",
                    spellcheck: "false", value: "{draft}", "aria-invalid": "{invalid}",
                    oninput: move |e| draft.set(e.value()),
                    onkeydown: move |e| if e.key() == Key::Enter { e.prevent_default(); add(&id_key) },
                }
                button { class: "btn btn--secondary btn--sm", r#type: "button", disabled: typed.is_empty(), onclick: move |_| add(&id_click), "Add address" }
            }
            if invalid { span { class: "field__error", "Enter a full address, like name@icloud.com" } }
        }
    }
}
