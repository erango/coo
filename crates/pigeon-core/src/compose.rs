//! Drafts, reply/forward construction, and MIME building.
//!
//! The From address is just a field on the draft: a reply to mail that came
//! in on account A can go out from account B. Threading survives because we
//! build the MIME ourselves with `In-Reply-To` / `References`.

use serde::{Deserialize, Serialize};

use crate::model::{Account, Address, MessageHeader, StoredAttachment, StoredBody};
use crate::text::{escape_html, html_to_text, text_to_html};
use crate::{Error, Result, auth};

/// An address we can send as, and the account that sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sender {
    pub account_id: String,
    pub address: Address,
}

impl Sender {
    /// The account's own address (not an alias), under its display name.
    pub fn primary(account: &Account) -> Sender {
        Sender { account_id: account.id.clone(), address: Address::new(Some(&account.display_name), &account.email) }
    }
}

/// Domains of iCloud Mail addresses.
pub const ICLOUD_DOMAINS: [&str; 3] = ["icloud.com", "me.com", "mac.com"];

/// Whether `email` is an iCloud Mail address (not an Apple ID at another
/// provider).
pub fn is_icloud_mail(email: &str) -> bool {
    email.trim().rsplit_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && ICLOUD_DOMAINS.iter().any(|d| domain.eq_ignore_ascii_case(d))
    })
}

/// A bare `name@domain.tld` (what a send-from address typed by hand must be).
pub fn is_plain_address(email: &str) -> bool {
    let bad = |c: char| c.is_whitespace() || "<>,;:\"()[]\\".contains(c);
    match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.contains('@')
                && domain.split('.').count() >= 2
                && domain.split('.').all(|p| !p.is_empty())
                && !email.chars().any(bad)
        }
        None => false,
    }
}

/// An iCloud account's send-from addresses: the ones added by hand (oldest
/// first), then the detected ones (most used first), minus the removed
/// ones; lowercased, no duplicates.
pub fn icloud_send_addresses(detected: &[String], overrides: &[(String, bool)]) -> Vec<String> {
    let removed: Vec<String> = overrides.iter().filter(|(_, r)| *r).map(|(e, _)| e.to_lowercase()).collect();
    let mut out: Vec<String> = Vec::new();
    let added = overrides.iter().filter(|(_, r)| !*r).map(|(e, _)| e);
    for e in added.chain(detected) {
        let e = e.trim().to_lowercase();
        if !e.is_empty() && !removed.contains(&e) && !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// Who an iCloud account sends as. Its SMTP only accepts its iCloud Mail
/// addresses and aliases: when the Apple ID is an address at another
/// provider and one of those is known, the Apple ID isn't offered and the
/// first known address comes first. Otherwise the account's own address
/// leads, followed by the rest.
pub fn icloud_senders(account: &Account, addresses: &[String]) -> Vec<Sender> {
    let at = |email: &str| Sender {
        account_id: account.id.clone(),
        address: Address::new(Some(&account.display_name), email),
    };
    let own = account.email.to_lowercase();
    let mut out = Vec::new();
    if is_icloud_mail(&own) || addresses.is_empty() {
        out.push(Sender::primary(account));
    }
    out.extend(addresses.iter().filter(|e| e.to_lowercase() != own).map(|e| at(e)));
    out
}

/// One account's senders with its main address (the one the user picked to
/// go by) first; unchanged when it's unset or no longer offered.
pub fn lead_with_main(mut senders: Vec<Sender>, main: Option<&str>) -> Vec<Sender> {
    if let Some(i) = main.and_then(|m| senders.iter().position(|s| s.address.email.eq_ignore_ascii_case(m))) {
        let s = senders.remove(i);
        senders.insert(0, s);
    }
    senders
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyKind {
    Reply,
    ReplyAll,
    Forward,
}

/// What the draft answers; kept independent of the sending account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyContext {
    pub kind: ReplyKind,
    pub account_id: String,
    pub message_id: String,
    pub thread_id: String,
    pub internet_message_id: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftAttachment {
    pub name: String,
    pub content_type: Option<String>,
    pub source: AttachmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentSource {
    File {
        path: String,
    },
    /// Carried over from the message being forwarded or replied to;
    /// fetched from that account's server at send time.
    Message {
        account_id: String,
        message_id: String,
        attachment_id: String,
        /// An image the quoted original shows inline (`cid:` in its HTML):
        /// it goes out as an inline part under the same Content-ID and has
        /// no chip in the composer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_id: Option<String>,
        /// For the composer's chip only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
    },
    /// A file picked from Google Drive, fetched with that Google account's
    /// sign-in at send time (straight into the message, never to disk).
    /// `export_mime` exports a Google Docs/Sheets/Slides file to that format.
    Drive {
        account_id: String,
        file_id: String,
        #[serde(default)]
        export_mime: Option<String>,
        /// For the composer's chip only (unknown for exports).
        #[serde(default)]
        size: Option<u64>,
    },
}

impl AttachmentSource {
    pub fn is_drive(&self) -> bool {
        matches!(self, AttachmentSource::Drive { .. })
    }

    /// The Content-ID of an inline image carried over with a quote.
    pub fn inline_cid(&self) -> Option<&str> {
        match self {
            AttachmentSource::Message { content_id, .. } => content_id.as_deref(),
            _ => None,
        }
    }
}

/// The original's attachments a reply or forward carries, fetched at send
/// time. Inline images the quoted HTML shows (`cid:`) always go along, so
/// the quote isn't left with broken images. A forward also carries every
/// other file, including parts marked inline that the HTML never shows
/// (Apple Mail sends ordinary attachments that way).
pub fn carried_attachments(
    account_id: &str,
    message_id: &str,
    quoted_html: &str,
    attachments: &[StoredAttachment],
    forward: bool,
) -> Vec<DraftAttachment> {
    let lower = quoted_html.to_ascii_lowercase();
    let shown = |cid: &str| lower.contains(&format!("cid:{}", cid.to_ascii_lowercase()));
    attachments
        .iter()
        .filter_map(|a| {
            let cid = a.meta.content_id.as_deref().filter(|c| a.meta.is_inline && !c.is_empty() && shown(c));
            (cid.is_some() || forward).then(|| DraftAttachment {
                name: a.meta.name.clone(),
                content_type: a.meta.content_type.clone(),
                source: AttachmentSource::Message {
                    account_id: account_id.to_string(),
                    message_id: message_id.to_string(),
                    attachment_id: a.meta.id.clone(),
                    content_id: cid.map(String::from),
                    size: a.meta.size,
                },
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub from: Sender,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    /// Composer HTML. Paragraphs carry `dir="auto"` or an explicit override.
    pub html: String,
    pub attachments: Vec<DraftAttachment>,
    pub reply: Option<ReplyContext>,
    pub updated_at: i64,
    /// What the user typed in the composer. `html` holds the quoted
    /// original (replies/forwards) that follows it; see [`Draft::full_html`].
    #[serde(default)]
    pub body_html: String,
    /// The copy saved on the From account's server, if any.
    #[serde(default)]
    pub server: Option<ServerDraft>,
    /// Send at this time (ms): a chosen "send later" time, or the end of the
    /// undo-send window.
    #[serde(default)]
    pub scheduled_at: Option<i64>,
    /// The schedule is the undo-send grace period, not a time the user picked.
    #[serde(default)]
    pub undo_window: bool,
}

/// A draft's server-side copy (Gmail draft id, Graph message id, IMAP id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerDraft {
    pub account_id: String,
    pub id: String,
}

impl Draft {
    pub fn new(from: Sender, now: i64) -> Self {
        Draft {
            id: auth::random_token(12),
            from,
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            html: String::new(),
            attachments: Vec::new(),
            reply: None,
            updated_at: now,
            body_html: String::new(),
            server: None,
            scheduled_at: None,
            undo_window: false,
        }
    }

    /// The message body as sent: typed text, then the quoted original.
    pub fn full_html(&self) -> String {
        format!("{}{}", self.body_html, self.html)
    }

    /// Change the sending address/account; everything else stays.
    pub fn switch_from(&mut self, from: Sender) {
        self.from = from;
    }

    /// The provider thread to file the sent message into: only meaningful
    /// when replying from the same account that received the original.
    pub fn thread_hint(&self) -> Option<&str> {
        let r = self.reply.as_ref()?;
        (r.account_id == self.from.account_id && r.kind != ReplyKind::Forward).then_some(r.thread_id.as_str())
    }

    /// Replying from a different account than the one the original reached.
    pub fn crosses_accounts(&self) -> bool {
        self.reply.as_ref().is_some_and(|r| r.account_id != self.from.account_id)
    }
}

/// Scheduled drafts whose time has come, earliest first.
pub fn due_drafts(drafts: Vec<Draft>, now: i64) -> Vec<Draft> {
    let mut due: Vec<Draft> = drafts.into_iter().filter(|d| d.scheduled_at.is_some_and(|t| t <= now)).collect();
    due.sort_by_key(|d| d.scheduled_at);
    due
}

/// Default sender for answering `original` (received on `account_id`): the
/// address of that account it was sent to, else the account's primary.
pub fn default_sender<'a>(account_id: &str, original: &MessageHeader, senders: &'a [Sender]) -> Option<&'a Sender> {
    let mine: Vec<&Sender> = senders.iter().filter(|s| s.account_id == account_id).collect();
    let addressed =
        |s: &&&Sender| original.to.iter().chain(&original.cc).any(|a| a.email.eq_ignore_ascii_case(&s.address.email));
    mine.iter().find(addressed).or(mine.first()).copied()
}

fn prefixed(prefix: &str, subject: &str) -> String {
    let s = subject.trim();
    let lower = s.to_lowercase();
    let already = match prefix {
        "Re:" => lower.starts_with("re:"),
        _ => lower.starts_with("fwd:") || lower.starts_with("fw:"),
    };
    if already { s.to_string() } else { format!("{prefix} {s}") }
}

fn quote_header(original: &MessageHeader) -> String {
    let when = chrono::DateTime::from_timestamp_millis(original.received_at)
        .map(|d| d.format("%a, %b %-d, %Y at %H:%M UTC").to_string())
        .unwrap_or_default();
    let who = original.from.as_ref().map(|a| match &a.name {
        Some(n) => format!("{n} <{}>", a.email),
        None => a.email.clone(),
    });
    format!("On {when}, {} wrote:", who.unwrap_or_else(|| "someone".into()))
}

fn original_html(body: &StoredBody) -> String {
    body.html.clone().or_else(|| body.text.as_deref().map(text_to_html)).unwrap_or_default()
}

/// Build a reply / reply-all / forward draft. `my_addresses` are every
/// address of the user, so reply-all never includes ourselves.
pub fn reply_draft(
    kind: ReplyKind,
    account_id: &str,
    original: &MessageHeader,
    body: &StoredBody,
    from: Sender,
    my_addresses: &[String],
    now: i64,
) -> Draft {
    let mut d = Draft::new(from, now);
    let is_me = |a: &Address| my_addresses.iter().any(|m| m.eq_ignore_ascii_case(&a.email));
    let mut references = body.references.clone();
    if let Some(id) = &original.internet_message_id
        && !references.contains(id)
    {
        references.push(id.clone());
    }
    d.reply = Some(ReplyContext {
        kind,
        account_id: account_id.to_string(),
        message_id: original.id.clone(),
        thread_id: original.thread_id.clone(),
        internet_message_id: original.internet_message_id.clone(),
        references,
    });

    match kind {
        ReplyKind::Reply | ReplyKind::ReplyAll => {
            d.subject = prefixed("Re:", &original.subject);
            // Answering our own sent message goes back to its recipients.
            let from_me = original.from.as_ref().is_some_and(is_me);
            d.to = match (&original.from, from_me) {
                (Some(f), false) => vec![f.clone()],
                _ => original.to.clone(),
            };
            if kind == ReplyKind::ReplyAll {
                let mut seen: Vec<String> = d.to.iter().map(|a| a.email.to_lowercase()).collect();
                for a in original.to.iter().chain(&original.cc) {
                    if !is_me(a) && !seen.contains(&a.email.to_lowercase()) {
                        seen.push(a.email.to_lowercase());
                        d.cc.push(a.clone());
                    }
                }
            }
            d.html = format!(
                "<div dir=\"auto\"><br></div><div class=\"pigeon-quote\"><div dir=\"auto\">{}</div>\
                 <blockquote style=\"margin:0 0 0 .8ex;border-inline-start:1px solid #ccc;padding-inline-start:1ex\">{}</blockquote></div>",
                escape_html(&quote_header(original)),
                original_html(body)
            );
            d.attachments = carried_attachments(account_id, &original.id, &d.html, &body.attachments, false);
        }
        ReplyKind::Forward => {
            d.subject = prefixed("Fwd:", &original.subject);
            let list = |v: &[Address]| v.iter().map(|a| escape_html(&a.email)).collect::<Vec<_>>().join(", ");
            d.html = format!(
                "<div dir=\"auto\"><br></div><div class=\"pigeon-quote\"><div>---------- Forwarded message ---------</div>\
                 <div>From: {}</div><div>Subject: {}</div><div>To: {}</div><br>{}</div>",
                original.from.as_ref().map(|a| escape_html(&a.email)).unwrap_or_default(),
                escape_html(&original.subject),
                list(&original.to),
                original_html(body)
            );
            d.attachments = carried_attachments(account_id, &original.id, &d.html, &body.attachments, true);
        }
    }
    d
}

fn strip_brackets(id: &str) -> &str {
    id.trim().trim_start_matches('<').trim_end_matches('>')
}

/// Serialize a draft to RFC 5322. `attachments` holds the bytes for
/// `draft.attachments`, same order. Returns (Message-ID, MIME bytes).
pub fn build_mime(draft: &Draft, attachments: &[Vec<u8>], now_secs: i64) -> Result<(String, Vec<u8>)> {
    use mail_builder::MessageBuilder;
    use mail_builder::headers::address::Address as MbAddress;

    if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
        return Err(Error::Other("no recipients".into()));
    }
    if attachments.len() != draft.attachments.len() {
        return Err(Error::Other("attachment bytes missing".into()));
    }
    let addr = |a: &Address| MbAddress::new_address(a.name.clone(), a.email.clone());
    let list = |v: &[Address]| MbAddress::new_list(v.iter().map(addr).collect());

    let domain = draft.from.address.email.rsplit_once('@').map(|(_, d)| d).unwrap_or("pigeon.invalid");
    let message_id = format!("{}@{domain}", auth::random_token(18));
    let html =
        format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{}</body></html>", draft.full_html());

    let mut mb = MessageBuilder::new()
        .from(addr(&draft.from.address))
        .subject(draft.subject.clone())
        .date(now_secs)
        .message_id(message_id.clone());
    let text = html_to_text(&draft.full_html());
    let inline = draft.attachments.iter().any(|a| a.source.inline_cid().is_some());
    mb =
        if inline { mb.body(related_body(html, text, draft, attachments)) } else { mb.html_body(html).text_body(text) };
    if !draft.to.is_empty() {
        mb = mb.to(list(&draft.to));
    }
    if !draft.cc.is_empty() {
        mb = mb.cc(list(&draft.cc));
    }
    if !draft.bcc.is_empty() {
        mb = mb.bcc(list(&draft.bcc));
    }
    if let Some(r) = &draft.reply
        && r.kind != ReplyKind::Forward
    {
        if let Some(parent) = &r.internet_message_id {
            mb = mb.in_reply_to(strip_brackets(parent).to_string());
        }
        if !r.references.is_empty() {
            mb = mb.references(r.references.iter().map(|id| strip_brackets(id).to_string()).collect::<Vec<_>>());
        }
    }
    if !inline {
        for (a, bytes) in draft.attachments.iter().zip(attachments) {
            mb = mb.attachment(content_type(a), a.name.clone(), bytes.clone());
        }
    }
    let mime = mb.write_to_vec()?;
    Ok((format!("<{message_id}>"), mime))
}

fn content_type(a: &DraftAttachment) -> String {
    a.content_type.clone().unwrap_or_else(|| "application/octet-stream".into())
}

/// The body when the quote shows inline images: the text/HTML alternatives
/// and those images in `multipart/related` (so `cid:` resolves), then the
/// files in `multipart/mixed`.
fn related_body<'x>(
    html: String,
    text: String,
    draft: &Draft,
    attachments: &[Vec<u8>],
) -> mail_builder::mime::MimePart<'x> {
    use mail_builder::mime::{BodyPart, MimePart};

    let alternative = MimePart::new(
        "multipart/alternative",
        vec![
            MimePart::new("text/plain", BodyPart::Text(text.into())),
            MimePart::new("text/html", BodyPart::Text(html.into())),
        ],
    );
    let mut related = vec![alternative];
    let mut files = Vec::new();
    for (a, bytes) in draft.attachments.iter().zip(attachments) {
        let part = MimePart::new(content_type(a), BodyPart::Binary(bytes.clone().into()));
        match a.source.inline_cid() {
            Some(cid) => related.push(part.inline().cid(cid.to_string())),
            None => files.push(part.attachment(a.name.clone())),
        }
    }
    let related = MimePart::new("multipart/related", related);
    if files.is_empty() {
        return related;
    }
    files.insert(0, related);
    MimePart::new("multipart/mixed", files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AttachmentMeta, StoredAttachment};

    fn sender(account: &str, email: &str) -> Sender {
        Sender { account_id: account.into(), address: Address::new(Some("Me"), email) }
    }

    #[test]
    fn primary_sender_is_the_accounts_own_address() {
        let a = Account {
            id: "google:me@x.io".into(),
            provider: crate::model::ProviderKind::Google,
            email: "me@x.io".into(),
            display_name: "Me".into(),
        };
        assert_eq!(Sender::primary(&a), sender("google:me@x.io", "me@x.io"));
    }

    #[test]
    fn icloud_domains_are_recognized() {
        assert!(is_icloud_mail("me@icloud.com"));
        assert!(is_icloud_mail(" Me@ME.com "));
        assert!(is_icloud_mail("x@mac.com"));
        assert!(!is_icloud_mail("me@gmail.com"));
        assert!(!is_icloud_mail("me@noticloud.com"));
        assert!(!is_icloud_mail("@icloud.com"));
        assert!(!is_icloud_mail("icloud.com"));
    }

    #[test]
    fn plain_addresses_only() {
        assert!(is_plain_address("me@icloud.com"));
        assert!(is_plain_address("first.last@sub.domain.dev"));
        for bad in [
            "",
            "me",
            "me@",
            "@x.com",
            "me@x",
            "me@x..com",
            "a@b@c.com",
            "Me <me@x.com>",
            "me @x.com",
            "a@x.com,b@y.com",
        ] {
            assert!(!is_plain_address(bad), "{bad}");
        }
    }

    #[test]
    fn icloud_addresses_merge_manual_and_detected() {
        let detected = vec!["main@icloud.com".to_string(), "old@me.com".to_string(), "alias@icloud.com".to_string()];
        let overrides = vec![
            ("Custom@Domain.dev".to_string(), false),
            ("old@me.com".to_string(), true),
            ("main@icloud.com".into(), false),
        ];
        assert_eq!(
            icloud_send_addresses(&detected, &overrides),
            ["custom@domain.dev", "main@icloud.com", "alias@icloud.com"]
        );
        assert!(icloud_send_addresses(&[], &[("x@icloud.com".into(), true)]).is_empty());
    }

    #[test]
    fn the_main_address_leads() {
        let gmail = Account {
            id: "icloud:me@gmail.com".into(),
            provider: crate::model::ProviderKind::ICloud,
            email: "me@gmail.com".into(),
            display_name: "Me".into(),
        };
        let emails = |v: Vec<Sender>| v.into_iter().map(|s| s.address.email).collect::<Vec<_>>();
        let known = ["me@icloud.com".to_string(), "sup@shop.dev".to_string()];
        let senders = || icloud_senders(&gmail, &known);
        assert_eq!(emails(lead_with_main(senders(), Some("SUP@shop.dev"))), ["sup@shop.dev", "me@icloud.com"]);
        assert_eq!(emails(lead_with_main(senders(), None)), ["me@icloud.com", "sup@shop.dev"]);
        assert_eq!(emails(lead_with_main(senders(), Some("gone@shop.dev"))), ["me@icloud.com", "sup@shop.dev"]);
    }

    #[test]
    fn icloud_senders_drop_a_foreign_apple_id() {
        let acct = |email: &str| Account {
            id: format!("icloud:{email}"),
            provider: crate::model::ProviderKind::ICloud,
            email: email.into(),
            display_name: "Me".into(),
        };
        let emails = |v: Vec<Sender>| v.into_iter().map(|s| s.address.email).collect::<Vec<_>>();
        let gmail = acct("me@gmail.com");
        let known = ["me@icloud.com".to_string(), "alias@icloud.com".to_string()];
        assert_eq!(emails(icloud_senders(&gmail, &known)), ["me@icloud.com", "alias@icloud.com"]);
        assert!(icloud_senders(&gmail, &known).iter().all(|s| s.account_id == gmail.id));
        assert_eq!(emails(icloud_senders(&gmail, &[])), ["me@gmail.com"], "nothing known yet: as before");
        let native = acct("me@icloud.com");
        assert_eq!(emails(icloud_senders(&native, &known)), ["me@icloud.com", "alias@icloud.com"], "no duplicate");
    }

    /// Hebrew subject, names and mixed RTL/LTR body survive build → parse.
    #[test]
    fn hebrew_mime_round_trips() {
        let subject =
            "פידבקים על הגרסה החדשה — feedback on v0.1 (סבב שני, ארוך מספיק כדי לפצל את הכותרת לכמה מילים מקודדות)";
        let mut d = Draft::new(
            Sender {
                account_id: "google:me@x.io".into(), address: Address::new(Some("ערן גולדין"), "me@x.io")
            },
            0,
        );
        d.to = vec![Address::new(Some("דנה כהן"), "dana@x.io")];
        d.subject = subject.into();
        let line = "שורה ארוכה מאוד עם עברית ו-English mixed together, ".repeat(8);
        d.body_html =
            format!("<div dir=\"auto\">שלום דנה,</div><div dir=\"auto\">{line}</div><div dir=\"ltr\">v0.1.0 ✓</div>");
        let (_, mime) = build_mime(&d, &[], 1_792_800_000).unwrap();

        let raw = String::from_utf8_lossy(&mime);
        assert!(raw.contains("\r\n"), "CRLF line endings");
        for l in raw.split("\r\n") {
            assert!(l.len() <= 998, "RFC 5322 line limit: {} bytes", l.len());
        }

        let m = mail_parser::MessageParser::default().parse(&mime).expect("parses");
        assert_eq!(m.subject(), Some(subject));
        let from = m.from().and_then(|a| a.first()).unwrap();
        assert_eq!(from.name(), Some("ערן גולדין"));
        assert_eq!(m.to().and_then(|a| a.first()).and_then(|a| a.name()), Some("דנה כהן"));
        let html = m.body_html(0).expect("html part");
        assert!(html.contains("שלום דנה,") && html.contains(line.trim_end()) && html.contains("dir=\"ltr\""), "{html}");
        let text = m.body_text(0).expect("text part");
        assert!(text.contains("שלום דנה,") && text.contains("v0.1.0 ✓"), "{text}");
    }

    fn original() -> MessageHeader {
        MessageHeader {
            id: "m1".into(),
            thread_id: "t1".into(),
            internet_message_id: Some("<orig@cust.com>".into()),
            subject: "Order שלום".into(),
            from: Some(Address::new(Some("Customer"), "c@cust.com")),
            to: vec![Address::new(None, "support@biz.com"), Address::new(None, "boss@biz.com")],
            cc: vec![Address::new(None, "me@work.com"), Address::new(None, "c@cust.com")],
            received_at: 0,
            ..Default::default()
        }
    }

    fn body() -> StoredBody {
        StoredBody {
            html: Some("<p>Hi</p><img src=\"cid:Logo@X\">".into()),
            references: vec!["<root@cust.com>".into()],
            attachments: vec![
                StoredAttachment {
                    meta: AttachmentMeta {
                        id: "a1".into(),
                        name: "inv.pdf".into(),
                        size: Some(2048),
                        ..Default::default()
                    },
                    local_path: None,
                },
                StoredAttachment {
                    meta: AttachmentMeta {
                        id: "a2".into(),
                        name: "logo.png".into(),
                        content_type: Some("image/png".into()),
                        content_id: Some("logo@x".into()),
                        is_inline: true,
                        ..Default::default()
                    },
                    local_path: None,
                },
                // Apple Mail style: a file sent inline that the HTML never shows.
                StoredAttachment {
                    meta: AttachmentMeta {
                        id: "a3".into(),
                        name: "scan.pdf".into(),
                        content_id: Some("scan@x".into()),
                        is_inline: true,
                        ..Default::default()
                    },
                    local_path: None,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn due_drafts_are_the_past_scheduled_ones_in_order() {
        let mk = |id: &str, at: Option<i64>| {
            let mut d = Draft::new(sender("a", "me@x.com"), 0);
            d.id = id.into();
            d.scheduled_at = at;
            d
        };
        let due = due_drafts(
            vec![mk("late", Some(50)), mk("none", None), mk("future", Some(500)), mk("early", Some(10))],
            100,
        );
        assert_eq!(due.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["early", "late"]);
        // Old drafts without the fields still deserialize.
        let old = r#"{"id":"x","from":{"account_id":"a","address":{"email":"me@x.com"}},"to":[],"cc":[],"bcc":[],"subject":"","html":"","attachments":[],"reply":null,"updated_at":0}"#;
        let d: Draft = serde_json::from_str(old).unwrap();
        assert_eq!((d.scheduled_at, d.undo_window), (None, false));
    }

    #[test]
    fn default_sender_prefers_the_addressed_alias() {
        let senders = vec![
            sender("google:main", "main@gmail.com"),
            sender("google:main", "support@biz.com"),
            sender("ms:work", "me@work.com"),
        ];
        assert_eq!(default_sender("google:main", &original(), &senders).unwrap().address.email, "support@biz.com");
        assert_eq!(default_sender("ms:work", &original(), &senders).unwrap().address.email, "me@work.com");
        assert!(default_sender("nope", &original(), &senders).is_none());
    }

    #[test]
    fn reply_all_excludes_me_and_dedupes() {
        let mine = vec!["support@biz.com".to_string(), "me@work.com".to_string()];
        let d = reply_draft(
            ReplyKind::ReplyAll,
            "google:main",
            &original(),
            &body(),
            sender("google:main", "support@biz.com"),
            &mine,
            0,
        );
        assert_eq!(d.to.iter().map(|a| a.email.as_str()).collect::<Vec<_>>(), ["c@cust.com"]);
        assert_eq!(d.cc.iter().map(|a| a.email.as_str()).collect::<Vec<_>>(), ["boss@biz.com"]);
        assert_eq!(d.subject, "Re: Order שלום");
        assert!(
            d.html.contains("<blockquote")
                && d.html.contains("<p>Hi</p>")
                && d.html.contains("Customer &lt;c@cust.com&gt;")
        );
        let r = d.reply.as_ref().unwrap();
        assert_eq!(r.references, ["<root@cust.com>", "<orig@cust.com>"]);
        assert_eq!(d.thread_hint(), Some("t1"));
        assert!(!d.crosses_accounts());
    }

    #[test]
    fn switching_from_to_another_account_keeps_content_and_threading_headers() {
        let mut d = reply_draft(
            ReplyKind::Reply,
            "google:main",
            &original(),
            &body(),
            sender("google:main", "support@biz.com"),
            &[],
            0,
        );
        d.html.insert_str(0, "<div dir=\"auto\">תשובה</div>");
        let before = d.clone();
        d.switch_from(sender("ms:work", "me@work.com"));
        assert_eq!((d.to.clone(), d.html.clone(), d.reply.clone()), (before.to, before.html, before.reply));
        assert!(d.crosses_accounts());
        assert_eq!(d.thread_hint(), None, "provider thread ids don't cross accounts");

        let (msgid, mime) = build_mime(&d, &[b"PNG".to_vec()], 1_700_000_000).unwrap();
        let mime = String::from_utf8(mime).unwrap();
        assert!(msgid.ends_with("@work.com>"));
        assert!(mime.contains("In-Reply-To: <orig@cust.com>"), "{mime}");
        assert!(mime.contains("References: <root@cust.com> <orig@cust.com>"), "{mime}");
        assert!(mime.contains("me@work.com"));
        assert!(mime.contains("Subject: =?utf-8?"), "non-ASCII subject is encoded: {mime}");
    }

    #[test]
    fn forward_carries_attachments_not_threading() {
        let d = reply_draft(
            ReplyKind::Forward,
            "google:main",
            &original(),
            &body(),
            sender("google:main", "x@gmail.com"),
            &[],
            0,
        );
        assert_eq!(d.subject, "Fwd: Order שלום");
        assert!(d.to.is_empty());
        let names: Vec<(&str, Option<&str>)> =
            d.attachments.iter().map(|a| (a.name.as_str(), a.source.inline_cid())).collect();
        assert_eq!(names, [("inv.pdf", None), ("logo.png", Some("logo@x")), ("scan.pdf", None)]);
        assert_eq!(
            d.attachments[0].source,
            AttachmentSource::Message {
                account_id: "google:main".into(),
                message_id: "m1".into(),
                attachment_id: "a1".into(),
                content_id: None,
                size: Some(2048),
            }
        );
        assert_eq!(d.thread_hint(), None);
        assert_eq!(prefixed("Fwd:", "FW: x"), "FW: x");
        assert_eq!(prefixed("Re:", "RE: x"), "RE: x");
    }

    #[test]
    fn replies_carry_only_the_images_the_quote_shows() {
        let d = reply_draft(
            ReplyKind::Reply,
            "google:main",
            &original(),
            &body(),
            sender("google:main", "x@gmail.com"),
            &[],
            0,
        );
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(
            (d.attachments[0].name.as_str(), d.attachments[0].source.inline_cid()),
            ("logo.png", Some("logo@x"))
        );
        // Nothing quoted that shows an image: nothing carried.
        assert!(carried_attachments("a", "m", "<p>plain</p>", &body().attachments, false).is_empty());
    }

    #[test]
    fn inline_images_go_out_as_related_parts() {
        use mail_parser::MimeHeaders;
        let mut d = reply_draft(
            ReplyKind::Forward,
            "google:main",
            &original(),
            &body(),
            sender("google:main", "x@gmail.com"),
            &[],
            0,
        );
        d.to.push(Address::new(None, "you@y.com"));
        let bytes = vec![b"%PDF".to_vec(), b"PNG".to_vec(), b"%PDF scan".to_vec()];
        let (_, mime) = build_mime(&d, &bytes, 0).unwrap();
        let raw = String::from_utf8_lossy(&mime);
        assert!(raw.contains("multipart/mixed") && raw.contains("multipart/related"), "{raw}");
        assert!(raw.contains("Content-ID: <logo@x>"), "{raw}");

        let m = mail_parser::MessageParser::default().parse(&mime).unwrap();
        assert!(m.body_html(0).unwrap().contains("cid:Logo@X"));
        let parts: Vec<(Option<&str>, Option<&str>, bool)> = m
            .attachments()
            .map(|p| {
                let inline = p.content_disposition().is_some_and(|c| c.c_type.eq_ignore_ascii_case("inline"));
                (p.attachment_name(), p.content_id(), inline)
            })
            .collect();
        assert_eq!(
            parts,
            [(None, Some("logo@x"), true), (Some("inv.pdf"), None, false), (Some("scan.pdf"), None, false)]
        );
    }

    #[test]
    fn message_sources_from_older_drafts_still_parse() {
        let old: AttachmentSource =
            serde_json::from_str(r#"{"kind":"message","account_id":"a","message_id":"m","attachment_id":"1"}"#)
                .unwrap();
        assert_eq!(old.inline_cid(), None);
        assert!(!serde_json::to_string(&old).unwrap().contains("content_id"));
    }

    #[test]
    fn drive_sources_round_trip_and_old_ones_still_parse() {
        let a = DraftAttachment {
            name: "Budget.xlsx".into(),
            content_type: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
            source: AttachmentSource::Drive {
                account_id: "google:me@x.io".into(),
                file_id: "f1".into(),
                export_mime: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
                size: None,
            },
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains(r#""kind":"drive""#), "{json}");
        assert_eq!(serde_json::from_str::<DraftAttachment>(&json).unwrap(), a);
        assert!(a.source.is_drive());
        let minimal: AttachmentSource =
            serde_json::from_str(r#"{"kind":"drive","account_id":"g","file_id":"f"}"#).unwrap();
        assert_eq!(
            minimal,
            AttachmentSource::Drive { account_id: "g".into(), file_id: "f".into(), export_mime: None, size: None }
        );
        let local: AttachmentSource = serde_json::from_str(r#"{"kind":"file","path":"/tmp/a"}"#).unwrap();
        assert!(!local.is_drive());
    }

    #[test]
    fn mime_requires_recipients_and_bytes() {
        let mut d = Draft::new(sender("a", "me@x.com"), 0);
        assert!(build_mime(&d, &[], 0).is_err());
        d.to.push(Address::new(None, "you@y.com"));
        d.attachments.push(DraftAttachment {
            name: "a.txt".into(),
            content_type: Some("text/plain".into()),
            source: AttachmentSource::File { path: "/tmp/a.txt".into() },
        });
        assert!(build_mime(&d, &[], 0).is_err());
        let (_, mime) = build_mime(&d, &[b"hello".to_vec()], 0).unwrap();
        let mime = String::from_utf8(mime).unwrap();
        assert!(mime.contains("multipart/mixed") && mime.contains("a.txt"));
    }
}
