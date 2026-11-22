//! Settings › Notifications, meeting alerts (meeting_alerts.rs): how loud,
//! each step on its own, the join shortcut, and which meetings alert.

use dioxus::prelude::*;

use super::settings::{Group, Row};
use crate::meeting_alerts::{HOTKEYS, Level};
use crate::prefs::{self, Prefs};
use crate::state::AppState;

const COUNTDOWN_CHOICES: [u32; 4] = [5, 10, 15, 30];
const BANNER_CHOICES: [u32; 3] = [1, 2, 5];
const AGAIN_CHOICES: [u32; 3] = [1, 2, 5];

/// A row of minute choices plus its on/off switch.
#[component]
fn Minutes(
    label: String,
    choices: Vec<u32>,
    value: u32,
    on: bool,
    #[props(default)] disabled: bool,
    pick: EventHandler<u32>,
    toggle: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "srow__inline",
            div { class: "segmented", role: "radiogroup", "aria-label": "{label}",
                for m in choices {
                    button { role: "radio", key: "{m}", r#type: "button", disabled: !on || disabled,
                        "aria-checked": "{value == m}", "aria-selected": "{value == m}",
                        onclick: move |_| pick.call(m), "{m} min" }
                }
            }
            input { class: "switch", r#type: "checkbox", checked: on, disabled, "aria-label": "{label}",
                onchange: move |_| toggle.call(()) }
        }
    }
}

#[component]
pub fn MeetingAlerts() -> Element {
    let s = use_context::<AppState>();
    let mut p = use_signal(prefs::get);
    let cur = p.read().clone();
    let mut set = move |f: Box<dyn FnOnce(&mut Prefs)>| {
        prefs::update(f);
        p.set(prefs::get());
        s.meeting_changed();
    };
    // Calendars are only listed once the calendar has been shown: ask for
    // them, as Settings › Calendar does.
    use_hook(move || {
        if s.calendar.peek().calendars.is_empty() {
            let d = chrono::Local::now().date_naive();
            let (from, to) = super::calendar::view_range(crate::state::CalView::Week, d, *s.first_weekday.peek());
            crate::send_cmd(pigeon_engine::service::Cmd::ShowCalendar { from, to });
        }
    });
    let level = Level::of(&cur);
    let level_hint = level.map(Level::hint).unwrap_or("Custom: each step is set below.");
    let order: Vec<String> = s.accounts.read().iter().map(|a| a.account.id.clone()).collect();
    let calendars = super::event_editor::in_sidebar_order(&s.calendar.read().calendars, &order);
    let names: Vec<(String, String)> = calendars
        .iter()
        .map(|(account, c)| {
            let key = format!("{account}|{}", c.id);
            let who = s
                .accounts
                .read()
                .iter()
                .find(|a| a.account.id == *account)
                .map(|a| s.account_label(&a.account.id, &a.account.email));
            (
                key,
                match who {
                    Some(w) => format!("{} · {w}", c.name),
                    None => c.name.clone(),
                },
            )
        })
        .collect();
    rsx! {
        Group { title: "Getting your attention",
            Row { label: "How loud", hint: "{level_hint}",
                div { class: "segmented", role: "radiogroup", "aria-label": "How loud",
                    for l in Level::ALL {
                        button { role: "radio", key: "{l.label()}", r#type: "button",
                            "aria-checked": "{level == Some(l)}", "aria-selected": "{level == Some(l)}",
                            onclick: move |_| set(Box::new(move |x| l.apply(x))), "{l.label()}" }
                    }
                }
            }
            Row { label: "Countdown in the menu bar", hint: "The meeting and minutes left, next to Coo's icon, from this long before.",
                Minutes { label: "Countdown in the menu bar", choices: COUNTDOWN_CHOICES.to_vec(), value: cur.meeting_countdown_minutes, on: cur.meeting_countdown,
                    pick: move |m| set(Box::new(move |x| x.meeting_countdown_minutes = m)),
                    toggle: move |_| set(Box::new(|x| x.meeting_countdown = !x.meeting_countdown)) }
            }
            Row { label: "Next up warms up", hint: "The card at the bottom of the sidebar changes color as the meeting gets close.",
                input { class: "switch", r#type: "checkbox", checked: cur.meeting_card_warm, "aria-label": "Next up warms up",
                    onchange: move |_| set(Box::new(|x| x.meeting_card_warm = !x.meeting_card_warm)) }
            }
            Row { label: "Banner in Coo", hint: "Across the top of Coo, with Join, In 1 min and Not going.",
                Minutes { label: "Banner in Coo", choices: BANNER_CHOICES.to_vec(), value: cur.meeting_banner_minutes, on: cur.meeting_banner,
                    pick: move |m| set(Box::new(move |x| x.meeting_banner_minutes = m)),
                    toggle: move |_| set(Box::new(|x| x.meeting_banner = !x.meeting_banner)) }
            }
            Row { label: "While you're writing", hint: "When a meeting starts, a strip in the message offers Save draft & join.",
                input { class: "switch", r#type: "checkbox", checked: cur.meeting_composer_strip, "aria-label": "While you're writing",
                    onchange: move |_| set(Box::new(|x| x.meeting_composer_strip = !x.meeting_composer_strip)) }
            }
            Row { label: "Panel over every app", hint: "When a meeting starts, even with Coo in the background. It stays until you join or dismiss it.",
                input { class: "switch", r#type: "checkbox", checked: cur.meeting_panel, "aria-label": "Panel over every app",
                    onchange: move |_| set(Box::new(|x| x.meeting_panel = !x.meeting_panel)) }
            }
            Row { label: "Bring the panel back", hint: "Once, this long after the start, if you put it away without joining.",
                Minutes { label: "Bring the panel back", choices: AGAIN_CHOICES.to_vec(), value: cur.meeting_panel_again_minutes, on: cur.meeting_panel_again,
                    disabled: !cur.meeting_panel,
                    pick: move |m| set(Box::new(move |x| x.meeting_panel_again_minutes = m)),
                    toggle: move |_| set(Box::new(|x| x.meeting_panel_again = !x.meeting_panel_again)) }
            }
            Row { label: "Join from any app", hint: "Joins the meeting that's on, or about to start, wherever you are. ⇧J joins inside Coo.",
                div { class: "segmented", role: "radiogroup", "aria-label": "Join from any app",
                    for (value, keys) in HOTKEYS {
                        button { role: "radio", key: "{keys}", r#type: "button",
                            "aria-checked": "{cur.meeting_join_hotkey == value}", "aria-selected": "{cur.meeting_join_hotkey == value}",
                            onclick: move |_| {
                                set(Box::new(move |x| x.meeting_join_hotkey = value.to_string()));
                                crate::meeting_panel::apply_hotkey();
                            },
                            "{keys}" }
                    }
                }
            }
        }
        Group { title: "Which meetings",
            Row { label: "Tentative or not answered", hint: "Off: they stop at the reminder. On: they get every step, like accepted ones.",
                input { class: "switch", r#type: "checkbox", checked: cur.meeting_alert_tentative, "aria-label": "Tentative or not answered",
                    onchange: move |_| set(Box::new(|x| x.meeting_alert_tentative = !x.meeting_alert_tentative)) }
            }
            Row { label: "Just you, no meeting link", hint: "Off: focus time and reminders to yourself stop at the reminder.",
                input { class: "switch", r#type: "checkbox", checked: cur.meeting_alert_solo, "aria-label": "Just you, no meeting link",
                    onchange: move |_| set(Box::new(|x| x.meeting_alert_solo = !x.meeting_alert_solo)) }
            }
            Row { label: "Calendars that stay quiet", hint: "Their events only show in Next up: no reminder, no alerts.", stack: true,
                div { class: "quiet-calendars",
                    for (key, name) in names {
                        label { class: "check-row", key: "{key}",
                            input { r#type: "checkbox", checked: cur.meeting_quiet_calendars.contains(&key),
                                onchange: {
                                    let key = key.clone();
                                    move |_| {
                                        let key = key.clone();
                                        set(Box::new(move |x| {
                                            if let Some(i) = x.meeting_quiet_calendars.iter().position(|k| *k == key) {
                                                x.meeting_quiet_calendars.remove(i);
                                            } else {
                                                x.meeting_quiet_calendars.push(key);
                                            }
                                        }))
                                    }
                                } }
                            span { dir: "auto", "{name}" }
                        }
                    }
                }
            }
        }
    }
}
