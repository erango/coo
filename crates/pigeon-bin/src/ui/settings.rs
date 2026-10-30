//! Settings: nine pages (General, Appearance, Accounts & agents, Reading,
//! Writing, Calendar, Notifications, Privacy, Keyboard shortcuts), each a
//! title over groups of rows (`Group`, `Row`). Opens with ⌘, or from ⌘K;
//! while open it takes over the sidebar (page list, search, Back). Esc,
//! Back or ⌘1/⌘2 leave.

use dioxus::prelude::*;
use pigeon_core::compose::{is_icloud_mail, is_plain_address};
use pigeon_core::model::ProviderKind;
use pigeon_engine::agent_setup::{self as agents, Agent, Status};
use pigeon_engine::service::{AccountNav, Cmd};

use super::Icon;
use super::zoom::{self, Step};
use crate::send_cmd;
use crate::state::{AppState, Mode, Theme};
use crate::{prefs, themes};

/// Settings subpages: (title, icon, search keywords). Keywords keep the
/// old page names (Composing, Signatures, AI agents, About) findable.
pub const PAGES: &[(&str, &str, &str)] = &[
    (
        "General",
        "gear",
        "window close menu bar quit startup update updates version check automatic about help report bug",
    ),
    (
        "Appearance",
        "sun",
        "theme themes dark light mode color colour palette text size font zoom list cards bands classic style avatars hover actions buttons row toolbar sender photos google microsoft outlook contacts gravatar logos",
    ),
    (
        "Accounts & agents",
        "at",
        "account accounts sign in sign out add microsoft google gmail icloud order nickname name rename alias aliases address main ai agent agents claude mcp assistant connect",
    ),
    (
        "Reading",
        "mailopen",
        "read unread mark seconds delay archive delete next message after move attachments files top bottom open",
    ),
    (
        "Writing",
        "edit",
        "composing compose new message reply replies window docked draft drafts save autosave ask undo send default from sender signature signatures sign-off footer",
    ),
    (
        "Calendar",
        "cal",
        "week start sunday monday saturday video call calls meet google zoom meeting link location default calendar new events",
    ),
    (
        "Notifications",
        "bell",
        "notify sound sounds alert banner reminder reminders meeting focus do not disturb mute muted volume dock badge unread count icon test",
    ),
    ("Privacy", "lock", "remote images tracking trusted senders crash reports sentry"),
    ("Keyboard shortcuts", "dash", "keys shortcut shortcuts hotkey gmail outlook style custom"),
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

/// Settings › Accounts & agents with one account's row scrolled into view,
/// briefly highlighted, its nickname field focused (sidebar account menu).
pub fn show_account(s: AppState, account_id: &str) {
    super::open_settings_at(s, "Accounts & agents");
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

/// The page a title names, including the pages that were merged away
/// (callers and debug hooks still use them); unknown titles are General.
pub fn page_for(title: &str) -> &'static str {
    let merged = match title.to_lowercase().as_str() {
        "accounts" | "ai agents" => Some("Accounts & agents"),
        "composing" | "signatures" => Some("Writing"),
        "about & updates" | "about" => Some("General"),
        _ => None,
    };
    merged
        .or_else(|| PAGES.iter().find(|(t, _, _)| t.eq_ignore_ascii_case(title)).map(|(t, _, _)| *t))
        .unwrap_or("General")
}

/// Switch Settings to a page.
pub fn show_page(s: AppState, title: &str) {
    let mut p = s.settings_page;
    p.set(page_for(title).to_string());
}

/// A group of settings: a small caption over its rows, in one rounded box.
/// `perch` seats the Settings pigeon on the box's top edge, at the right.
#[component]
pub fn Group(#[props(into)] title: String, #[props(default)] perch: bool, children: Element) -> Element {
    rsx! {
        section { class: "sgroup",
            if perch {
                div { class: "sgroup__head",
                    h2 { class: "sgroup__title", "{title}" }
                    super::settings_bird::Perch {}
                }
            } else if !title.is_empty() {
                h2 { class: "sgroup__title", "{title}" }
            }
            div { class: "sgroup__box", {children} }
        }
    }
}

/// One setting: its name and a one-line hint on the left, the control on
/// the right; `stack` puts the control below, across the row.
#[component]
pub fn Row(
    #[props(into)] label: String,
    #[props(into, default)] hint: String,
    #[props(default)] stack: bool,
    children: Element,
) -> Element {
    rsx! {
        div { class: "srow", "data-stack": stack.then_some("true"),
            div { class: "srow__text",
                span { class: "srow__label", "{label}" }
                if !hint.is_empty() { span { class: "srow__hint", "{hint}" } }
            }
            div { class: "srow__control", {children} }
        }
    }
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
    let current = page_for(&page.read()).to_string();
    let shown = matching_pages(&query.read());
    let back = match *s.mode.read() {
        Mode::Mail => "Back to Mail",
        Mode::Calendar => "Back to Calendar",
    };
    rsx! {
        div { class: "sidebar__controls settings-side",
            // Esc leaves too (shortcuts.rs Close), so no keycap here.
            button { class: "settings-back", r#type: "button", title: "{back} (Esc)", onclick: move |_| open.set(false),
                Icon { name: "chev-left", class: "icon--sm" }
                span { "{back}" }
            }
            label { class: "settings-nav__search",
                Icon { name: "search", class: "icon--sm" }
                input { r#type: "search", placeholder: "Search settings", "aria-label": "Search settings", value: "{query}", autocomplete: "off",
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
                match page_for(&current) {
                    "Appearance" => rsx! { Appearance {} },
                    "Accounts & agents" => rsx! { Accounts {} },
                    "Reading" => rsx! { super::settings_sections::Reading {} },
                    "Writing" => rsx! { super::settings_sections::Writing {} },
                    "Calendar" => rsx! { super::settings_sections::CalendarSettings {} },
                    "Notifications" => rsx! { super::settings_sections::Notifications {} },
                    "Privacy" => rsx! { super::settings_sections::Privacy {} },
                    "Keyboard shortcuts" => rsx! { super::settings_sections::Shortcuts {} },
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
        assert_eq!(matching_pages("signature").first(), Some(&"Writing"));
        assert_eq!(matching_pages("week").first(), Some(&"Calendar"));
        assert_eq!(matching_pages("dark").first(), Some(&"Appearance"));
        assert_eq!(matching_pages("nickname").first(), Some(&"Accounts & agents"));
        assert_eq!(matching_pages("claude").first(), Some(&"Accounts & agents"));
        assert_eq!(matching_pages("from").first(), Some(&"Writing"));
        assert_eq!(matching_pages("gravatar").first(), Some(&"Appearance"));
        assert_eq!(matching_pages("attachments").first(), Some(&"Reading"));
        assert_eq!(matching_pages("update").first(), Some(&"General"));
        assert_eq!(matching_pages("").len(), PAGES.len());
        assert!(matching_pages("zzz").is_empty());
    }

    #[test]
    fn merged_pages_still_open() {
        assert_eq!(page_for("Composing"), "Writing");
        assert_eq!(page_for("Signatures"), "Writing");
        assert_eq!(page_for("AI agents"), "Accounts & agents");
        assert_eq!(page_for("Accounts"), "Accounts & agents");
        assert_eq!(page_for("About & updates"), "General");
        assert_eq!(page_for("calendar"), "Calendar");
        assert_eq!(page_for("nope"), "General");
    }
}

#[component]
fn Appearance() -> Element {
    let s = use_context::<AppState>();
    let mut theme = s.theme;
    let mut palette = s.palette;
    let mut scale = use_signal(|| prefs::get().font_scale);
    let current = *theme.read();
    let dark = s.dark();
    let selected = palette.read().clone();
    let modes = [(Theme::Light, "Light"), (Theme::Dark, "Dark"), (Theme::System, "Match system")];
    let mut step = move |st: Step| {
        let before = *scale.peek();
        zoom::change(s, st);
        let after = prefs::get().font_scale;
        scale.set(after);
        if let Some(c) = super::settings_bird::size_cue(before, after) {
            super::settings_bird::cue(c);
        }
    };
    let pct = (*scale.read() * 100.0).round();
    let (mut list_style, mut list_avatars) = (s.list_style, s.list_avatars);
    let style_now = *list_style.read();
    let avatars_on = *list_avatars.read();

    rsx! {
        div { class: "settings__title", "Appearance" }
        Group { title: "Look", perch: true,
            Row { label: "Mode", hint: "Match system follows macOS.",
                div { class: "segmented", role: "radiogroup", "aria-label": "Appearance mode",
                    for (t, label) in modes {
                        button { role: "radio", "aria-selected": "{current == t}", "aria-checked": "{current == t}",
                            onclick: move |_| theme.set(t), "{label}" }
                    }
                }
            }
            Row { label: "Theme", hint: "Each comes in light and dark; these show the one you're in.", stack: true,
                div { class: "theme-grid", role: "radiogroup", "aria-label": "Color theme",
                    for (id, name) in themes::choices() {
                        ThemeCard { key: "{id}", id, name, checked: selected == id, dark,
                            on_pick: move |id: &'static str| {
                                if *palette.peek() != id {
                                    super::settings_bird::cue(super::settings_bird::Cue::Twirl);
                                }
                                palette.set(id.to_string());
                            } }
                    }
                }
            }
            Row { label: "Text size", hint: "Also ⌘+ and ⌘−. Messages and the composer follow.",
                div { class: "text-size",
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Smaller text (⌘-)", onclick: move |_| step(Step::Smaller), "−" }
                    span { class: "text-size__value", "{pct}%" }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Bigger text (⌘+)", onclick: move |_| step(Step::Bigger), "+" }
                    button { class: "btn btn--ghost btn--sm", r#type: "button", disabled: pct == 100.0,
                        onclick: move |_| step(Step::Reset), "Reset" kbd { class: "kbd", "⌘0" } }
                }
            }
        }
        Group { title: "Message list",
            Row { label: "Style", stack: true,
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
            HoverActions {}
        }
        Group { title: "Sender photos",
            Row { label: "Show sender photos", hint: "Initials when there's no photo.",
                input { class: "switch", r#type: "checkbox", checked: avatars_on, "aria-label": "Show sender photos",
                    onchange: move |_| {
                        let on = !avatars_on;
                        list_avatars.set(on);
                        prefs::update(move |p| p.list_avatars = on);
                    }
                }
            }
            GoogleContactPhotos {}
            MicrosoftContactPhotos {}
            GravatarPhotos {}
        }
    }
}

/// Sender photos from Gravatar and company logos (BIMI), off by default:
/// Gravatar gets a one-way hash of each sender's address.
#[component]
fn GravatarPhotos() -> Element {
    let mut on = use_signal(|| prefs::get().photos_gravatar);
    rsx! {
        Row { label: "Gravatar and company logos",
            hint: "Sends Gravatar a scrambled form of each sender's address; logos come from the sender's domain.",
            input { class: "switch", r#type: "checkbox", checked: *on.read(), "aria-label": "Gravatar and company logos",
                onchange: move |_| {
                    let now = !*on.peek();
                    on.set(now);
                    prefs::update(move |x| x.photos_gravatar = now);
                }
            }
        }
    }
}

/// Sender photos from Google contacts: on/off, and an Allow button for each
/// Google account whose sign-in doesn't let Coo see its contacts yet.
#[component]
fn GoogleContactPhotos() -> Element {
    let s = use_context::<AppState>();
    let mut on = use_signal(|| prefs::get().photos_google);
    let google: Vec<String> = s
        .accounts
        .read()
        .iter()
        .filter(|a| a.account.provider == ProviderKind::Google)
        .map(|a| a.account.id.clone())
        .collect();
    if google.is_empty() {
        return rsx! {};
    }
    let allowed = s.google_contacts.read().clone();
    let missing: Vec<String> = google.into_iter().filter(|id| !allowed.contains(id)).collect();
    let email = |id: &str| id.split_once(':').map_or(id, |(_, e)| e).to_string();
    let first_missing = missing.first().cloned();
    rsx! {
        Row { label: "From your Google contacts", hint: "People you've saved or mailed with, and coworkers.",
            input { class: "switch", r#type: "checkbox", checked: *on.read(), "aria-label": "Photos from Google contacts",
                onchange: move |_| {
                    let now = !*on.peek();
                    on.set(now);
                    prefs::update(move |p| p.photos_google = now);
                    let mut photos = s.photos;
                    photos.write().retain(|_, uri| uri.is_some());
                    // Turning it on asks for the first account that needs it.
                    if now && let Some(id) = first_missing.clone() {
                        send_cmd(Cmd::AllowGoogleContacts { account_id: id });
                    }
                }
            }
        }
        if *on.read() {
            for id in missing {
                Row { key: "{id}", label: email(&id), hint: "Needs your OK in the browser, once.",
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: { let id = id.clone(); move |_| send_cmd(Cmd::AllowGoogleContacts { account_id: id.clone() }) },
                        "Allow contacts…" }
                }
            }
        }
    }
}

/// Sender photos from Microsoft 365: Outlook contacts and coworkers, with
/// an Allow button for each account that hasn't allowed it yet.
#[component]
fn MicrosoftContactPhotos() -> Element {
    let s = use_context::<AppState>();
    let mut on = use_signal(|| prefs::get().photos_microsoft);
    let microsoft: Vec<String> = s
        .accounts
        .read()
        .iter()
        .filter(|a| a.account.provider == ProviderKind::Microsoft)
        .map(|a| a.account.id.clone())
        .collect();
    if microsoft.is_empty() {
        return rsx! {};
    }
    let allowed = s.microsoft_photos.read().clone();
    let missing: Vec<String> = microsoft.into_iter().filter(|id| !allowed.contains(id)).collect();
    let email = |id: &str| id.split_once(':').map_or(id, |(_, e)| e).to_string();
    let first_missing = missing.first().cloned();
    rsx! {
        Row { label: "From Microsoft 365", hint: "Your Outlook contacts, and coworkers' own photos.",
            input { class: "switch", r#type: "checkbox", checked: *on.read(), "aria-label": "Photos from Microsoft 365",
                onchange: move |_| {
                    let now = !*on.peek();
                    on.set(now);
                    prefs::update(move |p| p.photos_microsoft = now);
                    let mut photos = s.photos;
                    photos.write().retain(|_, uri| uri.is_some());
                    // Turning it on asks for the first account that needs it.
                    if now && let Some(id) = first_missing.clone() {
                        send_cmd(Cmd::AllowMicrosoftPhotos { account_id: id });
                    }
                }
            }
        }
        if *on.read() {
            for id in missing {
                Row { key: "{id}", label: email(&id), hint: "Needs your OK in the browser, once. Some organizations need an admin to approve coworkers' photos.",
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: { let id = id.clone(); move |_| send_cmd(Cmd::AllowMicrosoftPhotos { account_id: id.clone() }) },
                        "Allow…" }
                }
            }
        }
    }
}

/// Settings › Appearance › Hover actions: the buttons a row shows under the
/// pointer, up to five, in order, from anything in its right-click menu.
#[component]
fn HoverActions() -> Element {
    use super::row_actions::{ALL, CHOSEN, DEFAULT, MAX, find, set};
    let chosen = CHOSEN.read().clone();
    let full = chosen.len() >= MAX;
    let rest: Vec<_> = ALL.iter().filter(|a| !chosen.iter().any(|c| c == a.id)).collect();
    let is_default = chosen.iter().map(String::as_str).eq(DEFAULT.iter().copied());
    let moved = |from: usize, to: usize| {
        let mut v = CHOSEN.peek().clone();
        if to < v.len() {
            v.swap(from, to);
            set(v);
        }
    };
    rsx! {
        Row { label: "Hover actions", hint: "Up to 5 buttons a message shows under the pointer, from its right-click menu.", stack: true,
            div { class: "hover-actions",
                // The buttons as a row shows them.
                div { class: "hover-actions__preview", "aria-hidden": "true",
                    for id in chosen.iter() {
                        if let Some(a) = find(id) {
                            span { class: "icon-btn icon-btn--sm", key: "{id}", Icon { name: a.icon } }
                        }
                    }
                    if chosen.is_empty() { span { class: "t-caption", "No buttons" } }
                }
                div { class: "hover-actions__list", role: "list", "aria-label": "Row buttons, in order",
                    for (i, id) in chosen.iter().enumerate() {
                        if let Some(a) = find(id) {
                            {
                                let (last, id_rm) = (i + 1 == chosen.len(), id.clone());
                                rsx! {
                                    div { class: "hover-actions__item", role: "listitem", key: "{id}",
                                        Icon { name: a.icon, class: "icon--sm" }
                                        span { class: "hover-actions__label", "{a.label}" }
                                        button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Move {a.label} earlier",
                                            disabled: i == 0, onclick: move |_| if i > 0 { moved(i, i - 1) },
                                            Icon { name: "chev-up", class: "icon--xs" } }
                                        button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Move {a.label} later",
                                            disabled: last, onclick: move |_| moved(i, i + 1),
                                            Icon { name: "chev-down", class: "icon--xs" } }
                                        button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Remove {a.label}",
                                            onclick: move |_| {
                                                let v: Vec<String> = CHOSEN.peek().iter().filter(|x| **x != id_rm).cloned().collect();
                                                set(v);
                                            },
                                            Icon { name: "x", class: "icon--xs" } }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "hover-actions__add",
                    for a in rest {
                        button { class: "chip hover-actions__chip", r#type: "button", key: "{a.id}", disabled: full,
                            title: if full { "Up to 5: remove one first" } else { "Add to the row" },
                            onclick: move |_| {
                                let mut v = CHOSEN.peek().clone();
                                v.push(a.id.to_string());
                                set(v);
                            },
                            Icon { name: a.icon, class: "icon--xs" }
                            "{a.label}"
                        }
                    }
                }
                span { class: "srow__hint",
                    "Right-click a row's buttons to come back here. "
                    if !is_default {
                        a { href: "#", onclick: move |e| { e.prevent_default(); set(DEFAULT.iter().map(|s| s.to_string()).collect()); },
                            "Restore defaults" }
                    }
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

/// A palette as a tiny window in the mode in use: its sidebar ground, a
/// sheet with text lines and an accent button (mixed as app.css does).
#[component]
fn ThemeCard(
    id: &'static str,
    name: &'static str,
    checked: bool,
    dark: bool,
    on_pick: EventHandler<&'static str>,
) -> Element {
    let variants = themes::FAMILIES.iter().find(|f| f.id == id).map(|f| f.variants).unwrap_or(("Dark", "Light"));
    let [bg0, bg2, bg3, text, accent] = themes::swatch(id, dark);
    let variant = if dark { variants.0 } else { variants.1 };
    rsx! {
        button { class: "theme-card", r#type: "button", role: "radio", "aria-checked": "{checked}",
            title: "{name}: {variants.1} / {variants.0}",
            onclick: move |_| on_pick.call(id),
            div { class: "tp", "data-dark": dark.then_some("true"), "aria-hidden": "true",
                style: "--p-bg0:{bg0};--p-bg2:{bg2};--p-bg3:{bg3};--p-text:{text};--p-accent:{accent}",
                div { class: "tp__side",
                    span { class: "tp__pill" }
                    span { class: "tp__line" }
                    span { class: "tp__line tp__line--short" }
                }
                div { class: "tp__sheet",
                    span { class: "tp__line tp__line--title" }
                    span { class: "tp__line" }
                    span { class: "tp__line tp__line--short" }
                    span { class: "tp__btn" }
                }
            }
            div { class: "theme-card__name", "{name}" }
            div { class: "theme-card__variants", "{variant}" }
        }
    }
}

#[component]
fn Accounts() -> Element {
    let s = use_context::<AppState>();
    let signing_in = s.signing_in;
    let sheet_open = s.add_account.read().is_some();

    rsx! {
        div { class: "settings__title", "Accounts & agents" }
        Group { title: "Mail accounts",
            Row { label: "Accounts", hint: "Click a name to give it a nickname. Drag accounts in the sidebar to reorder them.", stack: true,
                AccountList {}
                div { class: "account-detail__actions",
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: move |_| super::add_account::open(s, ""), Icon { name: "plus", class: "icon--sm" } "Add account…" }
                    if *signing_in.read() && !sheet_open { span { class: "srow__hint", style: "align-self:center", "Finish signing in in your browser…" } }
                }
            }
        }
        Group { title: "AI agents", AgentList {} }
    }
}

/// Settings › Accounts & agents › AI agents: each agent Coo can set up,
/// with its status as the setup dialog shows it, and a button that opens
/// the dialog on that agent.
#[component]
fn AgentList() -> Element {
    let s = use_context::<AppState>();
    let mut entries = use_signal(agents::existing_entries);
    // Read again when the setup dialog closes (it may have changed one).
    use_effect(move || {
        if !*s.agent_setup_open.read() {
            entries.set(agents::existing_entries());
        }
    });
    let list = entries.read().clone();
    rsx! {
        Row { label: "Agents", hint: "Apps that can read your mail and calendar through Coo, on this Mac.", stack: true,
            div { class: "agent-list",
                for agent in Agent::ALL {
                    {
                        let status = Status::of(agent, &list);
                        let can_send = matches!(status, Status::Connected { allow_write: true });
                        rsx! {
                            div { class: "agent-list__row", key: "{agent.label()}",
                                span { class: "agent-list__icon", Icon { name: super::agent_setup::agent_icon(agent), class: "icon--sm" } }
                                span { class: "agent-list__name", "{agent.label()}" }
                                span { class: super::agent_setup::chip_class(status, can_send),
                                    span { class: "agent-chip__dot" }
                                    "{status.label(can_send)}"
                                }
                                button { class: "btn btn--secondary btn--sm", r#type: "button",
                                    onclick: move |_| {
                                        *super::agent_setup::PICK.write() = Some(agent);
                                        let mut o = s.agent_setup_open;
                                        o.set(true);
                                    },
                                    if status.is_connected() { "Manage" } else { "Set up" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// "New messages come from": the default account for new messages, or
/// Automatic (the top account in the sidebar). Replies and forwards keep
/// coming from the account the mail came to; the composer's From picker
/// changes one message only. Settings › Writing.
#[component]
pub fn DefaultFromField() -> Element {
    let s = use_context::<AppState>();
    let accounts = s.accounts.read().clone();
    let chosen = s.chosen_default().unwrap_or_default();
    let top = accounts.first().map(|a| s.account_label(&a.account.id, &a.account.email)).unwrap_or_default();
    rsx! {
        Row { label: "New messages come from", hint: "Automatic is the top account in the sidebar. Replies come from the address the message was sent to.",
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
