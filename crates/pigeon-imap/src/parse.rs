//! Pure helpers: ids, cursors, folder roles, header mapping, search and
//! SMTP envelope handling. Everything here is unit-tested without a server.

use mail_parser::{MessageParser, MimeHeaders};
use pigeon_core::model::{Address, FolderRole, MessageHeader};
use pigeon_core::search::SearchQuery;
use pigeon_core::{Error, Result};

/// Message id: `{uidvalidity}:{uid}:{mailbox}`. IMAP ids are per folder; a
/// move is an expunge here plus a new UID there.
pub fn message_id(uidvalidity: u32, uid: u32, mailbox: &str) -> String {
    format!("{uidvalidity}:{uid}:{mailbox}")
}

/// (uidvalidity, uid, mailbox).
pub fn split_id(id: &str) -> Result<(u32, u32, &str)> {
    let mut it = id.splitn(3, ':');
    let bad = || Error::Other(format!("not an IMAP message id: {id}"));
    let v = it.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
    let u = it.next().and_then(|u| u.parse().ok()).ok_or_else(bad)?;
    let m = it.next().filter(|m| !m.is_empty()).ok_or_else(bad)?;
    Ok((v, u, m))
}

/// Per-folder sync position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// First sync, newest first: everything below `below` (exclusive) in
    /// the window is still to fetch; `top` is the highest UID at the start.
    Boot { uidvalidity: u32, below: u32, top: u32 },
    /// Caught up through `max_uid`.
    Sync { uidvalidity: u32, max_uid: u32 },
}

impl Cursor {
    pub fn parse(s: &str) -> Result<Cursor> {
        let parts: Vec<&str> = s.split(':').collect();
        let n = |i: usize| parts.get(i).and_then(|v| v.parse::<u32>().ok());
        let bad = || Error::Other(format!("bad IMAP cursor {s}"));
        Ok(match parts.first().copied() {
            Some("boot") => Cursor::Boot {
                uidvalidity: n(1).ok_or_else(bad)?,
                below: n(2).ok_or_else(bad)?,
                top: n(3).ok_or_else(bad)?,
            },
            Some("sync") => Cursor::Sync { uidvalidity: n(1).ok_or_else(bad)?, max_uid: n(2).ok_or_else(bad)? },
            _ => return Err(bad()),
        })
    }

    pub fn uidvalidity(&self) -> u32 {
        match *self {
            Cursor::Boot { uidvalidity, .. } | Cursor::Sync { uidvalidity, .. } => uidvalidity,
        }
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Cursor::Boot { uidvalidity, below, top } => write!(f, "boot:{uidvalidity}:{below}:{top}"),
            Cursor::Sync { uidvalidity, max_uid } => write!(f, "sync:{uidvalidity}:{max_uid}"),
        }
    }
}

/// Role from RFC 6154 SPECIAL-USE attributes, falling back to the usual
/// names (iCloud: "Sent Messages", "Deleted Messages", "Junk").
pub fn role_for(name: &str, special: &[&str]) -> Option<FolderRole> {
    for s in special {
        let r = match s.to_ascii_lowercase().as_str() {
            "\\sent" => Some(FolderRole::Sent),
            "\\drafts" => Some(FolderRole::Drafts),
            "\\trash" => Some(FolderRole::Trash),
            "\\junk" => Some(FolderRole::Spam),
            "\\archive" => Some(FolderRole::Archive),
            "\\flagged" => Some(FolderRole::Starred),
            _ => None,
        };
        if r.is_some() {
            return r;
        }
    }
    let leaf = name.rsplit(['/', '.']).next().unwrap_or(name).to_ascii_lowercase();
    match leaf.as_str() {
        "inbox" => Some(FolderRole::Inbox),
        "sent" | "sent messages" | "sent items" | "sent mail" => Some(FolderRole::Sent),
        "drafts" => Some(FolderRole::Drafts),
        "trash" | "deleted messages" | "deleted items" | "bin" => Some(FolderRole::Trash),
        "junk" | "spam" | "junk e-mail" | "bulk mail" => Some(FolderRole::Spam),
        "archive" | "archives" => Some(FolderRole::Archive),
        _ => None,
    }
}

/// Parent mailbox of `name` given the hierarchy delimiter.
pub fn parent_of(name: &str, delimiter: Option<&str>) -> Option<String> {
    let d = delimiter.filter(|d| !d.is_empty())?;
    name.rsplit_once(d).map(|(p, _)| p.to_string())
}

/// Thread id from References / In-Reply-To: the conversation's root
/// message-id, so replies across folders land in one thread.
pub fn thread_id(references: &[String], in_reply_to: Option<&str>, message_id: Option<&str>, fallback: &str) -> String {
    references
        .first()
        .map(String::as_str)
        .or(in_reply_to)
        .or(message_id)
        .map(|m| m.trim().trim_start_matches('<').trim_end_matches('>').to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn addrs(a: Option<&mail_parser::Address>) -> Vec<Address> {
    a.map(|a| a.iter().filter_map(|x| Some(Address::new(x.name(), x.address()?))).collect()).unwrap_or_default()
}

fn ids(v: &mail_parser::HeaderValue) -> Vec<String> {
    if let Some(list) = v.as_text_list() {
        return list.iter().map(|s| format!("<{}>", s.trim_start_matches('<').trim_end_matches('>'))).collect();
    }
    v.as_text().map(|s| vec![format!("<{}>", s.trim_start_matches('<').trim_end_matches('>'))]).unwrap_or_default()
}

/// Flags as seen in a FETCH, for the header mapping.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub draft: bool,
}

/// A header block (`BODY.PEEK[HEADER]`) to our model.
pub fn header_from_raw(
    raw: &[u8],
    id: String,
    folder_id: &str,
    flags: Flags,
    internal_date: Option<i64>,
) -> MessageHeader {
    let parsed = MessageParser::default().parse_headers(raw);
    let (subject, from, to, cc, message_id, in_reply_to, references, date, multipart_mixed) = match &parsed {
        Some(m) => (
            m.subject().unwrap_or_default().to_string(),
            addrs(m.from()).into_iter().next(),
            addrs(m.to()),
            addrs(m.cc()),
            m.message_id().map(|i| format!("<{i}>")),
            ids(m.in_reply_to()).into_iter().next(),
            ids(m.references()),
            m.date().map(|d| d.to_timestamp() * 1000),
            m.root_part().is_content_type("multipart", "mixed"),
        ),
        None => (String::new(), None, vec![], vec![], None, None, vec![], None, false),
    };
    MessageHeader {
        thread_id: thread_id(&references, in_reply_to.as_deref(), message_id.as_deref(), &id),
        internet_message_id: message_id,
        subject,
        from,
        to,
        cc,
        snippet: String::new(),
        received_at: internal_date.or(date).unwrap_or(0),
        is_read: flags.seen,
        is_flagged: flags.flagged,
        has_attachments: multipart_mixed,
        is_draft: flags.draft,
        folder_ids: vec![folder_id.to_string()],
        id,
    }
}

/// A short plain-text preview from a full message.
pub fn snippet(raw: &[u8]) -> String {
    let Some(m) = MessageParser::default().parse(raw) else { return String::new() };
    let text = m
        .body_text(0)
        .map(|t| t.into_owned())
        .or_else(|| m.body_html(0).map(|h| pigeon_core::text::html_to_text(&h)))
        .unwrap_or_default();
    pigeon_core::text::preview_text(&text).chars().take(200).collect()
}

fn imap_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn imap_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%-d-%b-%Y").to_string()).unwrap_or_default()
}

/// UID SEARCH criteria for a parsed query (ASCII terms; non-ASCII needs
/// `CHARSET UTF-8`, which the caller adds when this returns true).
pub fn search_criteria(q: &SearchQuery) -> (String, bool) {
    let mut c: Vec<String> = Vec::new();
    for t in &q.text {
        c.push(format!("TEXT {}", imap_quote(t)));
    }
    for f in &q.from {
        c.push(format!("FROM {}", imap_quote(f)));
    }
    for t in &q.to {
        c.push(format!("TO {}", imap_quote(t)));
    }
    match q.unread {
        Some(true) => c.push("UNSEEN".into()),
        Some(false) => c.push("SEEN".into()),
        None => {}
    }
    if q.flagged {
        c.push("FLAGGED".into());
    }
    if let Some(b) = q.before {
        c.push(format!("BEFORE {}", imap_date(b)));
    }
    if let Some(a) = q.after {
        c.push(format!("SINCE {}", imap_date(a)));
    }
    if c.is_empty() {
        c.push("ALL".into());
    }
    let joined = c.join(" ");
    let utf8 = !joined.is_ascii();
    (joined, utf8)
}

/// `SINCE` date for the sync window.
pub fn since(ms: i64) -> String {
    imap_date(ms)
}

/// Compact a UID list to an IMAP set (`1:4,7,9:10`).
pub fn uid_set(uids: &[u32]) -> String {
    let mut v = uids.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        let mut end = start;
        while i + 1 < v.len() && v[i + 1] == end + 1 {
            i += 1;
            end = v[i];
        }
        out.push(if start == end { start.to_string() } else { format!("{start}:{end}") });
        i += 1;
    }
    out.join(",")
}

/// SMTP envelope recipients from To/Cc/Bcc, and the message with Bcc
/// headers removed (Bcc recipients must not see each other).
pub fn envelope(mime: &[u8]) -> Result<(Option<String>, Vec<String>, Vec<u8>)> {
    let m = MessageParser::default().parse_headers(mime).ok_or_else(|| Error::Other("unreadable MIME".into()))?;
    let from = addrs(m.from()).into_iter().next().map(|a| a.email);
    let mut rcpt: Vec<String> = Vec::new();
    for a in addrs(m.to()).into_iter().chain(addrs(m.cc())).chain(addrs(m.bcc())) {
        if !rcpt.iter().any(|r| r.eq_ignore_ascii_case(&a.email)) {
            rcpt.push(a.email);
        }
    }
    Ok((from, rcpt, strip_bcc(mime)))
}

/// Drop `Bcc:` header fields (with continuation lines) from the header block.
pub fn strip_bcc(mime: &[u8]) -> Vec<u8> {
    let split = mime.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 2).unwrap_or(mime.len());
    let (head, body) = mime.split_at(split);
    let head = String::from_utf8_lossy(head);
    let mut out = String::with_capacity(head.len());
    let mut skipping = false;
    for line in head.split_inclusive("\r\n") {
        let continuation = line.starts_with(' ') || line.starts_with('\t');
        if continuation {
            if !skipping {
                out.push_str(line);
            }
            continue;
        }
        skipping = line.len() >= 4 && line[..4].eq_ignore_ascii_case("bcc:");
        if !skipping {
            out.push_str(line);
        }
    }
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// SMTP DATA transparency (RFC 5321 4.5.2): a leading `.` is doubled.
pub fn dot_stuff(mime: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(mime.len() + 16);
    let mut at_line_start = true;
    for &b in mime {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    if !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// (code, is_last_line) from one SMTP reply line like `250-PIPELINING`.
pub fn smtp_line(line: &str) -> Option<(u16, bool)> {
    let code: u16 = line.get(..3)?.parse().ok()?;
    Some((code, line.as_bytes().get(3) != Some(&b'-')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_cursors_roundtrip() {
        let id = message_id(1700, 42, "Archive/2026: Q4");
        assert_eq!(split_id(&id).unwrap(), (1700, 42, "Archive/2026: Q4"));
        assert!(split_id("x:1:INBOX").is_err() && split_id("1:2:").is_err());
        for c in [Cursor::Boot { uidvalidity: 9, below: 300, top: 512 }, Cursor::Sync { uidvalidity: 9, max_uid: 512 }]
        {
            assert_eq!(Cursor::parse(&c.to_string()).unwrap(), c);
        }
        assert!(Cursor::parse("nope:1").is_err());
    }

    #[test]
    fn roles_from_special_use_and_icloud_names() {
        assert_eq!(role_for("Sent Messages", &[]), Some(FolderRole::Sent));
        assert_eq!(role_for("Deleted Messages", &[]), Some(FolderRole::Trash));
        assert_eq!(role_for("Whatever", &["\\Junk"]), Some(FolderRole::Spam));
        assert_eq!(role_for("INBOX", &[]), Some(FolderRole::Inbox));
        assert_eq!(role_for("Projects/Sent", &[]), Some(FolderRole::Sent));
        assert_eq!(role_for("Receipts", &[]), None);
        assert_eq!(parent_of("Projects/Acme", Some("/")), Some("Projects".into()));
        assert_eq!(parent_of("INBOX", Some("/")), None);
    }

    #[test]
    fn header_mapping_with_hebrew_and_threading() {
        let raw = "From: =?windows-1255?B?4/Dk?= <dana@x.co.il>\r\nTo: me@icloud.com, Other <o@y.com>\r\n\
            Subject: =?UTF-8?B?16nXnNeV150=?=\r\nMessage-ID: <c@x>\r\nIn-Reply-To: <b@x>\r\n\
            References: <a@x> <b@x>\r\nDate: Fri, 23 Oct 2026 10:00:00 +0000\r\n\
            Content-Type: multipart/mixed; boundary=zz\r\n\r\n";
        let h = header_from_raw(
            raw.as_bytes(),
            message_id(1, 5, "INBOX"),
            "INBOX",
            Flags { seen: false, flagged: true, draft: false },
            None,
        );
        assert_eq!(h.subject, "שלום");
        assert_eq!(h.from.as_ref().unwrap().email, "dana@x.co.il");
        assert_eq!(h.from.as_ref().unwrap().name.as_deref(), Some("דנה"), "windows-1255 encoded word");
        assert_eq!(h.to.len(), 2);
        assert_eq!(h.thread_id, "a@x");
        assert_eq!(h.internet_message_id.as_deref(), Some("<c@x>"));
        assert!(h.has_attachments && h.is_flagged && !h.is_read);
        assert_eq!(h.received_at, 1_792_749_600_000);
        assert_eq!(h.folder_ids, ["INBOX"]);
        let lone = header_from_raw(b"Subject: hi\r\n\r\n", "1:9:INBOX".into(), "INBOX", Flags::default(), Some(7));
        assert_eq!((lone.thread_id.as_str(), lone.received_at), ("1:9:INBOX", 7));
    }

    #[test]
    fn search_and_sets() {
        let q = pigeon_core::search::parse(r#"invoice from:dana is:unread after:2026-01-02"#);
        assert_eq!(search_criteria(&q), (r#"TEXT "invoice" FROM "dana" UNSEEN SINCE 2-Jan-2026"#.into(), false));
        assert!(search_criteria(&pigeon_core::search::parse("שלום")).1);
        assert_eq!(uid_set(&[9, 1, 2, 3, 4, 7, 10, 2]), "1:4,7,9:10");
        assert_eq!(since(1_792_749_600_000), "23-Oct-2026");
    }

    #[test]
    fn envelope_strips_bcc_and_stuffs_dots() {
        let mime = b"From: Me <me@icloud.com>\r\nTo: a@x.com\r\nCc: b@x.com, A@x.com\r\nBcc: secret@z.com,\r\n hidden@z.com\r\nSubject: s\r\n\r\nhi\r\n.line\r\n";
        let (from, rcpt, clean) = envelope(mime).unwrap();
        assert_eq!(from.as_deref(), Some("me@icloud.com"));
        assert_eq!(rcpt, ["a@x.com", "b@x.com", "secret@z.com", "hidden@z.com"]);
        let clean = String::from_utf8(clean).unwrap();
        assert!(!clean.contains("Bcc") && !clean.contains("hidden@z.com"));
        assert!(clean.contains("Subject: s\r\n\r\nhi"));
        assert_eq!(dot_stuff(b".a\r\nb\r\n.c"), b"..a\r\nb\r\n..c\r\n");
        assert_eq!(smtp_line("250-PIPELINING"), Some((250, false)));
        assert_eq!(smtp_line("235 2.7.0 ok"), Some((235, true)));
    }
}
