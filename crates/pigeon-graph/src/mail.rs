use std::collections::{HashMap, HashSet};

use chrono::{Duration, Utc};
use futures::future::join_all;
use pigeon_core::model::{Folder, FolderKind, FolderRole, MailChange, MessageHeader};
use pigeon_core::provider::{MailPage, MailProvider};
use pigeon_core::{Error, Result};

use crate::types::{self, Id, MailFolder, Page};
use crate::{BASE, Graph};

const SCOPE_PREFIX: &str = "folder:";

const MESSAGE_FIELDS: &str = "conversationId,internetMessageId,subject,from,toRecipients,ccRecipients,\
    bodyPreview,receivedDateTime,isRead,flag,hasAttachments,isDraft";

const WELL_KNOWN: [(&str, FolderRole); 6] = [
    ("inbox", FolderRole::Inbox),
    ("sentitems", FolderRole::Sent),
    ("drafts", FolderRole::Drafts),
    ("deleteditems", FolderRole::Trash),
    ("junkemail", FolderRole::Spam),
    ("archive", FolderRole::Archive),
];

impl MailProvider for Graph {
    async fn folders(&self) -> Result<Vec<Folder>> {
        let fields = "$select=id,displayName,parentFolderId,childFolderCount&$top=250";
        let mut raw: Vec<MailFolder> = self.get_all(&format!("{BASE}/me/mailFolders?{fields}")).await?;
        // Breadth-first through child folders.
        let mut i = 0;
        while i < raw.len() {
            if raw[i].child_folder_count > 0 {
                let url = format!("{BASE}/me/mailFolders/{}/childFolders?{fields}", raw[i].id);
                let children: Vec<MailFolder> = self.get_all(&url).await?;
                raw.extend(children);
            }
            i += 1;
        }

        // v1.0 has no wellKnownName on folders; resolve the aliases instead.
        // Missing ones (e.g. no archive folder) just fail and are skipped.
        let lookups = WELL_KNOWN.map(|(alias, role)| async move {
            let id: Result<Id> = self.get(&format!("{BASE}/me/mailFolders/{alias}?$select=id")).await;
            id.ok().map(|id| (id.id, role))
        });
        let roles: HashMap<String, FolderRole> = join_all(lookups).await.into_iter().flatten().collect();
        Ok(normalize_folders(raw, &roles))
    }

    fn mail_scopes(&self, folders: &[Folder]) -> Vec<String> {
        folders.iter().map(|f| format!("{SCOPE_PREFIX}{}", f.id)).collect()
    }

    async fn mail_page(&self, scope: &str, cursor: Option<&str>) -> Result<MailPage> {
        let folder_id =
            scope.strip_prefix(SCOPE_PREFIX).ok_or_else(|| Error::Other(format!("unknown Graph scope {scope}")))?;
        let url = match cursor {
            Some(c) => c.to_string(),
            None => {
                let since = (Utc::now() - Duration::days(self.mail_window_days)).format("%Y-%m-%dT%H:%M:%SZ");
                format!(
                    "{BASE}/me/mailFolders/{folder_id}/messages/delta?$select={MESSAGE_FIELDS}\
                     &$filter=receivedDateTime+ge+{since}&$orderby=receivedDateTime+desc"
                )
            }
        };
        let page: Page<types::Message> = self.get(&url).await.map_err(|e| match e {
            Error::Api { status: 410, .. } => Error::CursorExpired,
            Error::Api { ref body, .. } if body.contains("SyncStateNotFound") || body.contains("resyncRequired") => {
                Error::CursorExpired
            }
            e => e,
        })?;
        to_page(page, folder_id)
    }
}

fn normalize_folders(raw: Vec<MailFolder>, roles: &HashMap<String, FolderRole>) -> Vec<Folder> {
    let ids: HashSet<String> = raw.iter().map(|f| f.id.clone()).collect();
    raw.into_iter()
        .map(|f| Folder {
            role: roles.get(&f.id).copied(),
            // Top-level folders point at the hidden root; treat as no parent.
            parent_id: f.parent_folder_id.filter(|p| ids.contains(p)),
            name: f.display_name,
            id: f.id,
            kind: FolderKind::Folder,
        })
        .collect()
}

fn to_page(page: Page<types::Message>, folder_id: &str) -> Result<MailPage> {
    let changes = page.value.into_iter().map(|m| to_change(m, folder_id)).collect();
    let (cursor, more) = match (page.next_link, page.delta_link) {
        (Some(next), _) => (next, true),
        (None, Some(delta)) => (delta, false),
        (None, None) => return Err(Error::Other("Graph delta page without next/delta link".into())),
    };
    Ok(MailPage { changes, cursor, more })
}

fn to_change(m: types::Message, folder_id: &str) -> MailChange {
    if m.removed.is_some() {
        return MailChange::Unlink { id: m.id, folder_id: folder_id.to_string() };
    }
    MailChange::Upsert(MessageHeader {
        thread_id: m.conversation_id.unwrap_or_else(|| m.id.clone()),
        internet_message_id: m.internet_message_id,
        subject: m.subject.unwrap_or_default(),
        from: m.from.and_then(|r| r.email_address.to_address()),
        to: m.to_recipients.iter().filter_map(|r| r.email_address.to_address()).collect(),
        cc: m.cc_recipients.iter().filter_map(|r| r.email_address.to_address()).collect(),
        snippet: m.body_preview.unwrap_or_default(),
        received_at: m.received_date_time.as_deref().and_then(types::parse_rfc3339).unwrap_or(0),
        is_read: m.is_read.unwrap_or(false),
        is_flagged: m.flag.and_then(|f| f.flag_status).is_some_and(|s| s == "flagged"),
        has_attachments: m.has_attachments.unwrap_or(false),
        is_draft: m.is_draft.unwrap_or(false),
        folder_ids: vec![folder_id.to_string()],
        id: m.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_page_maps_upserts_and_removals() {
        let json = r#"{
          "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/F/messages/delta?$skiptoken=x",
          "value": [
            {"id": "AAMk1", "conversationId": "C1", "subject": "Hi",
             "from": {"emailAddress": {"name": "Ann", "address": "ann@x.com"}},
             "toRecipients": [{"emailAddress": {"name": "", "address": "me@x.com"}}],
             "ccRecipients": [{"emailAddress": {"name": "No Address"}}],
             "bodyPreview": "preview", "receivedDateTime": "2026-10-23T08:00:00Z",
             "isRead": false, "flag": {"flagStatus": "flagged"}, "hasAttachments": true, "isDraft": false},
            {"id": "AAMk2", "@removed": {"reason": "deleted"}}
          ]
        }"#;
        let page: Page<types::Message> = serde_json::from_str(json).unwrap();
        let out = to_page(page, "F").unwrap();
        assert!(out.more);
        assert!(out.cursor.contains("skiptoken"));
        let MailChange::Upsert(h) = &out.changes[0] else { panic!("expected upsert") };
        assert_eq!(h.thread_id, "C1");
        assert_eq!(h.from.as_ref().unwrap().display(), "Ann");
        assert_eq!(h.to[0].name, None);
        assert!(h.cc.is_empty());
        assert!(h.is_flagged && h.has_attachments && !h.is_read);
        assert_eq!(h.received_at, 1_792_742_400_000);
        assert_eq!(h.folder_ids, ["F"]);
        assert_eq!(out.changes[1], MailChange::Unlink { id: "AAMk2".into(), folder_id: "F".into() });
    }

    #[test]
    fn final_delta_page_stops() {
        let page: Page<types::Message> =
            serde_json::from_str(r#"{"@odata.deltaLink": "https://x/delta?$deltatoken=t", "value": []}"#).unwrap();
        let out = to_page(page, "F").unwrap();
        assert!(!out.more);
        assert!(out.cursor.ends_with("deltatoken=t"));
    }

    #[test]
    fn folders_get_roles_and_parents() {
        let raw = vec![
            MailFolder {
                id: "in".into(),
                display_name: "Inbox".into(),
                parent_folder_id: Some("root".into()),
                child_folder_count: 1,
            },
            MailFolder {
                id: "sub".into(),
                display_name: "Receipts".into(),
                parent_folder_id: Some("in".into()),
                child_folder_count: 0,
            },
        ];
        let roles = HashMap::from([("in".to_string(), FolderRole::Inbox)]);
        let f = normalize_folders(raw, &roles);
        assert_eq!((f[0].role, f[0].parent_id.as_deref()), (Some(FolderRole::Inbox), None));
        assert_eq!((f[1].role, f[1].parent_id.as_deref()), (None, Some("in")));
    }
}
