//! Mail rules and their activity log (`crate::rules`).

use rusqlite::{OptionalExtension, params};

use super::{MSG_COL_COUNT, MSG_COLS, Store, header_from_row};
use crate::Result;
use crate::model::MessageRow;
use crate::rules::{Logged, Rule};

/// Activity is kept this long.
const KEEP_LOG_MS: i64 = 30 * 24 * 3_600_000;

impl Store {
    /// Add or replace a rule.
    pub fn save_rule(&self, r: &Rule) -> Result<()> {
        self.conn.execute(
            "INSERT INTO rules (id, json, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET json = excluded.json",
            params![r.id, serde_json::to_string(r)?, r.created_at],
        )?;
        Ok(())
    }

    /// Every rule, oldest first.
    pub fn rules(&self) -> Result<Vec<Rule>> {
        let mut stmt = self.conn.prepare_cached("SELECT json FROM rules ORDER BY created_at, id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?)?);
        }
        Ok(out)
    }

    pub fn rule(&self, id: &str) -> Result<Option<Rule>> {
        let json: Option<String> = self
            .conn
            .prepare_cached("SELECT json FROM rules WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    /// Remove a rule (its activity stays until it ages out). Returns
    /// whether it existed.
    pub fn delete_rule(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM rules WHERE id = ?1", [id])? > 0)
    }

    /// Record what a rule did to a message, once per message; counts the
    /// hit. Returns whether it's new.
    pub fn log_rule(&self, entry: &Logged) -> Result<bool> {
        let added = self.conn.execute(
            "INSERT OR IGNORE INTO rule_log (rule_id, account_id, message_id, at, json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.rule_id, entry.account_id, entry.message_id, entry.at, serde_json::to_string(entry)?],
        )? > 0;
        if added && let Some(mut r) = self.rule(&entry.rule_id)? {
            r.hits += 1;
            r.last_hit_at = Some(entry.at);
            self.save_rule(&r)?;
        }
        Ok(added)
    }

    /// What rules did, newest first, at most `limit`.
    pub fn rule_log(&self, limit: u32, now: i64) -> Result<Vec<Logged>> {
        self.conn.execute("DELETE FROM rule_log WHERE at < ?1", [now - KEEP_LOG_MS])?;
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, at, undone_at, json FROM rule_log ORDER BY at DESC, id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at, undone_at, json) = row?;
            if let Ok(mut e) = serde_json::from_str::<Logged>(&json) {
                (e.id, e.at, e.undone_at) = (id, at, undone_at);
                out.push(e);
            }
        }
        Ok(out)
    }

    pub fn rule_log_entries(&self, ids: &[i64]) -> Result<Vec<Logged>> {
        Ok(self.rule_log(u32::MAX, crate::now_ms())?.into_iter().filter(|e| ids.contains(&e.id)).collect())
    }

    pub fn set_rule_log_undone(&self, ids: &[i64], at: i64) -> Result<()> {
        let mut stmt = self.conn.prepare_cached("UPDATE rule_log SET undone_at = ?1 WHERE id = ?2")?;
        for id in ids {
            stmt.execute(params![at, id])?;
        }
        Ok(())
    }

    /// Messages received since `since` (epoch ms), newest first, at most
    /// `limit`, in one account or all; with whether each is in the inbox.
    pub fn messages_since(&self, account_id: Option<&str>, since: i64, limit: u32) -> Result<Vec<(MessageRow, bool)>> {
        let sql = format!(
            "SELECT {MSG_COLS}, EXISTS (SELECT 1 FROM message_folders mf JOIN folders f
                    ON f.account_id = mf.account_id AND f.id = mf.folder_id
                    WHERE mf.message_pk = m.pk AND f.role = 'inbox')
             FROM messages m
             WHERE m.received_at >= ?1 AND (?2 IS NULL OR m.account_id = ?2) AND m.is_draft = 0
             ORDER BY m.received_at DESC LIMIT ?3"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params![since, account_id, limit], |r| {
                Ok((MessageRow { account_id: r.get(0)?, header: header_from_row(r)? }, r.get(MSG_COL_COUNT)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::{Rule, Then, When};
    use crate::store::Store;

    #[test]
    fn rules_round_trip_and_log_once() {
        let store = Store::open_in_memory().unwrap();
        let when = When { from: vec!["@x.io".into()], ..Default::default() };
        let r = Rule::new("X", None, when, Then { archive: true, ..Default::default() }, 5);
        store.save_rule(&r).unwrap();
        assert_eq!(store.rules().unwrap(), vec![r.clone()]);
        let entry = crate::rules::Logged {
            id: 0,
            rule_id: r.id.clone(),
            rule_name: "X".into(),
            account_id: "a".into(),
            message_id: "m".into(),
            thread_id: "t".into(),
            from: "Dana".into(),
            subject: "Hi".into(),
            what: "Skip inbox".into(),
            how: "In Coo".into(),
            then: r.then.clone(),
            at: 10,
            undone_at: None,
        };
        assert!(store.log_rule(&entry).unwrap());
        assert!(!store.log_rule(&entry).unwrap(), "once per message");
        assert_eq!(store.rule(&r.id).unwrap().unwrap().hits, 1);
        let log = store.rule_log(10, 20).unwrap();
        assert_eq!(log.len(), 1);
        store.set_rule_log_undone(&[log[0].id], 30).unwrap();
        assert_eq!(store.rule_log(10, 40).unwrap()[0].undone_at, Some(30));
        assert!(store.delete_rule(&r.id).unwrap());
        assert!(store.rules().unwrap().is_empty());
    }
}
