//! Settings: Appearance (mode, color theme, text size) and Accounts (order,
//! sign out, add). Opens with ⌘, or from ⌘K; Esc closes.

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::{AccountNav, Cmd};

use super::Icon;
use super::zoom::{self, Step};
use crate::send_cmd;
use crate::state::{AppState, Theme};
use crate::{prefs, themes};

#[component]
pub fn Settings() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.settings_open;
    rsx! {
        main { class: "pane pane--reader",
            div { class: "settings",
                div { class: "settings__crumbs",
                    span { "Settings" }
                    span { class: "spacer" }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close settings (Esc)", onclick: move |_| open.set(false),
                        Icon { name: "x" }
                    }
                }
                Appearance {}
                Accounts {}
                div { class: "settings__title", style: "margin-top:12px", "AI agents" }
                div { class: "settings__hint", "Let Claude or another agent search and read your mail and calendar through Pigeon." }
                div {
                    button { class: "btn btn--secondary", r#type: "button",
                        onclick: move |_| { let mut o = s.agent_setup_open; o.set(true); },
                        Icon { name: "link", class: "icon--sm" }
                        "Connect an AI agent…"
                    }
                }
            }
        }
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
    let mut icloud = use_signal(|| false);
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
        div { class: "settings__hint", "Drag accounts in the sidebar to reorder them; this list follows the same order." }
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
    let name = if a.display_name.is_empty() { a.email.clone() } else { a.display_name.clone() };
    let (id1, id2) = (a.id.clone(), a.id.clone());
    rsx! {
        div { class: "account-row", "data-hue": "{nav.hue}", "aria-selected": "{confirming}",
            span { class: "account-row__grip", Icon { name: "grip", class: "icon--sm" } }
            span { class: "dot" }
            div { class: "account-row__text",
                span { class: "account-row__name", dir: "auto", "{name}" }
                span { class: "account-row__addr", "{a.email} · {provider}" }
            }
            span { class: "account-row__status", "data-state": if auth { "auth" } else { "ok" },
                if auth { Icon { name: "lock", class: "icon--xs" } "Sign in again" } else { "Connected" }
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
