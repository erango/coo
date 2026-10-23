//! Display formatting: times, list grouping, initials, sizes.

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone};

pub fn local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// Row time: `10:24` today, `Thu` this week, `Oct 3` this year, else `3/10/24`.
pub fn row_time(ms: i64, now: DateTime<Local>) -> String {
    let Some(t) = local(ms) else { return String::new() };
    let days = (now.date_naive() - t.date_naive()).num_days();
    if days == 0 {
        t.format("%H:%M").to_string()
    } else if (1..7).contains(&days) {
        t.format("%a").to_string()
    } else if t.year() == now.year() {
        t.format("%b %-d").to_string()
    } else {
        t.format("%-d/%-m/%y").to_string()
    }
}

/// Group header for the list: Today / Yesterday / This week / Older.
pub fn group(ms: i64, now: DateTime<Local>) -> &'static str {
    let Some(t) = local(ms) else { return "Older" };
    match (now.date_naive() - t.date_naive()).num_days() {
        i64::MIN..=0 => "Today",
        1 => "Yesterday",
        2..=6 => "This week",
        _ => "Older",
    }
}

pub fn long_time(ms: i64, now: DateTime<Local>) -> String {
    let Some(t) = local(ms) else { return String::new() };
    let mins = (now - t).num_minutes();
    let ago = match mins {
        i64::MIN..=0 => String::new(),
        1..=59 => format!(" · {mins} min ago"),
        60..=719 => format!(" · {} h ago", mins / 60),
        _ => String::new(),
    };
    if t.date_naive() == now.date_naive() {
        format!("{}{ago}", t.format("%H:%M"))
    } else {
        format!("{}{ago}", t.format("%a, %b %-d · %H:%M"))
    }
}

pub fn initials(name: &str) -> String {
    let mut out: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if out.is_empty() {
        out = name.chars().next().map(String::from).unwrap_or_default();
    }
    out.to_uppercase()
}

/// "Name <address>", or just the address when there's no distinct name.
pub fn with_address(a: &pigeon_core::model::Address) -> String {
    match a.name.as_deref().map(str::trim).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(&a.email)) {
        Some(n) => format!("{n} <{}>", a.email),
        None => a.email.clone(),
    }
}

/// How many of `total` recipients to list before "+N" (all of them when
/// only one more would be hidden).
pub fn recipients_shown(total: usize, max: usize) -> usize {
    if total <= max + 1 { total } else { max }
}

pub fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// 00:00 (local) of the first day (`first`) of the week containing `t`, epoch ms.
pub fn week_start(t: DateTime<Local>, first: chrono::Weekday) -> i64 {
    let start = crate::week::start_of_week(t.date_naive(), first);
    Local
        .from_local_datetime(&start.and_time(NaiveTime::MIN))
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or_default()
}

/// Same local calendar day.
pub fn same_day(a_ms: i64, b_ms: i64) -> bool {
    matches!((local(a_ms), local(b_ms)), (Some(a), Some(b)) if a.date_naive() == b.date_naive())
}

pub fn hhmm(ms: i64) -> String {
    local(ms).map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// "in 8 min" / "now" / "" for an upcoming start.
pub fn soon(start_ms: i64, now_ms: i64) -> String {
    match (start_ms - now_ms) / 60_000 {
        i64::MIN..=0 => "now".into(),
        m @ 1..=59 => format!("in {m} min"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_with_addresses() {
        use pigeon_core::model::Address;
        let a = |n: Option<&str>, e: &str| Address { name: n.map(String::from), email: e.into() };
        assert_eq!(with_address(&a(Some("Dana Cohen"), "dana@x.io")), "Dana Cohen <dana@x.io>");
        assert_eq!(with_address(&a(None, "dana@x.io")), "dana@x.io");
        assert_eq!(with_address(&a(Some("dana@x.io"), "dana@x.io")), "dana@x.io");
        assert_eq!(with_address(&a(Some("  "), "dana@x.io")), "dana@x.io");
        assert_eq!(recipients_shown(3, 3), 3);
        assert_eq!(recipients_shown(4, 3), 4, "never '+1'");
        assert_eq!(recipients_shown(9, 3), 3);
    }

    #[test]
    fn row_times_and_groups() {
        let now = Local.with_ymd_and_hms(2026, 10, 23, 12, 0, 0).unwrap();
        let ms = |d: DateTime<Local>| d.timestamp_millis();
        let today = Local.with_ymd_and_hms(2026, 10, 23, 9, 5, 0).unwrap();
        let yesterday = Local.with_ymd_and_hms(2026, 10, 22, 9, 5, 0).unwrap();
        let older = Local.with_ymd_and_hms(2026, 9, 2, 9, 5, 0).unwrap();
        assert_eq!(row_time(ms(today), now), "09:05");
        assert_eq!(row_time(ms(yesterday), now), "Thu");
        assert_eq!(row_time(ms(older), now), "Sep 2");
        assert_eq!(group(ms(today), now), "Today");
        assert_eq!(group(ms(yesterday), now), "Yesterday");
        assert_eq!(group(ms(older), now), "Older");
    }

    #[test]
    fn misc() {
        assert_eq!(initials("priya raman"), "PR");
        assert_eq!(initials("שלום"), "ש");
        assert_eq!(size(2_516_582), "2.4 MB");
        let fri = Local.with_ymd_and_hms(2026, 10, 23, 15, 0, 0).unwrap();
        let mon = Local.with_ymd_and_hms(2026, 10, 19, 0, 0, 0).unwrap();
        assert_eq!(week_start(fri, chrono::Weekday::Mon), mon.timestamp_millis());
        let sun = Local.with_ymd_and_hms(2026, 10, 18, 0, 0, 0).unwrap();
        assert_eq!(week_start(fri, chrono::Weekday::Sun), sun.timestamp_millis());
        // Week spanning the DST change (Sun Oct 25): starts at that Sunday's local midnight.
        let tue = Local.with_ymd_and_hms(2026, 10, 27, 9, 0, 0).unwrap();
        let sun25 = Local.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap();
        assert_eq!(week_start(tue, chrono::Weekday::Sun), sun25.timestamp_millis());
        assert_eq!(soon(8 * 60_000 + 5, 0), "in 8 min");
    }
}
