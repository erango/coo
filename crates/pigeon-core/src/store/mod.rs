//! Local SQLite store: the single source of truth the UI renders from.
//!
//! `Store` is synchronous and owns one connection. The engine keeps it on a
//! dedicated thread (or behind a mutex in the CLI); never hold it across an
//! `.await`.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::model::*;
use crate::{Error, Result, now_ms, text};

mod contacts;
mod outbox;
pub use outbox::undo_ops;

const MIGRATIONS: &[&str] = &[
    include_str!("001_init.sql"),
    include_str!("002_bodies.sql"),
    include_str!("003_outbox.sql"),
    include_str!("004_drafts.sql"),
    include_str!("005_invites.sql"),
];

/// Which folder a list query reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderSel {
    /// Every folder with this role; across all accounts unless one is given
    /// (unified inbox).
    Role(FolderRole),
    /// One concrete folder; requires `account_id`.
    Id(String),
}

#[derive(Debug, Clone)]
pub struct ListQuery {
    pub account_id: Option<String>,
    pub folder: FolderSel,
    pub limit: u32,
    /// Only items received strictly before this (pagination).
    pub before: Option<i64>,
}

impl ListQuery {
    pub fn unified(role: FolderRole, limit: u32) -> Self {
        ListQuery { account_id: None, folder: FolderSel::Role(role), limit, before: None }
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA busy_timeout = 5000;",
        )?;
        migrate(&mut conn)?;
        Ok(Store { conn })
    }

    // ---- accounts ----

    pub fn upsert_account(&self, a: &Account) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET email = excluded.email, display_name = excluded.display_name",
            params![a.id, a.provider.as_str(), a.email, a.display_name, now_ms()],
        )?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt =
            self.conn.prepare_cached("SELECT id, provider, email, display_name FROM accounts ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                provider: ProviderKind::parse(&r.get::<_, String>(1)?).unwrap_or(ProviderKind::Microsoft),
                email: r.get(2)?,
                display_name: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes the account and, via cascades, everything synced for it.
    pub fn delete_account(&self, account_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
        Ok(())
    }

    // ---- folders ----

    pub fn replace_folders(&mut self, account_id: &str, folders: &[Folder]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM folders WHERE account_id = ?1", [account_id])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO folders (account_id, id, name, parent_id, role, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for f in folders {
                ins.execute(params![
                    account_id,
                    f.id,
                    f.name,
                    f.parent_id,
                    f.role.map(FolderRole::as_str),
                    f.kind.as_str()
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Folders with locally computed counts; role folders first.
    pub fn folders(&self, account_id: &str) -> Result<Vec<FolderInfo>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.id, f.name, f.parent_id, f.role, f.kind, COUNT(m.pk), COALESCE(SUM(m.is_read = 0), 0)
             FROM folders f
             LEFT JOIN message_folders mf ON mf.account_id = f.account_id AND mf.folder_id = f.id
             LEFT JOIN messages m ON m.pk = mf.message_pk
             WHERE f.account_id = ?1
             GROUP BY f.id
             ORDER BY f.role IS NULL, f.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(FolderInfo {
                folder: Folder {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    parent_id: r.get(2)?,
                    role: r.get::<_, Option<String>>(3)?.as_deref().and_then(FolderRole::parse),
                    kind: FolderKind::parse(&r.get::<_, String>(4)?),
                },
                total: r.get(5)?,
                unread: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- mail ----

    /// Apply one page of provider changes and advance the scope's cursor in a
    /// single transaction, so an interrupted sync resumes exactly.
    pub fn apply_mail(&mut self, account_id: &str, scope: &str, changes: &[MailChange], cursor: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        apply_changes(&tx, account_id, changes)?;
        set_cursor(&tx, account_id, scope, cursor)?;
        tx.commit()?;
        Ok(())
    }

    /// Store messages found outside normal sync (server search results).
    pub fn apply_found(&mut self, account_id: &str, changes: &[MailChange]) -> Result<()> {
        let tx = self.conn.transaction()?;
        apply_changes(&tx, account_id, changes)?;
        tx.commit()?;
        Ok(())
    }

    /// Run a parsed search over the local store, newest first.
    pub fn search_query(&self, q: &crate::search::SearchQuery, limit: u32) -> Result<Vec<MessageRow>> {
        use rusqlite::types::Value;
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut join = String::new();
        let mut cond: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        let like = |v: &str| Value::Text(format!("%{}%", v.replace(['%', '_'], "")));
        if let Some(fts) = text::fts_query(&q.text.join(" ")) {
            join.push_str(" JOIN messages_fts ON messages_fts.rowid = m.pk");
            cond.push("messages_fts MATCH ?".into());
            args.push(Value::Text(fts));
        }
        for f in &q.from {
            cond.push("(m.from_addr LIKE ? OR m.from_name LIKE ?)".into());
            args.push(like(f));
            args.push(like(f));
        }
        for t in &q.to {
            cond.push("m.recipients LIKE ?".into());
            args.push(like(t));
        }
        if q.has_attachment {
            cond.push("m.has_attachments".into());
        }
        match q.unread {
            Some(true) => cond.push("NOT m.is_read".into()),
            Some(false) => cond.push("m.is_read".into()),
            None => {}
        }
        if q.flagged {
            cond.push("m.is_flagged".into());
        }
        if let Some(a) = &q.account {
            cond.push("m.account_id LIKE ?".into());
            args.push(like(a));
        }
        if let Some(b) = q.before {
            cond.push("m.received_at < ?".into());
            args.push(Value::Integer(b));
        }
        if let Some(a) = q.after {
            cond.push("m.received_at >= ?".into());
            args.push(Value::Integer(a));
        }
        if let Some(r) = q.role {
            cond.push(
                "EXISTS (SELECT 1 FROM message_folders mf JOIN folders fo ON fo.account_id = mf.account_id
                 AND fo.id = mf.folder_id WHERE mf.message_pk = m.pk AND fo.role = ?)"
                    .into(),
            );
            args.push(Value::Text(r.as_str().into()));
        }
        if cond.is_empty() {
            return Ok(Vec::new());
        }
        args.push(Value::Integer(i64::from(limit)));
        let sql = format!(
            "SELECT {MSG_COLS} FROM messages m{join} WHERE {} ORDER BY m.received_at DESC LIMIT ?",
            cond.join(" AND ")
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
            Ok(MessageRow { account_id: r.get(0)?, header: header_from_row(r)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_messages(&self, q: &ListQuery) -> Result<Vec<MessageRow>> {
        let (join, cond, key) = folder_filter(q)?;
        let sql = format!(
            "SELECT {MSG_COLS} FROM message_folders mf JOIN messages m ON m.pk = mf.message_pk {join}
             WHERE {cond} AND mf.received_at < ?3
             ORDER BY mf.received_at DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![key, q.account_id, q.before.unwrap_or(i64::MAX), q.limit], |r| {
            Ok(MessageRow { account_id: r.get(0)?, header: header_from_row(r)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_threads(&self, q: &ListQuery) -> Result<Vec<ThreadRow>> {
        let (join, cond, key) = folder_filter(q)?;
        // SQLite fills bare columns from the row that produced MAX().
        let sql = format!(
            "SELECT {MSG_COLS}, MAX(m.received_at) AS latest, SUM(m.is_read = 0),
                    group_concat(COALESCE(m.from_name, m.from_addr), char(31)),
                    (SELECT COUNT(*) FROM messages t WHERE t.account_id = m.account_id AND t.thread_id = m.thread_id)
             FROM message_folders mf JOIN messages m ON m.pk = mf.message_pk {join}
             WHERE {cond} AND mf.received_at < ?3
             GROUP BY m.account_id, m.thread_id
             ORDER BY latest DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![key, q.account_id, q.before.unwrap_or(i64::MAX), q.limit], |r| {
            let latest = header_from_row(r)?;
            let mut participants: Vec<String> = Vec::new();
            for p in r.get::<_, Option<String>>(MSG_COL_COUNT + 2)?.unwrap_or_default().split('\u{1f}') {
                if !p.is_empty() && !participants.iter().any(|x| x == p) {
                    participants.push(p.to_string());
                }
            }
            Ok(ThreadRow {
                account_id: r.get(0)?,
                thread_id: latest.thread_id.clone(),
                latest,
                unread_count: r.get(MSG_COL_COUNT + 1)?,
                participants,
                message_count: r.get(MSG_COL_COUNT + 3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// All messages of a thread, oldest first.
    pub fn thread_messages(&self, account_id: &str, thread_id: &str) -> Result<Vec<MessageHeader>> {
        let sql = format!(
            "SELECT {MSG_COLS} FROM messages m WHERE m.account_id = ?1 AND m.thread_id = ?2 ORDER BY m.received_at"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![account_id, thread_id], header_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Local full-text search over subject, sender, recipients and snippet.
    pub fn search(&self, input: &str, limit: u32) -> Result<Vec<MessageRow>> {
        let Some(query) = text::fts_query(input) else { return Ok(Vec::new()) };
        let sql = format!(
            "SELECT {MSG_COLS} FROM messages_fts JOIN messages m ON m.pk = messages_fts.rowid
             WHERE messages_fts MATCH ?1 ORDER BY m.received_at DESC LIMIT ?2"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![query, limit], |r| {
            Ok(MessageRow { account_id: r.get(0)?, header: header_from_row(r)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- bodies & attachments ----

    /// Store a fetched body and its attachment list. Returns false if the
    /// message is no longer in the store.
    pub fn save_body(&mut self, account_id: &str, message_id: &str, body: &StoredBody) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let Some(pk) = message_pk(&tx, account_id, message_id)? else { return Ok(false) };
        tx.prepare_cached(
            "INSERT OR REPLACE INTO bodies (message_pk, html, text, fetched_at, has_remote, in_reply_to, references_json,
                 invite_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?
        .execute(params![
            pk,
            body.html,
            body.text,
            body.fetched_at,
            body.has_remote_content,
            body.in_reply_to,
            serde_json::to_string(&body.references)?,
            body.invite.as_ref().map(serde_json::to_string).transpose()?
        ])?;
        tx.prepare_cached("DELETE FROM attachments WHERE message_pk = ?1")?.execute([pk])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO attachments (message_pk, id, name, content_type, size, content_id, is_inline, local_path,
                     position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for (position, a) in body.attachments.iter().enumerate() {
                let m = &a.meta;
                ins.execute(params![
                    pk,
                    m.id,
                    m.name,
                    m.content_type,
                    m.size.map(|n| n as i64),
                    m.content_id,
                    m.is_inline,
                    a.local_path,
                    position as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn body(&self, account_id: &str, message_id: &str) -> Result<Option<StoredBody>> {
        let Some(pk) = message_pk(&self.conn, account_id, message_id)? else { return Ok(None) };
        let body = self
            .conn
            .prepare_cached(
                "SELECT html, text, has_remote, in_reply_to, references_json, fetched_at, invite_json
                 FROM bodies WHERE message_pk = ?1",
            )?
            .query_row([pk], |r| {
                Ok(StoredBody {
                    html: r.get(0)?,
                    text: r.get(1)?,
                    has_remote_content: r.get(2)?,
                    in_reply_to: r.get(3)?,
                    references: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                    fetched_at: r.get(5)?,
                    attachments: Vec::new(),
                    invite: r.get::<_, Option<String>>(6)?.and_then(|j| serde_json::from_str(&j).ok()),
                })
            })
            .optional()?;
        let Some(mut body) = body else { return Ok(None) };
        body.attachments = self.attachments_of(pk)?;
        Ok(Some(body))
    }

    pub fn attachments(&self, account_id: &str, message_id: &str) -> Result<Vec<StoredAttachment>> {
        match message_pk(&self.conn, account_id, message_id)? {
            Some(pk) => self.attachments_of(pk),
            None => Ok(Vec::new()),
        }
    }

    fn attachments_of(&self, pk: i64) -> Result<Vec<StoredAttachment>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, content_type, size, content_id, is_inline, local_path FROM attachments
             WHERE message_pk = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([pk], |r| {
            Ok(StoredAttachment {
                meta: AttachmentMeta {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    content_type: r.get(2)?,
                    size: r.get::<_, Option<i64>>(3)?.map(|n| n as u64),
                    content_id: r.get(4)?,
                    is_inline: r.get(5)?,
                },
                local_path: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_attachment_path(
        &self,
        account_id: &str,
        message_id: &str,
        attachment_id: &str,
        path: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE attachments SET local_path = ?4
             WHERE id = ?3 AND message_pk = (SELECT pk FROM messages WHERE account_id = ?1 AND id = ?2)",
            params![account_id, message_id, attachment_id, path],
        )?;
        Ok(())
    }

    // ---- drafts ----

    pub fn save_draft(&self, draft: &crate::compose::Draft) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO drafts (id, account_id, json, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![draft.id, draft.from.account_id, serde_json::to_string(draft)?, draft.updated_at],
        )?;
        Ok(())
    }

    pub fn draft(&self, id: &str) -> Result<Option<crate::compose::Draft>> {
        let json: Option<String> = self
            .conn
            .prepare_cached("SELECT json FROM drafts WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    pub fn drafts(&self) -> Result<Vec<crate::compose::Draft>> {
        let mut stmt = self.conn.prepare_cached("SELECT json FROM drafts ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?)?);
        }
        Ok(out)
    }

    pub fn delete_draft(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM drafts WHERE id = ?1", [id])?;
        Ok(())
    }

    /// One message with its folder membership.
    pub fn message(&self, account_id: &str, message_id: &str) -> Result<Option<MessageHeader>> {
        let sql = format!("SELECT {MSG_COLS}, m.pk FROM messages m WHERE m.account_id = ?1 AND m.id = ?2");
        let row = self
            .conn
            .prepare_cached(&sql)?
            .query_row(params![account_id, message_id], |r| Ok((header_from_row(r)?, r.get::<_, i64>(MSG_COL_COUNT)?)))
            .optional()?;
        let Some((mut h, pk)) = row else { return Ok(None) };
        let mut stmt = self.conn.prepare_cached("SELECT folder_id FROM message_folders WHERE message_pk = ?1")?;
        h.folder_ids = stmt.query_map([pk], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(Some(h))
    }

    // ---- sync cursors ----

    pub fn cursor(&self, account_id: &str, scope: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT cursor FROM sync_state WHERE account_id = ?1 AND scope = ?2")?
            .query_row(params![account_id, scope], |r| r.get(0))
            .optional()?)
    }

    pub fn clear_cursor(&self, account_id: &str, scope: &str) -> Result<()> {
        self.conn.execute("DELETE FROM sync_state WHERE account_id = ?1 AND scope = ?2", params![account_id, scope])?;
        Ok(())
    }

    // ---- calendar ----

    pub fn replace_calendars(&mut self, account_id: &str, calendars: &[Calendar]) -> Result<()> {
        let tx = self.conn.transaction()?;
        // Keep the user's visibility choice across refreshes.
        let ids: Vec<&str> = calendars.iter().map(|c| c.id.as_str()).collect();
        tx.execute(
            "DELETE FROM calendars WHERE account_id = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            params![account_id, serde_json::to_string(&ids)?],
        )?;
        tx.execute(
            "DELETE FROM events WHERE account_id = ?1 AND calendar_id NOT IN (SELECT value FROM json_each(?2))",
            params![account_id, serde_json::to_string(&ids)?],
        )?;
        tx.execute(
            "DELETE FROM sync_state WHERE account_id = ?1 AND scope LIKE 'cal:%'
             AND substr(scope, 5) NOT IN (SELECT value FROM json_each(?2))",
            params![account_id, serde_json::to_string(&ids)?],
        )?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO calendars (account_id, id, name, color, is_primary, can_edit) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (account_id, id) DO UPDATE SET name = excluded.name, color = excluded.color,
                     is_primary = excluded.is_primary, can_edit = excluded.can_edit",
            )?;
            for c in calendars {
                ins.execute(params![account_id, c.id, c.name, c.color, c.is_primary, c.can_edit])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn calendars(&self, account_id: &str) -> Result<Vec<Calendar>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, color, is_primary, can_edit FROM calendars WHERE account_id = ?1
             ORDER BY is_primary DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Calendar {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                is_primary: r.get(3)?,
                can_edit: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replace every event of one calendar overlapping `[from, to)` with `events`.
    pub fn replace_events(
        &mut self,
        account_id: &str,
        calendar_id: &str,
        from: i64,
        to: i64,
        events: &[Event],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND start_at < ?4 AND end_at > ?3",
            params![account_id, calendar_id, from, to],
        )?;
        for e in events {
            insert_event(&tx, account_id, calendar_id, e)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply one page of calendar changes and (on the last page) persist the
    /// scope's cursor, atomically. `reset` drops the calendar's events first
    /// (start of a full snapshot).
    pub fn apply_events(
        &mut self,
        account_id: &str,
        calendar_id: &str,
        scope: &str,
        changes: &[CalendarChange],
        reset: bool,
        cursor: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        if reset {
            tx.execute(
                "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2",
                params![account_id, calendar_id],
            )?;
        }
        for change in changes {
            match change {
                CalendarChange::Upsert(e) => insert_event(&tx, account_id, calendar_id, e)?,
                CalendarChange::Delete { id } => {
                    tx.prepare_cached(
                        "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND (id = ?3 OR series_id = ?3)",
                    )?
                    .execute(params![account_id, calendar_id, id])?;
                }
            }
        }
        if let Some(cursor) = cursor {
            set_cursor(&tx, account_id, scope, cursor)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Store one event as returned by the server after a write.
    pub fn upsert_event(&self, account_id: &str, event: &Event) -> Result<()> {
        insert_event(&self.conn, account_id, &event.calendar_id, event)
    }

    pub fn delete_event(&self, account_id: &str, calendar_id: &str, event_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM events WHERE account_id = ?1 AND calendar_id = ?2 AND id = ?3",
            params![account_id, calendar_id, event_id],
        )?;
        Ok(())
    }

    pub fn set_my_response(&self, account_id: &str, calendar_id: &str, event_id: &str, r: Response) -> Result<()> {
        self.conn.execute(
            "UPDATE events SET my_response = ?4 WHERE account_id = ?1 AND calendar_id = ?2 AND id = ?3",
            params![account_id, calendar_id, event_id, r.as_str()],
        )?;
        Ok(())
    }

    /// One stored event, cancelled or not.
    pub fn event(&self, account_id: &str, calendar_id: &str, event_id: &str) -> Result<Option<Event>> {
        let sql =
            format!("SELECT {EVENT_COLS} FROM events e WHERE e.account_id = ?1 AND e.calendar_id = ?2 AND e.id = ?3");
        Ok(self
            .conn
            .prepare_cached(&sql)?
            .query_row(params![account_id, calendar_id, event_id], |r| Ok(event_from_row(r)?.event))
            .optional()?)
    }

    /// The local event an invite refers to: by provider event id (or its
    /// series) in that account, else by iCalendar UID in that account, else
    /// by UID in any account. Among a series' instances, the one closest to
    /// the invite's start wins.
    pub fn invite_event(&self, account_id: &str, invite: &Invite) -> Result<Option<EventRow>> {
        let near = invite.start_at;
        let pick = |sql: &str, args: &[&dyn rusqlite::ToSql]| -> Result<Option<EventRow>> {
            let sql = format!("SELECT {EVENT_COLS} FROM events e WHERE {sql} ORDER BY ABS(e.start_at - ?9) LIMIT 1");
            let mut stmt = self.conn.prepare_cached(&sql)?;
            let mut all: Vec<&dyn rusqlite::ToSql> = args.to_vec();
            while all.len() < 8 {
                all.push(&rusqlite::types::Null);
            }
            all.push(&near);
            Ok(stmt.query_row(all.as_slice(), event_from_row).optional()?)
        };
        if let Some(id) = &invite.event_id
            && let Some(e) = pick("e.account_id = ?1 AND (e.id = ?2 OR e.series_id = ?2)", &[&account_id, id])?
        {
            return Ok(Some(e));
        }
        let Some(uid) = &invite.uid else { return Ok(None) };
        if let Some(e) = pick("e.account_id = ?1 AND e.ical_uid = ?2", &[&account_id, uid])? {
            return Ok(Some(e));
        }
        pick("e.ical_uid = ?2", &[&account_id, uid])
    }

    /// Non-cancelled events overlapping `[from, to)` on visible calendars.
    pub fn events_between(&self, from: i64, to: i64) -> Result<Vec<EventRow>> {
        let sql = format!(
            "SELECT {EVENT_COLS}
             FROM events e JOIN calendars c ON c.account_id = e.account_id AND c.id = e.calendar_id
             WHERE c.visible AND NOT e.cancelled AND e.start_at < ?2 AND e.end_at > ?1
             ORDER BY e.start_at, e.end_at"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![from, to], event_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if current > MIGRATIONS.len() {
        return Err(Error::Other(format!("database schema v{current} is newer than this build")));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

const EVENT_COLS: &str = "e.account_id, e.calendar_id, e.id, e.series_id, e.title, e.start_at, e.end_at, e.all_day,
    e.timezone, e.location, e.organizer_name, e.organizer_addr, e.attendees_json, e.my_response, e.online_url,
    e.cancelled, e.updated_at, e.ical_uid";

fn event_from_row(r: &Row) -> rusqlite::Result<EventRow> {
    let organizer = r
        .get::<_, Option<String>>(11)?
        .map(|email| Address::new(r.get::<_, Option<String>>(10).ok().flatten().as_deref(), &email));
    Ok(EventRow {
        account_id: r.get(0)?,
        event: Event {
            calendar_id: r.get(1)?,
            id: r.get(2)?,
            series_id: r.get(3)?,
            title: r.get(4)?,
            start_at: r.get(5)?,
            end_at: r.get(6)?,
            all_day: r.get(7)?,
            timezone: r.get(8)?,
            location: r.get(9)?,
            organizer,
            attendees: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
            my_response: Response::parse(&r.get::<_, String>(13)?),
            online_url: r.get(14)?,
            cancelled: r.get(15)?,
            updated_at: r.get(16)?,
            ical_uid: r.get(17)?,
        },
    })
}

fn insert_event(conn: &Connection, account_id: &str, calendar_id: &str, e: &Event) -> Result<()> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO events (account_id, calendar_id, id, series_id, title, start_at, end_at, all_day,
             timezone, location, organizer_name, organizer_addr, attendees_json, my_response, online_url,
             cancelled, updated_at, ical_uid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
    )?
    .execute(params![
        account_id,
        calendar_id,
        e.id,
        e.series_id,
        e.title,
        e.start_at,
        e.end_at,
        e.all_day,
        e.timezone,
        e.location,
        e.organizer.as_ref().and_then(|o| o.name.as_deref()),
        e.organizer.as_ref().map(|o| o.email.as_str()),
        serde_json::to_string(&e.attendees)?,
        e.my_response.as_str(),
        e.online_url,
        e.cancelled,
        e.updated_at,
        e.ical_uid,
    ])?;
    Ok(())
}

fn message_pk(conn: &Connection, account_id: &str, message_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .prepare_cached("SELECT pk FROM messages WHERE account_id = ?1 AND id = ?2")?
        .query_row(params![account_id, message_id], |r| r.get(0))
        .optional()?)
}

fn apply_changes(tx: &Transaction, account_id: &str, changes: &[MailChange]) -> Result<()> {
    for change in changes {
        match change {
            MailChange::Upsert(h) => upsert_message(tx, account_id, h)?,
            MailChange::Unlink { id, folder_id } => {
                tx.prepare_cached(
                    "DELETE FROM message_folders WHERE folder_id = ?2
                     AND message_pk = (SELECT pk FROM messages WHERE account_id = ?1 AND id = ?3)",
                )?
                .execute(params![account_id, folder_id, id])?;
                tx.prepare_cached(
                    "DELETE FROM messages WHERE account_id = ?1 AND id = ?2
                     AND NOT EXISTS (SELECT 1 FROM message_folders WHERE message_pk = messages.pk)",
                )?
                .execute(params![account_id, id])?;
            }
            MailChange::Delete { id } => {
                tx.prepare_cached("DELETE FROM messages WHERE account_id = ?1 AND id = ?2")?
                    .execute(params![account_id, id])?;
            }
        }
    }
    outbox::reapply_pending(tx, account_id)
}

fn set_cursor(tx: &Transaction, account_id: &str, scope: &str, cursor: &str) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO sync_state (account_id, scope, cursor, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (account_id, scope) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at",
    )?
    .execute(params![account_id, scope, cursor, now_ms()])?;
    Ok(())
}

fn upsert_message(tx: &Transaction, account_id: &str, h: &MessageHeader) -> Result<()> {
    let recipients =
        h.to.iter()
            .chain(&h.cc)
            .map(|a| match &a.name {
                Some(n) => format!("{n} {}", a.email),
                None => a.email.clone(),
            })
            .collect::<Vec<_>>()
            .join(" ");
    let pk: i64 = tx
        .prepare_cached(
            "INSERT INTO messages (account_id, id, thread_id, internet_message_id, subject, from_name, from_addr,
                 to_json, cc_json, recipients, snippet, received_at, is_read, is_flagged, has_attachments, is_draft)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT (account_id, id) DO UPDATE SET
                 thread_id = excluded.thread_id, internet_message_id = excluded.internet_message_id,
                 subject = excluded.subject, from_name = excluded.from_name, from_addr = excluded.from_addr,
                 to_json = excluded.to_json, cc_json = excluded.cc_json, recipients = excluded.recipients,
                 snippet = excluded.snippet, received_at = excluded.received_at, is_read = excluded.is_read,
                 is_flagged = excluded.is_flagged, has_attachments = excluded.has_attachments,
                 is_draft = excluded.is_draft
             RETURNING pk",
        )?
        .query_row(
            params![
                account_id,
                h.id,
                h.thread_id,
                h.internet_message_id,
                h.subject,
                h.from.as_ref().and_then(|a| a.name.as_deref()),
                h.from.as_ref().map(|a| a.email.as_str()),
                serde_json::to_string(&h.to)?,
                serde_json::to_string(&h.cc)?,
                recipients,
                h.snippet,
                h.received_at,
                h.is_read,
                h.is_flagged,
                h.has_attachments,
                h.is_draft,
            ],
            |r| r.get(0),
        )?;
    tx.prepare_cached("DELETE FROM message_folders WHERE message_pk = ?1")?.execute([pk])?;
    let mut ins = tx.prepare_cached(
        "INSERT OR IGNORE INTO message_folders (message_pk, account_id, folder_id, received_at) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for folder_id in &h.folder_ids {
        ins.execute(params![pk, account_id, folder_id, h.received_at])?;
    }
    Ok(())
}

const MSG_COLS: &str = "m.account_id, m.id, m.thread_id, m.internet_message_id, m.subject, m.from_name, m.from_addr,
    m.to_json, m.cc_json, m.snippet, m.received_at, m.is_read, m.is_flagged, m.has_attachments, m.is_draft";
const MSG_COL_COUNT: usize = 15;

fn header_from_row(r: &Row) -> rusqlite::Result<MessageHeader> {
    let from = r
        .get::<_, Option<String>>(6)?
        .map(|email| Address::new(r.get::<_, Option<String>>(5).ok().flatten().as_deref(), &email));
    Ok(MessageHeader {
        id: r.get(1)?,
        thread_id: r.get(2)?,
        internet_message_id: r.get(3)?,
        subject: r.get(4)?,
        from,
        to: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
        cc: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
        snippet: r.get(9)?,
        received_at: r.get(10)?,
        is_read: r.get(11)?,
        is_flagged: r.get(12)?,
        has_attachments: r.get(13)?,
        is_draft: r.get(14)?,
        folder_ids: Vec::new(),
    })
}

/// (extra join, WHERE condition, value bound to ?1). ?2 is the account filter.
fn folder_filter(q: &ListQuery) -> Result<(&'static str, &'static str, String)> {
    Ok(match &q.folder {
        FolderSel::Role(role) => (
            "JOIN folders f ON f.account_id = mf.account_id AND f.id = mf.folder_id",
            "f.role = ?1 AND (?2 IS NULL OR mf.account_id = ?2)",
            role.as_str().to_string(),
        ),
        FolderSel::Id(id) => {
            if q.account_id.is_none() {
                return Err(Error::Other("listing a folder by id needs an account".into()));
            }
            ("", "mf.folder_id = ?1 AND mf.account_id = ?2", id.clone())
        }
    })
}

#[cfg(test)]
mod tests;
