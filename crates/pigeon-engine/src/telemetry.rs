//! Crash and error reporting via Sentry.
//!
//! Off unless a DSN is configured (`PIGEON_SENTRY_DSN`, or baked in at build
//! time), and `PIGEON_TELEMETRY=off` always wins. Nothing personal leaves the
//! machine: no default PII, and every message, breadcrumb and tag is scrubbed
//! of email addresses and URL query strings before sending. Mail content is
//! never logged in the first place.

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sentry::protocol::{Breadcrumb, Event, Value};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub type Guard = sentry::ClientInitGuard;

fn dsn() -> Option<String> {
    if std::env::var("PIGEON_TELEMETRY").is_ok_and(|v| v.eq_ignore_ascii_case("off") || v == "0") {
        return None;
    }
    std::env::var("PIGEON_SENTRY_DSN")
        .ok()
        .or(option_env!("PIGEON_SENTRY_DSN").map(String::from))
        .filter(|d| !d.trim().is_empty())
}

/// A bug report the user wrote and chose to send (Help › Report a Bug…).
#[derive(Debug, Clone, PartialEq)]
pub struct BugReport {
    /// Their description, sent as written.
    pub what: String,
    /// Where to reply, if they gave one (sent as the event's user email).
    pub reply_to: Option<String>,
    /// App and environment details (version, macOS, settings; no content).
    pub details: Vec<(String, String)>,
}

/// Can bug reports be sent from this build (a DSN is configured)? Unlike
/// crash reports these go out even with Settings › Privacy › Crash reports
/// off: the user sends each one on purpose.
pub fn reports_available() -> bool {
    report_dsn().is_some()
}

fn report_dsn() -> Option<String> {
    std::env::var("PIGEON_SENTRY_DSN")
        .ok()
        .or(option_env!("PIGEON_SENTRY_DSN").map(String::from))
        .filter(|d| !d.trim().is_empty())
}

/// The Sentry event for a report: its own issue (fingerprinted by id),
/// tagged `user_report`, title from the first line.
pub fn report_event(r: &BugReport) -> Event<'static> {
    let id = Event::default().event_id;
    let first = r.what.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("(no description)");
    let title: String = first.chars().take(80).collect();
    let mut e = Event {
        event_id: id,
        level: sentry::Level::Info,
        message: Some(format!("Bug report: {title}")),
        fingerprint: Cow::Owned(vec![Cow::Borrowed("user-report"), Cow::Owned(id.to_string())]),
        ..Default::default()
    };
    e.tags.insert("kind".into(), "user_report".into());
    e.extra.insert("description".into(), Value::String(r.what.clone()));
    for (k, v) in &r.details {
        e.extra.insert(k.clone(), Value::String(v.clone()));
    }
    if let Some(email) = r.reply_to.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        e.user = Some(sentry::User { email: Some(email.to_string()), ..Default::default() });
    }
    e
}

/// Each log file is cut at this size; the one before is kept as `.1.log`.
const LOG_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Debug detail for Coo's own code in the log file; everything else at info.
const LOG_FILTER: &str = "info,pigeon=debug,pigeon_app=debug,pigeon_bin=debug,pigeon_core=debug,pigeon_engine=debug,\
     pigeon_graph=debug,pigeon_google=debug,pigeon_imap=debug,pigeon_caldav=debug,pigeon_zoom=debug,coo=debug";

/// Where `release` (e.g. `coo-app@0.1.60`) keeps its log: `logs/coo-app.log`
/// in the data folder.
fn log_path(release: &str) -> Option<PathBuf> {
    let name = release.split('@').next().filter(|n| !n.is_empty()).unwrap_or("coo");
    let dir = pigeon_core::paths::data_dir().ok()?.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{name}.log")))
}

fn previous(path: &Path) -> PathBuf {
    path.with_extension("1.log")
}

/// The log file. Every line is scrubbed like a Sentry event before it's
/// written, so the file never holds addresses or URL query strings, and it
/// only leaves the Mac attached to a bug report the user sends.
struct LogFile {
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
}

impl LogFile {
    fn open(path: PathBuf) -> Self {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        LogFile { path, file: Mutex::new(file) }
    }

    fn write_line(&self, buf: &[u8]) {
        let text = String::from_utf8_lossy(buf);
        let clean = scrub(&text);
        let mut f = self.file.lock().unwrap_or_else(|p| p.into_inner());
        if f.as_ref().and_then(|f| f.metadata().ok()).is_some_and(|m| m.len() > LOG_MAX_BYTES) {
            *f = None;
            let _ = std::fs::rename(&self.path, previous(&self.path));
            *f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).ok();
        }
        if let Some(f) = f.as_mut() {
            let _ = f.write_all(clean.as_bytes());
        }
    }
}

struct LogWriter(Arc<LogFile>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write_line(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The debug log to attach to a bug report: the previous file, then the
/// current one, scrubbed again (older builds wrote unscrubbed lines).
pub fn debug_log(release: &str) -> Option<Vec<u8>> {
    let path = log_path(release)?;
    let mut out = String::new();
    for p in [previous(&path), path] {
        if let Ok(bytes) = std::fs::read(&p) {
            out.push_str(&scrub(&String::from_utf8_lossy(&bytes)));
        }
    }
    (!out.is_empty()).then(|| out.into_bytes())
}

/// Send a report on a one-off client (blocking; run off the UI thread),
/// with the debug log attached when there is one.
pub fn send_report(release: &'static str, r: &BugReport) -> Result<(), String> {
    let dsn = report_dsn().ok_or("Bug reports aren't set up in this build.")?;
    let mut opts = sentry::ClientOptions::default();
    opts.release = Some(Cow::Borrowed(release));
    opts.environment = Some(Cow::Borrowed(if cfg!(debug_assertions) { "development" } else { "production" }));
    opts.send_default_pii = false;
    let client = sentry::Client::from((dsn, opts));
    let mut scope = sentry::Scope::default();
    if let Some(log) = debug_log(release) {
        scope.add_attachment(sentry::protocol::Attachment {
            buffer: log,
            filename: "coo-debug.log".into(),
            content_type: Some("text/plain".into()),
            ty: None,
        });
    }
    client.capture_event(report_event(r), Some(&scope));
    if client.flush(Some(std::time::Duration::from_secs(10))) {
        Ok(())
    } else {
        Err("Couldn't send the report. Check your internet connection and try again.".into())
    }
}

/// Whether a Sentry client is active in this process.
pub fn enabled() -> bool {
    sentry::Hub::current().client().is_some_and(|c| c.is_enabled())
}

/// Set up logging (stderr) and, when configured, Sentry. `release` is e.g.
/// `pigeon-app@0.1.0`. Keep the guard alive for the life of the process so
/// pending events flush on exit.
pub fn init(release: &'static str, default_filter: &str) -> Option<Guard> {
    let guard = dsn().map(|dsn| {
        let mut opts = sentry::ClientOptions::default();
        opts.release = Some(Cow::Borrowed(release));
        opts.environment = Some(Cow::Borrowed(if cfg!(debug_assertions) { "development" } else { "production" }));
        opts.send_default_pii = false;
        opts.attach_stacktrace = true;
        opts.before_send = Some(Arc::new(|mut e| {
            scrub_event(&mut e);
            Some(e)
        }));
        opts.before_breadcrumb = Some(Arc::new(|mut b| {
            scrub_breadcrumb(&mut b);
            Some(b)
        }));
        sentry::init((dsn, opts))
    });

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter.into());
    let fmt = tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_filter(filter);
    // error! becomes a Sentry event; warn!/info! become breadcrumbs on it.
    let sentry_layer = guard.as_ref().map(|_| {
        sentry::integrations::tracing::layer()
            .event_filter(|md| match *md.level() {
                tracing::Level::ERROR => sentry::integrations::tracing::EventFilter::Event,
                tracing::Level::WARN | tracing::Level::INFO => sentry::integrations::tracing::EventFilter::Breadcrumb,
                _ => sentry::integrations::tracing::EventFilter::Ignore,
            })
            .with_filter(EnvFilter::new(
                "pigeon=info,pigeon_core=info,pigeon_engine=info,pigeon_graph=info,pigeon_google=info,pigeon_bin=info",
            ))
    });
    // Everything Coo does, in detail, to a local file (scrubbed); sent only
    // with a bug report.
    let file_layer = log_path(release).map(|path| {
        let file = Arc::new(LogFile::open(path));
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_thread_names(true)
            .with_writer(move || LogWriter(file.clone()))
            .with_filter(EnvFilter::new(LOG_FILTER))
    });
    let _ = tracing_subscriber::registry().with(fmt).with(sentry_layer).with(file_layer).try_init();
    tracing::info!(release, "started");

    if guard.is_some() {
        tracing::info!(release, "crash reporting on");
    }
    guard
}

fn is_local(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '\'')
}

fn is_domain(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '-')
}

/// Replace email addresses with `<email>` and drop URL query strings.
pub fn scrub(s: &str) -> Cow<'_, str> {
    if !s.contains('@') && !s.contains('?') {
        return Cow::Borrowed(s);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '@' {
            // Walk back over the local part already copied to `out`.
            let mut start = out.len();
            for (idx, ch) in out.char_indices().rev() {
                if is_local(ch) {
                    start = idx;
                } else {
                    break;
                }
            }
            let mut end = i + 1;
            while end < chars.len() && is_domain(chars[end]) {
                end += 1;
            }
            let domain: String = chars[i + 1..end].iter().collect();
            let domain = domain.trim_end_matches('.');
            // A real domain ends in letters; `coo-app@0.1.60` is a version.
            let tld = domain.rsplit('.').next().unwrap_or_default();
            if start < out.len() && domain.contains('.') && tld.chars().any(char::is_alphabetic) {
                out.truncate(start);
                out.push_str("<email>");
                i += 1 + domain.chars().count();
                continue;
            }
        }
        if c == '?' {
            // Query string of a URL: drop until whitespace/quote/bracket.
            let tail: String = out.chars().rev().take_while(|ch| !ch.is_whitespace()).collect();
            if tail.contains("//") || tail.starts_with('/') || tail.ends_with('/') {
                out.push_str("?<redacted>");
                while i < chars.len() && !(chars[i].is_whitespace() || matches!(chars[i], '"' | '\'' | ')' | ']' | '>'))
                {
                    i += 1;
                }
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    Cow::Owned(out)
}

fn scrub_value(v: &mut Value) {
    match v {
        Value::String(s) => {
            if let Cow::Owned(new) = scrub(s) {
                *s = new;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(scrub_value),
        Value::Object(o) => o.values_mut().for_each(scrub_value),
        _ => {}
    }
}

fn scrub_opt(s: &mut Option<String>) {
    if let Some(v) = s
        && let Cow::Owned(new) = scrub(v)
    {
        *v = new;
    }
}

/// Panic messages quote the offending data in backticks (e.g. a slice of
/// an email snippet); keep the shape, drop the content.
pub fn redact_quoted(s: &str) -> Cow<'_, str> {
    if s.matches('`').count() < 2 {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for part in s.split('`') {
        if inside {
            out.push_str("`<redacted>`");
        } else {
            out.push_str(part);
        }
        inside = !inside;
    }
    Cow::Owned(out)
}

fn redact_opt(s: &mut Option<String>) {
    if let Some(v) = s
        && let Cow::Owned(new) = redact_quoted(v)
    {
        *v = new;
    }
}

pub fn scrub_event(e: &mut Event<'static>) {
    redact_opt(&mut e.message);
    for ex in e.exception.values.iter_mut() {
        redact_opt(&mut ex.value);
    }
    scrub_opt(&mut e.message);
    if let Some(le) = e.logentry.as_mut() {
        le.message = scrub(&le.message).into_owned();
    }
    for ex in e.exception.values.iter_mut() {
        scrub_opt(&mut ex.value);
    }
    for v in e.extra.values_mut() {
        scrub_value(v);
    }
    // tracing fields (e.g. `error = %e`) land in contexts, not extra.
    for c in e.contexts.values_mut() {
        if let sentry::protocol::Context::Other(map) = c {
            for v in map.values_mut() {
                scrub_value(v);
            }
        }
    }
    for v in e.tags.values_mut() {
        *v = scrub(v).into_owned();
    }
    for b in e.breadcrumbs.values.iter_mut() {
        scrub_breadcrumb(b);
    }
    e.user = None;
    e.server_name = None;
}

pub fn scrub_breadcrumb(b: &mut Breadcrumb) {
    scrub_opt(&mut b.message);
    for v in b.data.values_mut() {
        scrub_value(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_file_is_scrubbed_and_cut() {
        let dir = std::env::temp_dir().join(format!("coo-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("coo-test.log");
        let log = LogFile::open(path.clone());
        log.write_line(b"sync done for google:someone@example.com https://x.com/a?token=abc\n");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("someone@example.com") && !text.contains("token=abc"), "{text}");
        assert!(text.contains("<email>"));
        assert_eq!(scrub("release=coo-app@0.1.60"), "release=coo-app@0.1.60");
        let big = vec![b'x'; LOG_MAX_BYTES as usize + 1];
        log.write_line(&big);
        log.write_line(b"after\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
        assert!(previous(&path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrubs_emails() {
        assert_eq!(scrub("sync failed for sam@halyard.co: 503"), "sync failed for <email>: 503");
        assert_eq!(scrub("google:dana.cohen+x@mail.co.il"), "google:<email>");
        assert_eq!(scrub("<a@b.com>, c.d@e.org"), "<<email>>, <email>");
        assert_eq!(scrub("price @ 5pm"), "price @ 5pm");
        assert_eq!(scrub("user@localhost"), "user@localhost");
        assert!(matches!(scrub("nothing here"), Cow::Borrowed(_)));
    }

    #[test]
    fn scrubs_url_queries() {
        assert_eq!(
            scrub("503 from https://graph.microsoft.com/v1.0/me/messages/delta?$skiptoken=abc&x=1: busy"),
            "503 from https://graph.microsoft.com/v1.0/me/messages/delta?<redacted> busy"
        );
        assert_eq!(scrub("what? really?"), "what? really?");
    }

    #[test]
    fn tracing_field_contexts_are_scrubbed() {
        let mut e = Event::default();
        let mut fields = sentry::protocol::Map::new();
        fields.insert("error".into(), "a.b@gmail.com: 403 from https://x.com/m/1?format=metadata".into());
        e.contexts.insert("Rust Tracing Fields".into(), sentry::protocol::Context::Other(fields));
        scrub_event(&mut e);
        let sentry::protocol::Context::Other(m) = &e.contexts["Rust Tracing Fields"] else { panic!() };
        let s = m["error"].as_str().unwrap();
        assert!(!s.contains("a.b@gmail.com") && !s.contains("format=metadata"), "{s}");
    }

    #[test]
    fn panic_payloads_are_redacted() {
        let msg = "byte index 12 is not a char boundary; it is inside 'ת' (bytes 11..13) of `שם הפונה: דנה 0500000000`";
        assert_eq!(
            redact_quoted(msg),
            "byte index 12 is not a char boundary; it is inside 'ת' (bytes 11..13) of `<redacted>`"
        );
        let mut e = Event::default();
        e.exception.values.push(sentry::protocol::Exception { value: Some(msg.into()), ..Default::default() });
        scrub_event(&mut e);
        assert!(!e.exception.values[0].value.as_deref().unwrap().contains("0500000000"));
        assert_eq!(redact_quoted("no quotes"), "no quotes");
    }

    #[test]
    fn scrubs_events_and_breadcrumbs() {
        let mut e = Event { message: Some("failed for me@x.com".into()), ..Default::default() };
        e.extra.insert("account".into(), Value::String("google:me@x.com".into()));
        e.breadcrumbs.values.push(Breadcrumb { message: Some("to a@b.io".into()), ..Default::default() });
        scrub_event(&mut e);
        assert_eq!(e.message.as_deref(), Some("failed for <email>"));
        assert_eq!(e.extra["account"], Value::String("google:<email>".into()));
        assert_eq!(e.breadcrumbs.values[0].message.as_deref(), Some("to <email>"));
    }

    #[test]
    fn bug_reports_are_their_own_issue_with_the_reply_address() {
        let r = BugReport {
            what: "\n  List jumps when archiving\nsteps: …".into(),
            reply_to: Some(" me@example.com ".into()),
            details: vec![("version".into(), "0.1.12 (190)".into())],
        };
        let e = report_event(&r);
        assert_eq!(e.message.as_deref(), Some("Bug report: List jumps when archiving"));
        assert_eq!(e.tags.get("kind").map(String::as_str), Some("user_report"));
        assert_eq!(e.fingerprint.len(), 2);
        assert_eq!(e.user.as_ref().and_then(|u| u.email.as_deref()), Some("me@example.com"));
        assert_eq!(e.extra.get("version"), Some(&Value::String("0.1.12 (190)".into())));
        let anon = report_event(&BugReport { reply_to: Some("  ".into()), ..r });
        assert!(anon.user.is_none());
    }
}
