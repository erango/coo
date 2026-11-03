//! `coo watch`: wait for new mail that matches, printing one line per
//! message, for agents that monitor mail in the background (Claude Code
//! runs it as a background task and wakes when it prints).
//!
//! The Coo app does the syncing; a watch only looks at what lands in the
//! local store, so it never talks to a mail server itself. Each watch
//! registers itself under `<data dir>/watches/<id>.json` so `coo watches`
//! lists the running ones and `coo unwatch` (or the MCP `stop_watch` tool)
//! stops them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pigeon_core::model::MessageRow;
use pigeon_core::{Error, Result};
use pigeon_engine::Pigeon;
use serde::{Deserialize, Serialize};

use crate::out::MessageOut;

/// How often the store is checked for new messages.
const POLL: Duration = Duration::from_secs(2);
/// How often a watch checks that the Coo app (which syncs) is running.
const APP_CHECK: Duration = Duration::from_secs(30);
/// Messages received this long before the watch started still count (a
/// sync can lag a little); older ones that arrive later (a label filled in,
/// a server search) are history, not new mail.
const GRACE_MS: i64 = 10 * 60 * 1000;

/// What a watch waits for. Each list matches any of its values; the lists
/// that are given must all match. No filters: any new mail.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Filters {
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
    /// Also report what you send yourself (off: only incoming mail).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub include_mine: bool,
}

impl Filters {
    /// Whether `m` is new mail this watch reports. `mine`: the user's own
    /// addresses, lowercased.
    pub fn matches(&self, m: &MessageRow, mine: &[String], started_at: i64) -> bool {
        let h = &m.header;
        let from = h.from.as_ref().map(|a| a.email.to_lowercase()).unwrap_or_default();
        let any = |list: &[String], f: &dyn Fn(&str) -> bool| list.is_empty() || list.iter().any(|v| f(v));
        !h.is_draft
            && h.received_at >= started_at - GRACE_MS
            && (self.include_mine || !mine.contains(&from))
            && any(&self.threads, &|t| h.thread_id == t)
            && any(&self.from, &|f| from.contains(&f.to_lowercase()))
            && any(&self.subjects, &|s| h.subject.to_lowercase().contains(&s.to_lowercase()))
            && any(&self.accounts, &|a| m.account_id.to_lowercase().contains(&a.to_lowercase()))
    }
}

/// A running watch, as registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchOut {
    pub id: String,
    pub pid: u32,
    pub started_at: String,
    pub filters: Filters,
}

fn dir(app: &Pigeon) -> PathBuf {
    app.data_dir.join("watches")
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// The running watches; registrations whose process is gone are cleared.
pub fn list(app: &Pigeon) -> Vec<WatchOut> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(app)) else { return out };
    for e in entries.flatten() {
        let path = e.path();
        let Some(w) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<WatchOut>(&b).ok()) else {
            continue;
        };
        if alive(w.pid) {
            out.push(w);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    out.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    out
}

/// Stop watch `id` (or every watch with `None`); returns the ones stopped.
pub fn stop(app: &Pigeon, id: Option<&str>) -> Result<Vec<WatchOut>> {
    let running = list(app);
    let picked: Vec<WatchOut> = running.into_iter().filter(|w| id.is_none_or(|id| w.id == id)).collect();
    if let (Some(id), true) = (id, picked.is_empty()) {
        return Err(Error::Other(format!("no running watch {id} (see `coo watches`)")));
    }
    for w in &picked {
        // SAFETY: plain signal to a process we registered; it cleans up on TERM.
        unsafe { libc::kill(w.pid as i32, libc::SIGTERM) };
        let _ = std::fs::remove_file(dir(app).join(format!("{}.json", w.id)));
    }
    Ok(picked)
}

/// Removes the registration when the watch ends, however it ends.
struct Registration(PathBuf);

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn register(app: &Pigeon, w: &WatchOut) -> Result<Registration> {
    let dir = dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| Error::Other(format!("can't register the watch: {e}")))?;
    let path = dir.join(format!("{}.json", w.id));
    std::fs::write(&path, serde_json::to_vec(w)?)
        .map_err(|e| Error::Other(format!("can't register the watch: {e}")))?;
    Ok(Registration(path))
}

/// The user's own addresses (accounts and their main addresses), lowercased.
fn my_addresses(app: &Pigeon) -> Vec<String> {
    let mut mine = Vec::new();
    for a in app.accounts().unwrap_or_default() {
        if let Ok(Some(main)) = app.store().main_address(&a.id) {
            mine.push(main.to_lowercase());
        }
        mine.push(a.email.to_lowercase());
    }
    mine
}

fn app_running(data_dir: &Path) -> bool {
    let client = pigeon_core::broker::Client::new(data_dir, false);
    client.call(&pigeon_core::broker::Request::Ping).is_ok()
}

/// One line on stdout, flushed (the agent wakes on it). False when nobody
/// reads anymore.
fn say(json: bool, value: &serde_json::Value, human: &str) -> bool {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let line = if json { value.to_string() } else { human.to_string() };
    writeln!(out, "{line}").and_then(|_| out.flush()).is_ok()
}

/// Watch until stopped (or the first match with `once`).
pub async fn run(app: &Pigeon, json: bool, filters: Filters, once: bool) -> Result<()> {
    let id = pigeon_core::auth::random_token(4).to_lowercase();
    let started_at = pigeon_core::now_ms();
    let w = WatchOut {
        id: id.clone(),
        pid: std::process::id(),
        started_at: crate::out::iso(started_at),
        filters: filters.clone(),
    };
    let _registered = register(app, &w)?;
    let mut mark = app.store().last_message_pk()?;
    let mine = my_addresses(app);
    let started = serde_json::json!({ "event": "watching", "id": id, "filters": filters,
        "stop": format!("coo unwatch {id}") });
    if !say(json, &started, &format!("Watching for new mail (watch {id}; stop with `coo unwatch {id}`)")) {
        return Ok(());
    }
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut poll = tokio::time::interval(POLL);
    let mut check = tokio::time::interval(APP_CHECK);
    let mut app_up: Option<bool> = None;
    loop {
        tokio::select! {
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
            // Only an app that shares sign-ins answers pings (not dev file stores).
            _ = check.tick(), if app.uses_broker() => {
                let data_dir = app.data_dir.clone();
                let up = tokio::task::spawn_blocking(move || app_running(&data_dir)).await.unwrap_or(false);
                if app_up != Some(up) {
                    // Without the app nothing syncs: say so once, and again when it's back.
                    let said = match (app_up, up) {
                        (_, false) => say(json, &serde_json::json!({ "event": "coo_not_running",
                            "message": "Coo isn't running, so no new mail arrives; the watch resumes when it's open" }),
                            "Coo isn't running, so no new mail arrives; the watch resumes when it's open"),
                        (Some(false), true) => say(json, &serde_json::json!({ "event": "coo_running" }), "Coo is running again"),
                        _ => true,
                    };
                    if !said {
                        break;
                    }
                    app_up = Some(up);
                }
            }
            _ = poll.tick() => {
                let rows = app.store().messages_after(mark, 500)?;
                let Some((last, _)) = rows.last() else { continue };
                mark = *last;
                for (_, m) in rows.iter().filter(|(_, m)| filters.matches(m, &mine, started_at)) {
                    let msg = MessageOut::new(&m.account_id, &m.header);
                    let from = msg.from.as_ref().map(|a| a.email.clone()).unwrap_or_default();
                    let mut v = serde_json::to_value(&msg)?;
                    v["event"] = "mail".into();
                    v["watch"] = id.clone().into();
                    let human = format!("New mail from {from}: {} ({} {} {})", msg.subject, msg.account, msg.thread_id, msg.id);
                    if !say(json, &v, &human) || once {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Address, MessageHeader};

    use super::*;

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
}
