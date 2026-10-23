//! Settings sections beyond Appearance/Accounts: Composing (undo send,
//! signatures), Notifications, Keyboard shortcuts, Privacy.

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

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

#[component]
pub fn Composing() -> Element {
    let s = use_context::<AppState>();
    use_hook(move || {
        if s.senders.read().is_empty() {
            send_cmd(Cmd::LoadSenders);
        }
    });
    let mut undo = use_signal(|| prefs::get().undo_send_seconds);
    let senders = s.senders.read().clone();
    let current = *undo.read();

    rsx! {
        div { class: "settings__title", style: "margin-top:12px", "Composing" }
        div { class: "settings-group",
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
            div { class: "settings-field settings-field--top",
                span { class: "settings-field__label", "Signatures" }
                div { style: "flex:1;display:flex;flex-direction:column;gap:10px",
                    if senders.is_empty() { span { class: "settings-field__hint", "Loading your addresses…" } }
                    for sender in senders {
                        SignatureField { key: "{sender.address.email}", email: sender.address.email.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn SignatureField(email: String) -> Element {
    let key = email.to_lowercase();
    let initial = prefs::get().signatures.get(&key).cloned().unwrap_or_default();
    let mut text = use_signal(|| initial);
    rsx! {
        label { style: "display:flex;flex-direction:column;gap:4px",
            span { class: "settings-field__hint", style: "margin:0", "{email}" }
            textarea { class: "input", dir: "auto", rows: "3", style: "min-height:64px;resize:vertical",
                placeholder: "Signature for {email} (plain text; each line keeps its own direction)",
                value: "{text}",
                oninput: move |e| {
                    let v = e.value();
                    text.set(v.clone());
                    let k = key.clone();
                    prefs::update(move |p| {
                        if v.trim().is_empty() { p.signatures.remove(&k); } else { p.signatures.insert(k, v); }
                    });
                }
            }
        }
    }
}
