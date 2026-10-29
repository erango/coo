//! Operations shared by the CLI and the MCP server. Each returns the JSON
//! shapes from `out.rs`; the CLI prints them for humans or as JSON.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use pigeon_core::compose::{AttachmentSource, Draft, DraftAttachment, ReplyKind, Sender};
use pigeon_core::model::*;
use pigeon_core::store::{FolderSel, ListQuery};
use pigeon_core::{Error, Result};
use pigeon_engine::{MailAction, Pigeon};

use crate::out::*;

/// Why a write was refused. Surfaces as exit code 4 / MCP tool error.
pub const WRITE_BLOCKED: &str = "write blocked";

/// Whether mutating commands may run.
#[derive(Debug, Clone, Copy)]
pub struct Writes {
    pub allowed: bool,
    pub reason: &'static str,
}

impl Writes {
    /// CLI policy: `PIGEON_AGENT_READONLY` always blocks; otherwise a human
    /// at a terminal may write, and non-interactive callers need `--yes`.
    pub fn for_cli(yes: bool) -> Self {
        use std::io::IsTerminal;
        if readonly_env() {
            return Writes { allowed: false, reason: "PIGEON_AGENT_READONLY is set" };
        }
        if yes || std::io::stdin().is_terminal() {
            return Writes { allowed: true, reason: "" };
        }
        Writes { allowed: false, reason: "non-interactive write needs --yes" }
    }

    /// MCP policy: read-only unless the server was started with --allow-write.
    pub fn for_mcp(allow_write: bool) -> Self {
        if readonly_env() {
            Writes { allowed: false, reason: "PIGEON_AGENT_READONLY is set" }
        } else if allow_write {
            Writes { allowed: true, reason: "" }
        } else {
            Writes {
                allowed: false,
                reason: "this agent is set up read-only. To let it send, open Coo › Help › Connect an AI Agent, turn on \
                         sending and click Update, then restart the agent",
            }
        }
    }

    pub fn check(self) -> Result<()> {
        if self.allowed { Ok(()) } else { Err(Error::Other(format!("{WRITE_BLOCKED}: {}", self.reason))) }
    }
}

fn readonly_env() -> bool {
    std::env::var("PIGEON_AGENT_READONLY").is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// Parse a time for agent-friendly ranges: `now`, `today`, `tomorrow`,
/// `+3d` / `-2d` / `+6h` relative to now, `YYYY-MM-DD` (local midnight),
/// `YYYY-MM-DDTHH:MM` (local), or full RFC 3339.
pub fn parse_when(s: &str, now: chrono::DateTime<Local>) -> Result<i64> {
    let s = s.trim();
    let midnight = |d: NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))
            .earliest()
            .map(|t| t.timestamp_millis())
            .ok_or_else(|| Error::Other(format!("no local midnight on {d}")))
    };
    match s {
        "now" => return Ok(now.timestamp_millis()),
        "today" => return midnight(now.date_naive()),
        "tomorrow" => return midnight(now.date_naive() + Duration::days(1)),
        "yesterday" => return midnight(now.date_naive() - Duration::days(1)),
        _ => {}
    }
    if let Some(rel) = s.strip_prefix('+').or_else(|| s.strip_prefix('-')) {
        let sign = if s.starts_with('-') { -1 } else { 1 };
        let (num, unit) = rel.split_at(rel.len().saturating_sub(1));
        if let Ok(n) = num.parse::<i64>() {
            let n = n * sign;
            // Days/weeks are calendar days at the same wall-clock time (so a
            // DST change in between doesn't shift the hour); h/m are exact.
            let wall = |days: i64| {
                Local
                    .from_local_datetime(&(now.naive_local() + Duration::days(days)))
                    .earliest()
                    .map(|t| t.timestamp_millis())
                    .ok_or_else(|| Error::Other(format!("{s} lands in a DST gap")))
            };
            return match unit {
                "d" => wall(n),
                "w" => wall(n * 7),
                "h" => Ok((now + Duration::hours(n)).timestamp_millis()),
                "m" => Ok((now + Duration::minutes(n)).timestamp_millis()),
                _ => Err(Error::Other(format!("bad relative time {s:?} (use +3d, -6h, +2w)"))),
            };
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return midnight(d);
    }
    if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Local
            .from_local_datetime(&t)
            .earliest()
            .map(|t| t.timestamp_millis())
            .ok_or_else(|| Error::Other(format!("{s} doesn't exist locally (DST)")));
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp_millis())
        .map_err(|_| Error::Other(format!("can't read time {s:?} (try today, +7d, 2026-10-23, 2026-10-23T14:00)")))
}

pub fn parse_action(app: &Pigeon, account: &Account, action: &str, to: Option<&str>) -> Result<MailAction> {
    Ok(match action {
        "read" | "mark_read" => MailAction::MarkRead,
        "unread" | "mark_unread" => MailAction::MarkUnread,
        "flag" | "star" => MailAction::Flag,
        "unflag" | "unstar" => MailAction::Unflag,
        "archive" => MailAction::Archive,
        "trash" | "delete" => MailAction::Trash,
        "spam" | "junk" => MailAction::Spam,
        "not_spam" | "not-spam" | "not_junk" => MailAction::NotSpam,
        "phishing" | "phish" => MailAction::Phishing,
        "move" => MailAction::MoveTo(folder_id(
            app,
            account,
            to.ok_or_else(|| Error::Other("move needs a target folder".into()))?,
        )?),
        other => {
            return Err(Error::Other(format!(
                "unknown action {other:?} (read, unread, flag, unflag, archive, trash, move, spam, not_spam, phishing)"
            )));
        }
    })
}

/// A folder by id, name (case-insensitive) or role.
pub fn folder_id(app: &Pigeon, account: &Account, name: &str) -> Result<String> {
    let folders = app.store().folders(&account.id)?;
    folders
        .iter()
        .find(|f| {
            f.folder.id == name
                || f.folder.name.eq_ignore_ascii_case(name)
                || f.folder.role.is_some_and(|r| r.as_str().eq_ignore_ascii_case(name))
        })
        .map(|f| f.folder.id.clone())
        .ok_or_else(|| Error::Other(format!("no folder {name:?} in {}", account.email)))
}

pub fn accounts(app: &Pigeon) -> Result<Vec<AccountOut>> {
    Ok(app.accounts()?.iter().map(AccountOut::from).collect())
}

pub fn folders(app: &Pigeon, account: &str) -> Result<Vec<FolderOut>> {
    let a = app.find_account(account)?;
    Ok(app.store().folders(&a.id)?.iter().map(|f| FolderOut::new(&a.id, f)).collect())
}

pub enum Listing {
    Threads(Vec<ThreadOut>),
    Messages(Vec<MessageOut>),
}

/// List a mailbox: a role across all accounts (`inbox`, `sent`, ...) or,
/// with an account, a named folder.
pub fn list(
    app: &Pigeon,
    account: Option<&str>,
    folder: &str,
    threads: bool,
    unread_only: bool,
    limit: u32,
) -> Result<Listing> {
    let account = account.map(|a| app.find_account(a)).transpose()?;
    let sel = match FolderRole::parse(folder) {
        Some(role) => FolderSel::Role(role),
        None => {
            let a = account.as_ref().ok_or_else(|| {
                Error::Other(format!("folder {folder:?} needs an account (or use a role: inbox, sent, ...)"))
            })?;
            FolderSel::Id(folder_id(app, a, folder)?)
        }
    };
    let q = ListQuery { account_id: account.map(|a| a.id), folder: sel, limit, before: None };
    let store = app.store();
    Ok(if threads {
        Listing::Threads(
            store
                .list_threads(&q)?
                .iter()
                .filter(|t| !unread_only || t.unread_count > 0)
                .map(ThreadOut::from)
                .collect(),
        )
    } else {
        Listing::Messages(
            store
                .list_messages(&q)?
                .iter()
                .filter(|m| !unread_only || !m.header.is_read)
                .map(|m| MessageOut::new(&m.account_id, &m.header))
                .collect(),
        )
    })
}

/// Search with the app's operators. `server` also asks every account's
/// server for older matches first.
pub async fn search(app: &Pigeon, query: &str, server: bool, limit: u32) -> Result<Vec<MessageOut>> {
    let q = pigeon_core::search::parse(query);
    if q.is_empty() {
        return Err(Error::Other("empty query".into()));
    }
    if server {
        app.search_servers(&q, limit.min(100)).await?;
    }
    Ok(app.store().search_query(&q, limit)?.iter().map(|m| MessageOut::new(&m.account_id, &m.header)).collect())
}

fn body_text(body: &StoredBody) -> String {
    match (&body.text, &body.html) {
        (Some(t), _) if !t.trim().is_empty() => t.clone(),
        (_, Some(h)) => pigeon_core::text::html_to_text(h),
        (Some(t), None) => t.clone(),
        (None, None) => String::new(),
    }
}

/// One message with its plain-text body (fetched on first use).
pub async fn get_message(app: &Pigeon, account: &str, message_id: &str, max_chars: usize) -> Result<FullMessageOut> {
    let a = app.find_account(account)?;
    let h = app
        .store()
        .message(&a.id, message_id)?
        .ok_or_else(|| Error::Other(format!("message {message_id} not found in {} (sync first?)", a.email)))?;
    let body = app.body(&a, message_id).await?;
    let mut text = body_text(&body);
    let truncated = text.chars().count() > max_chars;
    if truncated {
        text = text.chars().take(max_chars).collect();
    }
    Ok(FullMessageOut {
        message: MessageOut::new(&a.id, &h),
        text,
        truncated,
        attachments: body
            .attachments
            .iter()
            .map(|x| AttachmentOut {
                id: x.meta.id.clone(),
                name: x.meta.name.clone(),
                content_type: x.meta.content_type.clone(),
                size: x.meta.size,
                inline: x.meta.is_inline,
            })
            .collect(),
        in_reply_to: body.in_reply_to,
        references: body.references,
        body_unavailable: None,
    })
}

/// Every message of a thread, oldest first, with bodies. `max_chars` is the
/// budget for all bodies together; older messages are trimmed first.
pub async fn get_thread(app: &Pigeon, account: &str, thread_id: &str, max_chars: usize) -> Result<Vec<FullMessageOut>> {
    let a = app.find_account(account)?;
    let messages = app.store().thread_messages(&a.id, thread_id)?;
    if messages.is_empty() {
        return Err(Error::Other(format!("thread {thread_id} not found in {}", a.email)));
    }
    let per = (max_chars / messages.len()).max(500);
    let mut out = Vec::with_capacity(messages.len());
    for (i, m) in messages.iter().enumerate() {
        // The newest message gets the whole remaining budget.
        let budget = if i + 1 == messages.len() { max_chars.saturating_sub(per * i).max(per) } else { per };
        // A body that can't be fetched now (not cached, server unreachable)
        // doesn't sink the whole thread: its snippet stands in, flagged.
        out.push(match get_message(app, &a.id, &m.id, budget).await {
            Ok(full) => full,
            Err(e) => FullMessageOut {
                message: MessageOut::new(&a.id, m),
                text: m.snippet.clone(),
                truncated: true,
                attachments: Vec::new(),
                in_reply_to: None,
                references: Vec::new(),
                body_unavailable: Some(format!("only the preview is shown: {}", describe(&e).1)),
            },
        });
    }
    Ok(out)
}

pub fn agenda(app: &Pigeon, from: i64, to: i64, account: Option<&str>) -> Result<Vec<EventOut>> {
    let account = account.map(|a| app.find_account(a)).transpose()?;
    Ok(app
        .store()
        .events_between(from, to)?
        .iter()
        .filter(|e| account.as_ref().is_none_or(|a| a.id == e.account_id))
        .map(EventOut::from)
        .collect())
}

pub async fn free_busy(app: &Pigeon, account: &str, emails: &[String], from: i64, to: i64) -> Result<Vec<BusyOut>> {
    let a = app.find_account(account)?;
    Ok(app
        .free_busy(&a, emails, from, to)
        .await?
        .into_iter()
        .map(|(email, busy)| BusyOut {
            email,
            busy: busy.into_iter().map(|(s, e)| SpanOut { start: iso(s), end: iso(e) }).collect(),
        })
        .collect())
}

pub async fn sync(app: &Pigeon, account: Option<&str>) -> Result<Vec<SyncOut>> {
    let reports = match account {
        Some(a) => vec![app.sync_account(&app.find_account(a)?).await],
        None => app.sync_all().await?,
    };
    Ok(reports
        .into_iter()
        .map(|r| SyncOut {
            account: r.account.id.clone(),
            mail: match &r.mail {
                Ok(s) => SyncPartOut {
                    ok: true,
                    error: None,
                    stats: serde_json::json!({"folders": s.folders, "pages": s.pages, "changes": s.changes}),
                },
                Err(e) => SyncPartOut { ok: false, error: Some(e.to_string()), stats: serde_json::json!({}) },
            },
            calendar: match &r.calendar {
                Ok(s) => SyncPartOut {
                    ok: true,
                    error: None,
                    stats: serde_json::json!({"calendars": s.calendars, "events": s.events}),
                },
                Err(e) => SyncPartOut { ok: false, error: Some(e.to_string()), stats: serde_json::json!({}) },
            },
        })
        .collect())
}

pub async fn act(
    app: &Pigeon,
    w: Writes,
    account: &str,
    ids: &[String],
    action: &str,
    to: Option<&str>,
    from: Option<&str>,
) -> Result<ActOut> {
    w.check()?;
    if ids.is_empty() {
        return Err(Error::Other("no message ids".into()));
    }
    let a = app.find_account(account)?;
    let action = parse_action(app, &a, action, to)?;
    let from = from.map(|f| folder_id(app, &a, f)).transpose()?;
    let handle = app.act(&a, ids, from.as_deref(), &action)?;
    let r = app.flush(&a).await?;
    Ok(ActOut {
        undo_handle: handle,
        sent: r.sent,
        deferred: r.deferred,
        rejected: r.rejected.iter().map(|(op, e)| format!("{op:?}: {e}")).collect(),
        needs_sign_in: r.needs_sign_in,
    })
}

pub async fn undo(app: &Pigeon, w: Writes, account: &str, handle: i64) -> Result<ActOut> {
    w.check()?;
    let a = app.find_account(account)?;
    app.undo(&a, handle)?;
    let r = app.flush(&a).await?;
    Ok(ActOut {
        undo_handle: handle,
        sent: r.sent,
        deferred: r.deferred,
        rejected: r.rejected.iter().map(|(op, e)| format!("{op:?}: {e}")).collect(),
        needs_sign_in: r.needs_sign_in,
    })
}

pub async fn find_sender(app: &Pigeon, email: &str) -> Result<Sender> {
    app.senders()
        .await?
        .into_iter()
        .find(|s| s.address.email.eq_ignore_ascii_case(email))
        .ok_or_else(|| Error::Other(format!("{email} is not one of your sending addresses (see `senders`)")))
}

pub fn addresses(list: &[String]) -> Vec<Address> {
    list.iter().flat_map(|s| pigeon_core::text::parse_address_list(s)).collect()
}

pub struct NewMail<'a> {
    pub from: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub subject: &'a str,
    pub body: &'a str,
    pub attach: &'a [String],
}

/// Compose and send. With `dry_run`, saves a draft and returns it unsent.
pub async fn send(app: &Pigeon, w: Writes, m: NewMail<'_>, dry_run: bool) -> Result<DraftOut> {
    if !dry_run {
        w.check()?;
    }
    let mut d = Draft::new(find_sender(app, m.from).await?, pigeon_core::now_ms());
    d.to = addresses(m.to);
    d.cc = addresses(m.cc);
    d.bcc = addresses(m.bcc);
    d.subject = m.subject.to_string();
    d.html = pigeon_core::text::text_to_html(m.body);
    for path in m.attach {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned());
        d.attachments.push(DraftAttachment {
            name: name.unwrap_or_else(|| "attachment".into()),
            content_type: None,
            source: AttachmentSource::File { path: path.clone() },
        });
    }
    if dry_run {
        app.save_draft(&mut d)?;
        return Ok(DraftOut::new(&d, None));
    }
    let id = app.send(&d).await?;
    Ok(DraftOut::new(&d, Some(id)))
}

pub struct Reply<'a> {
    pub account: &'a str,
    pub message_id: &'a str,
    pub kind: ReplyKind,
    pub from: Option<&'a str>,
    pub to: &'a [String],
    pub body: &'a str,
}

/// Reply / reply-all / forward. With `dry_run`, saves the draft unsent.
pub async fn reply(app: &Pigeon, w: Writes, r: Reply<'_>, dry_run: bool) -> Result<DraftOut> {
    if !dry_run {
        w.check()?;
    }
    let a = app.find_account(r.account)?;
    let from = match r.from {
        Some(f) => Some(find_sender(app, f).await?),
        None => None,
    };
    let mut d = app.reply(&a, r.message_id, r.kind, from).await?;
    d.html.insert_str(0, &pigeon_core::text::text_to_html(r.body));
    if !r.to.is_empty() {
        d.to = addresses(r.to);
    }
    if d.to.is_empty() && d.cc.is_empty() {
        return Err(Error::Other("no recipients (forward needs `to`)".into()));
    }
    app.save_draft(&mut d)?;
    if dry_run {
        return Ok(DraftOut::new(&d, None));
    }
    let id = app.send(&d).await?;
    Ok(DraftOut::new(&d, Some(id)))
}

pub fn parse_response(s: &str) -> Result<Response> {
    Ok(match s {
        "accept" | "accepted" | "yes" => Response::Accepted,
        "tentative" | "maybe" => Response::Tentative,
        "decline" | "declined" | "no" => Response::Declined,
        other => return Err(Error::Other(format!("unknown response {other:?} (accept, tentative, decline)"))),
    })
}

pub async fn rsvp(
    app: &Pigeon,
    w: Writes,
    account: &str,
    calendar_id: &str,
    event_id: &str,
    response: &str,
    comment: Option<&str>,
) -> Result<serde_json::Value> {
    w.check()?;
    let a = app.find_account(account)?;
    let r = parse_response(response)?;
    app.rsvp(&a, calendar_id, event_id, r, comment).await?;
    Ok(serde_json::json!({"ok": true, "response": r.as_str()}))
}

pub struct NewEvent<'a> {
    pub account: &'a str,
    pub title: &'a str,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub attendees: &'a [String],
    pub location: Option<&'a str>,
    pub description: Option<&'a str>,
    pub calendar: Option<&'a str>,
}

pub async fn create_event(app: &Pigeon, w: Writes, n: NewEvent<'_>) -> Result<EventOut> {
    w.check()?;
    if n.end <= n.start {
        return Err(Error::Other("end must be after start".into()));
    }
    let a = app.find_account(n.account)?;
    let draft = EventDraft {
        calendar_id: match n.calendar {
            Some(c) => c.to_string(),
            None => app.primary_calendar(&a)?,
        },
        title: n.title.to_string(),
        start_at: n.start,
        end_at: n.end,
        all_day: n.all_day,
        timezone: None,
        location: n.location.map(String::from),
        description: n.description.map(String::from),
        attendees: addresses(n.attendees)
            .into_iter()
            .map(|address| Attendee { address, response: Response::None, optional: false })
            .collect(),
        conferencing: Default::default(),
    };
    let e = app.create_event(&a, &draft).await?;
    Ok(EventOut::from(&EventRow { account_id: a.id.clone(), event: e }))
}

pub async fn delete_event(app: &Pigeon, w: Writes, account: &str, calendar_id: &str, event_id: &str) -> Result<()> {
    w.check()?;
    let a = app.find_account(account)?;
    app.delete_event(&a, calendar_id, event_id).await
}

/// What failed, for agents and people: a stable kind plus one plain sentence
/// on what to do. Provider refusals keep the provider's own message (a 403
/// usually names the missing permission).
pub fn describe(e: &Error) -> (&'static str, String) {
    match e {
        e if e.to_string().starts_with(WRITE_BLOCKED) => ("write_blocked", e.to_string()),
        Error::SignInRequired(account) => (
            "sign_in_required",
            format!(
                "The sign-in for {} has expired. Open Coo and sign in again (Settings › Accounts).",
                account.split_once(':').map_or(account.as_str(), |(_, email)| email)
            ),
        ),
        Error::TokenStore(msg) => ("sign_in_unavailable", msg.clone()),
        Error::Api { status, url, body } => {
            let who = if url.contains("googleapis.com") {
                "Google"
            } else if url.contains("graph.microsoft.com") {
                "Microsoft"
            } else {
                "The mail server"
            };
            let said = provider_message(body).unwrap_or_else(|| body.chars().take(300).collect());
            let hint = match status {
                401 => " Open Coo and sign in to this account again.",
                403 => " The account may not have granted Coo this permission; signing in again in Coo asks for it.",
                429 => " Too many requests right now; try again in a minute.",
                _ => "",
            };
            ("provider_error", format!("{who} refused the request ({status}): {said}.{hint}"))
        }
        e if e.is_network() => {
            ("network", format!("Couldn't reach the server: {e}. Check the connection and try again."))
        }
        e => ("error", e.to_string()),
    }
}

/// `error.message` from a Google / Microsoft JSON error body.
fn provider_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let m =
        v["error"]["message"].as_str().or_else(|| v["error_description"].as_str()).or_else(|| v["error"].as_str())?;
    Some(m.trim().trim_end_matches('.').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 23, 15, 30, 0).unwrap()
    }

    #[test]
    fn relative_and_absolute_times() {
        let n = now();
        let ms = |y, m, d, h, mi| Local.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap().timestamp_millis();
        assert_eq!(parse_when("now", n).unwrap(), n.timestamp_millis());
        assert_eq!(parse_when("today", n).unwrap(), ms(2026, 10, 23, 0, 0));
        assert_eq!(parse_when("tomorrow", n).unwrap(), ms(2026, 10, 24, 0, 0));
        assert_eq!(parse_when("+2d", n).unwrap(), ms(2026, 10, 25, 15, 30));
        assert_eq!(parse_when("-6h", n).unwrap(), ms(2026, 10, 23, 9, 30));
        assert_eq!(parse_when("+1w", n).unwrap(), ms(2026, 10, 30, 15, 30));
        assert_eq!(parse_when("2026-11-01", n).unwrap(), ms(2026, 11, 1, 0, 0));
        assert_eq!(parse_when("2026-11-01T09:15", n).unwrap(), ms(2026, 11, 1, 9, 15));
        assert_eq!(parse_when("2026-11-01T09:15:00Z", n).unwrap(), 1_793_524_500_000);
        assert!(parse_when("next tuesday", n).is_err());
        assert!(parse_when("+3x", n).is_err());
    }

    #[test]
    fn errors_say_what_failed() {
        let blocked = Error::Other(format!("{WRITE_BLOCKED}: server is read-only (start it with --allow-write)"));
        assert_eq!(describe(&blocked).0, "write_blocked");
        let (kind, msg) = describe(&Error::SignInRequired("google:a@x.com".into()));
        assert_eq!(kind, "sign_in_required");
        assert!(msg.contains("a@x.com") && msg.contains("Open Coo"), "{msg}");
        let body = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes.","status":"PERMISSION_DENIED"}}"#;
        let (kind, msg) = describe(&Error::Api {
            status: 403,
            url: "https://gmail.googleapis.com/gmail/v1/users/me/messages/send".into(),
            body: body.into(),
        });
        assert_eq!(kind, "provider_error");
        assert!(
            msg.starts_with("Google refused the request (403): Request had insufficient authentication scopes."),
            "{msg}"
        );
        assert!(msg.contains("permission"), "{msg}");
        let (kind, msg) = describe(&Error::TokenStore(pigeon_core::broker::APP_NOT_RUNNING.into()));
        assert_eq!(kind, "sign_in_unavailable");
        assert!(msg.contains("Open Coo"));
    }

    #[test]
    fn write_policy() {
        assert!(Writes::for_mcp(false).check().unwrap_err().to_string().starts_with(WRITE_BLOCKED));
        assert!(Writes::for_mcp(true).check().is_ok() || readonly_env());
        assert!(parse_response("maybe").is_ok() && parse_response("sure").is_err());
    }
}
