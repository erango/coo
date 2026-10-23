//! "What's new since last sync" for new-mail notifications.
//!
//! Each account has a watermark: the newest inbox `received_at` already
//! seen. The first look at an account (app start, or its first sync) only
//! sets the watermark, so existing mail and bootstrap never notify.

use std::collections::HashMap;

use pigeon_core::model::MessageRow;

/// Per-account watermark. `None` inside means "seen, inbox was empty":
/// the next look is still treated as bootstrap.
#[derive(Debug, Default)]
pub struct Marks(HashMap<String, Option<i64>>);

impl Marks {
    /// Messages to announce for `account` given its current newest inbox
    /// rows, advancing the watermark. Unread, not drafts, not sent by one of
    /// `mine` (case-insensitive), strictly newer than the watermark.
    pub fn fresh(&mut self, account: &str, inbox: &[MessageRow], mine: &[String]) -> Vec<MessageRow> {
        let newest = inbox.iter().map(|r| r.header.received_at).max();
        let Some(mark) = self.0.get(account).copied() else {
            self.0.insert(account.to_string(), newest);
            return Vec::new();
        };
        let Some(mark) = mark else {
            // Empty at first look: this is the bootstrap filling it.
            self.0.insert(account.to_string(), newest);
            return Vec::new();
        };
        let is_mine = |r: &MessageRow| {
            r.header.from.as_ref().is_some_and(|f| mine.iter().any(|m| m.eq_ignore_ascii_case(&f.email)))
        };
        let mut out: Vec<MessageRow> = inbox
            .iter()
            .filter(|r| r.header.received_at > mark && !r.header.is_read && !r.header.is_draft && !is_mine(r))
            .cloned()
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.header.received_at));
        if let Some(n) = newest {
            self.0.insert(account.to_string(), Some(n.max(mark)));
        }
        out
    }

    pub fn forget(&mut self, account: &str) {
        self.0.remove(account);
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Address, MessageHeader};

    use super::*;

    fn row(id: &str, at: i64, read: bool, from: &str) -> MessageRow {
        MessageRow {
            account_id: "a".into(),
            header: MessageHeader {
                id: id.into(),
                thread_id: id.into(),
                received_at: at,
                is_read: read,
                from: Some(Address::new(None, from)),
                ..Default::default()
            },
        }
    }

    #[test]
    fn first_look_only_sets_the_watermark() {
        let mut m = Marks::default();
        assert!(m.fresh("a", &[row("old", 10, false, "x@y.z")], &[]).is_empty());
        let got = m.fresh("a", &[row("new", 20, false, "x@y.z"), row("old", 10, false, "x@y.z")], &[]);
        assert_eq!(got.iter().map(|r| r.header.id.as_str()).collect::<Vec<_>>(), ["new"]);
        assert!(m.fresh("a", &[row("new", 20, false, "x@y.z")], &[]).is_empty(), "announced once");
    }

    #[test]
    fn empty_inbox_at_first_look_is_bootstrap() {
        let mut m = Marks::default();
        assert!(m.fresh("a", &[], &[]).is_empty());
        // First sync fills it: no flood of notifications.
        assert!(m.fresh("a", &[row("b1", 5, false, "x@y.z"), row("b2", 6, false, "x@y.z")], &[]).is_empty());
        assert_eq!(m.fresh("a", &[row("n", 9, false, "x@y.z")], &[]).len(), 1);
    }

    #[test]
    fn skips_read_drafts_and_own_mail_newest_first() {
        let mut m = Marks::default();
        m.fresh("a", &[row("seed", 1, true, "x@y.z")], &[]);
        let mut draft = row("draft", 5, false, "x@y.z");
        draft.header.is_draft = true;
        let got = m.fresh(
            "a",
            &[
                row("read", 4, true, "x@y.z"),
                draft,
                row("mine", 6, false, "Me@Work.com"),
                row("n1", 7, false, "p@q.r"),
                row("n2", 8, false, "p@q.r"),
            ],
            &["me@work.com".to_string()],
        );
        assert_eq!(got.iter().map(|r| r.header.id.as_str()).collect::<Vec<_>>(), ["n2", "n1"]);
    }
}
