//! Event editor: create and edit events, with a free/busy strip for guests.
//!
//! The form keeps raw strings (date, times, guests) so typing never fights
//! parsing; `to_draft` validates and converts on save.

use std::time::Duration;

use chrono::{Local, NaiveDate, NaiveTime, TimeZone};
use dioxus::prelude::*;
use pigeon_core::model::{Attendee, EventDraft, EventRow, Response};
use pigeon_core::text::parse_address_list;
use pigeon_engine::service::Cmd;

use super::{DAY_MS, Icon};
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, next_token};

/// First hour and number of hours the availability strip shows.
const STRIP_FIRST_HOUR: u32 = 8;
const STRIP_SPAN: u32 = 10;
const FREE_BUSY_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventForm {
    /// (account_id, event_id) when editing an existing event.
    pub editing: Option<(String, String)>,
    pub account_id: String,
    /// Empty = the account's primary calendar.
    pub calendar_id: String,
    pub title: String,
    /// `YYYY-MM-DD`
    pub date: String,
    /// `HH:MM`
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub location: String,
    pub description: String,
    /// Comma-separated addresses.
    pub guests: String,
}

fn hhmm(minutes: u32) -> String {
    let m = minutes.min(24 * 60 - 1);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Minutes since midnight for a click `y` px into a day column, floored to
/// the half hour.
pub fn slot_from_offset(y: f64, hour_px: f64) -> u32 {
    let minutes = (y.max(0.0) / hour_px * 60.0) as u32;
    (minutes / 30 * 30).min(23 * 60 + 30)
}

/// A new one-hour event at `minutes` on `day`.
pub fn form_for_slot(day: NaiveDate, minutes: u32, account_id: &str, calendar_id: &str) -> EventForm {
    EventForm {
        account_id: account_id.to_string(),
        calendar_id: calendar_id.to_string(),
        date: day.format("%Y-%m-%d").to_string(),
        start: hhmm(minutes),
        end: hhmm(minutes + 60),
        ..Default::default()
    }
}

/// Prefill from an existing event (times shown in local time).
pub fn form_from_event(row: &EventRow) -> EventForm {
    let e = &row.event;
    let (date, start, end) = if e.all_day {
        let d = chrono::DateTime::from_timestamp_millis(e.start_at).map(|d| d.date_naive()).unwrap_or_default();
        (d, String::new(), String::new())
    } else {
        let s = format::local(e.start_at).unwrap_or_else(Local::now);
        (s.date_naive(), format::hhmm(e.start_at), format::hhmm(e.end_at))
    };
    EventForm {
        editing: Some((row.account_id.clone(), e.id.clone())),
        account_id: row.account_id.clone(),
        calendar_id: e.calendar_id.clone(),
        title: e.title.clone(),
        date: date.format("%Y-%m-%d").to_string(),
        start,
        end,
        all_day: e.all_day,
        location: e.location.clone().unwrap_or_default(),
        description: String::new(),
        guests: e.attendees.iter().map(|a| a.address.email.clone()).collect::<Vec<_>>().join(", "),
    }
}

fn local_ms(date: NaiveDate, time: NaiveTime) -> Option<i64> {
    Local.from_local_datetime(&date.and_time(time)).earliest().map(|d| d.timestamp_millis())
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// Validate the form and build what the engine sends.
pub fn to_draft(f: &EventForm) -> Result<EventDraft, String> {
    let title = f.title.trim();
    if title.is_empty() {
        return Err("Add a title".into());
    }
    let date = parse_date(&f.date).ok_or("Pick a date")?;
    let (start_at, end_at) = if f.all_day {
        let start = date.and_hms_opt(0, 0, 0).ok_or("Pick a date")?.and_utc().timestamp_millis();
        (start, start + DAY_MS)
    } else {
        let start = parse_time(&f.start).ok_or("Start time looks wrong (HH:MM)")?;
        let end = parse_time(&f.end).ok_or("End time looks wrong (HH:MM)")?;
        let s = local_ms(date, start).ok_or("That start time doesn't exist today (DST)")?;
        let e = local_ms(date, end).ok_or("That end time doesn't exist today (DST)")?;
        if e <= s {
            return Err("End must be after start".into());
        }
        (s, e)
    };
    let attendees = parse_address_list(&f.guests)
        .into_iter()
        .map(|address| Attendee { address, response: Response::None, optional: false })
        .collect();
    let opt = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
    Ok(EventDraft {
        calendar_id: f.calendar_id.clone(),
        title: title.to_string(),
        start_at,
        end_at,
        all_day: f.all_day,
        timezone: None,
        location: opt(&f.location),
        description: opt(&f.description),
        attendees,
        conferencing: Default::default(),
    })
}

/// One-line footer summary.
pub fn summary(f: &EventForm) -> String {
    let day = parse_date(&f.date).map(|d| d.format("%a, %b %-d").to_string()).unwrap_or_default();
    let when = if f.all_day { "all day".to_string() } else { format!("{}–{}", f.start, f.end) };
    let guests = parse_address_list(&f.guests).len();
    match guests {
        0 => format!("{day} · {when}"),
        1 => format!("{day} · {when} · 1 guest"),
        n => format!("{day} · {when} · {n} guests"),
    }
}

/// Busy intervals clipped to the strip, in hours from the strip's first
/// hour: (start, len, clashes with [slot_from, slot_to)).
pub fn strip_blocks(busy: &[(i64, i64)], strip_start: i64, slot: Option<(i64, i64)>) -> Vec<(f64, f64, bool)> {
    let strip_end = strip_start + i64::from(STRIP_SPAN) * 3_600_000;
    busy.iter()
        .filter(|(s, e)| *e > strip_start && *s < strip_end)
        .map(|&(s, e)| {
            let (s, e) = (s.max(strip_start), e.min(strip_end));
            let clash = slot.is_some_and(|(a, b)| s < b && e > a);
            ((s - strip_start) as f64 / 3_600_000.0, (e - s) as f64 / 3_600_000.0, clash)
        })
        .collect()
}

/// Open a blank one-hour event on `day` at `minutes`, on the first writable
/// calendar (primary first) the store knows about.
pub fn open_new(s: AppState, day: NaiveDate, minutes: u32) {
    let cals = s.calendar.read().calendars.clone();
    let mut writable: Vec<_> = cals.iter().filter(|(_, c)| c.can_edit).collect();
    writable.sort_by_key(|(_, c)| !c.is_primary);
    let (account_id, calendar_id) = match writable.first() {
        Some((a, c)) => (a.clone(), c.id.clone()),
        None => (s.accounts.read().first().map(|a| a.account.id.clone()).unwrap_or_default(), String::new()),
    };
    let mut editor = s.editor;
    editor.set(Some(form_for_slot(day, minutes, &account_id, &calendar_id)));
    crate::bridge::focus("event-title");
}

/// Next round hour today.
pub fn open_new_now(s: AppState) {
    let now = Local::now();
    let minutes = (now.time().format("%H").to_string().parse::<u32>().unwrap_or(9) + 1) * 60;
    open_new(s, now.date_naive(), minutes.min(23 * 60));
}

pub fn open_edit(s: AppState, row: &EventRow) {
    let mut editor = s.editor;
    editor.set(Some(form_from_event(row)));
}

fn close(s: AppState) {
    let mut editor = s.editor;
    editor.set(None);
}

#[component]
pub fn EventEditor() -> Element {
    let s = use_context::<AppState>();
    let mut editor = s.editor;
    let mut error = use_signal(|| None::<String>);
    let mut fb_token = use_signal(|| 0u64);
    let Some(form) = editor.read().clone() else { return rsx! {} };

    // Debounced free/busy for the guests over the strip window.
    let fb_key = (form.account_id.clone(), form.guests.clone(), form.date.clone());
    use_effect(use_reactive!(|fb_key| {
        let (account_id, guests, date) = fb_key;
        let emails: Vec<String> = parse_address_list(&guests).into_iter().map(|a| a.email).collect();
        let token = next_token();
        fb_token.set(token);
        let Some(day) = parse_date(&date) else { return };
        let Some(from) = local_ms(day, NaiveTime::from_hms_opt(STRIP_FIRST_HOUR, 0, 0).unwrap_or_default()) else {
            return;
        };
        if emails.is_empty() || account_id.is_empty() {
            return;
        }
        spawn(async move {
            tokio::time::sleep(FREE_BUSY_DEBOUNCE).await;
            if *fb_token.peek() == token {
                send_cmd(Cmd::FreeBusy {
                    account_id,
                    emails,
                    from,
                    to: from + i64::from(STRIP_SPAN) * 3_600_000,
                    token,
                });
            }
        });
    }));

    let hue = s.hue(&form.account_id);
    let calendars = s.calendar.read().calendars.clone();
    let accounts = s.accounts.read().clone();
    let editing = form.editing.clone();
    let guests = parse_address_list(&form.guests);
    let strip_start = parse_date(&form.date)
        .and_then(|d| local_ms(d, NaiveTime::from_hms_opt(STRIP_FIRST_HOUR, 0, 0).unwrap_or_default()))
        .unwrap_or_default();
    let slot = to_draft(&EventForm { title: "x".into(), ..form.clone() })
        .ok()
        .filter(|d| !d.all_day)
        .map(|d| (d.start_at, d.end_at));
    let (token, fb) = s.free_busy.read().clone();
    let fb = if token == *fb_token.read() { fb } else { Vec::new() };
    let clashes = fb.iter().filter(|(_, busy)| strip_blocks(busy, strip_start, slot).iter().any(|b| b.2)).count();
    let calendar_value = format!("{}|{}", form.account_id, form.calendar_id);

    let mut set = move |f: fn(&mut EventForm, String), v: String| {
        if let Some(form) = editor.write().as_mut() {
            f(form, v);
        }
        error.set(None);
    };
    let save = move |_| {
        let Some(form) = editor.read().clone() else { return };
        match to_draft(&form) {
            Ok(draft) => {
                match &form.editing {
                    Some((account_id, event_id)) => {
                        send_cmd(Cmd::UpdateEvent { account_id: account_id.clone(), event_id: event_id.clone(), draft })
                    }
                    None => send_cmd(Cmd::CreateEvent { account_id: form.account_id.clone(), draft }),
                }
                close(s);
            }
            Err(e) => error.set(Some(e)),
        }
    };

    rsx! {
        div { class: "composer-overlay editor-overlay", onclick: move |_| close(s),
            div { class: "event-editor", role: "dialog", "aria-label": "Event", onclick: move |e| e.stop_propagation(),
                div { class: "event-editor__header",
                    span { class: "spacer t-2", style: "font-size:var(--fs-ui);font-weight:var(--fw-semibold)",
                        if editing.is_some() { "Edit event" } else { "New event" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)", onclick: move |_| close(s), Icon { name: "x" } }
                }
                div { class: "event-editor__body",
                    input { class: "input--bare event-editor__title", id: "event-title", dir: "auto", placeholder: "Add a title",
                        value: "{form.title}", oninput: move |e| set(|f, v| f.title = v, e.value()) }

                    div { class: "ee-row",
                        Icon { name: "cal", class: "ee-row__icon" }
                        div { class: "ee-row__content", "data-hue": "{hue}",
                            span { class: "dot" }
                            select { class: "select", value: "{calendar_value}", disabled: editing.is_some(),
                                onchange: move |e| {
                                    let v = e.value();
                                    if let Some((a, c)) = v.split_once('|') {
                                        let (a, c) = (a.to_string(), c.to_string());
                                        if let Some(form) = editor.write().as_mut() { form.account_id = a; form.calendar_id = c; }
                                    }
                                },
                                for a in accounts.iter() {
                                    optgroup { label: "{a.account.email}",
                                        for (_, c) in calendars.iter().filter(|(id, c)| id == &a.account.id && c.can_edit) {
                                            option { value: "{a.account.id}|{c.id}", selected: a.account.id == form.account_id && c.id == form.calendar_id, "{c.name}" }
                                        }
                                        if !calendars.iter().any(|(id, c)| id == &a.account.id && c.can_edit) {
                                            option { value: "{a.account.id}|", selected: a.account.id == form.account_id && form.calendar_id.is_empty(), "Default calendar" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "ee-row",
                        Icon { name: "clock", class: "ee-row__icon" }
                        div { class: "ee-row__content",
                            input { class: "input", r#type: "date", value: "{form.date}", oninput: move |e| set(|f, v| f.date = v, e.value()) }
                            if !form.all_day {
                                input { class: "input", r#type: "time", value: "{form.start}", step: "300",
                                    oninput: move |e| set(|f, v| f.start = v, e.value()) }
                                span { class: "t-3", "–" }
                                input { class: "input", r#type: "time", value: "{form.end}", step: "300",
                                    oninput: move |e| set(|f, v| f.end = v, e.value()) }
                            }
                            label { class: "ee-row__note", style: "display:inline-flex;align-items:center;gap:8px;margin-left:8px",
                                input { class: "switch", r#type: "checkbox", checked: form.all_day,
                                    onchange: move |e| { let on = e.checked(); if let Some(f) = editor.write().as_mut() { f.all_day = on; } } }
                                "All day"
                            }
                        }
                    }

                    div { class: "ee-row ee-row--top",
                        Icon { name: "users", class: "ee-row__icon" }
                        div { class: "ee-row__content", style: "flex-direction:column;align-items:stretch",
                            input { class: "input", dir: "auto", placeholder: "Add guests: name@example.com, …", value: "{form.guests}",
                                oninput: move |e| set(|f, v| f.guests = v, e.value()) }
                            if !guests.is_empty() {
                                div { style: "display:flex;gap:6px;flex-wrap:wrap;margin-top:8px",
                                    for g in guests.iter() {
                                        span { class: "guest-chip", key: "{g.email}",
                                            span { class: "avatar avatar--22", "data-hue": "{hue}", "{format::initials(g.display())}" }
                                            span { dir: "auto", "{g.display()}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !guests.is_empty() {
                        Availability { guests: guests.iter().map(|g| g.email.clone()).collect::<Vec<_>>(), fb: fb.clone(), strip_start, slot, clashes }
                    }

                    div { class: "ee-row",
                        Icon { name: "pin", class: "ee-row__icon" }
                        div { class: "ee-row__content",
                            input { class: "input--bare", style: "flex:1", dir: "auto", placeholder: "Add location", value: "{form.location}",
                                oninput: move |e| set(|f, v| f.location = v, e.value()) }
                        }
                    }

                    div { class: "ee-row ee-row--top",
                        Icon { name: "video", class: "ee-row__icon" }
                        div { class: "ee-row__content", style: "flex-direction:column;align-items:flex-start;gap:6px",
                            div { class: "segmented conf-picker", role: "radiogroup", "aria-label": "Conferencing",
                                button { role: "radio", "aria-checked": "true", "aria-selected": "true", "None" }
                                button { role: "radio", "aria-checked": "false", disabled: true, "Zoom" }
                                button { role: "radio", "aria-checked": "false", disabled: true, "Google Meet" }
                                button { role: "radio", "aria-checked": "false", disabled: true, "Microsoft Teams" }
                            }
                            span { class: "conf-note", "Zoom, Google Meet and Teams links arrive in phase 2." }
                        }
                    }

                    div { class: "ee-row ee-row--top",
                        Icon { name: "text", class: "ee-row__icon" }
                        div { class: "ee-row__content",
                            textarea { class: "input", style: "flex:1;min-height:88px", dir: "auto", placeholder: "Description",
                                value: "{form.description}", oninput: move |e| set(|f, v| f.description = v, e.value()) }
                        }
                    }
                }
                div { class: "event-editor__footer",
                    span { class: "event-editor__summary",
                        match error.read().clone() {
                            Some(e) => rsx! { span { style: "color:var(--error)", "{e}" } },
                            None => rsx! { "{summary(&form)}" },
                        }
                    }
                    if let Some((account_id, event_id)) = editing.clone() {
                        {
                            let calendar_id = form.calendar_id.clone();
                            rsx! {
                                button { class: "btn btn--ghost", r#type: "button", style: "color:var(--error)",
                                    onclick: move |_| {
                                        send_cmd(Cmd::DeleteEvent { account_id: account_id.clone(), calendar_id: calendar_id.clone(), event_id: event_id.clone() });
                                        close(s);
                                    },
                                    Icon { name: "trash", class: "icon--sm" } "Delete"
                                }
                            }
                        }
                    }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| close(s), "Cancel" }
                    button { class: "btn btn--primary", r#type: "button", onclick: save,
                        if editing.is_some() { "Save" } else { "Create event" }
                    }
                }
            }
        }
    }
}

#[component]
fn Availability(
    guests: Vec<String>,
    fb: pigeon_core::model::FreeBusy,
    strip_start: i64,
    slot: Option<(i64, i64)>,
    clashes: usize,
) -> Element {
    let slot_pos = slot.map(|(a, b)| ((a - strip_start) as f64 / 3_600_000.0, (b - a) as f64 / 3_600_000.0));
    rsx! {
        div { class: "availability", style: "--span:{STRIP_SPAN}",
            div { class: "availability__head",
                span { class: "spacer", "Availability" }
                span { class: "availability__legend", i { style: "background:rgba(163,166,174,.38)" } "Busy" }
                span { class: "availability__legend", i { class: "unknown" } "Unknown" }
            }
            div { class: "availability__grid",
                div { class: "availability__hours",
                    for h in 0..STRIP_SPAN { span { key: "{h}", "{STRIP_FIRST_HOUR + h}" } }
                }
                for g in guests.iter() {
                    {
                        let busy = fb.iter().find(|(e, _)| e.eq_ignore_ascii_case(g)).map(|(_, b)| b.clone());
                        rsx! {
                            div { class: "avail-row", key: "{g}",
                                span { class: "avail-row__name", dir: "auto", "{g}" }
                                div { class: "avail-row__track",
                                    match busy {
                                        None => rsx! { div { class: "avail-block", "data-kind": "unknown", style: "--start:0;--len:{STRIP_SPAN}" } },
                                        Some(b) => rsx! {
                                            for (i, (start, len, clash)) in strip_blocks(&b, strip_start, slot).into_iter().enumerate() {
                                                div { class: "avail-block", key: "{i}", "data-kind": if clash { "clash" } else { "busy" },
                                                    style: "--start:{start};--len:{len}" }
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some((start, len)) = slot_pos {
                    if start + len > 0.0 && start < f64::from(STRIP_SPAN) {
                        div { class: "avail-slot", style: "--start:{start.max(0.0)};--len:{len.min(f64::from(STRIP_SPAN) - start.max(0.0))}" }
                    }
                }
            }
            if clashes > 0 {
                div { class: "availability__foot",
                    span { class: "availability__warn", Icon { name: "alert", class: "icon--sm" }
                        if clashes == 1 { "1 guest is busy then" } else { "{clashes} guests are busy then" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> EventForm {
        EventForm {
            title: "Sync".into(),
            date: "2026-10-23".into(),
            start: "14:00".into(),
            end: "15:30".into(),
            guests: "a@x.com, Bob <b@y.org>".into(),
            ..Default::default()
        }
    }

    #[test]
    fn slots_from_clicks() {
        assert_eq!(slot_from_offset(0.0, 60.0), 0);
        assert_eq!(slot_from_offset(629.0, 60.0), 10 * 60);
        assert_eq!(slot_from_offset(640.0, 60.0), 10 * 60 + 30);
        assert_eq!(slot_from_offset(-5.0, 60.0), 0);
        assert_eq!(slot_from_offset(5000.0, 60.0), 23 * 60 + 30);
        let f = form_for_slot(NaiveDate::from_ymd_opt(2026, 10, 23).unwrap(), 23 * 60 + 30, "a", "c");
        assert_eq!((f.start.as_str(), f.end.as_str(), f.date.as_str()), ("23:30", "23:59", "2026-10-23"));
    }

    #[test]
    fn drafts_validate_and_convert() {
        let d = to_draft(&form()).unwrap();
        assert_eq!(d.end_at - d.start_at, 90 * 60_000);
        assert_eq!(d.attendees.len(), 2);
        assert_eq!(d.attendees[1].address.name.as_deref(), Some("Bob"));
        assert_eq!(d.location, None);

        let all_day = to_draft(&EventForm { all_day: true, start: String::new(), ..form() }).unwrap();
        assert_eq!(all_day.end_at - all_day.start_at, DAY_MS);
        assert_eq!(all_day.start_at % DAY_MS, 0, "UTC midnight");

        assert_eq!(to_draft(&EventForm { title: " ".into(), ..form() }).unwrap_err(), "Add a title");
        assert_eq!(to_draft(&EventForm { end: "13:00".into(), ..form() }).unwrap_err(), "End must be after start");
        assert!(to_draft(&EventForm { start: "2pm".into(), ..form() }).is_err());
        assert!(to_draft(&EventForm { date: "tomorrow".into(), ..form() }).is_err());
    }

    #[test]
    fn summary_line() {
        assert_eq!(summary(&form()), "Fri, Oct 23 · 14:00–15:30 · 2 guests");
        assert_eq!(summary(&EventForm { all_day: true, guests: String::new(), ..form() }), "Fri, Oct 23 · all day");
    }

    #[test]
    fn strip_blocks_clip_and_flag_clashes() {
        let h = 3_600_000;
        let start = 100 * h;
        let busy = [(start - h, start + h), (start + 3 * h, start + 4 * h), (start + 20 * h, start + 21 * h)];
        let blocks = strip_blocks(&busy, start, Some((start + 3 * h + h / 2, start + 5 * h)));
        assert_eq!(blocks, vec![(0.0, 1.0, false), (3.0, 1.0, true)]);
    }

    #[test]
    fn prefill_from_event_roundtrips() {
        let base = to_draft(&form()).unwrap();
        let row = EventRow {
            account_id: "acct".into(),
            event: pigeon_core::model::Event {
                id: "e1".into(),
                calendar_id: "cal".into(),
                title: "Sync".into(),
                start_at: base.start_at,
                end_at: base.end_at,
                attendees: base.attendees.clone(),
                ..Default::default()
            },
        };
        let f = form_from_event(&row);
        assert_eq!(f.editing, Some(("acct".into(), "e1".into())));
        assert_eq!((f.start.as_str(), f.end.as_str()), ("14:00", "15:30"));
        assert_eq!(f.guests, "a@x.com, b@y.org");
        let again = to_draft(&f).unwrap();
        assert_eq!((again.start_at, again.end_at), (base.start_at, base.end_at));
    }
}
