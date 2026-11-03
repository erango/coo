//! Watches: saved rules for mail you care about ("from @acme.com with
//! 'invoice' in the subject"). The app checks new mail against them as it
//! lands; a match goes to Needs you, where you or a connected assistant
//! picks it up. `coo watch` streams use the same filters.

use serde::{Deserialize, Serialize};

use crate::model::MessageRow;

/// Messages received this long before a watch started still count (a sync
/// can lag a little); older ones that land later (a label filled in, a
/// server search) are history, not new mail.
pub const GRACE_MS: i64 = 10 * 60 * 1000;
/// A saved watch catches mail received at most this long before it's
/// stored (mail that arrived while the Mac was off still counts).
pub const RECENT_MS: i64 = 3 * 24 * 3_600_000;

/// What a watch waits for. Each list matches any of its values; the lists
/// that are given must all match. No filters: any new mail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    /// Conversations (thread ids).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub threads: Vec<String>,
    /// Sender addresses or parts of them (`@acme.com`), any case.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub from: Vec<String>,
    /// Text the subject contains, any case.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subjects: Vec<String>,
    /// Account ids or addresses (parts match).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub accounts: Vec<String>,
    /// Also catch what you send yourself (off: only incoming mail).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub include_mine: bool,
}

impl Filters {
    /// Whether `m` is new mail these filters catch. `mine`: the user's own
    /// addresses, lowercased; `since`: when the watch started (epoch ms).
    pub fn matches(&self, m: &MessageRow, mine: &[String], since: i64) -> bool {
        let h = &m.header;
        let from = h.from.as_ref().map(|a| a.email.to_lowercase()).unwrap_or_default();
        let any = |list: &[String], f: &dyn Fn(&str) -> bool| list.is_empty() || list.iter().any(|v| f(v));
        !h.is_draft
            && h.received_at >= since - GRACE_MS
            && (self.include_mine || !mine.contains(&from))
            && any(&self.threads, &|t| h.thread_id == t)
            && any(&self.from, &|f| from.contains(&f.trim().to_lowercase()))
            && any(&self.subjects, &|s| h.subject.to_lowercase().contains(&s.trim().to_lowercase()))
            && any(&self.accounts, &|a| m.account_id.to_lowercase().contains(&a.trim().to_lowercase()))
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty() && self.from.is_empty() && self.subjects.is_empty() && self.accounts.is_empty()
    }
}

/// A saved watch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    pub id: String,
    pub name: String,
    pub filters: Filters,
    /// What the user wants an assistant to do with a match (may be empty).
    pub note: String,
    /// Also show a notification for each match.
    pub notify: bool,
    /// Who made it: empty for the user, else the assistant's name.
    pub created_by: String,
    pub created_at: i64,
    pub paused: bool,
    /// Stops catching after this moment (epoch ms).
    pub until: Option<i64>,
    pub hits: u32,
    pub last_hit_at: Option<i64>,
}

impl Watch {
    pub fn new(name: &str, filters: Filters, now: i64) -> Self {
        Watch {
            id: format!("w{}", crate::auth::random_token(4).to_lowercase()),
            name: name.trim().to_string(),
            filters,
            note: String::new(),
            notify: false,
            created_by: String::new(),
            created_at: now,
            paused: false,
            until: None,
            hits: 0,
            last_hit_at: None,
        }
    }

    /// Catching now: not paused, not past `until`.
    pub fn active(&self, now: i64) -> bool {
        !self.paused && self.until.is_none_or(|u| now < u)
    }

    /// Whether this watch catches `m`, stored just now. Only recent mail
    /// counts: a newly added account's history isn't news.
    pub fn catches(&self, m: &MessageRow, mine: &[String], now: i64) -> bool {
        self.active(now) && self.filters.matches(m, mine, self.created_at.max(now - RECENT_MS))
    }
}

/// One message a watch caught.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsItem {
    pub id: i64,
    pub watch_id: String,
    pub watch_name: String,
    /// The watch's note for the assistant.
    pub note: String,
    pub account_id: String,
    pub message_id: String,
    pub thread_id: String,
    pub caught_at: i64,
    /// Marked done (by the user or an assistant).
    pub done_at: Option<i64>,
    /// Who marked it done: empty for the user, else the assistant's name.
    pub done_by: String,
    /// What was done ("Drafted a reply"), when an assistant said.
    pub done_note: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Address, MessageHeader};

    fn row(account: &str, thread: &str, from: &str, subject: &str, at: i64) -> MessageRow {
        MessageRow {
            account_id: account.into(),
            header: MessageHeader {
                id: "m".into(),
                thread_id: thread.into(),
                internet_message_id: None,
                subject: subject.into(),
                from: Some(Address::new(None, from)),
                to: vec![],
                cc: vec![],
                snippet: String::new(),
                received_at: at,
                is_read: false,
                is_flagged: false,
                has_attachments: false,
                is_draft: false,
                folder_ids: vec![],
            },
        }
    }

    #[test]
    fn filters_combine_lists_and_skip_drafts_history_and_own_mail() {
        let mine = vec!["me@x.io".to_string()];
        let m = row("google:me@x.io", "t1", "Dana@Acme.com", "Invoice 42", 1_000_000);
        let none = Filters::default();
        assert!(none.matches(&m, &mine, 1_000_000), "no filters: any new mail");

        let f = Filters {
            from: vec!["@acme.com".into(), "bob@".into()],
            subjects: vec!["invoice".into()],
            ..Default::default()
        };
        assert!(f.matches(&m, &mine, 1_000_000));
        let f = Filters { from: vec!["@acme.com".into()], subjects: vec!["receipt".into()], ..Default::default() };
        assert!(!f.matches(&m, &mine, 1_000_000), "every given list must match");
        assert!(Filters { threads: vec!["t1".into()], ..Default::default() }.matches(&m, &mine, 1_000_000));
        assert!(!Filters { threads: vec!["t2".into()], ..Default::default() }.matches(&m, &mine, 1_000_000));
        assert!(!Filters { accounts: vec!["microsoft".into()], ..Default::default() }.matches(&m, &mine, 1_000_000));

        let mut draft = m.clone();
        draft.header.is_draft = true;
        assert!(!none.matches(&draft, &mine, 1_000_000));
        assert!(!none.matches(&m, &mine, 1_000_000 + GRACE_MS + 1), "older mail arriving late is history");

        let sent = row("google:me@x.io", "t1", "ME@x.io", "Re: Invoice 42", 1_000_000);
        assert!(!none.matches(&sent, &mine, 1_000_000));
        assert!(Filters { include_mine: true, ..Default::default() }.matches(&sent, &mine, 1_000_000));
    }

    #[test]
    fn paused_and_expired_watches_catch_nothing() {
        let m = row("google:me@x.io", "t1", "dana@acme.com", "Hi", 1_000_000);
        let mut w = Watch::new("All", Filters::default(), 1_000_000);
        assert!(w.id.starts_with('w'));
        assert!(w.catches(&m, &[], 1_000_000));
        w.paused = true;
        assert!(!w.catches(&m, &[], 1_000_000));
        w.paused = false;
        w.until = Some(1_000_000);
        assert!(!w.catches(&m, &[], 1_000_000));
    }
}
