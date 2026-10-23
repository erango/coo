//! `pigeon` — command-line access to the engine, for people and agents.
//!
//! Every command takes `--json` for machine-readable output (shapes in
//! `out.rs`). Writes from a non-interactive caller need `--yes`;
//! `PIGEON_AGENT_READONLY=1` blocks them entirely. `pigeon mcp` serves the
//! same operations over the Model Context Protocol.
//!
//! Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in required, 4 write blocked.

mod mcp;
mod ops;
mod out;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use pigeon_core::compose::ReplyKind;
use pigeon_core::model::ProviderKind;
use pigeon_engine::Pigeon;
use serde::Serialize;

use crate::ops::Writes;
use crate::out::*;

#[derive(Parser)]
#[command(
    name = "pigeon",
    about = "Pigeon mail + calendar from the command line",
    after_help = "Agents: add --json for structured output; writes need --yes when not run from a terminal; \
                  `pigeon mcp` starts an MCP server (read-only unless --allow-write)."
)]
struct Cli {
    /// Machine-readable JSON on stdout (errors too, with a non-zero exit code).
    #[arg(long, global = true)]
    json: bool,
    /// Allow writes (send, act, rsvp, ...) from a non-interactive caller.
    #[arg(long, global = true)]
    yes: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sign in to an account (microsoft | google) in the browser.
    Login { provider: String },
    /// Remove an account and its local data.
    Logout { account: String },
    /// List signed-in accounts.
    Accounts,
    /// Pull new mail and events (one account, or all concurrently).
    Sync { account: Option<String> },
    /// An account's folders/labels with unread/total counts.
    Folders { account: String },
    /// List a mailbox. Folder is a role across accounts (inbox, sent, drafts,
    /// trash, spam, archive, starred, important) or, with --account, a name/id.
    Ls {
        #[arg(short, long)]
        account: Option<String>,
        #[arg(short, long, default_value = "inbox")]
        folder: String,
        /// Group into conversations.
        #[arg(short, long)]
        threads: bool,
        #[arg(long)]
        unread: bool,
        #[arg(short = 'n', long, default_value_t = 30)]
        limit: u32,
    },
    /// Search: free text plus from: to: has:attachment is:unread is:flagged
    /// in: account: before: after:
    Search {
        query: Vec<String>,
        /// Also ask the servers for older matches.
        #[arg(long)]
        server: bool,
        #[arg(short = 'n', long, default_value_t = 30)]
        limit: u32,
    },
    /// One message with its plain-text body (fetched on first use).
    #[command(alias = "show")]
    Get {
        account: String,
        message_id: String,
        #[arg(long, default_value_t = 20_000)]
        max_chars: usize,
    },
    /// A whole conversation, oldest first, with bodies.
    Thread {
        account: String,
        thread_id: String,
        #[arg(long, default_value_t = 20_000)]
        max_chars: usize,
    },
    /// Download an attachment and print its local path.
    Attachment { account: String, message_id: String, attachment_id: String },
    /// Events in a range. Times: now, today, tomorrow, +7d, -6h, YYYY-MM-DD,
    /// YYYY-MM-DDTHH:MM, RFC 3339.
    #[command(alias = "events")]
    Agenda {
        #[arg(long, default_value = "now")]
        from: String,
        #[arg(long, default_value = "+7d")]
        to: String,
        #[arg(short, long)]
        account: Option<String>,
    },
    /// Busy intervals for people, as seen by one of your accounts.
    Freebusy {
        account: String,
        #[arg(value_delimiter = ',')]
        emails: Vec<String>,
        #[arg(long, default_value = "now")]
        from: String,
        #[arg(long, default_value = "+1d")]
        to: String,
    },
    /// Every address you can send from (all accounts + aliases).
    Senders,
    /// Change messages: read | unread | flag | unflag | archive | trash | move.
    Act {
        action: String,
        account: String,
        message_ids: Vec<String>,
        /// Target folder/label (name, id or role) for `move`.
        #[arg(long)]
        to: Option<String>,
        /// Folder being viewed (Gmail `move` removes this label).
        #[arg(long)]
        from: Option<String>,
    },
    /// Undo an action by its handle.
    Undo { account: String, handle: i64 },
    /// Compose and send a new message.
    Send {
        /// Sender address (see `senders`).
        #[arg(long)]
        from: String,
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        cc: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        bcc: Vec<String>,
        #[arg(long, default_value = "")]
        subject: String,
        /// Plain text; each line becomes an auto-direction paragraph.
        #[arg(long)]
        body: String,
        #[arg(long)]
        attach: Vec<String>,
        /// Save a draft instead of sending.
        #[arg(long)]
        dry_run: bool,
    },
    /// Reply / reply-all / forward, optionally from another account's address.
    Reply {
        account: String,
        message_id: String,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        forward: bool,
        #[arg(long)]
        from: Option<String>,
        /// Recipients (required for --forward).
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        #[arg(long)]
        body: String,
        /// Save a draft instead of sending.
        #[arg(long)]
        dry_run: bool,
    },
    /// Create an event (invites are sent to attendees).
    EventCreate {
        account: String,
        #[arg(long)]
        title: String,
        /// Start (see `agenda` for formats; a date with --all-day).
        #[arg(long)]
        start: String,
        /// Minutes (timed) or days (all-day).
        #[arg(long, default_value_t = 30)]
        duration: i64,
        #[arg(long)]
        all_day: bool,
        #[arg(long, value_delimiter = ',')]
        attendees: Vec<String>,
        #[arg(long)]
        location: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Calendar id (default: the account's primary calendar).
        #[arg(long)]
        calendar: Option<String>,
    },
    /// Delete an event (cancels it for attendees if you organize it).
    EventDelete { account: String, calendar_id: String, event_id: String },
    /// Answer an invitation: accept | tentative | decline.
    Rsvp {
        account: String,
        calendar_id: String,
        event_id: String,
        response: String,
        #[arg(long)]
        comment: Option<String>,
    },
    /// Serve mail and calendar over MCP on stdio (for Claude and other agents).
    Mcp {
        /// Expose write tools (mail actions, reply/send, RSVP, create event).
        #[arg(long)]
        allow_write: bool,
    },
    /// Send a test error to Sentry (verifies PIGEON_SENTRY_DSN + scrubbing).
    TelemetryTest,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    // Agents read stdout; keep stderr quiet unless RUST_LOG says otherwise.
    let quiet = cli.json || matches!(cli.cmd, Cmd::Mcp { .. });
    let _telemetry = pigeon_engine::telemetry::init(
        concat!("pigeon-cli@", env!("CARGO_PKG_VERSION")),
        if quiet { "warn" } else { "warn,pigeon=info" },
    );
    let json = cli.json;
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let (code, kind) = classify(&e);
            if json {
                let err = serde_json::json!({ "error": { "kind": kind, "message": e.to_string() } });
                println!("{err}");
            } else {
                eprintln!("error: {e}");
            }
            ExitCode::from(code)
        }
    }
}

fn classify(e: &pigeon_core::Error) -> (u8, &'static str) {
    match e {
        pigeon_core::Error::SignInRequired(_) => (3, "sign_in_required"),
        e if e.to_string().starts_with(ops::WRITE_BLOCKED) => (4, "write_blocked"),
        _ => (1, "error"),
    }
}

/// Print `value` as JSON, or with the human formatter.
fn emit<T: Serialize>(json: bool, value: &T, human: impl FnOnce(&T)) {
    if json {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else {
        human(value);
    }
}

async fn run(cli: Cli) -> pigeon_core::Result<()> {
    let json = cli.json;
    let w = Writes::for_cli(cli.yes);
    if let Cmd::TelemetryTest = cli.cmd {
        if !pigeon_engine::telemetry::enabled() {
            return Err(pigeon_core::Error::Other("telemetry is off: set PIGEON_SENTRY_DSN".into()));
        }
        // The address below must arrive as <email> in Sentry.
        tracing::error!(
            account = "microsoft:test@example.com",
            "telemetry test from pigeon-cli (scrub check: test@example.com)"
        );
        println!("sent a test error; check Sentry for “telemetry test from pigeon-cli”");
        return Ok(());
    }
    let app = Pigeon::open()?;
    let now = chrono::Local::now();
    match cli.cmd {
        Cmd::TelemetryTest => unreachable!(),
        Cmd::Mcp { allow_write } => mcp::serve(app, allow_write).await?,
        Cmd::Login { provider } => {
            let kind = ProviderKind::parse(&provider)
                .ok_or_else(|| pigeon_core::Error::Other(format!("unknown provider {provider:?}")))?;
            let account = app
                .login(kind, |url| eprintln!("Opening the browser to sign in. If it does not open, visit:\n{url}\n"))
                .await?;
            emit(json, &AccountOut::from(&account), |a| println!("Signed in as {} ({})", a.email, a.id));
        }
        Cmd::Logout { account } => {
            w.check()?;
            let a = app.find_account(&account)?;
            app.logout(&a.id)?;
            emit(json, &serde_json::json!({"removed": a.id}), |_| println!("Removed {}", a.email));
        }
        Cmd::Accounts => emit(json, &ops::accounts(&app)?, |v| {
            for a in v {
                println!("{:<10} {:<36} {}", a.provider, a.email, a.name);
            }
        }),
        Cmd::Sync { account } => {
            let started = std::time::Instant::now();
            let reports = ops::sync(&app, account.as_deref()).await?;
            emit(json, &reports, |v| {
                for r in v {
                    let part = |name: &str, p: &SyncPartOut| match &p.error {
                        None => println!("{}: {name} {}", r.account, p.stats),
                        Some(e) => println!("{}: {name} FAILED: {e}", r.account),
                    };
                    part("mail", &r.mail);
                    part("calendar", &r.calendar);
                }
                println!("done in {:.1}s", started.elapsed().as_secs_f64());
            });
        }
        Cmd::Folders { account } => emit(json, &ops::folders(&app, &account)?, |v| {
            for f in v {
                println!("{:>6} {:>6}  {:<10} {:<32} {}", f.unread, f.total, f.role.unwrap_or(""), f.name, f.id);
            }
        }),
        Cmd::Ls { account, folder, threads, unread, limit } => {
            match ops::list(&app, account.as_deref(), &folder, threads, unread, limit)? {
                ops::Listing::Threads(t) => emit(json, &t, |v| {
                    for t in v {
                        let count = if t.message_count > 1 { format!("({})", t.message_count) } else { String::new() };
                        println!(
                            "{} {} {:<28} {:<5} {}  {}  {}",
                            if t.unread_count > 0 { '*' } else { ' ' },
                            short_time(&t.latest.received_at),
                            clip(&t.participants.join(", "), 28),
                            count,
                            clip(&t.subject, 60),
                            short_account(&t.account),
                            t.thread_id
                        );
                    }
                }),
                ops::Listing::Messages(m) => emit(json, &m, |v| v.iter().for_each(print_message)),
            }
        }
        Cmd::Search { query, server, limit } => {
            emit(json, &ops::search(&app, &query.join(" "), server, limit).await?, |v| v.iter().for_each(print_message))
        }
        Cmd::Get { account, message_id, max_chars } => {
            emit(json, &ops::get_message(&app, &account, &message_id, max_chars).await?, print_full)
        }
        Cmd::Thread { account, thread_id, max_chars } => {
            emit(json, &ops::get_thread(&app, &account, &thread_id, max_chars).await?, |v| {
                for m in v {
                    print_full(m);
                    println!("{}", "-".repeat(72));
                }
            })
        }
        Cmd::Attachment { account, message_id, attachment_id } => {
            let a = app.find_account(&account)?;
            let path = app.attachment(&a, &message_id, &attachment_id).await?;
            emit(json, &serde_json::json!({"path": path}), |_| println!("{}", path.display()));
        }
        Cmd::Agenda { from, to, account } => {
            let (from, to) = (ops::parse_when(&from, now)?, ops::parse_when(&to, now)?);
            emit(json, &ops::agenda(&app, from, to, account.as_deref())?, |v| {
                for e in v {
                    let time = if e.all_day { format!("{}  all-day", e.start) } else { short_time(&e.start) };
                    println!(
                        "{}  {:<50} {:<9} {:<7} {}  {} {}",
                        time,
                        clip(&e.title, 50),
                        e.my_response,
                        if e.online_url.is_some() { "[video]" } else { "" },
                        short_account(&e.account),
                        e.calendar_id,
                        e.id
                    );
                }
            })
        }
        Cmd::Freebusy { account, emails, from, to } => {
            let (from, to) = (ops::parse_when(&from, now)?, ops::parse_when(&to, now)?);
            emit(json, &ops::free_busy(&app, &account, &emails, from, to).await?, |v| {
                for b in v {
                    let spans: Vec<String> =
                        b.busy.iter().map(|s| format!("{}–{}", short_time(&s.start), short_time(&s.end))).collect();
                    println!("{}: {}", b.email, if spans.is_empty() { "free".into() } else { spans.join(", ") });
                }
            })
        }
        Cmd::Senders => {
            let senders: Vec<serde_json::Value> = app
                .senders()
                .await?
                .into_iter()
                .map(|s| serde_json::json!({"email": s.address.email, "name": s.address.name, "account": s.account_id}))
                .collect();
            emit(json, &senders, |v| {
                for s in v {
                    println!(
                        "{:<40} {:<24} {}",
                        s["email"].as_str().unwrap_or_default(),
                        s["name"].as_str().unwrap_or_default(),
                        s["account"].as_str().unwrap_or_default()
                    );
                }
            });
        }
        Cmd::Act { action, account, message_ids, to, from } => {
            let r = ops::act(&app, w, &account, &message_ids, &action, to.as_deref(), from.as_deref()).await?;
            emit(json, &r, print_act);
        }
        Cmd::Undo { account, handle } => emit(json, &ops::undo(&app, w, &account, handle).await?, print_act),
        Cmd::Send { from, to, cc, bcc, subject, body, attach, dry_run } => {
            let m = ops::NewMail {
                from: &from,
                to: &to,
                cc: &cc,
                bcc: &bcc,
                subject: &subject,
                body: &body,
                attach: &attach,
            };
            emit(json, &ops::send(&app, w, m, dry_run).await?, print_draft);
        }
        Cmd::Reply { account, message_id, all, forward, from, to, body, dry_run } => {
            let kind = match (all, forward) {
                (_, true) => ReplyKind::Forward,
                (true, false) => ReplyKind::ReplyAll,
                _ => ReplyKind::Reply,
            };
            let r = ops::Reply {
                account: &account,
                message_id: &message_id,
                kind,
                from: from.as_deref(),
                to: &to,
                body: &body,
            };
            emit(json, &ops::reply(&app, w, r, dry_run).await?, print_draft);
        }
        Cmd::EventCreate { account, title, start, duration, all_day, attendees, location, description, calendar } => {
            let start_at = if all_day {
                let d = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                    .map_err(|e| pigeon_core::Error::Other(format!("bad --start date: {e}")))?;
                d.and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp_millis()
            } else {
                ops::parse_when(&start, now)?
            };
            let end_at = start_at + if all_day { duration.max(1) * 86_400_000 } else { duration * 60_000 };
            let e = ops::create_event(
                &app,
                w,
                ops::NewEvent {
                    account: &account,
                    title: &title,
                    start: start_at,
                    end: end_at,
                    all_day,
                    attendees: &attendees,
                    location: location.as_deref(),
                    description: description.as_deref(),
                    calendar: calendar.as_deref(),
                },
            )
            .await?;
            emit(json, &e, |e| println!("created {} in {}", e.id, e.calendar_id));
        }
        Cmd::EventDelete { account, calendar_id, event_id } => {
            ops::delete_event(&app, w, &account, &calendar_id, &event_id).await?;
            emit(json, &serde_json::json!({"deleted": event_id}), |_| println!("deleted"));
        }
        Cmd::Rsvp { account, calendar_id, event_id, response, comment } => {
            let r = ops::rsvp(&app, w, &account, &calendar_id, &event_id, &response, comment.as_deref()).await?;
            emit(json, &r, |r| println!("responded {}", r["response"].as_str().unwrap_or_default()));
        }
    }
    Ok(())
}

fn print_act(r: &ActOut) {
    println!("sent {}, deferred {}", r.sent, r.deferred);
    for e in &r.rejected {
        println!("rejected (rolled back): {e}");
    }
    if r.needs_sign_in {
        println!("sign-in required: run `pigeon login` again");
    }
    println!("undo handle: {}", r.undo_handle);
}

fn print_draft(d: &DraftOut) {
    let list = |v: &[AddrOut]| v.iter().map(|x| x.email.clone()).collect::<Vec<_>>().join(", ");
    println!("from:    {} ({})", d.from.email, d.from_account);
    if d.crosses_accounts {
        println!("         replying from a different account than the one that received it");
    }
    println!("to:      {}\ncc:      {}\nsubject: {}", list(&d.to), list(&d.cc), d.subject);
    match &d.message_id {
        Some(id) => println!("sent {id}"),
        None => println!("draft saved: {} (not sent)", d.draft_id),
    }
}

fn print_message(m: &MessageOut) {
    let flags = format!(
        "{}{}{}",
        if m.unread { '*' } else { ' ' },
        if m.flagged { '!' } else { ' ' },
        if m.has_attachments { '@' } else { ' ' }
    );
    let from = m.from.as_ref().map(|a| a.name.clone().unwrap_or_else(|| a.email.clone())).unwrap_or_default();
    println!(
        "{flags} {} {:<28} {}  {}  {}",
        short_time(&m.received_at),
        clip(&from, 28),
        clip(&m.subject, 70),
        short_account(&m.account),
        m.id
    );
}

fn print_full(m: &FullMessageOut) {
    let h = &m.message;
    let who = |a: &AddrOut| match &a.name {
        Some(n) => format!("{n} <{}>", a.email),
        None => a.email.clone(),
    };
    println!("From:    {}", h.from.as_ref().map(who).unwrap_or_default());
    println!("To:      {}", h.to.iter().map(who).collect::<Vec<_>>().join(", "));
    if !h.cc.is_empty() {
        println!("Cc:      {}", h.cc.iter().map(who).collect::<Vec<_>>().join(", "));
    }
    println!("Date:    {}\nSubject: {}\n", h.received_at, h.subject);
    println!("{}", m.text);
    if m.truncated {
        println!("[… truncated; use --max-chars]");
    }
    for a in m.attachments.iter().filter(|a| !a.inline) {
        println!("[attachment {}] {} {}", a.id, a.name, a.content_type.as_deref().unwrap_or(""));
    }
}

/// `Oct 23 10:24` from an RFC 3339 string (dates pass through).
fn short_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&chrono::Local).format("%b %d %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

fn short_account(id: &str) -> String {
    id.split_once(':').map(|(_, email)| email).unwrap_or(id).to_string()
}

fn clip(s: &str, max: usize) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() <= max {
        s
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
