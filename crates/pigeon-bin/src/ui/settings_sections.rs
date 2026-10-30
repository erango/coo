//! Settings pages beyond Appearance and Accounts & agents: General,
//! Reading, Writing (composing and signatures), Calendar (with video
//! calls), Notifications, Privacy and Keyboard shortcuts. Each is a title
//! over `Group`s of `Row`s (settings.rs).

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

use super::Icon;
use super::settings::{Group, Row};
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

/// What closing the main window does (prefs `close_to_menu_bar`).
const CLOSE_CHOICES: [(bool, &str); 2] = [(true, "Keep running in the menu bar"), (false, "Quit Coo")];

/// Settings › General: the window, updates and version, help.
#[component]
pub fn General() -> Element {
    let s = use_context::<AppState>();
    let mut keep = use_signal(|| prefs::get().close_to_menu_bar);
    let current = *keep.read();
    let ready = s.update_ready.read().clone();
    let available = crate::updater::available();
    let (version, build) = crate::updater::version();
    let mut auto = use_signal(crate::updater::automatic);
    rsx! {
        div { class: "settings__title", "General" }
        Group { title: "Window",
            Row { label: "Closing the window",
                hint: if current {
                    "Mail keeps syncing and notifications keep coming. Open Coo from its menu bar icon; ⌘Q quits."
                } else {
                    "No new-mail notifications or reminders until you open Coo again."
                },
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
            }
        }
        Group { title: "Updates",
            if available {
                Row { label: "Install updates automatically", hint: "Coo downloads new versions in the background and restarts into them when you're not using it.",
                    input { class: "switch", r#type: "checkbox", checked: *auto.read(), "aria-label": "Install updates automatically",
                        onchange: move |_| {
                            let on = !*auto.read();
                            crate::updater::set_automatic(on);
                            auto.set(on);
                        }
                    }
                }
            }
            Row { label: "Coo {version} ({build})",
                hint: match (&ready, available) {
                    (Some(v), _) => format!("Version {v} is ready."),
                    (None, true) => String::new(),
                    (None, false) => "Updates are off in development builds.".to_string(),
                },
                if let Some(v) = ready {
                    button { class: "btn btn--primary btn--sm", r#type: "button",
                        onclick: move |_| super::restart_to_update(), "Restart to update to {v}" }
                } else {
                    button { class: "btn btn--secondary btn--sm", r#type: "button", disabled: !available,
                        onclick: move |_| super::check_for_updates(s), "Check for updates" }
                }
            }
        }
        Group { title: "Help",
            Row { label: "Report a bug", hint: "Goes straight to the developer. Also in the Help menu.",
                button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| super::report_bug(s), "Report a bug…" }
            }
            Row { label: "About Coo", hint: "Credits and the website.",
                super::settings_bird::AboutBird {}
                button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| crate::about::show(), "Open" }
            }
        }
    }
}

/// Settings › Reading: when an opened message counts as read, what follows
/// archive or delete, where attachments sit.
#[component]
pub fn Reading() -> Element {
    let mut after = use_signal(|| prefs::get().after_remove);
    let after_now = after.read().clone();
    let mut att = use_signal(|| prefs::get().attachments_position);
    let att_now = att.read().clone();
    let mut read_after = use_signal(|| prefs::get().read_delay_secs);
    let read_now = *read_after.read();
    rsx! {
        div { class: "settings__title", "Reading" }
        Group { title: "Opening messages",
            Row { label: "Mark as read", hint: "Moving on to another message before then leaves it unread.",
                div { class: "srow__inline",
                    div { class: "segmented", role: "radiogroup", "aria-label": "When an opened message is marked read",
                        button { role: "radio", "aria-checked": "{read_now == 0}", "aria-selected": "{read_now == 0}",
                            onclick: move |_| {
                                prefs::update(|p| p.read_delay_secs = 0);
                                read_after.set(0);
                                super::send_read_delay();
                            },
                            "When opened"
                        }
                        button { role: "radio", "aria-checked": "{read_now > 0}", "aria-selected": "{read_now > 0}",
                            onclick: move |_| if read_now == 0 {
                                prefs::update(|p| p.read_delay_secs = 3);
                                read_after.set(3);
                                super::send_read_delay();
                            },
                            "After a few seconds"
                        }
                    }
                    if read_now > 0 {
                        label { class: "read-delay",
                            input { class: "input input--sm read-delay__input", r#type: "number", min: "1", max: "600", step: "1",
                                "aria-label": "Seconds before marking as read",
                                value: "{read_now}",
                                onchange: move |e| {
                                    let secs = e.value().trim().parse::<u32>().unwrap_or(read_now).clamp(1, 600);
                                    prefs::update(move |p| p.read_delay_secs = secs);
                                    read_after.set(secs);
                                    super::send_read_delay();
                                },
                            }
                            span { "seconds" }
                        }
                    }
                }
            }
            Row { label: "Attachments", hint: "Above: right under the sender, before the text.",
                div { class: "segmented", role: "radiogroup", "aria-label": "Where attachments show in a message",
                    for (value, label) in [("top", "Above the message"), ("bottom", "Below the message")] {
                        button { role: "radio", key: "{value}", "aria-checked": "{att_now == value}", "aria-selected": "{att_now == value}",
                            onclick: move |_| {
                                prefs::update(move |p| p.attachments_position = value.to_string());
                                att.set(value.to_string());
                            },
                            "{label}"
                        }
                    }
                }
            }
        }
        Group { title: "After you act",
            Row { label: "After archive or delete", hint: "Also when you move a message to another folder.",
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
            }
        }
    }
}

/// Where new messages open (prefs `compose_mode`).
const COMPOSE_MODES: [(&str, &str); 2] = [("window", "In a separate window"), ("docked", "Docked in the reading pane")];
/// When drafts are saved (prefs `draft_saving`).
const DRAFT_SAVING: [(&str, &str); 2] = [("ask", "Ask when closing"), ("auto", "Save as I write")];
/// Where replies open (prefs `reply_mode`).
const REPLY_MODES: [(&str, &str); 2] = [("inline", "Inline in the conversation"), ("window", "In a separate window")];

/// Settings › Writing: the composer, drafts and sending, signatures.
#[component]
pub fn Writing() -> Element {
    let mut undo = use_signal(|| prefs::get().undo_send_seconds);
    let mut saving = use_signal(|| prefs::get().draft_saving);
    let current_saving = saving.read().clone();
    let mut mode = use_signal(|| prefs::get().compose_mode);
    let mut rmode = use_signal(|| prefs::get().reply_mode);
    let current_rmode = rmode.read().clone();
    let current = *undo.read();
    let current_mode = mode.read().clone();

    rsx! {
        div { class: "settings__title", "Writing" }
        Group { title: "Composer",
            Row { label: "New messages",
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
            Row { label: "Replies",
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
            super::settings::DefaultFromField {}
        }
        Group { title: "Drafts and sending",
            Row { label: "Drafts",
                hint: if current_saving == "auto" {
                    "Drafts save as you write; the message shows when it last saved."
                } else {
                    "Closing a message you changed asks whether to keep it in Drafts."
                },
                div { class: "segmented", role: "radiogroup", "aria-label": "Saving drafts",
                    for (value, label) in DRAFT_SAVING {
                        button { role: "radio", "aria-checked": "{current_saving == value}", "aria-selected": "{current_saving == value}",
                            onclick: move |_| {
                                prefs::update(|p| p.draft_saving = value.to_string());
                                saving.set(value.to_string());
                            },
                            "{label}"
                        }
                    }
                }
            }
            Row { label: "Undo send", hint: "Messages wait this long before going out, so you can take them back.",
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
            }
        }
        Signatures {}
    }
}

/// Above this many addresses the signature list gets a filter box.
const SIG_FILTER_FROM: usize = 7;

/// Settings › Writing › Signatures: one default signature, then a compact
/// row per sending address choosing Default / Own / None. Only the row
/// being edited opens an editor, so twenty addresses stay a short list.
#[component]
fn Signatures() -> Element {
    let s = use_context::<AppState>();
    use_hook(move || {
        if s.senders.read().is_empty() {
            send_cmd(Cmd::LoadSenders);
        }
    });
    let editing = use_signal(|| None::<String>);
    let mut filter = use_signal(String::new);
    // Signatures go by address: one row even when two accounts share one.
    let mut senders = s.senders.read().clone();
    let mut seen = std::collections::HashSet::new();
    senders.retain(|x| seen.insert(x.address.email.to_lowercase()));
    let q = filter.read().trim().to_lowercase();
    let shown: Vec<_> =
        senders.iter().filter(|x| q.is_empty() || x.address.email.to_lowercase().contains(&q)).cloned().collect();

    rsx! {
        Group { title: "Signatures",
            Row { label: "Default signature", hint: "Used by every address set to Default below.", stack: true,
                SignatureField { email: prefs::DEFAULT_SIGNATURE.to_string(), placeholder: "Your signature. Select text to format, or paste one from a web page." }
            }
            Row { label: "Addresses", hint: "Each address uses the default, its own, or none.", stack: true,
                if senders.len() >= SIG_FILTER_FROM {
                    label { class: "sig-list__filter",
                        Icon { name: "search", class: "icon--sm" }
                        input { r#type: "search", placeholder: "Filter addresses", value: "{filter}",
                            oninput: move |e| filter.set(e.value()) }
                    }
                }
                div { class: "sig-list",
                    if senders.is_empty() { span { class: "srow__hint", "Loading your addresses…" } }
                    if !senders.is_empty() && shown.is_empty() { span { class: "srow__hint", "No address matches." } }
                    for sender in shown {
                        SignatureRow { key: "{sender.address.email}", email: sender.address.email.clone(), hue: s.hue(&sender.account_id), editing }
                    }
                }
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

/// Settings › Calendar: the week, new events, video calls.
#[component]
pub fn CalendarSettings() -> Element {
    let s = use_context::<AppState>();
    let mut choice = use_signal(|| prefs::get().week_start);
    let auto_label = format!("Automatic ({})", day_name(crate::week::system().first));
    let options: [(Option<u8>, String); 4] =
        [(None, auto_label), (Some(0), "Sunday".into()), (Some(1), "Monday".into()), (Some(6), "Saturday".into())];
    let current = *choice.read();
    rsx! {
        div { class: "settings__title", "Calendar" }
        Group { title: "Week",
            Row { label: "Week starts on", hint: "Week view, month grids and navigation use this.",
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
            }
        }
        Group { title: "New events",
            DefaultCalendar {}
            DefaultVideoCall {}
        }
        Group { title: "Video calls",
            ZoomRow {}
            Row { label: "Google Meet", hint: "Works with your Google calendars; nothing to connect.",
                span { class: "srow__value", "Ready" }
            }
        }
    }
}

/// "New events go to": the default calendar for new events (prefs
/// `default_calendar`), among the calendars that take them, or Automatic
/// (`event_editor::auto_calendar`). Also from the calendar sidebar's
/// right-click menus.
#[component]
fn DefaultCalendar() -> Element {
    let s = use_context::<AppState>();
    // Calendars are only listed once the calendar has been shown: ask for
    // them (today's week) when Settings opens first.
    use_hook(move || {
        if s.calendar.peek().calendars.is_empty() {
            let d = chrono::Local::now().date_naive();
            let (from, to) = super::calendar::view_range(crate::state::CalView::Week, d, *s.first_weekday.peek());
            send_cmd(Cmd::ShowCalendar { from, to });
        }
    });
    let accounts = s.accounts.read().clone();
    let order: Vec<String> = accounts.iter().map(|a| a.account.id.clone()).collect();
    let calendars = super::event_editor::in_sidebar_order(&s.calendar.read().calendars, &order);
    let chosen = super::event_editor::chosen_calendar(&calendars, &s.default_calendar.read());
    let value = chosen.as_ref().map(|(a, c)| format!("{a}|{c}")).unwrap_or_default();
    let auto = super::event_editor::auto_calendar(&calendars).and_then(|(a, c)| {
        let cal = calendars.iter().find(|(x, k)| *x == a && k.id == c)?.1.name.clone();
        let email = accounts.iter().find(|n| n.account.id == a).map(|n| n.account.email.clone()).unwrap_or_default();
        Some(format!("{cal} ({})", s.account_label(&a, &email)))
    });
    let auto_label = auto.map_or_else(|| "Automatic".to_string(), |a| format!("Automatic: {a}"));
    rsx! {
        Row { label: "Calendar", hint: "Where new events go; the event editor can pick another.",
            select { class: "select default-from", "aria-label": "Default calendar for new events",
                onchange: move |e| {
                    let v = e.value();
                    s.set_default_calendar(v.split_once('|'));
                },
                option { value: "", selected: value.is_empty(), "{auto_label}" }
                for a in accounts.iter() {
                    if calendars.iter().any(|(id, c)| id == &a.account.id && c.can_edit) {
                        optgroup { key: "{a.account.id}", label: "{s.account_label(&a.account.id, &a.account.email)}",
                            for (_, c) in calendars.iter().filter(|(id, c)| id == &a.account.id && c.can_edit) {
                                {
                                    let v = format!("{}|{}", a.account.id, c.id);
                                    rsx! { option { key: "{v}", value: "{v}", selected: v == value, "{c.name}" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The video call new events start with (prefs `default_video_call`).
const VIDEO_CHOICES: [(&str, &str); 3] = [("", "None"), ("meet", "Google Meet"), ("zoom", "Zoom")];

#[component]
fn DefaultVideoCall() -> Element {
    let mut p = use_signal(prefs::get);
    let cur = p.read().clone();
    rsx! {
        Row { label: "Video call", hint: "New events start with it; you can change it per event.",
            div { class: "segmented", role: "radiogroup", "aria-label": "Video call for new events",
                for (value, label) in VIDEO_CHOICES {
                    button { role: "radio", key: "{label}",
                        "aria-checked": "{cur.default_video_call == value}", "aria-selected": "{cur.default_video_call == value}",
                        onclick: move |_| { prefs::update(|x| x.default_video_call = value.to_string()); p.set(prefs::get()); },
                        "{label}"
                    }
                }
            }
        }
        if !cur.default_video_call.is_empty() {
            Row { label: "Only when there's no location", hint: "An event with a room or address gets no call added.",
                input { class: "switch", r#type: "checkbox", checked: cur.default_video_only_without_location,
                    "aria-label": "Only when the event has no location",
                    onchange: move |_| {
                        let on = !p.peek().default_video_only_without_location;
                        prefs::update(|x| x.default_video_only_without_location = on);
                        p.set(prefs::get());
                    }
                }
            }
        }
    }
}

/// Settings › Calendar › Video calls: the connected Zoom account.
#[component]
fn ZoomRow() -> Element {
    let s = use_context::<AppState>();
    let zoom = s.zoom.read().clone();
    let mut connecting = use_signal(|| false);
    // A reply (connected or a failure toast) ends the wait.
    use_effect(move || {
        let _ = s.zoom.read();
        connecting.set(false);
    });
    let available = pigeon_core::auth::zoom_client_id().is_some();
    match zoom {
        Some(z) => rsx! {
            Row { label: "Zoom", hint: "Connected as {z.display_name} ({z.email}). Coo makes and moves the meetings it adds.",
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| send_cmd(Cmd::ZoomDisconnect), "Disconnect" }
            }
        },
        None => rsx! {
            Row { label: "Zoom",
                hint: if !available {
                    "Zoom isn't set up in this build."
                } else if *connecting.read() {
                    "Finish signing in to Zoom in your browser…"
                } else {
                    "While Coo's Zoom app is in development, only its owner's Zoom organization can connect."
                },
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    disabled: *connecting.read() || !available,
                    onclick: move |_| { connecting.set(true); send_cmd(Cmd::ZoomConnect); },
                    Icon { name: "video", class: "icon--sm" } "Connect Zoom" }
            }
        },
    }
}

const REMINDER_CHOICES: [u32; 5] = [0, 5, 10, 15, 30];

/// Settings › Notifications: new mail, events, sounds.
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
    let quiet = cur.sound_theme == crate::sound::NONE;
    rsx! {
        div { class: "settings__title", "Notifications" }
        Group { title: "New mail",
            Row { label: "Notify me", hint: "Alerts for new inbox mail.",
                input { class: "switch", r#type: "checkbox", checked: cur.notify_new_mail, "aria-label": "Notify me about new mail",
                    onchange: move |_| set(|p| p.notify_new_mail = !p.notify_new_mail) }
            }
            Row { label: "Show",
                div { class: "segmented", role: "radiogroup", "aria-label": "New mail alert",
                    for (v, label) in [
                        ("banner", if cur.mail_sound { "Banner and sound" } else { "Banner" }),
                        ("background", "Only when Coo is in the background"),
                    ] {
                        button { role: "radio", key: "{v}", disabled: !cur.notify_new_mail,
                            "aria-checked": "{cur.mail_alert == v}", "aria-selected": "{cur.mail_alert == v}",
                            onclick: move |_| { prefs::update(move |x| x.mail_alert = v.to_string()); p.set(prefs::get()); },
                            "{label}"
                        }
                    }
                }
            }
            if crate::notify::permission() == crate::notify::Permission::Denied {
                Row { label: "Blocked by macOS", hint: "macOS is blocking Coo's notifications, so banners can't appear.",
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: move |_| { let _ = open::that_detached(crate::notify::SETTINGS_URL); },
                        "Open Notification Settings" }
                }
            }
            Row { label: "Dock badge", hint: "The unread inbox count on Coo's icon. Muted accounts don't count.",
                input { class: "switch", r#type: "checkbox", checked: cur.dock_badge, "aria-label": "Dock badge",
                    onchange: move |_| {
                        set(|p| p.dock_badge = !p.dock_badge);
                        super::refresh_badge(s);
                    } }
            }
            Row { label: "Accounts", hint: "Click a bell to mute an account. Muted accounts still sync; they just stay quiet.", stack: true,
                div { class: "bell-list",
                    for a in accounts {
                        {
                            let id = a.account.id.clone();
                            let on = !cur.notify_accounts_muted.contains(&id);
                            let name = s.account_label(&a.account.id, &a.account.email);
                            let tip = if on { "Notifying · click to mute" } else { "Muted · click to notify" };
                            let addr = (name != a.account.email).then(|| a.account.email.clone());
                            let bell = if on { "bell" } else { "bell-off" };
                            rsx! {
                                div { class: "bell-list__row", key: "{id}", "data-hue": "{a.hue}", "data-muted": (!on).then_some("true"),
                                    span { class: "dot" }
                                    span { class: "bell-list__text",
                                        span { class: "bell-list__name", dir: "auto", "{name}" }
                                        if let Some(addr) = addr { span { class: "bell-list__addr", "{addr}" } }
                                    }
                                    if !on { span { class: "bell-list__state", "Muted" } }
                                    button { class: "bell-list__bell", r#type: "button", "aria-pressed": "{on}",
                                        "aria-label": "{name}: {tip}", title: "{tip}",
                                        onclick: move |_| {
                                            let id = id.clone();
                                            prefs::update(move |x| {
                                                if let Some(i) = x.notify_accounts_muted.iter().position(|m| *m == id) {
                                                    x.notify_accounts_muted.remove(i);
                                                } else {
                                                    x.notify_accounts_muted.push(id);
                                                }
                                            });
                                            p.set(prefs::get());
                                            super::refresh_badge(s);
                                        },
                                        Icon { name: bell, class: "icon--sm" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Group { title: "Events",
            Row { label: "Remind me", hint: "A reminder before meetings start, with Join.",
                div { class: "srow__inline",
                    div { class: "segmented", role: "radiogroup", "aria-label": "Event reminder",
                        for m in REMINDER_CHOICES {
                            button { role: "radio", key: "{m}", disabled: !cur.notify_events,
                                "aria-checked": "{cur.event_reminder_minutes == m}", "aria-selected": "{cur.event_reminder_minutes == m}",
                                onclick: move |_| { prefs::update(move |x| x.event_reminder_minutes = m); p.set(prefs::get()); },
                                if m == 0 { "At start" } else { "{m} min" }
                            }
                        }
                    }
                    input { class: "switch", r#type: "checkbox", checked: cur.notify_events, "aria-label": "Event reminders",
                        onchange: move |_| set(|p| p.notify_events = !p.notify_events) }
                }
            }
            Row { label: "Break through Focus", hint: "Reminders show even in Do Not Disturb and Focus. New mail never does.",
                input { class: "switch", r#type: "checkbox", checked: cur.reminders_time_sensitive, disabled: !cur.notify_events,
                    "aria-label": "Break through Focus",
                    onchange: move |_| set(|p| p.reminders_time_sensitive = !p.reminders_time_sensitive) }
            }
        }
        Group { title: "Sounds", perch: true,
            Row { label: "Sound", hint: "Each set has a new-mail sound and a reminder sound. Click one to hear it.", stack: true,
                div { class: "chips", role: "radiogroup", "aria-label": "Sound theme",
                    {
                        let choices = crate::sound::choices().into_iter().chain(std::iter::once((crate::sound::NONE, "None")));
                        rsx! {
                            for (id, label) in choices {
                                button { class: "chip", role: "radio", key: "{id}", r#type: "button",
                                    "aria-checked": "{cur.sound_theme == id}",
                                    onclick: move |_| {
                                        prefs::update(move |x| x.sound_theme = id.to_string());
                                        crate::sound::play(id, crate::sound::Kind::Mail);
                                        if let Some(c) = super::settings_bird::sound_cue(id) { super::settings_bird::cue(c); }
                                        p.set(prefs::get());
                                    },
                                    if id == crate::sound::MACOS { "macOS" } else { "{label}" }
                                }
                            }
                        }
                    }
                }
            }
            Row { label: "New mail",
                div { class: "srow__inline",
                    button { class: "btn btn--ghost btn--sm", r#type: "button", disabled: quiet, title: "Play the new-mail sound",
                        onclick: move |_| {
                            let p = prefs::get();
                            crate::sound::play(&p.sound_theme, crate::sound::Kind::Mail);
                            if let Some(c) = super::settings_bird::sound_cue(&p.sound_theme) { super::settings_bird::cue(c); }
                        },
                        Icon { name: "play", class: "icon--xs" } "Play" }
                    input { class: "switch", r#type: "checkbox", checked: cur.mail_sound, disabled: quiet, "aria-label": "New mail sound",
                        onchange: move |_| set(|p| p.mail_sound = !p.mail_sound) }
                }
            }
            Row { label: "Event reminders",
                div { class: "srow__inline",
                    button { class: "btn btn--ghost btn--sm", r#type: "button", disabled: quiet, title: "Play the reminder sound",
                        onclick: move |_| {
                            let p = prefs::get();
                            crate::sound::play(&p.sound_theme, crate::sound::Kind::Meeting);
                            if let Some(c) = super::settings_bird::sound_cue(&p.sound_theme) { super::settings_bird::cue(c); }
                        },
                        Icon { name: "play", class: "icon--xs" } "Play" }
                    input { class: "switch", r#type: "checkbox", checked: cur.meeting_sound, disabled: quiet, "aria-label": "Reminder sound",
                        onchange: move |_| set(|p| p.meeting_sound = !p.meeting_sound) }
                }
            }
            Row { label: "Try it", hint: "Posts a real notification, the way new mail arrives.",
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| crate::notify::post(&crate::notify::Note {
                        title: "Coo".into(),
                        body: "This is how new mail sounds.".into(),
                        target: crate::notify::Target::Inbox,
                        join_url: None,
                        sound: (prefs::get().sound_theme != crate::sound::NONE).then_some(crate::sound::Kind::Mail),
                        banner: true,
                    }),
                    "Send a test"
                }
            }
        }
    }
}

#[component]
pub fn Shortcuts() -> Element {
    use crate::shortcuts::{STYLE, Style};
    let style = *STYLE.read();
    let rows = crate::shortcuts::rows(style);
    rsx! {
        div { class: "settings__title", "Keyboard shortcuts" }
        Group { title: "Style",
            Row { label: "Shortcut style",
                hint: if style == Style::Gmail {
                    "Single keys, as in Gmail: R reply, A reply all, F forward, E archive."
                } else {
                    "⌘ and ⌃ keys, as in Outlook for Mac: ⌘R reply, ⇧⌘R reply all, ⌘J forward, ⌃E archive."
                },
                div { class: "segmented", role: "radiogroup", "aria-label": "Keyboard shortcut style",
                    for (value, label) in [(Style::Gmail, "Gmail"), (Style::Outlook, "Outlook")] {
                        button { role: "radio", key: "{label}", "aria-checked": "{style == value}", "aria-selected": "{style == value}",
                            onclick: move |_| crate::shortcuts::set_style(value),
                            "{label}"
                        }
                    }
                }
            }
        }
        div { class: "shortcut-groups",
            for g in crate::shortcuts::GROUPS {
                section { class: "settings-group shortcut-group", key: "{g}",
                    h3 { class: "shortcut-group__title", "{g}" }
                    for row in rows.iter().filter(|x| x.group == g) {
                        ShortcutLine { key: "{row.what}", what: row.what, keys: row.keys.clone(), action: row.action, custom: row.custom }
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

/// The action whose key is being recorded in Settings, and a note about
/// the last key tried for one (why it can't be used).
static RECORDING: GlobalSignal<Option<crate::shortcuts::Do>> = Signal::global(|| None);
static SHORTCUT_NOTE: GlobalSignal<Option<(crate::shortcuts::Do, String)>> = Signal::global(|| None);

fn record(what: Option<crate::shortcuts::Do>) {
    *RECORDING.write() = what;
    *SHORTCUT_NOTE.write() = None;
    document::eval(&format!("window.__cooRecordKey && window.__cooRecordKey({})", what.is_some()));
}

/// The bridge's recorded key ("" when Esc cancelled).
pub fn on_shortcut_recorded(key: &str) {
    let Some(what) = RECORDING.write().take() else { return };
    if key.is_empty() {
        return;
    }
    let style = *crate::shortcuts::STYLE.peek();
    match crate::shortcuts::conflict(style, what, key) {
        Some(why) => *SHORTCUT_NOTE.write() = Some((what, why)),
        None => crate::shortcuts::set_custom(style, what, Some(key.to_string())),
    }
}

/// One shortcut in the list. An action with no built-in key can take the
/// user's: Add, or click the key to change it, × to remove it.
#[component]
fn ShortcutLine(what: &'static str, keys: String, action: crate::shortcuts::Do, custom: bool) -> Element {
    let recording = *RECORDING.read() == Some(action);
    let note = SHORTCUT_NOTE.read().as_ref().filter(|(a, _)| *a == action).map(|(_, why)| why.clone());
    rsx! {
        div { class: "shortcut", "data-custom": custom.then_some("true"),
            span { class: "shortcut__what", "{what}" }
            if !custom {
                kbd { class: "kbd", "{keys}" }
            } else if recording {
                span { class: "shortcut__rec", "Press the keys… (Esc cancels)" }
                button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| record(None), "Cancel" }
            } else if keys.is_empty() {
                button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| record(Some(action)), "Add shortcut" }
            } else {
                button { class: "kbd kbd--button", r#type: "button", title: "Change", onclick: move |_| record(Some(action)), "{keys}" }
                button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Remove shortcut", title: "Remove",
                    onclick: move |_| crate::shortcuts::set_custom(*crate::shortcuts::STYLE.peek(), action, None),
                    super::Icon { name: "x", class: "icon--xs" }
                }
            }
        }
        if let Some(why) = note {
            div { class: "shortcut__note", role: "status", "{why}" }
        }
    }
}

/// Settings › Privacy: remote images, trusted senders, crash reports.
#[component]
pub fn Privacy() -> Element {
    let s = use_context::<AppState>();
    let mut p = use_signal(prefs::get);
    let cur = p.read().clone();
    let always = cur.remote_images_always;
    rsx! {
        div { class: "settings__title", "Privacy" }
        Group { title: "Remote images",
            Row { label: "Remote images", hint: "They let senders see when you open a message.",
                div { class: "segmented", role: "radiogroup", "aria-label": "Remote images",
                    button { role: "radio", "aria-checked": "{!always}", "aria-selected": "{!always}",
                        onclick: move |_| { prefs::update(|x| x.remote_images_always = false); p.set(prefs::get()); }, "Ask" }
                    button { role: "radio", "aria-checked": "{always}", "aria-selected": "{always}",
                        onclick: move |_| { prefs::update(|x| x.remote_images_always = true); p.set(prefs::get()); }, "Always load" }
                }
            }
            if !always {
                Row { label: "Trusted senders",
                    hint: if cur.remote_images_senders.is_empty() { "None yet. Use “Always from…” on a message's image banner." } else { "Their images always load." },
                    stack: !cur.remote_images_senders.is_empty(),
                    div { class: "trusted-list",
                        for who in cur.remote_images_senders.clone() {
                            div { class: "trusted-list__row", key: "{who}",
                                span { class: "spacer", "{who}" }
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
        }
        Group { title: "Sharing",
            Row { label: "Crash reports", hint: "Anonymous: addresses and message text are removed. Applies next launch.",
                input { class: "switch", r#type: "checkbox", checked: cur.crash_reports, "aria-label": "Crash reports",
                    onchange: move |_| { prefs::update(|x| x.crash_reports = !x.crash_reports); p.set(prefs::get()); } }
            }
            Row { label: "Sender photos", hint: "Gravatar, company logos and contact photos are in Appearance.",
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| super::settings::show_page(s, "Appearance"), "Open" }
            }
        }
    }
}
