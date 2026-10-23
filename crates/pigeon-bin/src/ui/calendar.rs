use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use dioxus::prelude::*;
use pigeon_core::model::{EventRow, Response};
use pigeon_engine::service::Cmd;

use super::{DAY_MS, Icon};
use crate::format;
use crate::send_cmd;
use crate::state::AppState;

const HOUR_PX: f64 = 60.0;

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

fn day_of(week_start: i64, i: i64) -> chrono::DateTime<Local> {
    format::local(week_start).map(|d| d + Duration::days(i)).unwrap_or_else(Local::now)
}

fn rsvp_attr(r: Response) -> Option<&'static str> {
    match r {
        Response::Tentative => Some("tentative"),
        Response::Declined => Some("declined"),
        Response::None => Some("pending"),
        _ => None,
    }
}

#[component]
pub fn CalendarView() -> Element {
    let s = use_context::<AppState>();
    let mut week_start = s.week_start;
    let mut selected = use_signal(|| None::<(String, String)>);
    let ws = *week_start.read();
    let data = s.calendar.read().clone();
    let now = pigeon_core::now_ms();
    let first_day = day_of(ws, 0);
    let today = Local::now().date_naive();

    use_effect(|| {
        document::eval(
            "setTimeout(()=>{const g=document.querySelector('.week .grid'); if(g) g.scrollTop = 7.5*60;},0)",
        );
    });

    let events: Vec<EventRow> =
        data.events.iter().filter(|e| e.event.end_at > ws && e.event.start_at < ws + 7 * DAY_MS).cloned().collect();
    let (all_day, timed): (Vec<EventRow>, Vec<EventRow>) = events.into_iter().partition(|e| e.event.all_day);
    let selected_event = selected
        .read()
        .as_ref()
        .and_then(|(a, id)| data.events.iter().find(|e| &e.account_id == a && &e.event.id == id).cloned());

    rsx! {
        main { class: "pane pane--reader", style: "position:relative",
            super::toasts::Toasts {}
            header { class: "cal-header",
                h1 { class: "cal-header__title", "{first_day.format(\"%B %Y\")}" }
                span { class: "cal-header__week", "Week {first_day.iso_week().week()}" }
                span { class: "spacer" }
                button { class: "btn btn--secondary btn--sm", r#type: "button", style: "height:30px",
                    onclick: move |_| week_start.set(format::week_start(Local::now())), "Today" }
                button { class: "icon-btn", "aria-label": "Previous week", onclick: move |_| week_start.set(ws - 7 * DAY_MS), Icon { name: "chev-left" } }
                button { class: "icon-btn", "aria-label": "Next week", onclick: move |_| week_start.set(ws + 7 * DAY_MS), Icon { name: "chev-right" } }
                button { class: "btn btn--primary", r#type: "button", style: "height:30px;margin-left:8px",
                    onclick: move |_| super::event_editor::open_new_now(s),
                    Icon { name: "plus", class: "icon--sm" }
                    "New event"
                    kbd { class: "kbd", "C" }
                }
            }
            div { class: "week",
                div { class: "day-heads",
                    for i in 0..7 {
                        {
                            let d = day_of(ws, i);
                            let is_today = d.date_naive() == today;
                            rsx! {
                                div { class: "day-head", key: "{i}",
                                    "aria-current": is_today.then_some("date"),
                                    "data-weekend": (i >= 5).then_some("true"),
                                    span { class: "day-head__dow", "{d.format(\"%a\")}" }
                                    span { class: "day-head__num", "{d.day()}" }
                                }
                            }
                        }
                    }
                }
                div { class: "allday",
                    span { class: "allday__label", "all-day" }
                    div { class: "allday__track",
                        for e in all_day {
                            {
                                let start_day = ((e.event.start_at - ws).max(0) / DAY_MS).min(6);
                                let end_day = ((e.event.end_at - 1 - ws) / DAY_MS).clamp(start_day, 6);
                                let span = end_day - start_day + 1;
                                let hue = s.hue(&e.account_id);
                                rsx! {
                                    div { class: "allday-chip", key: "{e.account_id}{e.event.id}", "data-hue": "{hue}", dir: "auto",
                                        style: "--day:{start_day};--span:{span}", "{e.event.title}" }
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
                            for i in 0..7i64 {
                                {
                                    let day_start = format::local(ws)
                                        .and_then(|d| Local.from_local_datetime(&(d.date_naive() + Duration::days(i)).and_hms_opt(0, 0, 0)?).earliest())
                                        .map(|d| d.timestamp_millis())
                                        .unwrap_or(ws + i * DAY_MS);
                                    let day_end = day_start + DAY_MS;
                                    let mine: Vec<&EventRow> = timed.iter().filter(|e| e.event.start_at < day_end && e.event.end_at > day_start).collect();
                                    let spans: Vec<(i64, i64)> = mine.iter().map(|e| (e.event.start_at.max(day_start), e.event.end_at.min(day_end))).collect();
                                    let cols = layout_day(&spans);
                                    let is_today = day_of(ws, i).date_naive() == today;
                                    let day = day_of(ws, i).date_naive();
                                    rsx! {
                                        div { class: "day-col", key: "d{i}", "data-weekend": (i >= 5).then_some("true"),
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
                                                    rsx! {
                                                        div { class: "event", key: "{e.account_id}{e.event.id}", "data-hue": "{hue}",
                                                            "data-rsvp": rsvp_attr(e.event.my_response),
                                                            "data-past": (e.event.end_at < now).then_some("true"),
                                                            "data-short": short.then_some("true"),
                                                            "aria-selected": "{is_sel}",
                                                            style: "top:{top}px;height:{height}px;--col:{col};--cols:{ncols}",
                                                            onclick: move |ev| { ev.stop_propagation(); selected.set(Some(key.clone())); },
                                                            div { class: "event__title", dir: "auto",
                                                                if video { Icon { name: "video", class: "icon--xs" } }
                                                                "{e.event.title}"
                                                            }
                                                            if !short { div { class: "event__time", "{format::hhmm(e.event.start_at)}–{format::hhmm(e.event.end_at)}" } }
                                                        }
                                                    }
                                                }
                                            }
                                            if is_today && now > day_start && now < day_end {
                                                div { class: "now-line", style: "top:{(now - day_start) as f64 / 3_600_000.0 * HOUR_PX}px" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(e) = selected_event {
                            EventPopover { row: e, on_close: move |_| selected.set(None) }
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
    let mut week_start = s.week_start;
    let mut month = use_signal(|| {
        let d = format::local(*week_start.peek()).unwrap_or_else(Local::now).date_naive();
        NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
    });
    let m = *month.read();
    let grid_start = m - Duration::days(i64::from(m.weekday().num_days_from_monday()));
    let ws_date = format::local(*week_start.read()).map(|d| d.date_naive()).unwrap_or(m);
    let today = Local::now().date_naive();
    let calendars = s.calendar.read().calendars.clone();
    let accounts = s.accounts.read().clone();

    rsx! {
        div { class: "mini-month",
            div { class: "mini-month__head",
                span { class: "spacer", "{m.format(\"%B %Y\")}" }
                button { class: "icon-btn icon-btn--xs", "aria-label": "Previous month",
                    onclick: move |_| { let p = m - Duration::days(1); month.set(NaiveDate::from_ymd_opt(p.year(), p.month(), 1).unwrap_or(p)); },
                    Icon { name: "chev-left", class: "icon--sm" } }
                button { class: "icon-btn icon-btn--xs", "aria-label": "Next month",
                    onclick: move |_| { let n = m + Duration::days(32); month.set(NaiveDate::from_ymd_opt(n.year(), n.month(), 1).unwrap_or(n)); },
                    Icon { name: "chev-right", class: "icon--sm" } }
            }
            div { class: "mini-month__grid",
                for d in ["M", "T", "W", "T", "F", "S", "S"] { span { class: "mini-month__dow", "{d}" } }
                for i in 0..42 {
                    {
                        let date = grid_start + Duration::days(i);
                        let in_week = date >= ws_date && date < ws_date + Duration::days(7);
                        rsx! {
                            span { class: "mini-month__day", key: "{i}",
                                "data-outside": (date.month() != m.month()).then_some("true"),
                                "data-in-range": in_week.then_some("true"),
                                "data-range-start": (date == ws_date).then_some("true"),
                                "data-range-end": (date == ws_date + Duration::days(6)).then_some("true"),
                                "aria-current": (date == today).then_some("date"),
                                onclick: move |_| {
                                    if let Some(t) = Local.from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap_or_default()).earliest() {
                                        week_start.set(format::week_start(t));
                                    }
                                },
                                span { "{date.day()}" }
                            }
                        }
                    }
                }
            }
        }
        div { class: "cal-list",
            for a in accounts {
                {
                    let mine: Vec<_> = calendars.iter().filter(|(id, _)| id == &a.account.id).map(|(_, c)| c.clone()).collect();
                    rsx! {
                        div { class: "nav-section", key: "{a.account.id}", span { class: "spacer", dir: "auto", "{a.account.email}" } }
                        for c in mine {
                            label { class: "cal-list__item", "data-hue": "{a.hue}", key: "{c.id}",
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

#[cfg(test)]
mod tests {
    use super::layout_day;

    #[test]
    fn overlap_columns() {
        // a and b overlap; c overlaps b; d is alone later.
        let spans = [(0, 60), (30, 90), (60, 120), (200, 260)];
        assert_eq!(layout_day(&spans), vec![(0, 2), (1, 2), (0, 2), (0, 1)]);
        assert_eq!(layout_day(&[]), vec![]);
        // Three at once.
        assert_eq!(layout_day(&[(0, 10), (0, 10), (0, 10)]), vec![(0, 3), (1, 3), (2, 3)]);
    }
}
