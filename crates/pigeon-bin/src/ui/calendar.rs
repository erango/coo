//! Calendar: Day / Week / Month views, event popover, sidebar mini month.
//!
//! Drag on a time grid (empty space to create, an event to move, its
//! bottom edge to resize) is tracked by the bridge, which draws a ghost and
//! sends `cal-create | cal-move | cal-resize` on drop; snapping and the new
//! times are computed here.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};
use dioxus::prelude::*;
use pigeon_core::model::{Attendee, EventDraft, EventRow, Response};
use pigeon_engine::service::Cmd;

use super::account_menu::Target;
use super::{DAY_MS, Icon};
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, CalView};
use crate::week;

const HOUR_PX: f64 = 60.0;
/// Drag snapping, minutes.
const SNAP_MIN: i64 = 15;
/// All-day lanes shown before the rest collapse into "+N more" per day.
const ALLDAY_LANES: usize = 4;

/// All-day row layout: each span (first..=last day index) gets the lowest
/// lane free on all its days, so multi-day events keep one lane. With more
/// than `max` lanes, the last visible lane becomes per-day "+N more".
#[derive(Debug, PartialEq, Eq)]
pub struct AlldayLayout {
    /// Lane per input span, None when collapsed into "+N more".
    pub lanes: Vec<Option<usize>>,
    /// Lanes drawn (including the "+N more" lane).
    pub rows: usize,
    /// Hidden events per day (only when collapsing).
    pub more: Vec<usize>,
}

pub fn layout_allday(spans: &[(usize, usize)], days: usize, max: usize) -> AlldayLayout {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    // Earlier first; on ties the longer span takes the upper lane.
    order.sort_by_key(|&i| (spans[i].0, std::cmp::Reverse(spans[i].1)));
    let mut lane_end: Vec<usize> = Vec::new(); // last day used, per lane
    let mut lanes = vec![0usize; spans.len()];
    for &i in &order {
        let (a, b) = spans[i];
        match lane_end.iter().position(|&end| end < a) {
            Some(l) => {
                lane_end[l] = b;
                lanes[i] = l;
            }
            None => {
                lane_end.push(b);
                lanes[i] = lane_end.len() - 1;
            }
        }
    }
    let used = lane_end.len();
    if used <= max {
        return AlldayLayout { lanes: lanes.into_iter().map(Some).collect(), rows: used.max(1), more: vec![0; days] };
    }
    let keep = max.saturating_sub(1);
    let mut more = vec![0usize; days];
    let lanes = lanes
        .iter()
        .zip(spans)
        .map(|(&l, &(a, b))| {
            if l < keep {
                Some(l)
            } else {
                for d in more.iter_mut().take(b.min(days.saturating_sub(1)) + 1).skip(a) {
                    *d += 1;
                }
                None
            }
        })
        .collect();
    AlldayLayout { lanes, rows: max, more }
}

/// Chips per month cell before "+N more".
const MONTH_CHIPS: usize = 3;

/// Overlap columns for one day's timed events: (col, cols) per input, in
/// input order. Events that transitively overlap share a column count.
pub fn layout_day(spans: &[(i64, i64)]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| (spans[i].0, -(spans[i].1)));
    let mut out = vec![(0, 1); spans.len()];
    let mut cluster: Vec<usize> = Vec::new();
    let mut col_ends: Vec<i64> = Vec::new();
    let mut cluster_end = i64::MIN;
    let flush = |cluster: &mut Vec<usize>, col_ends: &mut Vec<i64>, out: &mut Vec<(usize, usize)>| {
        let cols = col_ends.len().max(1);
        for &i in cluster.iter() {
            out[i].1 = cols;
        }
        cluster.clear();
        col_ends.clear();
    };
    for i in order {
        let (start, end) = spans[i];
        if start >= cluster_end && !cluster.is_empty() {
            flush(&mut cluster, &mut col_ends, &mut out);
        }
        let col = match col_ends.iter().position(|&e| e <= start) {
            Some(c) => {
                col_ends[c] = end;
                c
            }
            None => {
                col_ends.push(end);
                col_ends.len() - 1
            }
        };
        out[i].0 = col;
        cluster.push(i);
        cluster_end = cluster_end.max(end);
    }
    flush(&mut cluster, &mut col_ends, &mut out);
    out
}

/// Local midnight of `d`, epoch ms.
pub fn midnight(d: NaiveDate) -> i64 {
    Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))
        .earliest()
        .map(|t| t.timestamp_millis())
        .unwrap_or_else(|| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp_millis())
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

/// Days a view shows; weeks start on `first`.
pub fn view_days(view: CalView, d: NaiveDate, first: Weekday) -> Vec<NaiveDate> {
    match view {
        CalView::Day => vec![d],
        CalView::Week => week::week_days(d, first),
        CalView::Month => week::month_grid(d, first),
    }
}

/// The [from, to) range of events a view needs.
pub fn view_range(view: CalView, d: NaiveDate, first: Weekday) -> (i64, i64) {
    let days = view_days(view, d, first);
    let first = days[0];
    let last = *days.last().expect("non-empty");
    (midnight(first), midnight(last + Duration::days(1)))
}

/// Previous / next period from `d`.
pub fn step(view: CalView, d: NaiveDate, forward: bool) -> NaiveDate {
    let sign = if forward { 1 } else { -1 };
    match view {
        CalView::Day => d + Duration::days(sign),
        CalView::Week => d + Duration::days(7 * sign),
        CalView::Month => {
            let f = first_of_month(d);
            let moved = if forward { f + Duration::days(32) } else { f - Duration::days(1) };
            first_of_month(moved)
        }
    }
}

/// Focus the calendar on `d` (keeps the week anchor in step).
pub fn go_to(s: AppState, d: NaiveDate) {
    let (mut day, mut ws) = (s.cal_day, s.week_start);
    day.set(d);
    ws.set(midnight(week::start_of_week(d, *s.first_weekday.peek())));
}

/// Events touching `day`, all-day first then by start; split into the
/// first `max` and how many more there are.
pub fn month_cell(events: &[EventRow], day: NaiveDate, max: usize) -> (Vec<EventRow>, usize) {
    let (from, to) = (midnight(day), midnight(day + Duration::days(1)));
    // All-day events are UTC-midnight floating dates.
    let utc = |d: NaiveDate| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp_millis();
    let (ufrom, uto) = (utc(day), utc(day + Duration::days(1)));
    let mut mine: Vec<EventRow> = events
        .iter()
        .filter(|e| {
            if e.event.all_day {
                e.event.start_at < uto && e.event.end_at > ufrom
            } else {
                e.event.start_at < to && e.event.end_at > from
            }
        })
        .cloned()
        .collect();
    mine.sort_by_key(|e| (!e.event.all_day, e.event.start_at));
    let more = mine.len().saturating_sub(max);
    mine.truncate(max);
    (mine, more)
}

/// Pixels into a day column → minutes since midnight, snapped.
pub fn snap_minutes(y: f64, hour_px: f64) -> i64 {
    let raw = (y.max(0.0) / hour_px * 60.0).round() as i64;
    ((raw + SNAP_MIN / 2) / SNAP_MIN * SNAP_MIN).clamp(0, 24 * 60)
}

/// A drag-created range on a day column: (start, end) ms, at least one snap step.
pub fn drag_create(day_start: i64, y0: f64, y1: f64, hour_px: f64) -> (i64, i64) {
    let (a, b) = (snap_minutes(y0.min(y1), hour_px), snap_minutes(y0.max(y1), hour_px));
    let b = if b - a < SNAP_MIN { (a + SNAP_MIN).min(24 * 60) } else { b };
    let a = a.min(b - SNAP_MIN);
    (day_start + a * 60_000, day_start + b * 60_000)
}

/// An event dropped with its top at `top_px` in the column of `day_start`:
/// same duration, snapped start.
pub fn drag_move(orig: (i64, i64), day_start: i64, top_px: f64, hour_px: f64) -> (i64, i64) {
    let start = day_start + snap_minutes(top_px, hour_px) * 60_000;
    (start, start + (orig.1 - orig.0))
}

/// A bottom edge dropped at `bottom_px` in its column: snapped end, never
/// shorter than one snap step.
pub fn drag_resize(orig: (i64, i64), day_start: i64, bottom_px: f64, hour_px: f64) -> (i64, i64) {
    let end = day_start + snap_minutes(bottom_px, hour_px) * 60_000;
    (orig.0, end.max(orig.0 + SNAP_MIN * 60_000))
}

/// Whether drag may change this event: an editable calendar, not part of
/// a recurring series (single-instance edits aren't supported yet).
fn draggable(s: &AppState, row: &EventRow) -> bool {
    row.event.series_id.is_none()
        && !row.event.all_day
        && s.calendar
            .read()
            .calendars
            .iter()
            .any(|(a, c)| a == &row.account_id && c.id == row.event.calendar_id && c.can_edit)
}

/// The update to send when only the time changes (descriptions aren't
/// stored locally; `None` leaves the server's copy alone).
fn reschedule_draft(row: &EventRow, start_at: i64, end_at: i64) -> EventDraft {
    let e = &row.event;
    EventDraft {
        calendar_id: e.calendar_id.clone(),
        title: e.title.clone(),
        start_at,
        end_at,
        all_day: false,
        timezone: e.timezone.clone(),
        location: e.location.clone(),
        description: None,
        attendees: e
            .attendees
            .iter()
            .map(|a| Attendee { address: a.address.clone(), response: Response::None, optional: a.optional })
            .collect(),
        conferencing: Default::default(),
    }
}

/// A finished drag from the bridge: `cal-create day_start|y0|y1`,
/// `cal-move account|event|day_start|top`, `cal-resize account|event|day_start|bottom`.
pub fn on_drag(s: AppState, kind: &str, v: &str) {
    let parts: Vec<&str> = v.split('|').collect();
    let num = |i: usize| parts.get(i).and_then(|p| p.parse::<f64>().ok());
    match kind {
        "cal-create" => {
            let (Some(day_start), Some(y0), Some(y1)) = (num(0), num(1), num(2)) else { return };
            let (start, end) = drag_create(day_start as i64, y0, y1, HOUR_PX);
            super::event_editor::open_range(s, start, end);
        }
        "cal-move" | "cal-resize" => {
            let (Some(account), Some(event), Some(day_start), Some(px)) = (parts.first(), parts.get(1), num(2), num(3))
            else {
                return;
            };
            let row =
                s.calendar.read().events.iter().find(|e| &e.account_id == account && &e.event.id == event).cloned();
            let Some(row) = row else { return };
            if !draggable(&s, &row) {
                return;
            }
            let orig = (row.event.start_at, row.event.end_at);
            let (start, end) = if kind == "cal-move" {
                drag_move(orig, day_start as i64, px, HOUR_PX)
            } else {
                drag_resize(orig, day_start as i64, px, HOUR_PX)
            };
            if (start, end) == orig {
                return;
            }
            // Optimistic: show it moved now; a failed update republishes the
            // server's version, which puts it back.
            let mut cal = s.calendar;
            if let Some(e) =
                cal.write().events.iter_mut().find(|e| e.account_id == row.account_id && e.event.id == row.event.id)
            {
                e.event.start_at = start;
                e.event.end_at = end;
            }
            send_cmd(Cmd::UpdateEvent {
                account_id: row.account_id.clone(),
                event_id: row.event.id.clone(),
                draft: reschedule_draft(&row, start, end),
            });
        }
        _ => {}
    }
}

fn rsvp_attr(r: Response) -> Option<&'static str> {
    match r {
        Response::Tentative => Some("tentative"),
        Response::Declined => Some("declined"),
        Response::None => Some("pending"),
        _ => None,
    }
}

fn title_for(view: CalView, d: NaiveDate, first: Weekday) -> (String, String) {
    match view {
        CalView::Day => (d.format("%A, %B %-d").to_string(), d.format("%Y").to_string()),
        CalView::Week => {
            let m = week::start_of_week(d, first);
            let n = week::week_number(m, first).map(|n| format!("Week {n}")).unwrap_or_default();
            (m.format("%B %Y").to_string(), n)
        }
        CalView::Month => (d.format("%B %Y").to_string(), String::new()),
    }
}

#[component]
pub fn CalendarView() -> Element {
    let s = use_context::<AppState>();
    let mut view = s.cal_view;
    let selected = use_signal(|| None::<(String, String)>);
    let v = *view.read();
    let day = *s.cal_day.read();
    let first = *s.first_weekday.read();
    let (title, sub) = title_for(v, day, first);
    let data = s.calendar.read().clone();
    let selected_event = selected
        .read()
        .as_ref()
        .and_then(|(a, id)| data.events.iter().find(|e| &e.account_id == a && &e.event.id == id).cloned());
    let views = [(CalView::Day, "Day"), (CalView::Week, "Week"), (CalView::Month, "Month")];
    let mut sel = selected;

    rsx! {
        main { class: "pane pane--reader", style: "position:relative",
            super::toasts::Toasts {}
            header { class: "cal-header",
                h1 { class: "cal-header__title", "{title}" }
                span { class: "cal-header__week", "{sub}" }
                span { class: "spacer" }
                div { class: "segmented segmented--sm", role: "tablist", "aria-label": "Calendar view",
                    for (cv, label) in views {
                        button { role: "tab", "aria-selected": "{v == cv}", onclick: move |_| view.set(cv), "{label}" }
                    }
                }
                button { class: "btn btn--secondary btn--sm", r#type: "button", style: "height:30px;margin-left:8px",
                    onclick: move |_| go_to(s, Local::now().date_naive()), "Today" }
                button { class: "icon-btn", "aria-label": "Previous", onclick: move |_| go_to(s, step(v, day, false)), Icon { name: "chev-left" } }
                button { class: "icon-btn", "aria-label": "Next", onclick: move |_| go_to(s, step(v, day, true)), Icon { name: "chev-right" } }
                button { class: "btn btn--primary", r#type: "button", style: "height:30px;margin-left:8px",
                    onclick: move |_| super::event_editor::open_new_now(s),
                    Icon { name: "plus", class: "icon--sm" }
                    "New event"
                    kbd { class: "kbd", "C" }
                }
            }
            if v == CalView::Month {
                MonthGrid { on_select: move |k| sel.set(Some(k)) }
            } else {
                TimeGrid { days: view_days(v, day, first), selected }
            }
            if let Some(e) = selected_event {
                EventPopover { row: e, on_close: move |_| sel.set(None) }
            }
        }
    }
}

#[component]
fn MonthGrid(on_select: EventHandler<(String, String)>) -> Element {
    let s = use_context::<AppState>();
    let mut view = s.cal_view;
    let day = *s.cal_day.read();
    let first = *s.first_weekday.read();
    let events = s.calendar.read().events.clone();
    let today = Local::now().date_naive();
    rsx! {
        div { class: "week",
            div { class: "day-heads", style: "padding-left:0",
                for w in week::order(first) {
                    div { class: "day-head", key: "{w}", "data-weekend": week::is_weekend(w).then_some("true"),
                        span { class: "day-head__dow", "{w}" } }
                }
            }
            div { class: "month",
                for date in week::month_grid(day, first) {
                    {
                        let (chips, more) = month_cell(&events, date, MONTH_CHIPS);
                        rsx! {
                            div { class: "month__cell", key: "{date}",
                                "data-weekend": week::is_weekend(date.weekday()).then_some("true"),
                                "data-outside": (date.month() != day.month()).then_some("true"),
                                "aria-current": (date == today).then_some("date"),
                                onclick: move |_| { go_to(s, date); view.set(CalView::Day); },
                                span { class: "month__num", "{date.day()}" }
                                for e in chips {
                                    {
                                        let hue = s.hue(&e.account_id);
                                        let key = (e.account_id.clone(), e.event.id.clone());
                                        let time = if e.event.all_day { String::new() } else { format::hhmm(e.event.start_at) };
                                        rsx! {
                                            div { class: "month__event", key: "{e.account_id}{e.event.id}", "data-hue": "{hue}",
                                                "data-rsvp": rsvp_attr(e.event.my_response),
                                                onclick: move |ev| { ev.stop_propagation(); on_select.call(key.clone()); },
                                                if !time.is_empty() { span { class: "t-3", "{time}" } }
                                                span { dir: "auto", style: "overflow:hidden;text-overflow:ellipsis", "{e.event.title}" }
                                            }
                                        }
                                    }
                                }
                                if more > 0 { span { class: "month__more", "+{more} more" } }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TimeGrid(days: Vec<NaiveDate>, selected: Signal<Option<(String, String)>>) -> Element {
    let s = use_context::<AppState>();
    let mut selected = selected;
    let data = s.calendar.read().clone();
    let now = pigeon_core::now_ms();
    let today = Local::now().date_naive();
    let n = days.len() as i64;
    let first = midnight(days[0]);
    let last = midnight(*days.last().expect("days") + Duration::days(1));

    use_effect(|| {
        document::eval(
            "setTimeout(()=>{const g=document.querySelector('.week .grid'); if(g && !g.dataset.scrolled){ g.scrollTop = 7.5*60; g.dataset.scrolled='1'; }},0)",
        );
    });

    let events: Vec<EventRow> =
        data.events.iter().filter(|e| e.event.end_at > first && e.event.start_at < last).cloned().collect();
    let (all_day, timed): (Vec<EventRow>, Vec<EventRow>) = events.into_iter().partition(|e| e.event.all_day);
    let draft_form = s.editor.read().clone();
    let draft = draft_form.as_ref().and_then(super::event_editor::draft_span);
    let allday_draft = draft_form
        .as_ref()
        .filter(|f| f.editing.is_none() && f.all_day)
        .and_then(|f| super::event_editor::parse_date(&f.date));
    let draft_title = draft_form
        .as_ref()
        .map(|f| if f.title.trim().is_empty() { "New event".to_string() } else { f.title.trim().to_string() })
        .unwrap_or_default();
    let utc0 = days[0].and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp_millis();
    let spans: Vec<(usize, usize)> = all_day
        .iter()
        .map(|e| {
            let a = ((e.event.start_at - utc0).max(0) / DAY_MS).min(n - 1);
            let b = ((e.event.end_at - 1 - utc0) / DAY_MS).clamp(a, n - 1);
            (a as usize, b as usize)
        })
        .collect();
    let layout = layout_allday(&spans, n as usize, ALLDAY_LANES);

    rsx! {
        div { class: "week", "data-days": "{n}",
            div { class: "day-heads",
                for (i, d) in days.iter().enumerate() {
                    div { class: "day-head", key: "{i}",
                        "aria-current": (*d == today).then_some("date"),
                        "data-weekend": week::is_weekend(d.weekday()).then_some("true"),
                        span { class: "day-head__dow", "{d.format(\"%a\")}" }
                        span { class: "day-head__num", "{d.day()}" }
                    }
                }
            }
            div { class: "allday", style: "--allday-rows:{layout.rows}",
                span { class: "allday__label", "all-day" }
                div { class: "allday__track",
                    // Empty space per day, under the chips: a click starts an
                    // all-day event that day. Lit while that new event is open.
                    for (d, date) in days.iter().copied().enumerate() {
                        div { class: "allday-slot", key: "slot{d}", "aria-hidden": "true",
                            style: "--day:{d};--days:{n}",
                            "data-draft": (allday_draft == Some(date)).then_some("true"),
                            onclick: move |_| { selected.set(None); super::event_editor::open_all_day(s, date); },
                        }
                    }
                    for (e, (start_day, end_day), lane) in all_day.iter().zip(spans.iter().copied()).zip(layout.lanes.iter().copied()).map(|((e, sp), l)| (e, sp, l)) {
                        if let Some(lane) = lane {
                            {
                                let span = end_day - start_day + 1;
                                let hue = s.hue(&e.account_id);
                                rsx! {
                                    div { class: "allday-chip", key: "{e.account_id}{e.event.id}", "data-hue": "{hue}", dir: "auto",
                                        title: "{e.event.title}",
                                        style: "--day:{start_day};--span:{span};--days:{n};--lane:{lane}", "{e.event.title}" }
                                }
                            }
                        }
                    }
                    for (d, hidden) in layout.more.iter().copied().enumerate() {
                        if hidden > 0 {
                            {
                                let date = days[d];
                                let lane = layout.rows - 1;
                                rsx! {
                                    button { class: "allday-more", key: "more{d}", r#type: "button",
                                        style: "--day:{d};--span:1;--days:{n};--lane:{lane}",
                                        onclick: move |_| { go_to(s, date); let mut v = s.cal_view; v.set(CalView::Day); },
                                        "+{hidden} more" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "grid",
                div { class: "grid__hours", style: "height:calc(24 * var(--hour-h))",
                    for h in 1..24 {
                        div { class: "hour-line", key: "h{h}", style: "top:{h * 60}px",
                            span { class: "hour-line__label", "{h}:00" }
                        }
                    }
                    div { class: "grid__days",
                        for (i, day) in days.iter().copied().enumerate() {
                            {
                                let day_start = midnight(day);
                                let day_end = midnight(day + Duration::days(1));
                                let mine: Vec<&EventRow> = timed.iter().filter(|e| e.event.start_at < day_end && e.event.end_at > day_start).collect();
                                let spans: Vec<(i64, i64)> = mine.iter().map(|e| (e.event.start_at.max(day_start), e.event.end_at.min(day_end))).collect();
                                let cols = layout_day(&spans);
                                rsx! {
                                    div { class: "day-col", key: "d{i}",
                                        "data-weekend": week::is_weekend(day.weekday()).then_some("true"),
                                        "data-day-start": "{day_start}",
                                        onclick: move |ev| {
                                            let y = ev.element_coordinates().y;
                                            selected.set(None);
                                            super::event_editor::open_new(s, day, super::event_editor::slot_from_offset(y, HOUR_PX));
                                        },
                                        for (k, e) in mine.into_iter().enumerate() {
                                            {
                                                let (start, end) = spans[k];
                                                let (col, ncols) = cols[k];
                                                let top = (start - day_start) as f64 / 3_600_000.0 * HOUR_PX;
                                                let height = ((end - start) as f64 / 3_600_000.0 * HOUR_PX - 2.0).max(14.0);
                                                let short = height < 40.0;
                                                let hue = s.hue(&e.account_id);
                                                let key = (e.account_id.clone(), e.event.id.clone());
                                                let is_sel = selected.read().as_ref() == Some(&key);
                                                let video = pigeon_core::meeting::meeting_link(&e.event).is_some();
                                                let drag = draggable(&s, e);
                                                rsx! {
                                                    div { class: "event", key: "{e.account_id}{e.event.id}", "data-hue": "{hue}",
                                                        "data-rsvp": rsvp_attr(e.event.my_response),
                                                        "data-past": (e.event.end_at < now).then_some("true"),
                                                        "data-short": short.then_some("true"),
                                                        "data-drag": drag.then(|| format!("{}|{}", e.account_id, e.event.id)),
                                                        "aria-selected": "{is_sel}",
                                                        style: "top:{top}px;height:{height}px;--col:{col};--cols:{ncols}",
                                                        onclick: move |ev| { ev.stop_propagation(); selected.set(Some(key.clone())); },
                                                        div { class: "event__title", dir: "auto",
                                                            if video { Icon { name: "video", class: "icon--xs" } }
                                                            "{e.event.title}"
                                                        }
                                                        if !short { div { class: "event__time", "{format::hhmm(e.event.start_at)}–{format::hhmm(e.event.end_at)}" } }
                                                        if drag { div { class: "event__resize" } }
                                                    }
                                                }
                                            }
                                        }
                                        // The new event being created, tracking the editor's times.
                                        if let Some((ds, de)) = draft.filter(|(ds, de)| *ds < day_end && *de > day_start) {
                                            {
                                                let (start, end) = (ds.max(day_start), de.min(day_end));
                                                let top = (start - day_start) as f64 / 3_600_000.0 * HOUR_PX;
                                                let height = ((end - start) as f64 / 3_600_000.0 * HOUR_PX - 2.0).max(14.0);
                                                rsx! {
                                                    div { class: "event event--draft", "aria-hidden": "true",
                                                        "data-short": (height < 40.0).then_some("true"),
                                                        style: "top:{top}px;height:{height}px;--col:0;--cols:1;pointer-events:none",
                                                        div { class: "event__title", dir: "auto", "{draft_title}" }
                                                        if height >= 40.0 { div { class: "event__time", "{format::hhmm(ds)}–{format::hhmm(de)}" } }
                                                    }
                                                }
                                            }
                                        }
                                        if day == today && now > day_start && now < day_end {
                                            div { class: "now-line", style: "top:{(now - day_start) as f64 / 3_600_000.0 * HOUR_PX}px" }
                                        }
                                    }
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
fn EventPopover(row: EventRow, on_close: EventHandler<()>) -> Element {
    let s = use_context::<AppState>();
    let e = row.event.clone();
    let hue = s.hue(&row.account_id);
    let now = pigeon_core::now_ms();
    let can_edit =
        s.calendar.read().calendars.iter().any(|(a, c)| a == &row.account_id && c.id == e.calendar_id && c.can_edit);
    let edit_row = row.clone();
    let when = format!(
        "{} · {}–{}",
        format::local(e.start_at).map(|d| d.format("%a, %b %-d").to_string()).unwrap_or_default(),
        format::hhmm(e.start_at),
        format::hhmm(e.end_at)
    );
    let soon = if e.start_at > now { format::soon(e.start_at, now) } else { String::new() };
    let calendar_name = s
        .calendar
        .read()
        .calendars
        .iter()
        .find(|(a, c)| a == &row.account_id && c.id == e.calendar_id)
        .map(|(_, c)| c.name.clone())
        .unwrap_or_default();
    let yes = e.attendees.iter().filter(|a| a.response == Response::Accepted).count();
    let maybe = e.attendees.iter().filter(|a| a.response == Response::Tentative).count();
    let can_rsvp = !e.attendees.is_empty() && e.my_response != Response::Organizer;
    let link = pigeon_core::meeting::meeting_link(&e);
    let url = link.as_ref().map(|l| l.url.clone());
    let service = link.as_ref().map(|l| l.service.label()).unwrap_or("meeting");
    let rsvp = move |response: Response| {
        send_cmd(Cmd::Rsvp {
            account_id: row.account_id.clone(),
            calendar_id: e.calendar_id.clone(),
            event_id: e.id.clone(),
            response,
        })
    };

    rsx! {
        div { class: "event-popover popover", role: "dialog", style: "right:24px;top:24px;left:auto",
            onclick: move |ev| ev.stop_propagation(),
            div { class: "event-popover__section",
                div { style: "display:flex;align-items:flex-start;gap:8px",
                    div { class: "spacer",
                        div { class: "event-popover__title", dir: "auto", "{row.event.title}" }
                        div { class: "event-popover__when", "{when}" if !soon.is_empty() { " · " span { class: "event-popover__soon", "{soon}" } } }
                    }
                    if can_edit {
                        button { class: "icon-btn icon-btn--sm", "aria-label": "Edit",
                            onclick: move |_| { super::event_editor::open_edit(s, &edit_row); on_close.call(()); },
                            Icon { name: "edit", class: "icon--sm" }
                        }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close", onclick: move |_| on_close.call(()), Icon { name: "x", class: "icon--sm" } }
                }
                if let Some(u) = url {
                    a { class: "btn btn--primary join-btn", href: "#",
                        onclick: move |ev| { ev.prevent_default(); crate::meeting::join(&u); },
                        Icon { name: "video" } "Join {service}"
                    }
                    span { class: "event-popover__link", "{link.as_ref().map(|l| l.url.clone()).unwrap_or_default()}" }
                }
            }
            div { class: "event-popover__section",
                div { class: "event-popover__fact", "data-hue": "{hue}", span { class: "dot" } "{calendar_name}" }
                if let Some(loc) = row.event.location.clone() {
                    div { class: "event-popover__fact", dir: "auto", Icon { name: "pin", class: "icon--sm" } "{loc}" }
                }
            }
            if !row.event.attendees.is_empty() {
                div { class: "event-popover__section",
                    span { class: "t-label", "{row.event.attendees.len()} guests · {yes} yes, {maybe} maybe" }
                    for a in row.event.attendees.iter().take(8) {
                        {
                            let r = match a.response { Response::Accepted | Response::Organizer => "yes", Response::Tentative => "maybe", Response::Declined => "no", Response::None => "pending" };
                            let icon = match r { "yes" => "check", "maybe" => "question", "no" => "x", _ => "dash" };
                            rsx! {
                                div { class: "guest", "data-rsvp": "{r}", key: "{a.address.email}",
                                    span { class: "avatar avatar--22", "data-hue": "{hue}",
                                        "{format::initials(a.address.display())}"
                                        span { class: "rsvp", "data-rsvp": "{r}", Icon { name: icon } }
                                    }
                                    span { class: "guest__name", dir: "auto", "{a.address.display()}" }
                                }
                            }
                        }
                    }
                }
            }
            if can_rsvp {
                {
                    let (r1, r2, r3) = (rsvp.clone(), rsvp.clone(), rsvp);
                    let mine = row.event.my_response;
                    rsx! {
                        div { class: "event-popover__rsvp",
                            span { class: "spacer t-2", style: "font-size:12.5px", "Going?" }
                            button { class: if mine == Response::Accepted { "btn btn--soft btn--sm" } else { "btn btn--secondary btn--sm" },
                                "aria-pressed": "{mine == Response::Accepted}", onclick: move |_| r1(Response::Accepted), "Yes" }
                            button { class: if mine == Response::Tentative { "btn btn--soft btn--sm" } else { "btn btn--secondary btn--sm" },
                                "aria-pressed": "{mine == Response::Tentative}", onclick: move |_| r2(Response::Tentative), "Maybe" }
                            button { class: if mine == Response::Declined { "btn btn--soft btn--sm" } else { "btn btn--secondary btn--sm" },
                                "aria-pressed": "{mine == Response::Declined}", onclick: move |_| r3(Response::Declined), "No" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn CalendarSidebar() -> Element {
    let s = use_context::<AppState>();
    let focus = *s.cal_day.read();
    let view = *s.cal_view.read();
    let first = *s.first_weekday.read();
    let mut month = use_signal(|| first_of_month(focus));
    // Follow the main view when it moves to another month.
    use_effect(move || {
        let f = first_of_month(*s.cal_day.read());
        if *month.peek() != f {
            month.set(f);
        }
    });
    let m = *month.read();
    let grid = week::month_grid(m, first);
    let shown = view_days(view, focus, first);
    let (range_start, range_end) = if view == CalView::Month {
        (first_of_month(focus), first_of_month(step(CalView::Month, focus, true)) - Duration::days(1))
    } else {
        (shown[0], *shown.last().expect("days"))
    };
    let today = Local::now().date_naive();
    let accounts = s.accounts.read().clone();
    let order: Vec<String> = accounts.iter().map(|a| a.account.id.clone()).collect();
    let calendars = super::event_editor::in_sidebar_order(&s.calendar.read().calendars, &order);
    // Quiet, as for mail: the default for new events shows in a tooltip.
    let pref = s.default_calendar.read().clone();
    let chosen = super::event_editor::chosen_calendar(&calendars, &pref);
    let default = chosen.clone().or_else(|| super::event_editor::auto_calendar(&calendars));
    let menu = s.account_menu.read().clone();
    let menu_on =
        |account: &str, target: &Target| menu.as_ref().is_some_and(|m| m.account_id == account && &m.target == target);

    rsx! {
        div { class: "mini-month",
            div { class: "mini-month__head",
                span { class: "spacer", "{m.format(\"%B %Y\")}" }
                button { class: "icon-btn icon-btn--xs", "aria-label": "Previous month",
                    onclick: move |_| month.set(step(CalView::Month, m, false)),
                    Icon { name: "chev-left", class: "icon--sm" } }
                button { class: "icon-btn icon-btn--xs", "aria-label": "Next month",
                    onclick: move |_| month.set(step(CalView::Month, m, true)),
                    Icon { name: "chev-right", class: "icon--sm" } }
            }
            div { class: "mini-month__grid",
                for w in week::order(first) { span { class: "mini-month__dow", key: "{w}", "{w.to_string().chars().next().unwrap_or(' ')}" } }
                for (i, date) in grid.into_iter().enumerate() {
                    span { class: "mini-month__day", key: "{i}",
                        "data-outside": (date.month() != m.month()).then_some("true"),
                        "data-in-range": (date >= range_start && date <= range_end).then_some("true"),
                        "data-range-start": (date == range_start).then_some("true"),
                        "data-range-end": (date == range_end).then_some("true"),
                        "aria-current": (date == today).then_some("date"),
                        onclick: move |_| go_to(s, date),
                        span { "{date.day()}" }
                    }
                }
            }
        }
        div { class: "cal-list",
            for a in accounts {
                {
                    let mine: Vec<_> = calendars.iter().filter(|(id, _)| id == &a.account.id).map(|(_, c)| c.clone()).collect();
                    let account_id = a.account.id.clone();
                    rsx! {
                        div { class: "nav-section", key: "{a.account.id}",
                            "data-context": menu_on(&a.account.id, &Target::CalAccount).then_some("true"),
                            oncontextmenu: move |e| {
                                e.prevent_default();
                                let p = e.client_coordinates();
                                super::account_menu::open_for(s, &account_id, Target::CalAccount, p.x, p.y);
                            },
                            span { class: "spacer", dir: "auto", "{s.account_label(&a.account.id, &a.account.email)}" }
                        }
                        for c in mine {
                            {
                                let here = Some((a.account.id.clone(), c.id.clone()));
                                let tip = match (default == here, chosen.is_some()) {
                                    (true, true) => "Default for new events",
                                    (true, false) => "Default for new events (automatic)",
                                    _ if !c.can_edit => "Read-only",
                                    _ => "",
                                };
                                let target = Target::Calendar(c.id.clone());
                                let on = menu_on(&a.account.id, &target);
                                let account_id = a.account.id.clone();
                                rsx! {
                                    label { class: "cal-list__item", "data-hue": "{a.hue}", key: "{c.id}", title: "{tip}",
                                        "data-context": on.then_some("true"),
                                        oncontextmenu: move |e| {
                                            e.prevent_default();
                                            let p = e.client_coordinates();
                                            super::account_menu::open_for(s, &account_id, target.clone(), p.x, p.y);
                                        },
                                        input { class: "check check--hue", r#type: "checkbox", checked: true, disabled: true }
                                        span { dir: "auto", "{c.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_columns() {
        // a and b overlap; c overlaps b; d is alone later.
        let spans = [(0, 60), (30, 90), (60, 120), (200, 260)];
        assert_eq!(layout_day(&spans), vec![(0, 2), (1, 2), (0, 2), (0, 1)]);
        assert_eq!(layout_day(&[]), vec![]);
        // Three at once.
        assert_eq!(layout_day(&[(0, 10), (0, 10), (0, 10)]), vec![(0, 3), (1, 3), (2, 3)]);
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn views_and_ranges() {
        let mon = Weekday::Mon;
        assert_eq!(view_days(CalView::Week, d(2026, 10, 23), mon)[0], d(2026, 10, 19));
        assert_eq!(view_days(CalView::Week, d(2026, 10, 23), Weekday::Sun)[0], d(2026, 10, 18));
        assert_eq!(view_days(CalView::Month, d(2026, 10, 23), Weekday::Sun)[0], d(2026, 9, 27));
        assert_eq!(view_days(CalView::Day, d(2026, 10, 23), mon), vec![d(2026, 10, 23)]);
        let (from, to) = view_range(CalView::Day, d(2026, 10, 25), mon); // DST ends in Israel/EU that night
        assert_eq!((from, to), (midnight(d(2026, 10, 25)), midnight(d(2026, 10, 26))));
        // A Sunday-start week across the DST change spans 7 calendar days.
        let (from, to) = view_range(CalView::Week, d(2026, 10, 27), Weekday::Sun);
        assert_eq!((from, to), (midnight(d(2026, 10, 25)), midnight(d(2026, 11, 1))));
        assert_eq!(step(CalView::Month, d(2026, 1, 31), true), d(2026, 2, 1));
        assert_eq!(step(CalView::Month, d(2026, 3, 15), false), d(2026, 2, 1));
        assert_eq!(step(CalView::Week, d(2026, 10, 23), false), d(2026, 10, 16));
        assert_eq!(title_for(CalView::Week, d(2026, 10, 23), mon).1, "Week 43");
        assert_eq!(title_for(CalView::Week, d(2026, 10, 23), Weekday::Sun).1, "");
    }

    #[test]
    fn drag_math_snaps_to_quarter_hours() {
        assert_eq!(snap_minutes(0.0, 60.0), 0);
        assert_eq!(snap_minutes(67.0, 60.0), 60); // 1:07 → 1:00
        assert_eq!(snap_minutes(68.0, 60.0), 75); // 1:08 → 1:15
        assert_eq!(snap_minutes(9999.0, 60.0), 1440);
        let day = 1_000_000_000;
        // Dragging upwards works too; a tiny drag still makes 15 minutes.
        assert_eq!(drag_create(day, 600.0, 540.0, 60.0), (day + 540 * 60_000, day + 600 * 60_000));
        assert_eq!(drag_create(day, 600.0, 602.0, 60.0), (day + 600 * 60_000, day + 615 * 60_000));
        assert_eq!(drag_create(day, 1440.0, 1440.0, 60.0), (day + 1425 * 60_000, day + 1440 * 60_000));
        // Move keeps the duration; resize keeps the start and a minimum length.
        let orig = (day + 600 * 60_000, day + 690 * 60_000);
        assert_eq!(
            drag_move(orig, day + DAY_MS, 122.0, 60.0),
            (day + DAY_MS + 120 * 60_000, day + DAY_MS + 210 * 60_000)
        );
        assert_eq!(drag_resize(orig, day, 750.0, 60.0), (orig.0, day + 750 * 60_000));
        assert_eq!(drag_resize(orig, day, 0.0, 60.0), (orig.0, orig.0 + 15 * 60_000));
    }

    #[test]
    fn allday_lanes_stack_overlaps_and_reuse_free_lanes() {
        // A: Mon-Wed, B: Tue, C: Thu (reuses lane 0), D: Mon-Sun (longest)
        let spans = [(0, 2), (1, 1), (3, 3), (0, 6)];
        let l = layout_allday(&spans, 7, 4);
        assert_eq!(l.lanes, vec![Some(1), Some(2), Some(1), Some(0)], "longer span on top, C reuses A's lane");
        assert_eq!(l.rows, 3);
        assert!(l.more.iter().all(|&m| m == 0));
        // A chain of back-to-back events shares one lane.
        let chain = layout_allday(&[(0, 1), (2, 3), (4, 6)], 7, 4);
        assert_eq!(chain.lanes, vec![Some(0); 3]);
        assert_eq!(chain.rows, 1);
        // Nothing all-day: still one (empty) row.
        assert_eq!(layout_allday(&[], 7, 4).rows, 1);
    }

    #[test]
    fn allday_overflow_collapses_into_per_day_more() {
        // Five events on Wednesday; one of them runs to Friday.
        let spans = [(2, 2), (2, 2), (2, 4), (2, 2), (2, 2)];
        let l = layout_allday(&spans, 7, 4);
        assert_eq!(l.rows, 4);
        assert_eq!(l.lanes.iter().filter(|x| x.is_some()).count(), 3);
        assert_eq!(l.more, vec![0, 0, 2, 0, 0, 0, 0]);
        // A collapsed multi-day event counts on each of its days, clipped to the week.
        let l = layout_allday(&[(0, 1), (0, 1), (1, 1), (1, 9)], 7, 3);
        assert_eq!(l.more, vec![0, 2, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn month_cells_put_all_day_first_and_count_overflow() {
        use pigeon_core::model::Event;
        let mk = |id: &str, start: i64, end: i64, all_day: bool| EventRow {
            account_id: "a".into(),
            event: Event {
                id: id.into(),
                title: id.into(),
                start_at: start,
                end_at: end,
                all_day,
                ..Default::default()
            },
        };
        let day = d(2026, 10, 23);
        let m = midnight(day);
        let utc = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis();
        let events = vec![
            mk("late", m + 15 * 3_600_000, m + 16 * 3_600_000, false),
            mk("early", m + 9 * 3_600_000, m + 10 * 3_600_000, false),
            mk("holiday", utc, utc + DAY_MS, true),
            mk("noon", m + 12 * 3_600_000, m + 13 * 3_600_000, false),
            mk("tomorrow", m + DAY_MS + 3_600_000, m + DAY_MS + 7_200_000, false),
        ];
        let (shown, more) = month_cell(&events, day, 3);
        assert_eq!(shown.iter().map(|e| e.event.id.as_str()).collect::<Vec<_>>(), ["holiday", "early", "noon"]);
        assert_eq!(more, 1);
    }
}
