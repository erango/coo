//! The daily hello: the first time Coo comes to the front each day, the
//! pigeon says good morning (or good evening, or happy first weekend day).
//! Which scene and what it says is decided here, from the local time, the
//! user's week (week.rs) and a few remembered days; ui/greeting.rs shows it.
//!
//! The day turns over at 04:00, so 1 am still belongs to "tonight". Priority:
//! a special day, then back after a week or more away, then a weekend day,
//! then the first workday before noon or the last workday after noon, then
//! the time of day. Everything is local; nothing is reported anywhere.

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, Timelike, Weekday};

/// The hour a greeting day starts.
pub const DAY_STARTS: u32 = 4;
/// Days without opening Coo before "Welcome back".
pub const AWAY_DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Coffee,
    LateMorning,
    Afternoon,
    Evening,
    Night,
    SmallHours,
    FirstWorkday,
    LastWorkday,
    WeekendFirst,
    WeekendSecond,
    Back,
    NewYear,
    BirdDay,
    LeapDay,
    PiDay,
    Spring,
    Summer,
    Autumn,
    Winter,
    Halloween,
    Anniversary,
}

impl Scene {
    pub const ALL: [Scene; 21] = [
        Scene::Coffee,
        Scene::LateMorning,
        Scene::Afternoon,
        Scene::Evening,
        Scene::Night,
        Scene::SmallHours,
        Scene::FirstWorkday,
        Scene::LastWorkday,
        Scene::WeekendFirst,
        Scene::WeekendSecond,
        Scene::Back,
        Scene::NewYear,
        Scene::BirdDay,
        Scene::LeapDay,
        Scene::PiDay,
        Scene::Spring,
        Scene::Summer,
        Scene::Autumn,
        Scene::Winter,
        Scene::Halloween,
        Scene::Anniversary,
    ];

    /// A short name (debug hook, CSS).
    pub fn id(self) -> &'static str {
        match self {
            Scene::Coffee => "coffee",
            Scene::LateMorning => "late-morning",
            Scene::Afternoon => "afternoon",
            Scene::Evening => "evening",
            Scene::Night => "night",
            Scene::SmallHours => "small-hours",
            Scene::FirstWorkday => "first-workday",
            Scene::LastWorkday => "last-workday",
            Scene::WeekendFirst => "weekend",
            Scene::WeekendSecond => "weekend-2",
            Scene::Back => "back",
            Scene::NewYear => "new-year",
            Scene::BirdDay => "bird-day",
            Scene::LeapDay => "leap-day",
            Scene::PiDay => "pi-day",
            Scene::Spring => "spring",
            Scene::Summer => "summer",
            Scene::Autumn => "autumn",
            Scene::Winter => "winter",
            Scene::Halloween => "halloween",
            Scene::Anniversary => "anniversary",
        }
    }

    pub fn from_id(id: &str) -> Option<Scene> {
        Scene::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// The greeting day a moment belongs to: before 04:00 it's still yesterday.
pub fn day_of(now: NaiveDateTime) -> NaiveDate {
    (now - Duration::hours(i64::from(DAY_STARTS))).date()
}

/// Has today's greeting not happened yet?
pub fn due(greeted: Option<NaiveDate>, now: NaiveDateTime) -> bool {
    greeted != Some(day_of(now))
}

/// What the scene choice depends on.
#[derive(Clone, Debug)]
pub struct Context {
    /// Local time now.
    pub now: NaiveDateTime,
    /// Weekend days as a bit set over `num_days_from_sunday` (week.rs).
    pub weekend: u8,
    /// The last greeting day Coo was used, before today.
    pub last_active: Option<NaiveDate>,
    /// The first day Coo had an account (the anniversary).
    pub since: Option<NaiveDate>,
    /// Southern hemisphere: the seasons swap.
    pub south: bool,
}

fn is_weekend(weekend: u8, d: Weekday) -> bool {
    weekend & (1 << d.num_days_from_sunday()) != 0
}

/// Whole years since the first day, when today is that anniversary.
fn anniversary_years(since: NaiveDate, day: NaiveDate) -> Option<u32> {
    let years = u32::try_from(day.year() - since.year()).ok().filter(|y| *y >= 1)?;
    (since.checked_add_months(Months::new(12 * years)) == Some(day)).then_some(years)
}

/// The first day of a season, on the usual equinox and solstice dates.
fn season(day: NaiveDate, south: bool) -> Option<Scene> {
    let north = match (day.month(), day.day()) {
        (3, 20) => Scene::Spring,
        (6, 21) => Scene::Summer,
        (9, 22) => Scene::Autumn,
        (12, 21) => Scene::Winter,
        _ => return None,
    };
    Some(if !south {
        north
    } else {
        match north {
            Scene::Spring => Scene::Autumn,
            Scene::Summer => Scene::Winter,
            Scene::Autumn => Scene::Spring,
            _ => Scene::Summer,
        }
    })
}

fn special(c: &Context, day: NaiveDate) -> Option<Scene> {
    if c.since.and_then(|s| anniversary_years(s, day)).is_some() {
        return Some(Scene::Anniversary);
    }
    let fixed = match (day.month(), day.day()) {
        (1, 1) => Some(Scene::NewYear),
        (1, 5) => Some(Scene::BirdDay),
        (2, 29) => Some(Scene::LeapDay),
        (3, 14) => Some(Scene::PiDay),
        (10, 31) => Some(Scene::Halloween),
        _ => None,
    };
    fixed.or_else(|| season(day, c.south))
}

fn time_of_day(hour: u32) -> Scene {
    match hour {
        4..=8 => Scene::Coffee,
        9..=11 => Scene::LateMorning,
        12..=16 => Scene::Afternoon,
        17..=20 => Scene::Evening,
        21..=23 | 0 => Scene::Night,
        _ => Scene::SmallHours,
    }
}

/// The scene for this moment.
pub fn pick(c: &Context) -> Scene {
    let day = day_of(c.now);
    let hour = c.now.hour();
    if let Some(s) = special(c, day) {
        return s;
    }
    if c.last_active.is_some_and(|l| (day - l).num_days() >= AWAY_DAYS) {
        return Scene::Back;
    }
    let wd = day.weekday();
    let we = |d: Weekday| is_weekend(c.weekend, d);
    if we(wd) {
        return if we(wd.pred()) { Scene::WeekendSecond } else { Scene::WeekendFirst };
    }
    // A greeting day runs to 04:00 the next morning: those small hours
    // count as after noon.
    let afternoon = !(DAY_STARTS..12).contains(&hour);
    if we(wd.pred()) && !afternoon {
        return Scene::FirstWorkday;
    }
    if we(wd.succ()) && afternoon {
        return Scene::LastWorkday;
    }
    time_of_day(hour)
}

/// Real numbers for the line (nothing made up).
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// Unread in the Inboxes.
    pub unread: u32,
    /// The next meeting today, as the clock shows it.
    pub next_meeting: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Greeting {
    pub scene: Scene,
    pub title: String,
    pub line: String,
}

fn weekday_name(d: Weekday) -> &'static str {
    match d {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// "3 unread, next meeting at 10:30." / "All quiet."
fn tally(f: &Facts, meeting: bool) -> String {
    match (f.unread, f.next_meeting.as_deref().filter(|_| meeting)) {
        (0, None) => "All quiet.".into(),
        (0, Some(t)) => format!("Nothing unread. Next meeting at {t}."),
        (n, None) => format!("{n} unread."),
        (n, Some(t)) => format!("{n} unread, next meeting at {t}."),
    }
}

/// What the greeting says. Weekday names only come from the real date.
pub fn words(scene: Scene, c: &Context, f: &Facts) -> Greeting {
    let day = day_of(c.now);
    let weekday = weekday_name(day.weekday());
    let n = f.unread;
    let (title, line): (String, String) = match scene {
        Scene::Coffee => ("Good morning".into(), format!("Coffee’s on. {}", tally(f, true))),
        Scene::LateMorning => ("Morning!".into(), format!("Wings stretched, crumbs ready. {}", tally(f, true))),
        Scene::Afternoon => ("Good afternoon".into(), format!("Lunch break? {}", tally(f, true))),
        Scene::Evening => ("Good evening".into(), format!("Tea time. {}", tally(f, false))),
        Scene::Night => (
            "Night owl mode".into(),
            match n {
                0 => "Well, night pigeon. All quiet.".into(),
                n => format!("Well, night pigeon. {n} unread, and it can wait till morning."),
            },
        ),
        Scene::SmallHours => ("Still up?".into(), "The mail will keep. So will the pigeon.".into()),
        Scene::FirstWorkday => ("New week, fresh start".into(), format!("Fresh crumbs. {}", tally(f, true))),
        Scene::LastWorkday => ("Last stretch before the weekend".into(), format!("Nearly there. {}", tally(f, true))),
        Scene::WeekendFirst => (format!("Happy {weekday}"), format!("Kite weather. {}", tally(f, false))),
        Scene::WeekendSecond => (format!("Slow {weekday}"), "Pancakes first. Mail later.".into()),
        Scene::Back => {
            let away = c.last_active.map(|l| (day - l).num_days()).unwrap_or(AWAY_DAYS);
            let rest = match n {
                0 => "All quiet.".into(),
                n => format!("{n} unread, no rush."),
            };
            ("Welcome back".into(), format!("You were away {away} days. {rest}"))
        }
        Scene::NewYear => ("Happy New Year".into(), "New year, same trusty pigeon.".into()),
        Scene::BirdDay => ("Happy Bird Day".into(), "Our day. Thanks for keeping a pigeon around.".into()),
        Scene::LeapDay => ("Leap day".into(), "A bonus day. Spend it wisely, or don’t.".into()),
        Scene::PiDay => (
            "Happy Pi Day".into(),
            match n {
                0 => "3.14159… unread? None, actually.".into(),
                n => format!("3.14159… unread? Fine, {n}."),
            },
        ),
        Scene::Spring => ("Spring is here".into(), "Things are growing. The inbox too, a little.".into()),
        Scene::Summer => ("Summer is here".into(), "The longest day. Plenty of light to read by.".into()),
        Scene::Autumn => ("Autumn is here".into(), "Leaves falling, mail landing.".into()),
        Scene::Winter => ("Winter is here".into(), "Scarf on. Cozy inbox weather.".into()),
        Scene::Halloween => (
            "Boo!".into(),
            match n {
                0 => "Only the pigeon in a sheet. Nothing spooky here.".into(),
                n => format!("Only the pigeon in a sheet. {n} unread, nothing spooky."),
            },
        ),
        Scene::Anniversary => {
            let years = c.since.and_then(|s| anniversary_years(s, day)).unwrap_or(1);
            let title = if years == 1 { "A year with Coo".to_string() } else { format!("{years} years with Coo") };
            (title, "Thanks for flying along. Cake’s on us.".into())
        }
    };
    Greeting { scene, title, line }
}

/// Is this IANA time zone south of the equator? Close enough for seasons.
pub fn southern_zone(zone: &str) -> bool {
    const SOUTH: &[&str] = &[
        "Australia/",
        "Antarctica/",
        "Pacific/Auckland",
        "Pacific/Chatham",
        "Pacific/Fiji",
        "Pacific/Tongatapu",
        "Pacific/Apia",
        "Pacific/Noumea",
        "Pacific/Efate",
        "Pacific/Tahiti",
        "Pacific/Easter",
        "NZ",
        "America/Argentina/",
        "America/Buenos_Aires",
        "America/Santiago",
        "America/Sao_Paulo",
        "America/Montevideo",
        "America/Asuncion",
        "America/La_Paz",
        "America/Lima",
        "America/Punta_Arenas",
        "America/Campo_Grande",
        "America/Cuiaba",
        "America/Recife",
        "America/Bahia",
        "America/Fortaleza",
        "America/Maceio",
        "America/Belem",
        "Brazil/",
        "Chile/",
        "Africa/Johannesburg",
        "Africa/Maputo",
        "Africa/Harare",
        "Africa/Lusaka",
        "Africa/Windhoek",
        "Africa/Gaborone",
        "Africa/Maseru",
        "Africa/Mbabane",
        "Africa/Blantyre",
        "Africa/Lubumbashi",
        "Africa/Luanda",
        "Africa/Dar_es_Salaam",
        "Africa/Kinshasa",
        "Africa/Brazzaville",
        "Indian/Antananarivo",
        "Indian/Mauritius",
        "Indian/Reunion",
        "Asia/Jakarta",
        "Asia/Makassar",
        "Asia/Jayapura",
        "Asia/Dili",
        "Pacific/Port_Moresby",
    ];
    SOUTH.iter().any(|p| zone.starts_with(p))
}

/// The Mac's time zone name (`/etc/localtime` points into zoneinfo).
pub fn local_zone() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    s.split_once("zoneinfo/").map(|(_, z)| z.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAT_SUN: u8 = 1 << 0 | 1 << 6;
    const FRI_SAT: u8 = 1 << 5 | 1 << 6;
    const SUN_ONLY: u8 = 1 << 0;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn ctx(now: &str, weekend: u8) -> Context {
        Context { now: at(now), weekend, last_active: None, since: None, south: false }
    }

    fn scene(now: &str, weekend: u8) -> Scene {
        pick(&ctx(now, weekend))
    }

    #[test]
    fn the_day_turns_over_at_four() {
        assert_eq!(day_of(at("2026-11-03 03:59")), d("2026-11-02"));
        assert_eq!(day_of(at("2026-11-03 04:00")), d("2026-11-03"));
        // Once a day: greeted at 23:00, 1 am is still that day; 4 am is new.
        let greeted = Some(day_of(at("2026-11-02 23:00")));
        assert!(!due(greeted, at("2026-11-03 01:30")));
        assert!(due(greeted, at("2026-11-03 04:00")));
        assert!(due(None, at("2026-11-03 12:00")));
    }

    #[test]
    fn time_of_day_bands() {
        // 2026-11-04 is a Wednesday.
        assert_eq!(scene("2026-11-04 04:00", SAT_SUN), Scene::Coffee);
        assert_eq!(scene("2026-11-04 08:59", SAT_SUN), Scene::Coffee);
        assert_eq!(scene("2026-11-04 09:00", SAT_SUN), Scene::LateMorning);
        assert_eq!(scene("2026-11-04 12:00", SAT_SUN), Scene::Afternoon);
        assert_eq!(scene("2026-11-04 17:00", SAT_SUN), Scene::Evening);
        assert_eq!(scene("2026-11-04 21:00", SAT_SUN), Scene::Night);
        assert_eq!(scene("2026-11-05 00:30", SAT_SUN), Scene::Night);
        assert_eq!(scene("2026-11-05 01:00", SAT_SUN), Scene::SmallHours);
        assert_eq!(scene("2026-11-05 03:59", SAT_SUN), Scene::SmallHours);
    }

    #[test]
    fn a_sat_sun_week() {
        // Mon 2026-11-02 … Sun 2026-11-08.
        assert_eq!(scene("2026-11-02 08:00", SAT_SUN), Scene::FirstWorkday);
        assert_eq!(scene("2026-11-02 12:30", SAT_SUN), Scene::Afternoon, "first workday only before noon");
        assert_eq!(scene("2026-11-06 10:00", SAT_SUN), Scene::LateMorning, "last workday only after noon");
        assert_eq!(scene("2026-11-06 14:00", SAT_SUN), Scene::LastWorkday);
        assert_eq!(scene("2026-11-07 08:00", SAT_SUN), Scene::WeekendFirst);
        assert_eq!(scene("2026-11-08 22:00", SAT_SUN), Scene::WeekendSecond);
        // Saturday's small hours (Sunday 2 am) still belong to Saturday.
        assert_eq!(scene("2026-11-08 02:00", SAT_SUN), Scene::WeekendFirst);
    }

    #[test]
    fn a_fri_sat_week() {
        // Sun 2026-11-01 starts the workweek, Thu 2026-11-05 ends it.
        assert_eq!(scene("2026-11-01 09:30", FRI_SAT), Scene::FirstWorkday);
        assert_eq!(scene("2026-11-02 09:30", FRI_SAT), Scene::LateMorning);
        assert_eq!(scene("2026-11-05 15:00", FRI_SAT), Scene::LastWorkday);
        assert_eq!(scene("2026-11-06 15:00", FRI_SAT), Scene::WeekendFirst);
        assert_eq!(scene("2026-11-07 10:00", FRI_SAT), Scene::WeekendSecond);
        let g = words(Scene::WeekendFirst, &ctx("2026-11-06 15:00", FRI_SAT), &Facts::default());
        assert_eq!(g.title, "Happy Friday");
        let g = words(Scene::WeekendSecond, &ctx("2026-11-07 10:00", FRI_SAT), &Facts::default());
        assert_eq!(g.title, "Slow Saturday");
    }

    #[test]
    fn a_one_day_weekend() {
        // Sunday only: Saturday is the last workday, Monday the first.
        assert_eq!(scene("2026-11-08 11:00", SUN_ONLY), Scene::WeekendFirst);
        assert_eq!(scene("2026-11-07 13:00", SUN_ONLY), Scene::LastWorkday);
        assert_eq!(scene("2026-11-09 07:00", SUN_ONLY), Scene::FirstWorkday);
        for day in 1..=7 {
            let s = scene(&format!("2026-11-{day:02} 10:00"), SUN_ONLY);
            assert_ne!(s, Scene::WeekendSecond);
        }
    }

    #[test]
    fn special_days_win_over_everything() {
        // Fixed secular dates.
        assert_eq!(scene("2027-01-01 10:00", SAT_SUN), Scene::NewYear);
        assert_eq!(scene("2027-01-05 10:00", SAT_SUN), Scene::BirdDay);
        assert_eq!(scene("2028-02-29 10:00", SAT_SUN), Scene::LeapDay);
        assert_eq!(scene("2027-03-14 10:00", SAT_SUN), Scene::PiDay, "a Sunday, still Pi Day");
        assert_eq!(scene("2026-10-31 10:00", SAT_SUN), Scene::Halloween, "a Saturday, still Halloween");
        // Small hours after Halloween are still Halloween.
        assert_eq!(scene("2026-11-01 02:00", SAT_SUN), Scene::Halloween);
        // Away a while on New Year's Day: the special day wins.
        let mut c = ctx("2027-01-01 10:00", SAT_SUN);
        c.last_active = Some(d("2026-12-10"));
        assert_eq!(pick(&c), Scene::NewYear);
    }

    #[test]
    fn seasons_in_both_hemispheres() {
        let mut c = ctx("2027-03-20 10:00", SAT_SUN);
        assert_eq!(pick(&c), Scene::Spring);
        c.south = true;
        assert_eq!(pick(&c), Scene::Autumn);
        let mut c = ctx("2027-06-21 10:00", SAT_SUN);
        assert_eq!(pick(&c), Scene::Summer);
        c.south = true;
        assert_eq!(pick(&c), Scene::Winter);
        let mut c = ctx("2027-09-22 10:00", SAT_SUN);
        assert_eq!(pick(&c), Scene::Autumn);
        c.south = true;
        assert_eq!(pick(&c), Scene::Spring);
        let mut c = ctx("2027-12-21 10:00", SAT_SUN);
        assert_eq!(pick(&c), Scene::Winter);
        c.south = true;
        assert_eq!(pick(&c), Scene::Summer);
        assert!(southern_zone("Australia/Sydney"));
        assert!(southern_zone("America/Argentina/Buenos_Aires"));
        assert!(!southern_zone("Asia/Jerusalem"));
        assert!(!southern_zone("America/New_York"));
    }

    #[test]
    fn the_anniversary() {
        let mut c = ctx("2027-11-02 10:00", SAT_SUN);
        c.since = Some(d("2026-11-02"));
        assert_eq!(pick(&c), Scene::Anniversary);
        assert_eq!(words(Scene::Anniversary, &c, &Facts::default()).title, "A year with Coo");
        c.now = at("2028-11-02 10:00");
        assert_eq!(words(Scene::Anniversary, &c, &Facts::default()).title, "2 years with Coo");
        c.now = at("2026-11-02 10:00");
        assert_eq!(pick(&c), Scene::FirstWorkday, "not on the first day itself");
        // It outranks New Year's Day.
        c.since = Some(d("2026-01-01"));
        c.now = at("2027-01-01 10:00");
        assert_eq!(pick(&c), Scene::Anniversary);
    }

    #[test]
    fn back_after_a_week_away() {
        let mut c = ctx("2026-11-04 10:00", SAT_SUN);
        c.last_active = Some(d("2026-10-29"));
        assert_eq!(pick(&c), Scene::LateMorning, "six days is not away");
        c.last_active = Some(d("2026-10-28"));
        assert_eq!(pick(&c), Scene::Back);
        let g = words(Scene::Back, &c, &Facts { unread: 214, next_meeting: None });
        assert_eq!(g.line, "You were away 7 days. 214 unread, no rush.");
        // It outranks the weekend.
        c.now = at("2026-11-07 10:00");
        assert_eq!(pick(&c), Scene::Back);
    }

    #[test]
    fn lines_use_real_numbers_only() {
        let c = ctx("2026-11-04 07:00", SAT_SUN);
        let quiet = Facts::default();
        assert_eq!(words(Scene::Coffee, &c, &quiet).line, "Coffee’s on. All quiet.");
        let busy = Facts { unread: 3, next_meeting: Some("10:30".into()) };
        assert_eq!(words(Scene::Coffee, &c, &busy).line, "Coffee’s on. 3 unread, next meeting at 10:30.");
        let meeting = Facts { unread: 0, next_meeting: Some("10:30".into()) };
        assert_eq!(words(Scene::Coffee, &c, &meeting).line, "Coffee’s on. Nothing unread. Next meeting at 10:30.");
        // Evenings don't bring up meetings.
        assert_eq!(words(Scene::Evening, &c, &busy).line, "Tea time. 3 unread.");
        // Fixed-day titles never name a weekday.
        for s in [Scene::FirstWorkday, Scene::LastWorkday] {
            let t = words(s, &c, &quiet).title;
            assert!(!["day", "Monday", "Friday"].iter().any(|w| t.ends_with(w)), "{t}");
        }
    }

    #[test]
    fn ids_round_trip() {
        for s in Scene::ALL {
            assert_eq!(Scene::from_id(s.id()), Some(s));
        }
        assert_eq!(Scene::from_id("nope"), None);
    }
}
