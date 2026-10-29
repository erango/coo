use std::collections::BTreeMap;

use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use mail_parser::{MessageParser, MimeHeaders};
use pigeon_core::model::*;
use pigeon_core::provider::{BodyProvider, DraftSync, MailActions, MailPage, MailProvider, MailSearch, MailSender};
use pigeon_core::search::SearchQuery;
use pigeon_core::{Error, Result, now_ms};

use crate::Imap;
use crate::conn::{Session, imap_err};
use crate::parse::{self, Cursor, Flags};

const SCOPE: &str = "imap:";
const PAGE: usize = 100;
const FLAGS_CHUNK: usize = 500;
const HEADER_QUERY: &str = "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.2048>)";

fn flags_of(f: &Fetch) -> Flags {
    let mut out = Flags::default();
    for fl in f.flags() {
        match fl {
            Flag::Seen => out.seen = true,
            Flag::Flagged => out.flagged = true,
            Flag::Draft => out.draft = true,
            _ => {}
        }
    }
    out
}

fn deleted(f: &Fetch) -> bool {
    f.flags().any(|fl| matches!(fl, Flag::Deleted))
}

async fn fetch_headers(s: &mut Session, mailbox: &str, uv: u32, uids: &[u32]) -> Result<Vec<MailChange>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let fetches: Vec<Fetch> = s
        .uid_fetch(parse::uid_set(uids), HEADER_QUERY)
        .await
        .map_err(imap_err)?
        .try_collect()
        .await
        .map_err(imap_err)?;
    Ok(fetches
        .iter()
        .filter(|f| !deleted(f))
        .filter_map(|f| {
            let uid = f.uid?;
            let header = f.header().unwrap_or_default();
            let id = parse::message_id(uv, uid, mailbox);
            let date = f.internal_date().map(|d| d.timestamp_millis());
            let mut h = parse::header_from_raw(header, id, mailbox, flags_of(f), date);
            if let Some(text) = f.text() {
                let mut partial = header.to_vec();
                partial.extend_from_slice(text);
                h.snippet = parse::snippet(&partial);
            }
            Some(MailChange::Upsert(h))
        })
        .collect())
}

async fn search(s: &mut Session, criteria: &str, utf8: bool) -> Result<Vec<u32>> {
    let q = if utf8 { format!("CHARSET UTF-8 {criteria}") } else { criteria.to_string() };
    let mut v: Vec<u32> = s.uid_search(q).await.map_err(imap_err)?.into_iter().collect();
    v.sort_unstable();
    Ok(v)
}

impl Imap {
    fn since_ms(&self) -> i64 {
        now_ms() - self.mail_window_days * 86_400_000
    }

    /// Run `f` on a pooled session; the session is dropped (not reused) if it errors.
    async fn with<T>(&self, f: impl AsyncFnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut lease = self.pool.get().await?;
        let r = f(lease.session()).await;
        if r.is_err() {
            lease.discard();
        }
        r
    }

    async fn page(&self, s: &mut Session, mailbox: &str, cursor: Option<&str>) -> Result<MailPage> {
        let mb = s.select(mailbox).await.map_err(imap_err)?;
        let uv = mb.uid_validity.unwrap_or(0);
        let since_ms = self.since_ms();
        let cursor = match cursor {
            None => Cursor::Boot {
                uidvalidity: uv,
                below: mb.uid_next.unwrap_or(1),
                top: mb.uid_next.unwrap_or(1).saturating_sub(1),
            },
            Some(c) => Cursor::parse(c)?,
        };
        if cursor.uidvalidity() != uv {
            return Err(Error::CursorExpired);
        }
        match cursor {
            Cursor::Boot { below, top, .. } => {
                let window = search(s, &format!("SINCE {}", parse::since(since_ms)), false).await?;
                let mut todo: Vec<u32> = window.into_iter().filter(|u| *u < below).collect();
                todo.reverse(); // newest first
                let batch: Vec<u32> = todo.iter().take(PAGE).copied().collect();
                let changes = fetch_headers(s, mailbox, uv, &batch).await?;
                let next = match batch.iter().min() {
                    Some(&lowest) if todo.len() > PAGE => Cursor::Boot { uidvalidity: uv, below: lowest, top },
                    _ => Cursor::Sync { uidvalidity: uv, max_uid: top },
                };
                // Keep going: the sync pass right after picks up anything new.
                Ok(MailPage { changes, cursor: next.to_string(), more: true })
            }
            Cursor::Sync { max_uid, .. } => {
                let fresh: Vec<u32> = search(s, &format!("UID {}:*", max_uid + 1), false)
                    .await?
                    .into_iter()
                    .filter(|u| *u > max_uid)
                    .collect();
                if !fresh.is_empty() {
                    let batch: Vec<u32> = fresh.iter().take(PAGE).copied().collect();
                    let changes = fetch_headers(s, mailbox, uv, &batch).await?;
                    let max = batch.iter().max().copied().unwrap_or(max_uid);
                    return Ok(MailPage {
                        changes,
                        cursor: Cursor::Sync { uidvalidity: uv, max_uid: max }.to_string(),
                        more: fresh.len() > PAGE,
                    });
                }
                // Caught up: flags + deletions over the window.
                let window = search(s, &format!("SINCE {}", parse::since(since_ms)), false).await?;
                let mut changes = Vec::new();
                let mut present = Vec::with_capacity(window.len());
                for chunk in window.chunks(FLAGS_CHUNK) {
                    let fetches: Vec<Fetch> = s
                        .uid_fetch(parse::uid_set(chunk), "(UID FLAGS)")
                        .await
                        .map_err(imap_err)?
                        .try_collect()
                        .await
                        .map_err(imap_err)?;
                    for f in fetches.iter().filter(|f| !deleted(f)) {
                        let Some(uid) = f.uid else { continue };
                        let id = parse::message_id(uv, uid, mailbox);
                        let fl = flags_of(f);
                        changes.push(MailChange::Flags { id: id.clone(), is_read: fl.seen, is_flagged: fl.flagged });
                        present.push(id);
                    }
                }
                // Day granularity on the server: only prune what's clearly inside the window.
                changes.push(MailChange::Retain {
                    folder_id: mailbox.to_string(),
                    ids: present,
                    since: since_ms + 86_400_000,
                });
                Ok(MailPage { changes, cursor: Cursor::Sync { uidvalidity: uv, max_uid }.to_string(), more: false })
            }
        }
    }

    async fn full_message(&self, message_id: &str) -> Result<Vec<u8>> {
        let (uv, uid, mailbox) = parse::split_id(message_id)?;
        let mailbox = mailbox.to_string();
        self.with(async |s| {
            let mb = s.examine(&mailbox).await.map_err(imap_err)?;
            if mb.uid_validity.unwrap_or(0) != uv {
                return Err(Error::Other("this message moved or the folder was rebuilt; sync to refresh".into()));
            }
            let fetches: Vec<Fetch> = s
                .uid_fetch(uid.to_string(), "BODY.PEEK[]")
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            fetches.first().and_then(|f| f.body().map(<[u8]>::to_vec)).ok_or_else(|| Error::Api {
                status: 404,
                url: format!("imap:{mailbox}/{uid}"),
                body: "no such message".into(),
            })
        })
        .await
    }

    async fn role_folder(&self, role: FolderRole) -> Result<Option<String>> {
        Ok(self.folders().await?.into_iter().find(|f| f.role == Some(role)).map(|f| f.id))
    }
}

impl MailProvider for Imap {
    async fn folders(&self) -> Result<Vec<Folder>> {
        self.with(async |s| {
            let names: Vec<async_imap::types::Name> =
                s.list(Some(""), Some("*")).await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
            let mut out = Vec::new();
            for n in &names {
                if n.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect)) {
                    continue;
                }
                let special: Vec<&str> = n
                    .attributes()
                    .iter()
                    .filter_map(|a| match a {
                        NameAttribute::Sent => Some("\\Sent"),
                        NameAttribute::Drafts => Some("\\Drafts"),
                        NameAttribute::Trash => Some("\\Trash"),
                        NameAttribute::Junk => Some("\\Junk"),
                        NameAttribute::Archive => Some("\\Archive"),
                        NameAttribute::Flagged => Some("\\Flagged"),
                        _ => None,
                    })
                    .collect();
                let name = n.name().to_string();
                let leaf = parse::parent_of(&name, n.delimiter())
                    .map(|p| name[p.len() + n.delimiter().unwrap_or("/").len()..].to_string())
                    .unwrap_or_else(|| if name.eq_ignore_ascii_case("INBOX") { "Inbox".into() } else { name.clone() });
                out.push(Folder {
                    role: parse::role_for(&name, &special),
                    parent_id: parse::parent_of(&name, n.delimiter()),
                    name: leaf,
                    id: name,
                    kind: FolderKind::Folder,
                });
            }
            Ok(out)
        })
        .await
    }

    fn mail_scopes(&self, folders: &[Folder]) -> Vec<String> {
        let rank = |f: &Folder| match f.role {
            Some(FolderRole::Inbox) => 0,
            Some(FolderRole::Sent) => 1,
            Some(FolderRole::Drafts) => 2,
            Some(FolderRole::Archive) => 3,
            None => 4,
            Some(_) => 5,
        };
        let mut ordered: Vec<&Folder> = folders.iter().collect();
        ordered.sort_by_key(|f| rank(f));
        ordered.into_iter().map(|f| format!("{SCOPE}{}", f.id)).collect()
    }

    /// Two folders at a time keeps one pooled connection for bodies/actions.
    fn mail_concurrency(&self) -> usize {
        2
    }

    async fn mail_page(&self, scope: &str, cursor: Option<&str>) -> Result<MailPage> {
        let mailbox = scope.strip_prefix(SCOPE).ok_or_else(|| Error::Other(format!("unknown IMAP scope {scope}")))?;
        self.with(async |s| self.page(s, mailbox, cursor).await).await
    }
}

fn content_type(p: &mail_parser::MessagePart) -> Option<String> {
    p.content_type().map(|ct| match &ct.c_subtype {
        Some(sub) => format!("{}/{}", ct.c_type, sub),
        None => ct.c_type.to_string(),
    })
}

/// Parse a full message into our body shape (pure; tested).
pub fn body_from_raw(raw: &[u8]) -> Result<MessageBody> {
    let m = MessageParser::default().parse(raw).ok_or_else(|| Error::Other("unreadable message".into()))?;
    let html = m.html_bodies().find(|p| p.is_text_html()).and_then(|p| p.text_contents()).map(String::from);
    let text = m.text_bodies().find(|p| !p.is_text_html()).and_then(|p| p.text_contents()).map(String::from);
    let mut attachments = Vec::new();
    let mut inline_data = Vec::new();
    for (i, p) in m.attachments().enumerate() {
        let content_id = p.content_id().map(|c| c.trim_start_matches('<').trim_end_matches('>').to_string());
        let disposition_attachment =
            p.content_disposition().is_some_and(|d| d.c_type.eq_ignore_ascii_case("attachment"));
        let is_inline = content_id.is_some() && !disposition_attachment;
        let meta = AttachmentMeta {
            id: i.to_string(),
            name: p.attachment_name().map(String::from).unwrap_or_else(|| format!("attachment-{}", i + 1)),
            content_type: content_type(p),
            size: Some(p.contents().len() as u64),
            content_id,
            is_inline,
        };
        if is_inline {
            inline_data.push((meta.id.clone(), p.contents().to_vec()));
        }
        attachments.push(meta);
    }
    let ids = |v: &mail_parser::HeaderValue| -> Vec<String> {
        v.as_text_list()
            .map(|l| l.iter().map(|s| format!("<{}>", s.trim_matches(['<', '>']))).collect())
            .or_else(|| v.as_text().map(|s| vec![format!("<{}>", s.trim_matches(['<', '>']))]))
            .unwrap_or_default()
    };
    Ok(MessageBody {
        html,
        text,
        attachments,
        inline_data,
        in_reply_to: ids(m.in_reply_to()).into_iter().next(),
        references: ids(m.references()),
        invite: None,
        origin: Some(pigeon_core::identity::origin_from_headers(m.headers_raw().map(|(n, v)| (n, v.trim())))),
    })
}

impl BodyProvider for Imap {
    async fn body(&self, message_id: &str) -> Result<MessageBody> {
        let raw = self.full_message(message_id).await?;
        let mut body = body_from_raw(&raw)?;
        // Calendar invites: the text/calendar part, if any.
        if let Some(m) = MessageParser::default().parse(&raw) {
            body.invite = m
                .parts
                .iter()
                .find(|p| p.is_content_type("text", "calendar"))
                .and_then(|p| p.text_contents())
                .and_then(pigeon_core::ics::parse_invite);
        }
        Ok(body)
    }

    async fn raw(&self, message_id: &str) -> Result<Vec<u8>> {
        self.full_message(message_id).await
    }

    async fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let raw = self.full_message(message_id).await?;
        let m = MessageParser::default().parse(&raw).ok_or_else(|| Error::Other("unreadable message".into()))?;
        let i: usize = attachment_id.parse().map_err(|_| Error::Other(format!("bad attachment id {attachment_id}")))?;
        m.attachments()
            .nth(i)
            .map(|p| p.contents().to_vec())
            .ok_or_else(|| Error::Other(format!("attachment {attachment_id} not found")))
    }
}

/// Group message ids by mailbox → UIDs (skipping ids from an older UIDVALIDITY
/// is left to the server: unknown UIDs are a no-op).
fn by_mailbox(ids: &[String]) -> BTreeMap<String, Vec<u32>> {
    let mut out: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for id in ids {
        if let Ok((_, uid, mailbox)) = parse::split_id(id) {
            out.entry(mailbox.to_string()).or_default().push(uid);
        }
    }
    out
}

impl MailActions for Imap {
    async fn apply(&self, op: &MailOp) -> Result<()> {
        let (flag, on) = match op {
            MailOp::SetRead { read, .. } => (Some("\\Seen"), *read),
            MailOp::SetFlagged { flagged, .. } => (Some("\\Flagged"), *flagged),
            MailOp::MoveTo { .. } | MailOp::Purge { .. } => (None, false),
            // IMAP has no spam report: junk is a move to the \Junk mailbox.
            MailOp::Trash { .. } | MailOp::Untrash { .. } | MailOp::Relabel { .. } | MailOp::Report { .. } => {
                return Err(Error::Other("IMAP folders: use a move".into()));
            }
        };
        let groups = by_mailbox(op.ids());
        self.with(async |s| {
            let caps = s.capabilities().await.map_err(imap_err)?;
            let can_move = caps.has_str("MOVE");
            let uidplus = caps.has_str("UIDPLUS");
            for (mailbox, uids) in &groups {
                s.select(mailbox).await.map_err(imap_err)?;
                let set = parse::uid_set(uids);
                match (flag, op) {
                    (Some(flag), _) => {
                        let q = format!("{}FLAGS.SILENT ({flag})", if on { "+" } else { "-" });
                        let _: Vec<Fetch> =
                            s.uid_store(&set, q).await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
                    }
                    (None, MailOp::MoveTo { folder_id, .. }) if folder_id != mailbox => {
                        if can_move {
                            s.uid_mv(&set, folder_id).await.map_err(imap_err)?;
                        } else {
                            s.uid_copy(&set, folder_id).await.map_err(imap_err)?;
                            let _: Vec<Fetch> = s
                                .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                                .await
                                .map_err(imap_err)?
                                .try_collect()
                                .await
                                .map_err(imap_err)?;
                            let _: Vec<u32> =
                                s.uid_expunge(&set).await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
                        }
                    }
                    (None, MailOp::Purge { .. }) => {
                        let _: Vec<Fetch> = s
                            .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                            .await
                            .map_err(imap_err)?
                            .try_collect()
                            .await
                            .map_err(imap_err)?;
                        // Without UIDPLUS, EXPUNGE takes every \Deleted
                        // message in the mailbox; it's Trash, so that's fine.
                        if uidplus {
                            let _: Vec<u32> =
                                s.uid_expunge(&set).await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
                        } else {
                            let _: Vec<u32> =
                                s.expunge().await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        })
        .await
    }
}

impl MailSearch for Imap {
    async fn search(&self, q: &SearchQuery, limit: u32) -> Result<Vec<MailChange>> {
        let (criteria, utf8) = parse::search_criteria(q);
        let folders = self.folders().await?;
        let wanted = |f: &&Folder| match q.role {
            Some(r) => f.role == Some(r),
            None => matches!(f.role, Some(FolderRole::Inbox | FolderRole::Sent | FolderRole::Archive)),
        };
        let targets: Vec<String> = folders.iter().filter(wanted).map(|f| f.id.clone()).collect();
        self.with(async |s| {
            let mut out = Vec::new();
            for mailbox in &targets {
                let mb = s.examine(mailbox).await.map_err(imap_err)?;
                let uv = mb.uid_validity.unwrap_or(0);
                let hits = search(s, &criteria, utf8).await?;
                let newest: Vec<u32> = hits.iter().rev().take(limit as usize).copied().collect();
                out.extend(fetch_headers(s, mailbox, uv, &newest).await?);
            }
            Ok(out)
        })
        .await
    }
}

impl MailSender for Imap {
    async fn send_as(&self) -> Result<Vec<Address>> {
        Ok(Vec::new())
    }

    async fn send_mime(&self, mime: &[u8], _thread_id: Option<&str>) -> Result<()> {
        crate::smtp::send(&self.cfg, &self.email, &self.password, mime).await?;
        // The submission server doesn't file a copy; save one in Sent.
        // Best effort: the message is already on its way.
        if let Ok(Some(sent)) = self.role_folder(FolderRole::Sent).await {
            let clean = parse::strip_bcc(mime);
            let r = self.with(async |s| s.append(&sent, Some("(\\Seen)"), None, &clean).await.map_err(imap_err)).await;
            if let Err(e) = r {
                tracing::warn!(error = %e, "could not save the sent copy");
            }
        }
        Ok(())
    }
}

/// The Message-ID of a MIME message, without angle brackets.
fn mime_message_id(mime: &[u8]) -> Option<String> {
    MessageParser::new().parse(mime).and_then(|m| m.message_id().map(String::from))
}

impl Imap {
    /// Expunge one message by our id (`uidvalidity:uid:mailbox`).
    async fn expunge_one(&self, id: &str) -> Result<()> {
        let (_, uid, mailbox) = parse::split_id(id)?;
        let mailbox = mailbox.to_string();
        let set = uid.to_string();
        self.with(async |s| {
            s.select(&mailbox).await.map_err(imap_err)?;
            let _: Vec<Fetch> = s
                .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                .await
                .map_err(imap_err)?
                .try_collect()
                .await
                .map_err(imap_err)?;
            let _: Vec<u32> = s.uid_expunge(&set).await.map_err(imap_err)?.try_collect().await.map_err(imap_err)?;
            Ok(())
        })
        .await
    }
}

impl DraftSync for Imap {
    /// APPEND to Drafts with \\Draft, find the new UID by Message-ID (no
    /// reliance on UIDPLUS), then expunge the previous version.
    async fn put_draft(&self, mime: &[u8], existing: Option<&str>, _thread_id: Option<&str>) -> Result<String> {
        let drafts = self
            .role_folder(FolderRole::Drafts)
            .await?
            .ok_or_else(|| Error::Other("this account has no Drafts folder".into()))?;
        let msgid = mime_message_id(mime).ok_or_else(|| Error::Other("draft has no Message-ID".into()))?;
        let id = self
            .with(async |s| {
                s.append(&drafts, Some("(\\Draft \\Seen)"), None, mime).await.map_err(imap_err)?;
                let mb = s.select(&drafts).await.map_err(imap_err)?;
                let uv = mb.uid_validity.unwrap_or(0);
                let hits = search(s, &format!("HEADER Message-ID \"{}\"", msgid.replace('"', "")), false).await?;
                let uid =
                    hits.into_iter().max().ok_or_else(|| Error::Other("saved draft not found in Drafts".into()))?;
                Ok(parse::message_id(uv, uid, &drafts))
            })
            .await?;
        if let Some(old) = existing
            && old != id
            && let Err(e) = self.expunge_one(old).await
        {
            tracing::warn!(error = %e, "could not remove the previous draft version");
        }
        Ok(id)
    }

    async fn delete_draft(&self, id: &str) -> Result<()> {
        match self.expunge_one(id).await {
            // A stale id (folder renamed/resynced) means it's gone already.
            Err(Error::Other(m)) if m.starts_with("not an IMAP message id") => Ok(()),
            r => r,
        }
    }

    async fn draft_for_message(&self, message_id: &str) -> Result<Option<String>> {
        Ok(Some(message_id.to_string()))
    }

    /// SMTP has no notion of a stored draft: send the MIME, then drop it.
    async fn send_draft(&self, id: &str, mime: &[u8], thread_id: Option<&str>) -> Result<()> {
        self.send_mime(mime, thread_id).await?;
        if let Err(e) = self.delete_draft(id).await {
            tracing::warn!(error = %e, "sent, but could not remove the draft");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_message_id_without_brackets() {
        let mime = b"Message-ID: <abc.123@example.com>\r\nSubject: x\r\n\r\nhi";
        assert_eq!(mime_message_id(mime).as_deref(), Some("abc.123@example.com"));
        assert_eq!(mime_message_id(b"Subject: x\r\n\r\nhi"), None);
    }

    #[test]
    fn parses_full_message_with_windows_1255_text_and_attachments() {
        // "שלום" in windows-1255 is F9 EC E5 ED.
        let mut raw: Vec<u8> =
            b"From: a@x.com\r\nTo: b@y.com\r\nSubject: s\r\nMessage-ID: <m@x>\r\nReferences: <r1@x> <r2@x>\r\n\
            In-Reply-To: <r2@x>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n\
            --B\r\nContent-Type: text/plain; charset=windows-1255\r\nContent-Transfer-Encoding: 8bit\r\n\r\n"
                .to_vec();
        raw.extend_from_slice(&[0xF9, 0xEC, 0xE5, 0xED]);
        raw.extend_from_slice(
            b"\r\n--B\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Disposition: inline\r\n\
            Content-Transfer-Encoding: base64\r\n\r\nUE5H\r\n--B\r\nContent-Type: application/pdf; name=\"inv.pdf\"\r\n\
            Content-Disposition: attachment; filename=\"inv.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--B--\r\n",
        );
        let b = body_from_raw(&raw).unwrap();
        assert_eq!(b.text.as_deref().map(str::trim), Some("שלום"));
        assert_eq!(b.references, ["<r1@x>", "<r2@x>"]);
        assert_eq!(b.in_reply_to.as_deref(), Some("<r2@x>"));
        assert_eq!(b.attachments.len(), 2);
        assert_eq!((b.attachments[0].is_inline, b.attachments[0].content_id.as_deref()), (true, Some("logo@x")));
        assert_eq!(b.inline_data, vec![("0".to_string(), b"PNG".to_vec())]);
        assert_eq!((b.attachments[1].name.as_str(), b.attachments[1].is_inline), ("inv.pdf", false));
        assert_eq!(b.attachments[1].content_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn groups_ids_by_mailbox() {
        let ids = vec!["1:5:INBOX".to_string(), "1:7:INBOX".into(), "3:2:Archive".into(), "junk".into()];
        let g = by_mailbox(&ids);
        assert_eq!(g["INBOX"], [5, 7]);
        assert_eq!(g["Archive"], [2]);
        assert_eq!(g.len(), 2);
    }
}
