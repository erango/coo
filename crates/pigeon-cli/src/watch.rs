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

pub use pigeon_core::watches::Filters;
use pigeon_core::{Error, Result};
use pigeon_engine::Pigeon;
use serde::{Deserialize, Serialize};

use crate::out::MessageOut;

/// How often the store is checked for new messages.
const POLL: Duration = Duration::from_secs(2);
/// How often a watch checks that the Coo app (which syncs) is running.
const APP_CHECK: Duration = Duration::from_secs(30);
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

/// One Needs-you item for agents: the message, which watch caught it and
/// its note, and whether it's done.
#[derive(Debug, Serialize)]
pub struct NeedsOut {
    pub id: i64,
    pub watch_id: String,
    pub watch: String,
    /// What the user wants done with it (the watch's note).
    pub note: String,
    pub caught_at: String,
    pub done: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub done_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub done_note: String,
    pub message: MessageOut,
}

/// A saved watch, for agents.
#[derive(Debug, Serialize)]
pub struct SavedOut {
    pub id: String,
    pub name: String,
    pub filters: Filters,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub notify: bool,
    pub paused: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    pub caught: u32,
}

impl SavedOut {
    fn new(w: &pigeon_core::watches::Watch) -> Self {
        SavedOut {
            id: w.id.clone(),
            name: w.name.clone(),
            filters: w.filters.clone(),
            note: w.note.clone(),
            notify: w.notify,
            paused: w.paused,
            created_by: w.created_by.clone(),
            until: w.until.map(crate::out::iso),
            caught: w.hits,
        }
    }
}

/// What the watches caught: open items (plus the last day's done ones with
/// `include_done`), newest first.
pub fn needs_you(app: &Pigeon, include_done: bool) -> Result<Vec<NeedsOut>> {
    let now = pigeon_core::now_ms();
    let store = app.store();
    let items = store.needs_you(include_done.then_some(24 * 3_600_000), now)?;
    let mut out = Vec::new();
    for n in items {
        let Some(h) = store.message(&n.account_id, &n.message_id)? else { continue };
        out.push(NeedsOut {
            id: n.id,
            watch_id: n.watch_id,
            watch: n.watch_name,
            note: n.note,
            caught_at: crate::out::iso(n.caught_at),
            done: n.done_at.is_some(),
            done_by: n.done_by,
            done_note: n.done_note,
            message: MessageOut::new(&n.account_id, &h),
        });
    }
    Ok(out)
}

/// Mark a Needs-you item done, saying what was done.
pub fn mark_done(app: &Pigeon, id: i64, note: &str, by: &str) -> Result<serde_json::Value> {
    if !app.store().set_needs_done(id, true, by, note.trim(), pigeon_core::now_ms())? {
        return Err(Error::Other(format!("no Needs-you item {id} (see needs_you)")));
    }
    Ok(serde_json::json!({ "done": id }))
}

/// Save a watch in Coo (it catches mail while Coo runs, even with no agent
/// open).
pub fn add_watch(
    app: &Pigeon,
    name: &str,
    filters: Filters,
    note: &str,
    notify: bool,
    until: Option<i64>,
    by: &str,
) -> Result<SavedOut> {
    if name.trim().is_empty() {
        return Err(Error::Other("give the watch a name".into()));
    }
    if filters.is_empty() {
        return Err(Error::Other("give at least one of from, subject, thread or account".into()));
    }
    let mut w = pigeon_core::watches::Watch::new(name, filters, pigeon_core::now_ms());
    w.note = note.trim().to_string();
    w.notify = notify;
    w.until = until;
    w.created_by = by.to_string();
    app.store().save_watch(&w)?;
    Ok(SavedOut::new(&w))
}

/// Saved watches and running `coo watch` streams.
pub fn all(app: &Pigeon) -> Result<serde_json::Value> {
    let saved: Vec<SavedOut> = app.store().watches()?.iter().map(SavedOut::new).collect();
    Ok(serde_json::json!({ "saved": saved, "running": list(app) }))
}

/// Remove a saved watch or stop a running stream, by id (`None`: every
/// running stream; saved watches are only removed one by one).
pub fn remove(app: &Pigeon, id: Option<&str>) -> Result<serde_json::Value> {
    if let Some(id) = id
        && app.store().delete_watch(id)?
    {
        return Ok(serde_json::json!({ "removed": id }));
    }
    Ok(serde_json::json!({ "stopped": stop(app, id)? }))
}
