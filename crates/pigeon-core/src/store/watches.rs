//! Watches and Needs you (`crate::watches`).

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::Result;
use crate::watches::{NeedsItem, Watch};

/// Done items are kept this long (so an assistant can see what was handled).
const KEEP_DONE_MS: i64 = 30 * 24 * 3_600_000;

impl Store {
    /// Add or replace a watch.
    pub fn save_watch(&self, w: &Watch) -> Result<()> {
        self.conn.execute(
            "INSERT INTO watches (id, json, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET json = excluded.json",
            params![w.id, serde_json::to_string(w)?, w.created_at],
        )?;
        Ok(())
    }

    /// Every watch, oldest first.
    pub fn watches(&self) -> Result<Vec<Watch>> {
        let mut stmt = self.conn.prepare_cached("SELECT json FROM watches ORDER BY created_at, id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?)?);
        }
        Ok(out)
    }

    pub fn watch(&self, id: &str) -> Result<Option<Watch>> {
        let json: Option<String> = self
            .conn
            .prepare_cached("SELECT json FROM watches WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    /// Remove a watch and what it caught. Returns whether it existed.
    pub fn delete_watch(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM watches WHERE id = ?1", [id])? > 0)
    }

    /// Record that `watch` caught a message (once per message); counts the
    /// hit. Returns whether it's new.
    pub fn catch(&self, watch: &Watch, account_id: &str, message_id: &str, thread_id: &str, now: i64) -> Result<bool> {
        let added = self.conn.execute(
            "INSERT OR IGNORE INTO needs_you (watch_id, account_id, message_id, thread_id, caught_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![watch.id, account_id, message_id, thread_id, now],
        )? > 0;
        if added && let Some(mut w) = self.watch(&watch.id)? {
            w.hits += 1;
            w.last_hit_at = Some(now);
            self.save_watch(&w)?;
        }
        Ok(added)
    }

    /// Needs you, newest first: what's open, plus what was done in the
    /// last `done_within` ms when given. Items whose message is gone (moved
    /// away on the server, deleted) are left out.
    pub fn needs_you(&self, done_within: Option<i64>, now: i64) -> Result<Vec<NeedsItem>> {
        self.conn.execute("DELETE FROM needs_you WHERE done_at < ?1", [now - KEEP_DONE_MS])?;
        let since = done_within.map_or(i64::MAX, |w| now - w);
        let mut stmt = self.conn.prepare_cached(
            "SELECT n.id, n.watch_id, w.json, n.account_id, n.message_id, n.thread_id, n.caught_at,
                    n.done_at, n.done_by, n.done_note
             FROM needs_you n JOIN watches w ON w.id = n.watch_id
             WHERE (n.done_at IS NULL OR n.done_at >= ?1)
               AND EXISTS (SELECT 1 FROM messages m WHERE m.account_id = n.account_id AND m.id = n.message_id)
             ORDER BY n.caught_at DESC, n.id DESC",
        )?;
        let rows = stmt
            .query_map([since], |r| {
                let watch: Option<Watch> = serde_json::from_str(&r.get::<_, String>(2)?).ok();
                Ok(NeedsItem {
                    id: r.get(0)?,
                    watch_id: r.get(1)?,
                    watch_name: watch.as_ref().map(|w| w.name.clone()).unwrap_or_default(),
                    note: watch.map(|w| w.note).unwrap_or_default(),
                    account_id: r.get(3)?,
                    message_id: r.get(4)?,
                    thread_id: r.get(5)?,
                    caught_at: r.get(6)?,
                    done_at: r.get(7)?,
                    done_by: r.get(8)?,
                    done_note: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Mark an item done (or open again with `done` false). `by`: empty for
    /// the user, else the assistant's name. Returns whether it exists.
    pub fn set_needs_done(&self, id: i64, done: bool, by: &str, note: &str, now: i64) -> Result<bool> {
        let changed = if done {
            self.conn.execute(
                "UPDATE needs_you SET done_at = ?2, done_by = ?3, done_note = ?4 WHERE id = ?1",
                params![id, now, by, note],
            )?
        } else {
            self.conn
                .execute("UPDATE needs_you SET done_at = NULL, done_by = '', done_note = '' WHERE id = ?1", [id])?
        };
        Ok(changed > 0)
    }

    /// Mark done every open item about this message (it was handled some
    /// other way, e.g. archived or answered from Needs you's reader).
    pub fn set_message_needs_done(&self, account_id: &str, message_id: &str, now: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE needs_you SET done_at = ?3 WHERE account_id = ?1 AND message_id = ?2 AND done_at IS NULL",
            params![account_id, message_id, now],
        )?)
    }
}
