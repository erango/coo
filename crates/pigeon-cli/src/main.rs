//! `pigeon` — dev harness for the engine: sign in, sync, and inspect the local
//! store without the desktop UI.

use std::process::ExitCode;

use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use pigeon_core::model::{FolderRole, MessageHeader, ProviderKind};
use pigeon_core::store::{FolderSel, ListQuery};
use pigeon_engine::{MailAction, Pigeon};

#[derive(Parser)]
#[command(name = "pigeon", about = "Pigeon engine harness")]
struct Cli {
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
    /// Sync one account, or all of them concurrently.
    Sync { account: Option<String> },
    /// Show an account's folders/labels with local counts.
    Folders { account: String },
    /// List messages. Folder is a role (inbox, sent, drafts, trash, spam,
    /// archive, starred, important) or, with --account, a folder name/id.
    Ls {
        #[arg(short, long)]
        account: Option<String>,
        #[arg(short, long, default_value = "inbox")]
        folder: String,
        /// Group into conversations.
        #[arg(short, long)]
        threads: bool,
        #[arg(short = 'n', long, default_value_t = 30)]
        limit: u32,
    },
    /// Local full-text search.
    Search {
        query: Vec<String>,
        #[arg(short = 'n', long, default_value_t = 30)]
        limit: u32,
    },
    /// Print a message body (fetched on first use) and its attachments.
    Show {
        account: String,
        message_id: String,
        /// Print the sanitized HTML instead of the text part.
        #[arg(long)]
        html: bool,
    },
    /// Download an attachment and print its local path.
    Attachment { account: String, message_id: String, attachment_id: String },
    /// Change messages on the server: read | unread | flag | unflag | archive |
    /// trash | move. Applied locally first, then sent. Prints an undo handle.
    Act {
        action: String,
        account: String,
        message_ids: Vec<String>,
        /// Target folder/label (name or id) for `move`.
        #[arg(long)]
        to: Option<String>,
        /// Folder being viewed (Gmail `move` removes this label).
        #[arg(long)]
        from: Option<String>,
    },
    /// List every address you can send from (all accounts + aliases).
    Senders,
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
    },
    /// Reply (or reply-all / forward) to a message, optionally from another
    /// account's address, and send it.
    Reply {
        account: String,
        message_id: String,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        forward: bool,
        /// Send from this address instead of the default.
        #[arg(long)]
        from: Option<String>,
        /// Recipients (required for --forward).
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        #[arg(long)]
        body: String,
        /// Build and print the draft without sending.
        #[arg(long)]
        dry_run: bool,
    },
    /// Undo an action by its handle.
    Undo { account: String, handle: i64 },
    /// Create an event (invites are sent to attendees).
    EventCreate {
        account: String,
        #[arg(long)]
        title: String,
        /// Local start, `YYYY-MM-DDTHH:MM` (or `YYYY-MM-DD` with --all-day).
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
    /// Busy times of people over the next N hours, as seen by an account.
    Freebusy {
        account: String,
        #[arg(value_delimiter = ',')]
        emails: Vec<String>,
        #[arg(long, default_value_t = 24)]
        hours: i64,
    },
    /// Upcoming events across all calendars.
    Events {
        #[arg(short, long, default_value_t = 7)]
        days: i64,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let _telemetry =
        pigeon_engine::telemetry::init(concat!("pigeon-cli@", env!("CARGO_PKG_VERSION")), "warn,pigeon=info");
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> pigeon_core::Result<()> {
    let app = Pigeon::open()?;
    match cli.cmd {
        Cmd::Login { provider } => {
            let kind = ProviderKind::parse(&provider)
                .ok_or_else(|| pigeon_core::Error::Other(format!("unknown provider {provider:?}")))?;
            let account = app
                .login(kind, |url| eprintln!("Opening the browser to sign in. If it does not open, visit:\n{url}\n"))
                .await?;
            println!("Signed in as {} ({})", account.email, account.id);
        }
        Cmd::Logout { account } => {
            let a = app.find_account(&account)?;
            app.logout(&a.id)?;
            println!("Removed {}", a.email);
        }
        Cmd::Accounts => {
            for a in app.accounts()? {
                println!("{:<10} {:<36} {}", a.provider.as_str(), a.email, a.display_name);
            }
        }
        Cmd::Sync { account } => {
            let started = std::time::Instant::now();
            let reports = match account {
                Some(a) => vec![app.sync_account(&app.find_account(&a)?).await],
                None => app.sync_all().await?,
            };
            for r in reports {
                match &r.mail {
                    Ok(s) => println!(
                        "{}: mail {} folders, {} pages, {} changes",
                        r.account.email, s.folders, s.pages, s.changes
                    ),
                    Err(e) => println!("{}: mail FAILED: {e}", r.account.email),
                }
                match &r.calendar {
                    Ok(s) => println!("{}: calendar {} calendars, {} events", r.account.email, s.calendars, s.events),
                    Err(e) => println!("{}: calendar FAILED: {e}", r.account.email),
                }
            }
            println!("done in {:.1}s", started.elapsed().as_secs_f64());
        }
        Cmd::Folders { account } => {
            let a = app.find_account(&account)?;
            for f in app.store().folders(&a.id)? {
                let role = f.folder.role.map(FolderRole::as_str).unwrap_or("");
                println!("{:>6} {:>6}  {:<10} {:<32} {}", f.unread, f.total, role, f.folder.name, f.folder.id);
            }
        }
        Cmd::Ls { account, folder, threads, limit } => {
            let account = account.map(|a| app.find_account(&a)).transpose()?;
            let sel = match FolderRole::parse(&folder) {
                Some(role) => FolderSel::Role(role),
                None => {
                    let a = account
                        .as_ref()
                        .ok_or_else(|| pigeon_core::Error::Other("--account is required for a named folder".into()))?;
                    let folders = app.store().folders(&a.id)?;
                    let f = folders
                        .iter()
                        .find(|f| f.folder.id == folder || f.folder.name.eq_ignore_ascii_case(&folder))
                        .ok_or_else(|| pigeon_core::Error::Other(format!("no folder {folder:?}")))?;
                    FolderSel::Id(f.folder.id.clone())
                }
            };
            let q = ListQuery { account_id: account.map(|a| a.id), folder: sel, limit, before: None };
            if threads {
                for t in app.store().list_threads(&q)? {
                    let who = t.participants.join(", ");
                    let count = if t.message_count > 1 { format!("({})", t.message_count) } else { String::new() };
                    println!(
                        "{} {} {:<28} {:<5} {}  {}",
                        if t.unread_count > 0 { '*' } else { ' ' },
                        when(t.latest.received_at),
                        clip(&who, 28),
                        count,
                        clip(&t.latest.subject, 60),
                        short_account(&t.account_id)
                    );
                }
            } else {
                for r in app.store().list_messages(&q)? {
                    print_message(&r.header, Some(&r.account_id));
                }
            }
        }
        Cmd::Search { query, limit } => {
            for r in app.store().search(&query.join(" "), limit)? {
                print_message(&r.header, Some(&r.account_id));
            }
        }
        Cmd::Show { account, message_id, html } => {
            let a = app.find_account(&account)?;
            let body = app.body(&a, &message_id).await?;
            if body.has_remote_content {
                eprintln!("[remote content blocked]");
            }
            let content = if html { body.html.or(body.text) } else { body.text.or(body.html) };
            println!("{}", content.unwrap_or_default());
            for att in &body.attachments {
                let m = &att.meta;
                println!(
                    "[{}] {} {} {}{}",
                    m.id,
                    m.name,
                    m.content_type.as_deref().unwrap_or("?"),
                    m.size.map(|s| format!("{s}B")).unwrap_or_default(),
                    if m.is_inline { " inline" } else { "" }
                );
            }
        }
        Cmd::Attachment { account, message_id, attachment_id } => {
            let a = app.find_account(&account)?;
            println!("{}", app.attachment(&a, &message_id, &attachment_id).await?.display());
        }
        Cmd::Act { action, account, message_ids, to, from } => {
            let a = app.find_account(&account)?;
            let folder_id = |name: &str| -> pigeon_core::Result<String> {
                let folders = app.store().folders(&a.id)?;
                folders
                    .iter()
                    .find(|f| {
                        f.folder.id == name
                            || f.folder.name.eq_ignore_ascii_case(name)
                            || f.folder.role.is_some_and(|r| r.as_str() == name)
                    })
                    .map(|f| f.folder.id.clone())
                    .ok_or_else(|| pigeon_core::Error::Other(format!("no folder {name:?}")))
            };
            let action = match action.as_str() {
                "read" => MailAction::MarkRead,
                "unread" => MailAction::MarkUnread,
                "flag" | "star" => MailAction::Flag,
                "unflag" | "unstar" => MailAction::Unflag,
                "archive" => MailAction::Archive,
                "trash" | "delete" => MailAction::Trash,
                "move" => MailAction::MoveTo(folder_id(
                    to.as_deref().ok_or_else(|| pigeon_core::Error::Other("move needs --to".into()))?,
                )?),
                other => return Err(pigeon_core::Error::Other(format!("unknown action {other:?}"))),
            };
            let from = from.as_deref().map(folder_id).transpose()?;
            let handle = app.act(&a, &message_ids, from.as_deref(), &action)?;
            print_flush(&app.flush(&a).await?);
            println!("undo handle: {handle}");
        }
        Cmd::Senders => {
            for s in app.senders().await? {
                println!("{:<40} {:<24} {}", s.address.email, s.address.name.unwrap_or_default(), s.account_id);
            }
        }
        Cmd::Send { from, to, cc, bcc, subject, body, attach } => {
            let sender = find_sender(&app, &from).await?;
            let mut d = pigeon_core::compose::Draft::new(sender, pigeon_core::now_ms());
            d.to = addresses(&to);
            d.cc = addresses(&cc);
            d.bcc = addresses(&bcc);
            d.subject = subject;
            d.html = pigeon_core::text::text_to_html(&body);
            for path in attach {
                let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned());
                d.attachments.push(pigeon_core::compose::DraftAttachment {
                    name: name.unwrap_or_else(|| "attachment".into()),
                    content_type: None,
                    source: pigeon_core::compose::AttachmentSource::File { path },
                });
            }
            println!("sent {}", app.send(&d).await?);
        }
        Cmd::Reply { account, message_id, all, forward, from, to, body, dry_run } => {
            let a = app.find_account(&account)?;
            let kind = match (all, forward) {
                (_, true) => pigeon_core::compose::ReplyKind::Forward,
                (true, false) => pigeon_core::compose::ReplyKind::ReplyAll,
                _ => pigeon_core::compose::ReplyKind::Reply,
            };
            let from = match from {
                Some(f) => Some(find_sender(&app, &f).await?),
                None => None,
            };
            let mut d = app.reply(&a, &message_id, kind, from).await?;
            d.html.insert_str(0, &pigeon_core::text::text_to_html(&body));
            if !to.is_empty() {
                d.to = addresses(&to);
            }
            app.save_draft(&mut d)?;
            let list =
                |v: &[pigeon_core::model::Address]| v.iter().map(|x| x.email.clone()).collect::<Vec<_>>().join(", ");
            println!("from:    {} ({})", d.from.address.email, d.from.account_id);
            if d.crosses_accounts() {
                println!("         replying from a different account than the one that received it");
            }
            println!("to:      {}\ncc:      {}\nsubject: {}", list(&d.to), list(&d.cc), d.subject);
            if dry_run {
                println!("draft saved: {}", d.id);
            } else {
                println!("sent {}", app.send(&d).await?);
            }
        }
        Cmd::Undo { account, handle } => {
            let a = app.find_account(&account)?;
            app.undo(&a, handle)?;
            print_flush(&app.flush(&a).await?);
        }
        Cmd::EventCreate { account, title, start, duration, all_day, attendees, location, description, calendar } => {
            let a = app.find_account(&account)?;
            let (start_at, end_at) = if all_day {
                let d = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                    .map_err(|e| pigeon_core::Error::Other(format!("bad --start date: {e}")))?;
                let s = d.and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp_millis();
                (s, s + duration.max(1) * 86_400_000)
            } else {
                let naive = chrono::NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M")
                    .map_err(|e| pigeon_core::Error::Other(format!("bad --start: {e}")))?;
                let local = Local
                    .from_local_datetime(&naive)
                    .single()
                    .ok_or_else(|| pigeon_core::Error::Other("ambiguous local time".into()))?;
                let s = local.timestamp_millis();
                (s, s + duration * 60_000)
            };
            let draft = pigeon_core::model::EventDraft {
                calendar_id: match calendar {
                    Some(c) => c,
                    None => app.primary_calendar(&a)?,
                },
                title,
                start_at,
                end_at,
                all_day,
                timezone: None,
                location,
                description,
                attendees: addresses(&attendees)
                    .into_iter()
                    .map(|address| pigeon_core::model::Attendee {
                        address,
                        response: pigeon_core::model::Response::None,
                        optional: false,
                    })
                    .collect(),
                conferencing: Default::default(),
            };
            let e = app.create_event(&a, &draft).await?;
            println!("created {} in {}", e.id, e.calendar_id);
        }
        Cmd::EventDelete { account, calendar_id, event_id } => {
            let a = app.find_account(&account)?;
            app.delete_event(&a, &calendar_id, &event_id).await?;
            println!("deleted");
        }
        Cmd::Rsvp { account, calendar_id, event_id, response, comment } => {
            let a = app.find_account(&account)?;
            let r = match response.as_str() {
                "accept" | "yes" => pigeon_core::model::Response::Accepted,
                "tentative" | "maybe" => pigeon_core::model::Response::Tentative,
                "decline" | "no" => pigeon_core::model::Response::Declined,
                other => return Err(pigeon_core::Error::Other(format!("unknown response {other:?}"))),
            };
            app.rsvp(&a, &calendar_id, &event_id, r, comment.as_deref()).await?;
            println!("responded {}", r.as_str());
        }
        Cmd::Freebusy { account, emails, hours } => {
            let a = app.find_account(&account)?;
            let now = chrono::Utc::now().timestamp_millis();
            for (email, busy) in app.free_busy(&a, &emails, now, now + hours * 3_600_000).await? {
                let spans: Vec<String> = busy.iter().map(|(s, e)| format!("{}–{}", when(*s), when(*e))).collect();
                println!("{email}: {}", if spans.is_empty() { "free".into() } else { spans.join(", ") });
            }
        }
        Cmd::Events { days } => {
            let now = chrono::Utc::now();
            let to = now + chrono::Duration::days(days);
            for row in app.store().events_between(now.timestamp_millis(), to.timestamp_millis())? {
                let e = row.event;
                let time = if e.all_day { format!("{}  all-day", day(e.start_at)) } else { when(e.start_at) };
                let join = if e.online_url.is_some() { "[video]" } else { "" };
                println!(
                    "{}  {:<50} {:<9} {:<7} {}  {} {}",
                    time,
                    clip(&e.title, 50),
                    e.my_response.as_str(),
                    join,
                    short_account(&row.account_id),
                    e.calendar_id,
                    e.id
                );
            }
        }
    }
    Ok(())
}

async fn find_sender(app: &Pigeon, email: &str) -> pigeon_core::Result<pigeon_core::compose::Sender> {
    app.senders().await?.into_iter().find(|s| s.address.email.eq_ignore_ascii_case(email)).ok_or_else(|| {
        pigeon_core::Error::Other(format!("{email} is not one of your sending addresses (see `senders`)"))
    })
}

fn addresses(list: &[String]) -> Vec<pigeon_core::model::Address> {
    list.iter().flat_map(|s| pigeon_core::text::parse_address_list(s)).collect()
}

fn print_flush(r: &pigeon_core::actions::FlushReport) {
    println!("sent {}, deferred {}", r.sent, r.deferred);
    for (op, err) in &r.rejected {
        println!("rejected (rolled back): {op:?}: {err}");
    }
    if r.needs_sign_in {
        println!("sign-in required: run `pigeon login` again");
    }
}

fn print_message(h: &MessageHeader, account_id: Option<&str>) {
    let flags = format!(
        "{}{}{}",
        if h.is_read { ' ' } else { '*' },
        if h.is_flagged { '!' } else { ' ' },
        if h.has_attachments { '@' } else { ' ' }
    );
    let from = h.from.as_ref().map(|a| a.display()).unwrap_or("(unknown)");
    println!(
        "{flags} {} {:<28} {}  {}  {}",
        when(h.received_at),
        clip(from, 28),
        clip(&h.subject, 70),
        account_id.map(short_account).unwrap_or_default(),
        h.id
    );
}

fn when(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|d| d.format("%b %d %H:%M").to_string()).unwrap_or_default()
}

fn day(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%b %d").to_string()).unwrap_or_default()
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
