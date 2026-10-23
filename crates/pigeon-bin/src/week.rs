//! First day of the week and the weekend: a user setting
//! (`prefs.week_start`, 0 = Sunday … 6 = Saturday) or, when unset, the
//! macOS calendar (Israel: weeks start Sunday, weekend is Friday–Saturday).
//! All calendar day math goes through here.

use std::sync::OnceLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Weekday from the prefs number (0 = Sunday).
pub fn from_index(i: u8) -> Weekday {
    (0..i % 7).fold(Weekday::Sun, |w, _| w.succ())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemWeek {
    pub first: Weekday,
    /// Weekend days as a bit set over `num_days_from_sunday`.
    pub weekend: u8,
}

const SAT_SUN: u8 = 1 << 0 | 1 << 6;

/// What macOS says (cached): NSCalendar's firstWeekday and weekend days,
/// else the global `AppleFirstWeekday` default, else Monday / Sat–Sun.
pub fn system() -> SystemWeek {
    static CELL: OnceLock<SystemWeek> = OnceLock::new();
    *CELL.get_or_init(detect)
}

#[cfg(target_os = "macos")]
fn detect() -> SystemWeek {
    use objc2_foundation::{NSCalendar, NSDate};
    let cal = NSCalendar::currentCalendar();
    // 1-based, Sunday = 1.
    let first = match cal.firstWeekday() {
        n @ 1..=7 => from_index((n - 1) as u8),
        _ => defaults_first_weekday().unwrap_or(Weekday::Mon),
    };
    // Probe one known week (2026-10-18 is a Sunday) at local noon.
    let mut weekend = 0u8;
    for i in 0..7u8 {
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 10, 18).expect("date") + Duration::days(i64::from(i));
        let Some(t) = noon.and_hms_opt(12, 0, 0).and_then(|t| t.and_local_timezone(chrono::Local).earliest()) else {
            continue;
        };
        let secs = t.timestamp() as f64;
        if cal.isDateInWeekend(&NSDate::dateWithTimeIntervalSince1970(secs)) {
            weekend |= 1 << i;
        }
    }
    SystemWeek { first, weekend: if weekend == 0 { SAT_SUN } else { weekend } }
}

#[cfg(not(target_os = "macos"))]
fn detect() -> SystemWeek {
    SystemWeek { first: defaults_first_weekday().unwrap_or(Weekday::Mon), weekend: SAT_SUN }
}

/// `defaults read -g AppleFirstWeekday` → `{ gregorian = 1; }` (1 = Sunday).
fn defaults_first_weekday() -> Option<Weekday> {
    let out = std::process::Command::new("defaults").args(["read", "-g", "AppleFirstWeekday"]).output().ok()?;
    parse_defaults(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_defaults(s: &str) -> Option<Weekday> {
    let rest = &s[s.find("gregorian")?..];
    let n: u8 = rest.split(|c: char| !c.is_ascii_digit()).find(|t| !t.is_empty())?.parse().ok()?;
    (1..=7).contains(&n).then(|| from_index(n - 1))
}

/// The effective first day: the setting, else the system's.
pub fn first_day() -> Weekday {
    crate::prefs::get().week_start.map(from_index).unwrap_or_else(|| system().first)
}

pub fn is_weekend(d: Weekday) -> bool {
    system().weekend & (1 << d.num_days_from_sunday()) != 0
}

/// The first day of the week containing `d`.
pub fn start_of_week(d: NaiveDate, first: Weekday) -> NaiveDate {
    let back = (7 + d.weekday().num_days_from_sunday() - first.num_days_from_sunday()) % 7;
    d - Duration::days(i64::from(back))
}

pub fn week_days(d: NaiveDate, first: Weekday) -> Vec<NaiveDate> {
    let s = start_of_week(d, first);
    (0..7).map(|i| s + Duration::days(i)).collect()
}

/// The 6×7 days a month grid shows, starting on `first`.
pub fn month_grid(d: NaiveDate, first: Weekday) -> Vec<NaiveDate> {
    let f = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d);
    let s = start_of_week(f, first);
    (0..42).map(|i| s + Duration::days(i)).collect()
}

/// Weekdays in display order.
pub fn order(first: Weekday) -> Vec<Weekday> {
    std::iter::successors(Some(first), |w| Some(w.succ())).take(7).collect()
}

/// Week number to show: ISO only when weeks start Monday (ISO weeks are
/// Monday-based; a Sunday-start label would be off by a day half the time).
pub fn week_number(d: NaiveDate, first: Weekday) -> Option<u32> {
    (first == Weekday::Mon).then(|| d.iso_week().week())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn index_round_trip() {
        assert_eq!(from_index(0), Weekday::Sun);
        assert_eq!(from_index(6), Weekday::Sat);
        assert!((0..7u8).all(|i| from_index(i).num_days_from_sunday() == u32::from(i)));
    }

    #[test]
    fn week_starts() {
        let fri = d(2026, 10, 23);
        assert_eq!(start_of_week(fri, Weekday::Mon), d(2026, 10, 19));
        assert_eq!(start_of_week(fri, Weekday::Sun), d(2026, 10, 18));
        assert_eq!(start_of_week(fri, Weekday::Sat), d(2026, 10, 17));
        // The first day itself starts its own week.
        assert_eq!(start_of_week(d(2026, 10, 18), Weekday::Sun), d(2026, 10, 18));
        assert_eq!(start_of_week(d(2026, 10, 18), Weekday::Mon), d(2026, 10, 12));
        // The DST week (clocks go back Sun Oct 25 in Israel/EU): still 7 calendar days.
        let w = week_days(d(2026, 10, 27), Weekday::Sun);
        assert_eq!((w[0], w[6], w.len()), (d(2026, 10, 25), d(2026, 10, 31), 7));
        assert_eq!(order(Weekday::Sun)[..2], [Weekday::Sun, Weekday::Mon]);
        assert_eq!(order(Weekday::Sat)[6], Weekday::Fri);
    }

    #[test]
    fn month_grids_follow_the_first_day() {
        // October 2026 starts on a Thursday.
        assert_eq!(month_grid(d(2026, 10, 23), Weekday::Mon)[0], d(2026, 9, 28));
        assert_eq!(month_grid(d(2026, 10, 23), Weekday::Sun)[0], d(2026, 9, 27));
        // March 2026 starts on a Sunday: a Sunday grid opens on the 1st.
        assert_eq!(month_grid(d(2026, 3, 9), Weekday::Sun)[0], d(2026, 3, 1));
        assert_eq!(month_grid(d(2026, 3, 9), Weekday::Mon)[0], d(2026, 2, 23));
    }

    #[test]
    fn week_numbers_and_defaults() {
        assert_eq!(week_number(d(2026, 10, 19), Weekday::Mon), Some(43));
        assert_eq!(week_number(d(2026, 10, 18), Weekday::Sun), None);
        assert_eq!(parse_defaults("{\n    gregorian = 1;\n}\n"), Some(Weekday::Sun));
        assert_eq!(parse_defaults("{ gregorian = 2; }"), Some(Weekday::Mon));
        assert_eq!(parse_defaults(""), None);
    }
}
