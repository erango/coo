//! Local search: a parsed query, optionally limited to a scope (the folder
//! or account the user is in), plus a count of what the scope leaves out.

use rusqlite::types::Value;

use super::{MSG_COLS, Store, header_from_row};
use crate::Result;
use crate::model::{FolderRole, MessageRow};
use crate::search::{Elsewhere, FolderCount, SearchQuery, SearchScope};
use crate::text;

/// How many folders the "elsewhere" breakdown names.
const ELSEWHERE_FOLDERS: usize = 4;

/// A query's FROM-joins and WHERE conditions (ANDed), with their bound
/// values in order. `None` when it matches nothing (no conditions at all).
struct Where {
    join: String,
    cond: Vec<String>,
    args: Vec<Value>,
}

fn query_where(q: &SearchQuery) -> Option<Where> {
    if q.is_empty() {
        return None;
    }
    let mut w = Where { join: String::new(), cond: Vec::new(), args: Vec::new() };
    let like = |v: &str| Value::Text(format!("%{}%", v.replace(['%', '_'], "")));
    if let Some(fts) = text::fts_query(&q.text.join(" ")) {
        w.join.push_str(" JOIN messages_fts ON messages_fts.rowid = m.pk");
        w.cond.push("messages_fts MATCH ?".into());
        w.args.push(Value::Text(fts));
    }
    for f in &q.from {
        w.cond.push("(m.from_addr LIKE ? OR m.from_name LIKE ?)".into());
        w.args.push(like(f));
        w.args.push(like(f));
    }
    for t in &q.to {
        w.cond.push("m.recipients LIKE ?".into());
        w.args.push(like(t));
    }
    if q.has_attachment {
        w.cond.push("m.has_attachments".into());
    }
    match q.unread {
        Some(true) => w.cond.push("NOT m.is_read".into()),
        Some(false) => w.cond.push("m.is_read".into()),
        None => {}
    }
    if q.flagged {
        w.cond.push("m.is_flagged".into());
    }
    if let Some(a) = &q.account {
        w.cond.push("m.account_id LIKE ?".into());
        w.args.push(like(a));
    }
    if let Some(b) = q.before {
        w.cond.push("m.received_at < ?".into());
        w.args.push(Value::Integer(b));
    }
    if let Some(a) = q.after {
        w.cond.push("m.received_at >= ?".into());
        w.args.push(Value::Integer(a));
    }
    if let Some(r) = q.role {
        w.cond.push(in_role().into());
        w.args.push(Value::Text(r.as_str().into()));
    }
    if let Some(f) = &q.folder {
        w.cond.push(
            "EXISTS (SELECT 1 FROM message_folders mf JOIN folders fo ON fo.account_id = mf.account_id
             AND fo.id = mf.folder_id WHERE mf.message_pk = m.pk AND fo.name = ? COLLATE NOCASE)"
                .into(),
        );
        w.args.push(Value::Text(f.clone()));
    }
    (!w.cond.is_empty()).then_some(w)
}

fn in_role() -> &'static str {
    "EXISTS (SELECT 1 FROM message_folders mf JOIN folders fo ON fo.account_id = mf.account_id
     AND fo.id = mf.folder_id WHERE mf.message_pk = m.pk AND fo.role = ?)"
}

/// The condition (and its values) a message in `scope` meets; `None` for
/// all mail.
fn scope_where(scope: &SearchScope) -> Option<(String, Vec<Value>)> {
    match scope {
        SearchScope::All => None,
        SearchScope::Account(a) => Some(("m.account_id = ?".into(), vec![Value::Text(a.clone())])),
        SearchScope::Role(r) => Some((in_role().into(), vec![Value::Text(r.as_str().into())])),
        SearchScope::Folder { account_id, folder_id } => Some((
            "EXISTS (SELECT 1 FROM message_folders mf WHERE mf.message_pk = m.pk
             AND mf.account_id = ? AND mf.folder_id = ?)"
                .into(),
            vec![Value::Text(account_id.clone()), Value::Text(folder_id.clone())],
        )),
    }
}

impl Store {
    /// Run a parsed search over the whole local store, newest first.
    pub fn search_query(&self, q: &SearchQuery, limit: u32) -> Result<Vec<MessageRow>> {
        self.search_scoped(q, &SearchScope::All, limit)
    }

    /// Run a parsed search within `scope`, newest first.
    pub fn search_scoped(&self, q: &SearchQuery, scope: &SearchScope, limit: u32) -> Result<Vec<MessageRow>> {
        let Some(mut w) = query_where(q) else { return Ok(Vec::new()) };
        if let Some((cond, args)) = scope_where(scope) {
            w.cond.push(cond);
            w.args.extend(args);
        }
        w.args.push(Value::Integer(i64::from(limit)));
        let sql = format!(
            "SELECT {MSG_COLS} FROM messages m{} WHERE {} ORDER BY m.received_at DESC LIMIT ?",
            w.join,
            w.cond.join(" AND ")
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(w.args), |r| {
            Ok(MessageRow { account_id: r.get(0)?, header: header_from_row(r)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Matches of `q` outside `scope`, and the folders they're in (grouped
    /// across accounts by role, else name). Gmail's category tabs and its
    /// Starred/Important labels aren't places mail lives, so they're left
    /// out of the breakdown. Empty for all mail.
    pub fn search_elsewhere(&self, q: &SearchQuery, scope: &SearchScope) -> Result<Elsewhere> {
        let (Some(mut w), Some((cond, args))) = (query_where(q), scope_where(scope)) else {
            return Ok(Elsewhere::default());
        };
        w.cond.push(format!("NOT {cond}"));
        w.args.extend(args);
        let conds = w.cond.join(" AND ");
        let total: u32 = self
            .conn
            .prepare_cached(&format!("SELECT COUNT(*) FROM messages m{} WHERE {conds}", w.join))?
            .query_row(rusqlite::params_from_iter(w.args.iter()), |r| r.get(0))?;
        if total == 0 {
            return Ok(Elsewhere::default());
        }
        let sql = format!(
            "SELECT xo.role, MIN(xo.name), COUNT(DISTINCT m.pk) AS n FROM messages m{}
             JOIN message_folders xf ON xf.message_pk = m.pk
             JOIN folders xo ON xo.account_id = xf.account_id AND xo.id = xf.folder_id
             WHERE {conds} AND xo.id NOT LIKE 'CATEGORY\\_%' ESCAPE '\\'
               AND (xo.role IS NULL OR xo.role NOT IN ('starred', 'important'))
             GROUP BY COALESCE(xo.role, LOWER(xo.name))
             ORDER BY n DESC, MIN(xo.name) LIMIT {ELSEWHERE_FOLDERS}",
            w.join
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(w.args.iter()), |r| {
            let role: Option<String> = r.get(0)?;
            Ok(FolderCount { role: role.as_deref().and_then(FolderRole::parse), name: r.get(1)?, count: r.get(2)? })
        })?;
        Ok(Elsewhere { total, folders: rows.collect::<rusqlite::Result<_>>()? })
    }
}
