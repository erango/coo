//! Milestones: a one-time card for a few moments worth a smile (the 1,000th
//! archive, a week of inbox zero, a year with Coo). Local only: the counts
//! live in prefs (`MilestoneState`), nothing is reported anywhere.
//!
//! Each milestone shows once ever, and never more than one card in 30 days.
//! One reached while that limit holds waits its turn, but only for 45 days:
//! older than that, it's quietly dropped (a card about something from two
//! months ago isn't a nice surprise any more).

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

/// Never more than one card in this long.
pub const GAP_MS: i64 = 30 * 24 * 3600 * 1000;
/// A milestone waiting its turn is dropped after this long.
pub const STALE_MS: i64 = 45 * 24 * 3600 * 1000;

/// The counts behind the milestones, saved in prefs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MilestoneState {
    /// Messages archived from Coo.
    pub archived: u64,
    /// Messages sent from Coo.
    pub sent: u64,
    /// The last local day (YYYY-MM-DD) the unified Inbox was empty, and how
    /// many days in a row it has been, ending that day.
    pub zero_day: String,
    pub zero_streak: u32,
    /// The first local day Coo had an account (for the anniversary). For
    /// people who had Coo before milestones existed: the day they arrived.
    pub since_day: String,
    /// Milestones reached and not yet shown, with when (epoch ms).
    pub waiting: Vec<(String, i64)>,
    /// Milestones already shown (or dropped): never again.
    pub done: Vec<String>,
    /// When the last card showed (epoch ms; 0: never).
    pub last_shown: i64,
}

/// One milestone: its id, the card's line, and the little badge on the
/// pigeon (an app icon name).
pub struct Milestone {
    pub id: &'static str,
    pub line: &'static str,
    pub badge: &'static str,
}

pub const ALL: &[Milestone] = &[
    Milestone { id: "archived-100", line: "100 archived. Nice and tidy.", badge: "archive" },
    Milestone { id: "archived-1000", line: "1,000 archived. Tidy!", badge: "archive" },
    Milestone { id: "archived-10000", line: "10,000 archived. The pigeon is impressed.", badge: "archive" },
    Milestone { id: "sent-100", line: "100 messages sent. Busy wings.", badge: "send" },
    Milestone { id: "sent-1000", line: "1,000 messages sent. That’s a lot of flights.", badge: "send" },
    Milestone { id: "zero-7", line: "A week of inbox zero. Calm skies.", badge: "sun" },
    Milestone { id: "zero-30", line: "A month of inbox zero. Legendary.", badge: "sun" },
    Milestone { id: "year-1", line: "A year with Coo. Thanks for flying along.", badge: "cal" },
];

pub fn find(id: &str) -> Option<&'static Milestone> {
    ALL.iter().find(|m| m.id == id)
}

const ARCHIVED: [u64; 3] = [100, 1_000, 10_000];
const SENT: [u64; 2] = [100, 1_000];
const ZERO: [u32; 2] = [7, 30];

/// The thresholds a count passes going from `before` to `after`.
fn crossed(before: u64, after: u64, at: &[u64]) -> Vec<u64> {
    at.iter().copied().filter(|t| before < *t && after >= *t).collect()
}

/// The inbox-zero streak after an empty inbox on `today`, given the last
/// such day and the streak ending on it.
pub fn streak_after(last: Option<NaiveDate>, streak: u32, today: NaiveDate) -> u32 {
    match last {
        Some(d) if d == today => streak.max(1),
        Some(d) if d.checked_add_days(Days::new(1)) == Some(today) => streak + 1,
        _ => 1,
    }
}

fn day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

impl MilestoneState {
    fn reach(&mut self, id: &str, now: i64) {
        if !self.done.iter().any(|d| d == id) && !self.waiting.iter().any(|(w, _)| w == id) {
            self.waiting.push((id.to_string(), now));
        }
    }

    /// `n` more messages archived.
    pub fn archived(&mut self, n: u64, now: i64) {
        let before = self.archived;
        self.archived += n;
        for t in crossed(before, self.archived, &ARCHIVED) {
            self.reach(&format!("archived-{t}"), now);
        }
    }

    /// One more message sent.
    pub fn sent(&mut self, now: i64) {
        let before = self.sent;
        self.sent += 1;
        for t in crossed(before, self.sent, &SENT) {
            self.reach(&format!("sent-{t}"), now);
        }
    }

    /// The unified Inbox is empty on `today`.
    pub fn inbox_zero(&mut self, today: NaiveDate, now: i64) {
        let streak = streak_after(day(&self.zero_day), self.zero_streak, today);
        let before = if day(&self.zero_day) == Some(today) { self.zero_streak } else { streak - 1 };
        self.zero_day = today.format("%Y-%m-%d").to_string();
        self.zero_streak = streak;
        for t in crossed(u64::from(before), u64::from(streak), &ZERO.map(u64::from)) {
            self.reach(&format!("zero-{t}"), now);
        }
    }

    /// Coo has an account on `today`: start the anniversary clock, or
    /// notice a year has passed.
    pub fn has_accounts(&mut self, today: NaiveDate, now: i64) {
        match day(&self.since_day) {
            None => self.since_day = today.format("%Y-%m-%d").to_string(),
            Some(since) if since.checked_add_months(chrono::Months::new(12)).is_some_and(|y| today >= y) => {
                self.reach("year-1", now);
            }
            Some(_) => {}
        }
    }

    /// The card to show now, if any (marking it shown), dropping waiting
    /// ones that went stale. One card per `GAP_MS`.
    pub fn take_due(&mut self, now: i64) -> Option<&'static Milestone> {
        let stale: Vec<String> =
            self.waiting.iter().filter(|(_, at)| now - at > STALE_MS).map(|(id, _)| id.clone()).collect();
        self.waiting.retain(|(id, _)| !stale.contains(id));
        self.done.extend(stale);
        if self.last_shown != 0 && now - self.last_shown < GAP_MS {
            return None;
        }
        // The most recent first (it's the one that just happened); of two
        // reached at once, the bigger (the later in `ALL`).
        let rank = |id: &str| ALL.iter().position(|m| m.id == id).unwrap_or(0);
        let (id, _) = self.waiting.iter().max_by_key(|(id, at)| (*at, rank(id)))?.clone();
        self.waiting.retain(|(w, _)| *w != id);
        self.done.push(id.clone());
        self.last_shown = now;
        find(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 3600 * 1000;

    fn d(s: &str) -> NaiveDate {
        day(s).unwrap()
    }

    #[test]
    fn thresholds_fire_once_when_crossed() {
        let mut m = MilestoneState::default();
        m.archived(99, 0);
        assert!(m.waiting.is_empty());
        m.archived(5, 10);
        assert_eq!(m.waiting, vec![("archived-100".to_string(), 10)]);
        // A bulk archive that jumps two thresholds reaches both.
        m.archived(10_000, 20);
        assert_eq!(m.waiting.len(), 3);
        assert_eq!(m.take_due(30).map(|x| x.id), Some("archived-10000"));
        assert!(m.take_due(31).is_none(), "one card a month");
        // Shown ones never come back.
        m.waiting.clear();
        m.archived = 0;
        m.archived(20_000, 40);
        assert!(!m.waiting.iter().any(|(id, _)| id == "archived-10000"));
        assert_eq!(m.waiting.len(), 2);
    }

    #[test]
    fn streaks_count_consecutive_days() {
        assert_eq!(streak_after(None, 0, d("2026-10-30")), 1);
        assert_eq!(streak_after(Some(d("2026-10-29")), 3, d("2026-10-30")), 4);
        assert_eq!(streak_after(Some(d("2026-10-30")), 4, d("2026-10-30")), 4);
        assert_eq!(streak_after(Some(d("2026-10-27")), 4, d("2026-10-30")), 1);
        let mut m = MilestoneState::default();
        let start = d("2026-10-01");
        for i in 0..7 {
            let today = start.checked_add_days(Days::new(i)).unwrap();
            m.inbox_zero(today, 0);
            m.inbox_zero(today, 0); // twice the same day counts once
        }
        assert_eq!(m.zero_streak, 7);
        assert_eq!(m.waiting, vec![("zero-7".to_string(), 0)]);
    }

    #[test]
    fn one_card_a_month_and_stale_ones_drop() {
        // Times as days since some start (0 means "never shown").
        let t = |days: i64| 1_000 * DAY + days * DAY;
        let mut m = MilestoneState { sent: 99, ..Default::default() };
        m.sent(t(0));
        assert_eq!(m.take_due(t(0)).map(|x| x.id), Some("sent-100"));
        m.archived(100, t(1));
        assert!(m.take_due(t(1)).is_none());
        // Still waiting at 30 days: shows then.
        assert_eq!(m.take_due(t(30)).map(|x| x.id), Some("archived-100"));
        // One reached and left waiting past 45 days is dropped for good.
        m.archived(900, t(31));
        assert!(m.take_due(t(77)).is_none());
        assert!(m.done.contains(&"archived-1000".to_string()));
    }

    #[test]
    fn a_year_with_coo() {
        let mut m = MilestoneState::default();
        m.has_accounts(d("2025-10-30"), 0);
        assert_eq!(m.since_day, "2025-10-30");
        m.has_accounts(d("2026-10-29"), 1);
        assert!(m.waiting.is_empty());
        m.has_accounts(d("2026-10-30"), 2);
        assert_eq!(m.waiting, vec![("year-1".to_string(), 2)]);
        m.has_accounts(d("2026-10-31"), 3);
        assert_eq!(m.waiting.len(), 1);
        assert!(ALL.iter().all(|x| find(x.id).is_some()));
    }
}
