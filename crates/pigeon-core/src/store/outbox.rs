//! Optimistic mutations: local effects, the outbox queue, and snapshots for
//! rollback/undo.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, message_pk};
use crate::model::*;
use crate::{Error, Result, now_ms};

impl Store {
    /// Apply `op` locally and queue it for the server, atomically. Returns
    /// the outbox id (the undo handle).
    pub fn enqueue(&mut self, account_id: &str, op: &MailOp) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let snapshot = snapshot(&tx, account_id, op.ids())?;
        apply_local(&tx, account_id, op)?;
        tx.prepare_cached(
            "INSERT INTO outbox (account_id, op, payload, snapshot_json, created_at, state, next_attempt_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0)",
        )?
        .execute(params![
            account_id,
            op_name(op),
            serde_json::to_string(op)?,
            serde_json::to_string(&snapshot)?,
            now_ms()
        ])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// Pending entries due for (re)sending, oldest first, stopping before the
    /// first one still backing off so ops never overtake each other.
    pub fn due_ops(&self, account_id: &str, now: i64) -> Result<Vec<OutboxEntry>> {
        entries(
            &self.conn,
            "WHERE account_id = ?1 AND state = 'pending' AND id < COALESCE(
                (SELECT MIN(id) FROM outbox WHERE account_id = ?1 AND state = 'pending' AND next_attempt_at > ?2),
                9223372036854775807)",
            params![account_id, now],
        )
    }

    pub fn outbox_entry(&self, id: i64) -> Result<Option<OutboxEntry>> {
        Ok(entries(&self.conn, "WHERE id = ?1", params![id])?.into_iter().next())
    }

    pub fn mark_op_done(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE outbox SET state = 'done', last_error = NULL WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn mark_op_retry(&self, id: i64, error: &str, next_attempt_at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2, next_attempt_at = ?3 WHERE id = ?1",
            params![id, error, next_attempt_at],
        )?;
        Ok(())
    }

    /// The server rejected the op: restore the pre-op state and drop it.
    pub fn rollback_op(&mut self, id: i64) -> Result<()> {
        let Some(entry) = self.outbox_entry(id)? else { return Ok(()) };
        let tx = self.conn.transaction()?;
        restore(&tx, &entry.account_id, &entry.snapshot)?;
        tx.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        reapply_pending(&tx, &entry.account_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Undo before the op reached the server: restore and drop it. Returns
    /// false if it was already sent (the caller then queues inverse ops).
    pub fn cancel_pending_op(&mut self, id: i64) -> Result<bool> {
        match self.outbox_entry(id)? {
            Some(e) if e.state == OutboxState::Pending && e.attempts == 0 => {
                self.rollback_op(id)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn message_states(&self, account_id: &str, ids: &[String]) -> Result<Vec<MessageState>> {
        snapshot(&self.conn, account_id, ids)
    }

    /// Forget sent entries older than `before` (past the undo window).
    pub fn prune_done_ops(&self, before: i64) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM outbox WHERE state = 'done' AND created_at < ?1", [before])?)
    }

    pub fn role_folder(&self, account_id: &str, role: FolderRole) -> Result<Option<String>> {
        role_folder(&self.conn, account_id, role)
    }
}

fn op_name(op: &MailOp) -> &'static str {
    match op {
        MailOp::SetRead { .. } => "set_read",
        MailOp::SetFlagged { .. } => "set_flagged",
        MailOp::Relabel { .. } => "relabel",
        MailOp::MoveTo { .. } => "move_to",
        MailOp::Trash { .. } => "trash",
        MailOp::Untrash { .. } => "untrash",
    }
}

fn entries(conn: &Connection, filter: &str, args: impl rusqlite::Params) -> Result<Vec<OutboxEntry>> {
    let sql = format!(
        "SELECT id, account_id, payload, snapshot_json, attempts, state, last_error FROM outbox {filter} ORDER BY id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(args, |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, u32>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, account_id, payload, snapshot, attempts, state, last_error) = row?;
        out.push(OutboxEntry {
            id,
            account_id,
            op: serde_json::from_str(&payload)?,
            snapshot: serde_json::from_str(&snapshot)?,
            attempts,
            state: if state == "done" { OutboxState::Done } else { OutboxState::Pending },
            last_error,
        });
    }
    Ok(out)
}

fn role_folder(conn: &Connection, account_id: &str, role: FolderRole) -> Result<Option<String>> {
    Ok(conn
        .prepare_cached("SELECT id FROM folders WHERE account_id = ?1 AND role = ?2")?
        .query_row(params![account_id, role.as_str()], |r| r.get(0))
        .optional()?)
}

fn snapshot(conn: &Connection, account_id: &str, ids: &[String]) -> Result<Vec<MessageState>> {
    let mut msg =
        conn.prepare_cached("SELECT pk, is_read, is_flagged FROM messages WHERE account_id = ?1 AND id = ?2")?;
    let mut folders =
        conn.prepare_cached("SELECT folder_id FROM message_folders WHERE message_pk = ?1 ORDER BY folder_id")?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let Some((pk, is_read, is_flagged)) =
            msg.query_row(params![account_id, id], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?))).optional()?
        else {
            continue;
        };
        let folder_ids = folders.query_map([pk], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        out.push(MessageState { id: id.clone(), is_read, is_flagged, folder_ids });
    }
    Ok(out)
}

fn add_folder(conn: &Connection, account_id: &str, id: &str, folder_id: &str) -> Result<()> {
    conn.prepare_cached(
        "INSERT OR IGNORE INTO message_folders (message_pk, account_id, folder_id, received_at, unread)
         SELECT pk, account_id, ?3, received_at, NOT is_read FROM messages WHERE account_id = ?1 AND id = ?2",
    )?
    .execute(params![account_id, id, folder_id])?;
    Ok(())
}

fn remove_folder(conn: &Connection, account_id: &str, id: &str, folder_id: &str) -> Result<()> {
    conn.prepare_cached(
        "DELETE FROM message_folders WHERE folder_id = ?3
         AND message_pk = (SELECT pk FROM messages WHERE account_id = ?1 AND id = ?2)",
    )?
    .execute(params![account_id, id, folder_id])?;
    Ok(())
}

fn set_folders(conn: &Connection, account_id: &str, id: &str, folder_ids: &[String]) -> Result<()> {
    let Some(pk) = message_pk(conn, account_id, id)? else { return Ok(()) };
    conn.prepare_cached("DELETE FROM message_folders WHERE message_pk = ?1")?.execute([pk])?;
    for f in folder_ids {
        add_folder(conn, account_id, id, f)?;
    }
    Ok(())
}

/// The op's effect on the local store. Idempotent, so it can be re-applied
/// on top of fresher server data until the op is confirmed.
pub(super) fn apply_local(conn: &Connection, account_id: &str, op: &MailOp) -> Result<()> {
    let set_flag = |col: &str, ids: &[String], v: bool| -> Result<()> {
        let sql = format!("UPDATE messages SET {col} = ?3 WHERE account_id = ?1 AND id = ?2");
        let mut stmt = conn.prepare_cached(&sql)?;
        for id in ids {
            stmt.execute(params![account_id, id, v])?;
        }
        Ok(())
    };
    match op {
        MailOp::SetRead { ids, read } => set_flag("is_read", ids, *read)?,
        MailOp::SetFlagged { ids, flagged } => {
            set_flag("is_flagged", ids, *flagged)?;
            // Gmail stars are a label too.
            let star = conn
                .prepare_cached("SELECT id FROM folders WHERE account_id = ?1 AND role = 'starred' AND kind = 'label'")?
                .query_row([account_id], |r| r.get::<_, String>(0))
                .optional()?;
            if let Some(star) = star {
                for id in ids {
                    if *flagged {
                        add_folder(conn, account_id, id, &star)?
                    } else {
                        remove_folder(conn, account_id, id, &star)?
                    }
                }
            }
        }
        MailOp::Relabel { ids, add, remove } => {
            for id in ids {
                for f in remove {
                    remove_folder(conn, account_id, id, f)?;
                }
                for f in add {
                    add_folder(conn, account_id, id, f)?;
                }
            }
        }
        MailOp::MoveTo { ids, folder_id } => {
            for id in ids {
                set_folders(conn, account_id, id, std::slice::from_ref(folder_id))?;
            }
        }
        MailOp::Trash { ids } => {
            let trash = role_folder(conn, account_id, FolderRole::Trash)?
                .ok_or_else(|| Error::Other("account has no trash folder".into()))?;
            for id in ids {
                set_folders(conn, account_id, id, std::slice::from_ref(&trash))?;
            }
        }
        MailOp::Untrash { ids } => {
            if let Some(trash) = role_folder(conn, account_id, FolderRole::Trash)? {
                for id in ids {
                    remove_folder(conn, account_id, id, &trash)?;
                }
            }
        }
    }
    Ok(())
}

fn restore(conn: &Connection, account_id: &str, snapshot: &[MessageState]) -> Result<()> {
    let mut stmt =
        conn.prepare_cached("UPDATE messages SET is_read = ?3, is_flagged = ?4 WHERE account_id = ?1 AND id = ?2")?;
    for s in snapshot {
        stmt.execute(params![account_id, s.id, s.is_read, s.is_flagged])?;
        set_folders(conn, account_id, &s.id, &s.folder_ids)?;
    }
    Ok(())
}

/// Re-apply unconfirmed ops after server data landed, so a sync racing an
/// outbox flush never makes an archived message pop back into the inbox.
pub(super) fn reapply_pending(conn: &Connection, account_id: &str) -> Result<()> {
    for e in entries(conn, "WHERE account_id = ?1 AND state = 'pending'", params![account_id])? {
        apply_local(conn, account_id, &e.op)?;
    }
    Ok(())
}

/// Ops that take messages from `current` back to `original` (undo after the
/// first op already reached the server).
pub fn undo_ops(
    labels: bool,
    trash_id: Option<&str>,
    original: &[MessageState],
    current: &[MessageState],
) -> Vec<MailOp> {
    let mut ops = Vec::new();
    for orig in original {
        let Some(cur) = current.iter().find(|c| c.id == orig.id) else { continue };
        let ids = vec![orig.id.clone()];
        if cur.is_read != orig.is_read {
            ops.push(MailOp::SetRead { ids: ids.clone(), read: orig.is_read });
        }
        if cur.is_flagged != orig.is_flagged {
            ops.push(MailOp::SetFlagged { ids: ids.clone(), flagged: orig.is_flagged });
        }
        if cur.folder_ids == orig.folder_ids {
            continue;
        }
        if labels {
            let is_trash = |f: &String| Some(f.as_str()) == trash_id;
            if cur.folder_ids.iter().any(is_trash) && !orig.folder_ids.iter().any(is_trash) {
                ops.push(MailOp::Untrash { ids: ids.clone() });
            }
            let add: Vec<String> =
                orig.folder_ids.iter().filter(|f| !is_trash(f) && !cur.folder_ids.contains(f)).cloned().collect();
            let remove: Vec<String> =
                cur.folder_ids.iter().filter(|f| !is_trash(f) && !orig.folder_ids.contains(f)).cloned().collect();
            if !add.is_empty() || !remove.is_empty() {
                ops.push(MailOp::Relabel { ids, add, remove });
            }
        } else if let Some(folder_id) = orig.folder_ids.first() {
            ops.push(MailOp::MoveTo { ids, folder_id: folder_id.clone() });
        }
    }
    ops
}
