//! Drag messages (a row, or the whole multi-selection when the row is in
//! it) onto a sidebar folder or label to move them there. The move is the
//! ordinary mail action, so its toast offers Undo.
//!
//! WebKit needs data set on `dragstart` and `dragover` cancelled
//! synchronously before it fires `drop`, so that part runs in JS (`JS`);
//! which folders accept the drag (same account, not the folder being
//! viewed) is decided there from the rows' and folders' data attributes.

use dioxus::prelude::*;
use pigeon_core::model::{FolderInfo, FolderRole};
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use crate::state::AppState;

/// Installed once (sidebar). `data-mail-drag` on a row lists the accounts
/// it would carry (one per line); `data-folder-drop` on a folder names its
/// account. Hovering a closed Folders group or a collapsed account (marked
/// `data-spring`) for a moment opens it, like Finder's spring-loaded
/// folders.
pub const JS: &str = r#"(() => {
  let accounts = null, spring = null, springAt = 0;
  const done = () => {
    accounts = null; spring = null;
    delete document.documentElement.dataset.mailDragging;
    document.querySelectorAll('[data-drop-target]').forEach(x => delete x.dataset.dropTarget);
  };
  document.addEventListener('dragstart', e => {
    const r = e.target.closest && e.target.closest('[data-mail-drag]');
    if (!r) return;
    accounts = new Set(r.dataset.mailDrag.split('\n'));
    e.dataTransfer.setData('text/plain', r.dataset.dragLabel || '');
    e.dataTransfer.effectAllowed = 'move';
    const pill = document.createElement('div');
    pill.className = 'drag-pill';
    pill.textContent = r.dataset.dragLabel || '';
    document.body.appendChild(pill);
    e.dataTransfer.setDragImage(pill, 14, 14);
    setTimeout(() => pill.remove(), 0);
    document.documentElement.dataset.mailDragging = 'true';
  }, true);
  document.addEventListener('dragover', e => {
    if (!accounts || !e.target.closest) return;
    const f = e.target.closest('[data-folder-drop]');
    const ok = f && !f.dataset.folderCurrent && accounts.size === 1 && accounts.has(f.dataset.folderDrop);
    document.querySelectorAll('[data-drop-target]').forEach(x => { if (x !== f) delete x.dataset.dropTarget; });
    if (ok) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; f.dataset.dropTarget = 'true'; }
    else { e.dataTransfer.dropEffect = 'none'; }
    const sp = e.target.closest('[data-spring]');
    if (sp !== spring) { spring = sp; springAt = performance.now(); }
    else if (sp && performance.now() - springAt > 650) { spring = null; sp.click(); }
  }, true);
  document.addEventListener('drop', () => setTimeout(done, 0), true);
  document.addEventListener('dragend', done, true);
})();"#;

/// "3 conversations" / "3 messages".
fn count_label(n: usize, threaded: bool) -> String {
    match (threaded, n) {
        (true, 1) => "1 conversation".into(),
        (true, _) => format!("{n} conversations"),
        (false, 1) => "1 message".into(),
        (false, _) => format!("{n} messages"),
    }
}

/// The account part of an `AppState::key` (`account|id`).
fn key_account(key: &str) -> &str {
    key.split_once('|').map_or(key, |(a, _)| a)
}

/// A row's drag attributes: (accounts it carries, one per line; label on
/// the drag pill). A row inside the multi-selection carries all of it.
pub fn row_attrs(s: &AppState, item: &ListItem, subject: &str) -> (String, String) {
    let key = AppState::key(item);
    let multi = s.multi.read();
    if multi.len() >= 2 && multi.contains(&key) {
        let mut accounts: Vec<&str> = multi.iter().map(|k| key_account(k)).collect();
        accounts.sort_unstable();
        accounts.dedup();
        let threads = matches!(item, ListItem::Thread(_));
        return (accounts.join("\n"), count_label(multi.len(), threads));
    }
    (item.account_id().to_string(), subject.to_string())
}

/// What dropping on `folder` does: Trash and Archive are those actions (so
/// the toast reads "Deleted…" / "Archived…"), other real folders and labels
/// a move. Virtual ones (drafts, starred, important, Gmail categories)
/// take nothing.
pub fn drop_action(folder: &FolderInfo) -> Option<MailAction> {
    let f = &folder.folder;
    match f.role {
        Some(FolderRole::Drafts | FolderRole::Starred | FolderRole::Important) => None,
        _ if f.id.starts_with("CATEGORY_") => None,
        Some(FolderRole::Trash) => Some(MailAction::Trash),
        Some(FolderRole::Archive) => Some(MailAction::Archive),
        _ => Some(MailAction::MoveTo(f.id.clone())),
    }
}

/// The folder being viewed is no target: it's the folder itself, or the
/// account's folder of the unified mailbox's role.
pub fn is_current(mailbox: &Mailbox, account_id: &str, folder: &FolderInfo) -> bool {
    match mailbox {
        Mailbox::Folder { account_id: a, folder_id } => a == account_id && *folder_id == folder.folder.id,
        Mailbox::Unified(role) => folder.folder.role == Some(*role),
    }
}

/// Drag started on `item`: remember what it carries.
pub fn start(s: AppState, item: &ListItem) {
    let many = super::multi::items(&s);
    let items = if many.iter().any(|i| AppState::key(i) == AppState::key(item)) { many } else { vec![item.clone()] };
    let mut d = s.mail_drag;
    d.set(Some(items));
}

/// Dropped on `folder` of `account_id`.
pub fn drop_on(s: AppState, account_id: &str, folder: &FolderInfo) {
    let mut d = s.mail_drag;
    let Some(items) = d.write().take() else { return };
    if items.is_empty()
        || items.iter().any(|i| i.account_id() != account_id)
        || is_current(&s.mailbox.read(), account_id, folder)
    {
        return;
    }
    let Some(action) = drop_action(folder) else { return };
    if let MailAction::MoveTo(id) = &action {
        super::move_picker::remember(account_id, id);
    }
    match items.as_slice() {
        [one] if !s.multi.read().contains(&AppState::key(one)) => super::act(s, one, action),
        _ => super::multi::act_many(s, &items, action),
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Folder, FolderKind};

    use super::*;

    fn folder(id: &str, role: Option<FolderRole>) -> FolderInfo {
        FolderInfo {
            folder: Folder { id: id.into(), name: id.into(), parent_id: None, role, kind: FolderKind::Folder },
            total: 0,
            unread: 0,
        }
    }

    #[test]
    fn drops_move_archive_or_delete() {
        assert_eq!(drop_action(&folder("p", None)), Some(MailAction::MoveTo("p".into())));
        assert_eq!(drop_action(&folder("in", Some(FolderRole::Inbox))), Some(MailAction::MoveTo("in".into())));
        assert_eq!(drop_action(&folder("t", Some(FolderRole::Trash))), Some(MailAction::Trash));
        assert_eq!(drop_action(&folder("a", Some(FolderRole::Archive))), Some(MailAction::Archive));
        assert_eq!(drop_action(&folder("d", Some(FolderRole::Drafts))), None);
        assert_eq!(drop_action(&folder("s", Some(FolderRole::Starred))), None);
        assert_eq!(drop_action(&folder("CATEGORY_SOCIAL", None)), None);
    }

    #[test]
    fn the_viewed_folder_takes_no_drop() {
        let inbox = folder("in", Some(FolderRole::Inbox));
        assert!(is_current(&Mailbox::Unified(FolderRole::Inbox), "a", &inbox));
        assert!(!is_current(&Mailbox::Unified(FolderRole::Archive), "a", &inbox));
        let here = Mailbox::Folder { account_id: "a".into(), folder_id: "in".into() };
        assert!(is_current(&here, "a", &inbox));
        assert!(!is_current(&here, "b", &inbox));
    }

    #[test]
    fn labels_and_accounts() {
        assert_eq!(count_label(3, true), "3 conversations");
        assert_eq!(count_label(1, false), "1 message");
        assert_eq!(key_account("google:a@b.c|t|1"), "google:a@b.c");
    }
}
