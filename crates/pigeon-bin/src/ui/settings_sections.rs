//! Settings sections beyond Appearance/Accounts: General (closing the
//! window), Composing (undo send), Signatures, Calendar, Notifications,
//! Keyboard shortcuts, Privacy.

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::prefs;
use crate::send_cmd;
use crate::state::AppState;

const UNDO_CHOICES: [(u32, &str); 5] = [(0, "Off"), (5, "5 s"), (10, "10 s"), (20, "20 s"), (30, "30 s")];

fn day_name(w: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match w {
        Sun => "Sunday",
        Mon => "Monday",
        Tue => "Tuesday",
        Wed => "Wednesday",
        Thu => "Thursday",
        Fri => "Friday",
        Sat => "Saturday",
    }
}

#[component]
pub fn CalendarSettings() -> Element {
    let s = use_context::<AppState>();
    let mut choice = use_signal(|| prefs::get().week_start);
    let auto_label = format!("Automatic ({})", day_name(crate::week::system().first));
    let options: [(Option<u8>, String); 4] =
        [(None, auto_label), (Some(0), "Sunday".into()), (Some(1), "Monday".into()), (Some(6), "Saturday".into())];
    let current = *choice.read();
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Calendar" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Week starts on" }
                div { class: "segmented", role: "radiogroup", "aria-label": "First day of the week",
                    for (value, label) in options {
                        button { role: "radio", key: "{label}", "aria-checked": "{current == value}", "aria-selected": "{current == value}",
                            onclick: move |_| {
                                prefs::update(|p| p.week_start = value);
                                choice.set(value);
                                let mut fw = s.first_weekday;
                                fw.set(crate::week::first_day());
                                let day = *s.cal_day.peek();
                                super::calendar::go_to(s, day);
                            },
                            "{label}"
                        }
                    }
                }
                span { class: "settings-field__hint", "Week view, the month grids and navigation use this." }
            }
        }
    }
}

/// What closing the main window does (prefs `close_to_menu_bar`).
const CLOSE_CHOICES: [(bool, &str); 2] = [(true, "Keeps Coo in the menu bar"), (false, "Quits Coo")];

#[component]
pub fn General() -> Element {
    let mut keep = use_signal(|| prefs::get().close_to_menu_bar);
    let current = *keep.read();
    let mut after = use_signal(|| prefs::get().after_remove);
    let after_now = after.read().clone();
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "General" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Closing the window" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Closing the window",
                    for (value, label) in CLOSE_CHOICES {
                        button { role: "radio", key: "{label}", "aria-checked": "{current == value}", "aria-selected": "{current == value}",
                            onclick: move |_| {
                                prefs::update(|p| p.close_to_menu_bar = value);
                                keep.set(value);
                            },
                            "{label}"
                        }
                    }
                }
                span { class: "settings-field__hint",
                    if current {
                        "Mail keeps syncing and notifications keep coming. Open Coo again from its menu bar icon; ⌘Q quits."
                    } else {
                        "Closing the window quits Coo: no new-mail notifications or reminders until you open it again."
                    }
                }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "After archive or delete" }
                div { class: "segmented", role: "radiogroup", "aria-label": "After archiving or deleting a message",
                    for (value, label) in [("next", "Open the next message"), ("close", "Close the message")] {
                        button { role: "radio", key: "{value}", "aria-checked": "{after_now == value}", "aria-selected": "{after_now == value}",
                            onclick: move |_| {
                                prefs::update(move |p| p.after_remove = value.to_string());
                                after.set(value.to_string());
                            },
                            "{label}"
                        }
                    }
                }
                span { class: "settings-field__hint", "Also applies when you move a message to another folder." }
            }
        }
    }
}

/// Where new messages open (prefs `compose_mode`).
const COMPOSE_MODES: [(&str, &str); 2] = [("window", "In a separate window"), ("docked", "Docked in the reading pane")];
/// Where replies open (prefs `reply_mode`).
const REPLY_MODES: [(&str, &str); 2] = [("inline", "Inline in the conversation"), ("window", "In a separate window")];

#[component]
pub fn Composing() -> Element {
    let mut undo = use_signal(|| prefs::get().undo_send_seconds);
    let mut mode = use_signal(|| prefs::get().compose_mode);
    let mut rmode = use_signal(|| prefs::get().reply_mode);
    let current_rmode = rmode.read().clone();
    let current = *undo.read();
    let current_mode = mode.read().clone();

    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Composing" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "New messages" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Open new messages",
                    for (value, label) in COMPOSE_MODES {
                        button { role: "radio", "aria-checked": "{current_mode == value}", "aria-selected": "{current_mode == value}",
                            onclick: move |_| {
                                prefs::update(|p| p.compose_mode = value.to_string());
                                mode.set(value.to_string());
                            },
                            "{label}"
                        }
                    }
                }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "Replies" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Open replies",
                    for (value, label) in REPLY_MODES {
                        button { role: "radio", "aria-checked": "{current_rmode == value}", "aria-selected": "{current_rmode == value}",
                            onclick: move |_| {
                                prefs::update(|p| p.reply_mode = value.to_string());
                                rmode.set(value.to_string());
                            },
                            "{label}"
                        }
                    }
                }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "Undo send" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Undo send window",
                    for (secs, label) in UNDO_CHOICES {
                        button { role: "radio", "aria-checked": "{current == secs}", "aria-selected": "{current == secs}",
                            onclick: move |_| {
                                prefs::update(|p| p.undo_send_seconds = secs);
                                undo.set(secs);
                            },
                            "{label}"
                        }
                    }
                }
                span { class: "settings-field__hint", "Messages wait this long before going out, so you can take them back." }
            }
        }
    }
}

/// Above this many addresses the Signatures page gets a filter box.
const SIG_FILTER_FROM: usize = 7;

/// Settings › Signatures: one default signature, then a compact row per
/// sending address choosing Default / Own / None. Only the row being edited
/// opens an editor, so twenty addresses stay a short list.
#[component]
pub fn Signatures() -> Element {
    let s = use_context::<AppState>();
    use_hook(move || {
        if s.senders.read().is_empty() {
            send_cmd(Cmd::LoadSenders);
        }
    });
    let editing = use_signal(|| None::<String>);
    let mut filter = use_signal(String::new);
    let senders = s.senders.read().clone();
    let q = filter.read().trim().to_lowercase();
    let shown: Vec<_> =
        senders.iter().filter(|x| q.is_empty() || x.address.email.to_lowercase().contains(&q)).cloned().collect();

    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Signatures" }
        div { class: "settings-group",
            div { class: "sig-default",
                div { class: "sig-default__head",
                    span { class: "settings-field__label", "Default signature" }
                    span { class: "settings-field__hint", "Used by every address set to Default below." }
                }
                SignatureField { email: prefs::DEFAULT_SIGNATURE.to_string(), placeholder: "Your signature. Select text to format, or paste one from a web page." }
            }
        }
        div { class: "sig-list__head",
            span { class: "settings-field__label", "Addresses" }
            if senders.len() >= SIG_FILTER_FROM {
                label { class: "sig-list__filter",
                    Icon { name: "search", class: "icon--sm" }
                    input { r#type: "search", placeholder: "Filter addresses", value: "{filter}",
                        oninput: move |e| filter.set(e.value()) }
                }
            }
        }
        div { class: "settings-group sig-list",
            if senders.is_empty() { span { class: "settings-field__hint", "Loading your addresses…" } }
            if !senders.is_empty() && shown.is_empty() { span { class: "settings-field__hint", "No address matches." } }
            for sender in shown {
                SignatureRow { key: "{sender.address.email}", email: sender.address.email.clone(), hue: s.hue(&sender.account_id), editing }
            }
        }
    }
}

/// First line of a signature, as plain text, for the collapsed row.
fn signature_preview(html: &str) -> String {
    let text = pigeon_core::text::html_to_text(html);
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(80).collect()
}

#[component]
fn SignatureRow(email: String, hue: String, editing: Signal<Option<String>>) -> Element {
    let key = email.to_lowercase();
    let mut mode = use_signal(|| prefs::get().signature_mode(&key));
    let open = editing.read().as_deref() == Some(key.as_str());
    let current = *mode.read();
    let p = prefs::get();
    let summary = match current {
        prefs::SigMode::None => "No signature".to_string(),
        prefs::SigMode::Default => {
            let d = signature_preview(&p.signature(&key));
            if d.is_empty() { "Default (empty)".into() } else { format!("Default · {d}") }
        }
        prefs::SigMode::Own => {
            let d = signature_preview(&p.signatures.get(&key).cloned().unwrap_or_default());
            if d.is_empty() { "Not written yet".into() } else { d }
        }
    };
    let (edit_icon, edit_label): (&'static str, &str) = if open { ("chev-up", "Done") } else { ("edit", "Edit") };
    let (k1, k2) = (key.clone(), key.clone());
    let mut set_mode = move |m: prefs::SigMode, k: String| {
        let v = m.key().to_string();
        let kk = k.clone();
        prefs::update(move |p| {
            p.signature_modes.insert(kk, v);
        });
        mode.set(m);
        editing.set((m == prefs::SigMode::Own).then_some(k));
    };
    rsx! {
        div { class: "sig-row", "data-open": open.then_some("true"),
            div { class: "sig-row__line",
                button { class: "sig-row__who", r#type: "button", title: if current == prefs::SigMode::Own { "Edit this signature" } else { "" },
                    disabled: current != prefs::SigMode::Own,
                    onclick: move |_| {
                        let next = if open { None } else { Some(k1.clone()) };
                        editing.set(next);
                    },
                    span { class: "dot dot--sm", "data-hue": "{hue}" }
                    span { class: "sig-row__email", "{email}" }
                    span { class: "sig-row__preview", dir: "auto", "{summary}" }
                    if current == prefs::SigMode::Own {
                        span { class: "sig-row__edit",
                            Icon { name: edit_icon, class: "icon--sm" }
                            "{edit_label}"
                        }
                    }
                }
                div { class: "segmented segmented--sm", role: "radiogroup", "aria-label": "Signature for {email}",
                    for (m, label) in [(prefs::SigMode::Default, "Default"), (prefs::SigMode::Own, "Own"), (prefs::SigMode::None, "None")] {
                        button { key: "{label}", role: "radio", "aria-checked": "{current == m}", "aria-selected": "{current == m}",
                            onclick: { let k = k2.clone(); move |_| set_mode(m, k.clone()) },
                            "{label}"
                        }
                    }
                }
            }
            if open && current == prefs::SigMode::Own {
                SignatureField { email: key.clone(), placeholder: "Signature for {email}. Select text to format, or paste one from a web page." }
            }
        }
    }
}

#[component]
fn SignatureField(email: String, placeholder: String) -> Element {
    let key = email.to_lowercase();
    // Rendered once; the editor owns its content afterwards and reports edits
    // through the bridge ("sig-input"), sanitized before saving.
    let initial =
        use_hook(|| super::compose::signature_body(&prefs::get().signatures.get(&key).cloned().unwrap_or_default()));
    rsx! {
        div { class: "input sig-editor", contenteditable: "true", dir: "auto",
            "data-editor": "", "data-sig-editor": "{key}",
            "data-placeholder": "{placeholder}",
            dangerous_inner_html: "{initial}" }
    }
}

const REMINDER_CHOICES: [u32; 5] = [0, 5, 10, 15, 30];

#[component]
pub fn Notifications() -> Element {
    let s = use_context::<AppState>();
    let mut p = use_signal(prefs::get);
    let cur = p.read().clone();
    let accounts = s.accounts.read().clone();
    let mut set = move |f: fn(&mut prefs::Prefs)| {
        prefs::update(f);
        p.set(prefs::get());
    };
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Notifications" }
        div { class: "settings-group",
            label { class: "settings-field",
                span { class: "settings-field__label", "New mail" }
                input { class: "switch", r#type: "checkbox", checked: cur.notify_new_mail,
                    onchange: move |_| set(|p| p.notify_new_mail = !p.notify_new_mail) }
                span { class: "settings-field__hint", "Alerts for new inbox mail." }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "Show" }
                div { class: "segmented", role: "radiogroup", "aria-label": "New mail alert",
                    for (v, label) in [("banner", "Banner and sound"), ("background", "Banner when Coo is in the background"), ("sound", "Sound only")] {
                        button { role: "radio", key: "{v}", disabled: !cur.notify_new_mail,
                            "aria-checked": "{cur.mail_alert == v}", "aria-selected": "{cur.mail_alert == v}",
                            onclick: move |_| { prefs::update(move |x| x.mail_alert = v.to_string()); p.set(prefs::get()); },
                            "{label}"
                        }
                    }
                }
            }
            if crate::notify::permission() == crate::notify::Permission::Denied {
                div { class: "banner banner--warning", style: "margin:4px 0 8px",
                    Icon { name: "alert", class: "icon--sm" }
                    span { "macOS is blocking Coo's notifications, so banners can't appear." }
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: move |_| { let _ = open::that_detached(crate::notify::SETTINGS_URL); },
                        "Open Notification Settings" }
                }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Dock badge" }
                input { class: "switch", r#type: "checkbox", checked: cur.dock_badge,
                    onchange: move |_| {
                        set(|p| p.dock_badge = !p.dock_badge);
                        let unread: u32 = s
                            .accounts
                            .peek()
                            .iter()
                            .flat_map(|a| a.folders.iter())
                            .filter(|f| f.folder.role == Some(pigeon_core::model::FolderRole::Inbox))
                            .map(|f| f.unread)
                            .sum();
                        crate::dock::set_badge(if prefs::get().dock_badge { unread } else { 0 });
                    } }
                span { class: "settings-field__hint", "Show the unread inbox count on Coo's Dock icon." }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Events" }
                input { class: "switch", r#type: "checkbox", checked: cur.notify_events,
                    onchange: move |_| set(|p| p.notify_events = !p.notify_events) }
                span { class: "settings-field__hint", "A reminder before meetings start, with Join." }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Through Focus" }
                input { class: "switch", r#type: "checkbox", checked: cur.reminders_time_sensitive, disabled: !cur.notify_events,
                    onchange: move |_| set(|p| p.reminders_time_sensitive = !p.reminders_time_sensitive) }
                span { class: "settings-field__hint", "Meeting reminders are Time Sensitive, so Do Not Disturb and Focus still show them. New mail never is. macOS lets you turn this off per Focus too." }
            }
            div { class: "settings-field settings-field--top",
                span { class: "settings-field__label", "Sounds" }
                div { style: "display:flex;flex-direction:column;gap:8px",
                    div { class: "segmented", role: "radiogroup", "aria-label": "Sound theme",
                        {
                            let choices = std::iter::once((crate::sound::NONE, "None")).chain(crate::sound::THEMES.iter().map(|t| (t.id, t.label)));
                            rsx! {
                                for (id, label) in choices {
                                    button { role: "radio", key: "{id}",
                                        "aria-checked": "{cur.sound_theme == id}", "aria-selected": "{cur.sound_theme == id}",
                                        title: "Click to hear the new-mail sound",
                                        onclick: move |_| {
                                            prefs::update(move |x| x.sound_theme = id.to_string());
                                            crate::sound::play(id, crate::sound::Kind::Mail, prefs::get().sound_volume);
                                            p.set(prefs::get());
                                        },
                                        "{label}"
                                    }
                                }
                            }
                        }
                    }
                    div { style: "display:flex;gap:8px;align-items:center",
                        button { class: "btn btn--ghost btn--sm", disabled: cur.sound_theme == crate::sound::NONE,
                            onclick: move |_| { let p = prefs::get(); crate::sound::play(&p.sound_theme, crate::sound::Kind::Mail, p.sound_volume); },
                            Icon { name: "mail", class: "icon--sm" } "New mail" }
                        button { class: "btn btn--ghost btn--sm", disabled: cur.sound_theme == crate::sound::NONE,
                            onclick: move |_| { let p = prefs::get(); crate::sound::play(&p.sound_theme, crate::sound::Kind::Meeting, p.sound_volume); },
                            Icon { name: "cal", class: "icon--sm" } "Meeting reminder" }
                        input { r#type: "range", min: "10", max: "100", step: "5", class: "settings-range", "aria-label": "Volume",
                            value: "{cur.sound_volume}", disabled: cur.sound_theme == crate::sound::NONE,
                            onchange: move |e| {
                                let v: u8 = e.value().parse().unwrap_or(60);
                                prefs::update(move |x| x.sound_volume = v);
                                crate::sound::play(&prefs::get().sound_theme, crate::sound::Kind::Mail, v);
                                p.set(prefs::get());
                            }
                        }
                    }
                    span { class: "settings-field__hint", "Each theme pairs a new-mail sound with a matching meeting reminder. Plays once per batch, only for the alerts turned on here." }
                }
            }
            div { class: "settings-field",
                span { class: "settings-field__label", "Remind me" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Event reminder",
                    for m in REMINDER_CHOICES {
                        button { role: "radio", key: "{m}", disabled: !cur.notify_events,
                            "aria-checked": "{cur.event_reminder_minutes == m}", "aria-selected": "{cur.event_reminder_minutes == m}",
                            onclick: move |_| { prefs::update(move |x| x.event_reminder_minutes = m); p.set(prefs::get()); },
                            if m == 0 { "At start" } else { "{m} min" }
                        }
                    }
                }
            }
            div { class: "settings-field settings-field--top",
                span { class: "settings-field__label", "Accounts" }
                div { style: "display:flex;flex-direction:column;gap:8px",
                    for a in accounts {
                        {
                            let id = a.account.id.clone();
                            let on = !cur.notify_accounts_muted.contains(&id);
                            rsx! {
                                label { key: "{id}", "data-hue": "{a.hue}", style: "display:flex;align-items:center;gap:10px",
                                    input { class: "check check--hue", r#type: "checkbox", checked: on,
                                        onchange: move |_| {
                                            let id = id.clone();
                                            prefs::update(move |x| {
                                                if let Some(i) = x.notify_accounts_muted.iter().position(|m| *m == id) {
                                                    x.notify_accounts_muted.remove(i);
                                                } else {
                                                    x.notify_accounts_muted.push(id);
                                                }
                                            });
                                            p.set(prefs::get());
                                        }
                                    }
                                    span { dir: "auto", "{s.account_label(&a.account.id, &a.account.email)}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Shortcuts() -> Element {
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Keyboard shortcuts" }
        div { class: "shortcut-groups",
            for g in crate::shortcuts::GROUPS {
                section { class: "settings-group shortcut-group", key: "{g}",
                    h3 { class: "shortcut-group__title", "{g}" }
                    for (_, _, keys, what) in crate::shortcuts::SHORTCUTS.iter().filter(|x| x.0 == g) {
                        div { class: "shortcut", key: "{keys}",
                            span { class: "shortcut__what", "{what}" }
                            kbd { class: "kbd", "{keys}" }
                        }
                    }
                }
            }
            section { class: "settings-group shortcut-group",
                h3 { class: "shortcut-group__title", "Formatting" }
                for (_, _, keys, what) in crate::richtext::SHORTCUTS.iter() {
                    div { class: "shortcut", key: "{keys}",
                        span { class: "shortcut__what", "{what}" }
                        kbd { class: "kbd", "{keys}" }
                    }
                }
                for (keys, what) in crate::richtext::TYPING_KEYS.iter() {
                    div { class: "shortcut", key: "{keys}",
                        span { class: "shortcut__what", "{what}" }
                        kbd { class: "kbd", "{keys}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Privacy() -> Element {
    let mut p = use_signal(prefs::get);
    let cur = p.read().clone();
    let always = cur.remote_images_always;
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Privacy" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Remote images" }
                div { class: "segmented", role: "radiogroup", "aria-label": "Remote images",
                    button { role: "radio", "aria-checked": "{!always}", "aria-selected": "{!always}",
                        onclick: move |_| { prefs::update(|x| x.remote_images_always = false); p.set(prefs::get()); }, "Ask" }
                    button { role: "radio", "aria-checked": "{always}", "aria-selected": "{always}",
                        onclick: move |_| { prefs::update(|x| x.remote_images_always = true); p.set(prefs::get()); }, "Always load" }
                }
                span { class: "settings-field__hint", "Remote images let senders see when you open a message." }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Sender photos" }
                input { class: "switch", r#type: "checkbox", checked: cur.photos_gravatar,
                    onchange: move |_| { prefs::update(|x| x.photos_gravatar = !x.photos_gravatar); p.set(prefs::get()); } }
                span { class: "settings-field__hint",
                    "Show photos from Gravatar. Coo sends Gravatar a one-way hash of each sender's address (never the address itself); off by default." }
            }
            if !always {
                div { class: "settings-field settings-field--top",
                    span { class: "settings-field__label", "Trusted senders" }
                    div { style: "display:flex;flex-direction:column;gap:6px",
                        if cur.remote_images_senders.is_empty() {
                            span { class: "settings-field__hint", style: "margin:0", "None yet. Use “Always from…” on a message's image banner." }
                        }
                        for who in cur.remote_images_senders.clone() {
                            div { key: "{who}", style: "display:flex;align-items:center;gap:8px",
                                span { style: "flex:1", "{who}" }
                                button { class: "icon-btn icon-btn--sm", "aria-label": "Remove {who}",
                                    onclick: move |_| {
                                        let w = who.clone();
                                        prefs::update(move |x| x.remote_images_senders.retain(|s| *s != w));
                                        p.set(prefs::get());
                                    },
                                    Icon { name: "x", class: "icon--sm" }
                                }
                            }
                        }
                    }
                }
            }
            label { class: "settings-field",
                span { class: "settings-field__label", "Crash reports" }
                input { class: "switch", r#type: "checkbox", checked: cur.crash_reports,
                    onchange: move |_| { prefs::update(|x| x.crash_reports = !x.crash_reports); p.set(prefs::get()); } }
                span { class: "settings-field__hint", "Send anonymous crash reports (addresses and message text are removed). Applies next launch." }
            }
        }
    }
}

/// Version, update check and automatic installs (Sparkle; release builds only).
#[component]
pub fn AboutUpdates() -> Element {
    let s = use_context::<AppState>();
    let ready = s.update_ready.read().clone();
    let available = crate::updater::available();
    let (version, build) = crate::updater::version();
    let mut auto = use_signal(crate::updater::automatic);
    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "About & updates" }
        div { class: "settings-group",
            div { class: "settings-field",
                span { class: "settings-field__label", "Version" }
                span { "Coo {version} ({build})" }
                if let Some(v) = ready {
                    button { class: "btn btn--primary btn--sm", r#type: "button",
                        onclick: move |_| super::restart_to_update(), "Restart to update to {v}" }
                } else {
                    button { class: "btn btn--secondary btn--sm", r#type: "button", disabled: !available,
                        onclick: move |_| crate::updater::check_now(), "Check for updates" }
                }
            }
            if available {
                label { class: "settings-field",
                    span { class: "settings-field__label", "Automatic" }
                    input { class: "switch", r#type: "checkbox", checked: *auto.read(),
                        onchange: move |_| {
                            let on = !*auto.read();
                            crate::updater::set_automatic(on);
                            auto.set(on);
                        }
                    }
                    span { class: "settings-field__hint", "Install updates automatically: Coo downloads new versions in the background. Restart when it says an update is ready, or let it update itself while it's in the menu bar." }
                }
            } else {
                div { class: "settings-field",
                    span { class: "settings-field__label" }
                    span { class: "settings-field__hint", "Updates are off in development builds." }
                }
            }
        }
    }
}
