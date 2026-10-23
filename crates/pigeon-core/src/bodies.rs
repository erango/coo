//! Lazy body + attachment loading: fetch once, sanitize once, keep inline
//! images and downloaded attachments under the cache dir.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::model::{AttachmentMeta, StoredAttachment, StoredBody};
use crate::provider::BodyProvider;
use crate::store::Store;
use crate::sync::lock;
use crate::{Error, Result, html, now_ms};

/// Cached body, or fetch + sanitize + store it.
pub async fn load_body<P: BodyProvider>(
    p: &P,
    account_id: &str,
    message_id: &str,
    store: &Mutex<Store>,
    cache_dir: &Path,
) -> Result<StoredBody> {
    if let Some(body) = lock(store).body(account_id, message_id)? {
        return Ok(body);
    }
    let raw = p.body(message_id).await?;
    let sanitized = raw.html.as_deref().map(html::sanitize);
    let dir = message_dir(cache_dir, account_id, message_id);
    let mut attachments: Vec<StoredAttachment> =
        raw.attachments.into_iter().map(|meta| StoredAttachment { meta, local_path: None }).collect();
    for (id, bytes) in raw.inline_data {
        if let Some(a) = attachments.iter_mut().find(|a| a.meta.id == id) {
            a.local_path = Some(write_attachment(&dir, &a.meta, &bytes)?.to_string_lossy().into_owned());
        }
    }
    let body = StoredBody {
        has_remote_content: sanitized.as_ref().is_some_and(|s| s.has_remote_content),
        html: sanitized.map(|s| s.html),
        text: raw.text,
        in_reply_to: raw.in_reply_to,
        references: raw.references,
        attachments,
        fetched_at: now_ms(),
    };
    lock(store).save_body(account_id, message_id, &body)?;
    Ok(body)
}

/// Local path of an attachment, downloading it first if needed.
pub async fn download_attachment<P: BodyProvider>(
    p: &P,
    account_id: &str,
    message_id: &str,
    attachment_id: &str,
    store: &Mutex<Store>,
    cache_dir: &Path,
) -> Result<PathBuf> {
    let known = lock(store).attachments(account_id, message_id)?;
    let att = known
        .into_iter()
        .find(|a| a.meta.id == attachment_id)
        .ok_or_else(|| Error::Other(format!("unknown attachment {attachment_id}; load the body first")))?;
    if let Some(path) = att.local_path.as_deref().map(PathBuf::from)
        && path.exists()
    {
        return Ok(path);
    }
    let bytes = p.attachment(message_id, attachment_id).await?;
    let path = write_attachment(&message_dir(cache_dir, account_id, message_id), &att.meta, &bytes)?;
    lock(store).set_attachment_path(account_id, message_id, attachment_id, &path.to_string_lossy())?;
    Ok(path)
}

fn short_hash(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn message_dir(cache_dir: &Path, account_id: &str, message_id: &str) -> PathBuf {
    cache_dir.join("attachments").join(short_hash(account_id)).join(short_hash(message_id))
}

/// Filesystem-safe version of an attachment name.
fn safe_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    let mut out: String = trimmed.chars().take(120).collect();
    if out.is_empty() {
        out = "attachment".into();
    }
    out
}

fn write_attachment(dir: &Path, meta: &AttachmentMeta, bytes: &[u8]) -> Result<PathBuf> {
    // Per-attachment subdir keeps the original file name for "open"/"save as".
    let dir = dir.join(short_hash(&meta.id));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(safe_name(&meta.name));
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::*;

    struct Fake {
        body_calls: AtomicUsize,
        attachment_calls: AtomicUsize,
    }

    impl BodyProvider for Fake {
        async fn body(&self, _: &str) -> Result<MessageBody> {
            self.body_calls.fetch_add(1, Ordering::SeqCst);
            Ok(MessageBody {
                html: Some(r#"<p>hi<script>x()</script><img src="https://t/p.gif"><img src="cid:logo"></p>"#.into()),
                text: Some("hi".into()),
                attachments: vec![
                    AttachmentMeta {
                        id: "1".into(),
                        name: "logo.png".into(),
                        content_id: Some("logo".into()),
                        is_inline: true,
                        ..Default::default()
                    },
                    AttachmentMeta { id: "2".into(), name: "../../etc/report.pdf".into(), ..Default::default() },
                ],
                inline_data: vec![("1".into(), b"PNG".to_vec())],
                in_reply_to: Some("<a@x>".into()),
                references: vec!["<a@x>".into()],
            })
        }

        async fn attachment(&self, _: &str, id: &str) -> Result<Vec<u8>> {
            self.attachment_calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("bytes of {id}").into_bytes())
        }
    }

    #[tokio::test]
    async fn loads_once_sanitizes_and_caches_files() {
        let cache = std::env::temp_dir().join(format!("pigeon-bodies-{}", crate::auth::random_token(6)));
        let store = Mutex::new(Store::open_in_memory().unwrap());
        let acct = Account {
            id: "google:a@b.c".into(),
            provider: ProviderKind::Google,
            email: "a@b.c".into(),
            display_name: String::new(),
        };
        {
            let mut s = store.lock().unwrap();
            s.upsert_account(&acct).unwrap();
            let h = MessageHeader { id: "m".into(), thread_id: "t".into(), ..Default::default() };
            s.apply_mail(&acct.id, "mail", &[MailChange::Upsert(h)], "c").unwrap();
        }
        let p = Fake { body_calls: AtomicUsize::new(0), attachment_calls: AtomicUsize::new(0) };

        let b = load_body(&p, &acct.id, "m", &store, &cache).await.unwrap();
        assert!(b.has_remote_content);
        assert!(!b.html.as_deref().unwrap().contains("script"));
        let inline = &b.attachments[0];
        assert_eq!(std::fs::read(inline.local_path.as_ref().unwrap()).unwrap(), b"PNG");
        assert_eq!(b.attachments[1].local_path, None);

        let again = load_body(&p, &acct.id, "m", &store, &cache).await.unwrap();
        assert_eq!(again, b);
        assert_eq!(p.body_calls.load(Ordering::SeqCst), 1);

        let path = download_attachment(&p, &acct.id, "m", "2", &store, &cache).await.unwrap();
        assert!(path.starts_with(&cache), "no path traversal: {path:?}");
        assert_eq!(path.file_name().unwrap(), "_.._etc_report.pdf");
        assert_eq!(std::fs::read(&path).unwrap(), b"bytes of 2");
        download_attachment(&p, &acct.id, "m", "2", &store, &cache).await.unwrap();
        assert_eq!(p.attachment_calls.load(Ordering::SeqCst), 1);

        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("a/b\\c:d.pdf"), "a_b_c_d.pdf");
        assert_eq!(safe_name("..hidden"), "hidden");
        assert_eq!(safe_name("  "), "attachment");
        assert_eq!(safe_name("דוח.pdf"), "דוח.pdf");
    }
}
