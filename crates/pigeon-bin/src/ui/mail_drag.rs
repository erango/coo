//! Drag messages (a row, or the whole multi-selection when the row is in
//! it) onto a sidebar folder or label to move them there. The move is the
//! ordinary mail action, so its toast offers Undo.
//!
//! The drag is a native one (dragout.rs), so a message dropped on Finder is
//! saved there as .eml; inside Coo the page decides which folders accept it
//! (same account, not the folder being viewed) from the rows' and folders'
//! data attributes (`JS`).

use dioxus::prelude::*;
use pigeon_core::model::{FolderInfo, FolderKind, FolderRole};
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use crate::state::AppState;

/// The page side of a message drag, installed with the bridge (main
/// window). `data-mail-drag` on a row lists the accounts it would carry (one
/// per line) and `data-mail-key` names it; `data-folder-drop` on a folder
/// names its account. A press and a 5 px move on a row asks Rust for the
/// native drag (`mail-drag`, dragout.rs), which then reports the pointer
/// (`__pigeonMailDragAt`), a release inside Coo (`__pigeonMailDropAt`: the
/// folder or composer there gets a `drop` event, handled as before) and the
/// end. Hovering a closed Folders group or a collapsed account (marked
/// `data-spring`) for a moment opens it, like Finder's spring-loaded folders.
pub const JS: &str = r#"
  let mailAccounts = null, spring = null, springAt = 0, press = null;
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || !e.target.closest) return;
    const r = e.target.closest('[data-mail-drag]');
    press = r && !e.target.closest('button, a, input') ? { r, x: e.clientX, y: e.clientY } : null;
  }, true);
  document.addEventListener('mousemove', (e) => {
    if (!press) return;
    if (!(e.buttons & 1)) { press = null; return; }
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
    mailAccounts = new Set(press.r.dataset.mailDrag.split('\n'));
    document.documentElement.dataset.mailDragging = 'true';
    send('mail-drag', press.r.dataset.mailKey);
    press = null;
  }, true);
  document.addEventListener('mouseup', () => { press = null; }, true);
  const mailTarget = (x, y) => {
    const el = document.elementFromPoint(x, y);
    if (!el || !el.closest || !mailAccounts) return null;
    const c = el.closest('[data-mail-attach]');
    if (c) return c;
    const f = el.closest('[data-folder-drop]');
    return f && !f.dataset.folderCurrent && mailAccounts.size === 1 && mailAccounts.has(f.dataset.folderDrop) ? f : null;
  };
  window.__pigeonMailDragAt = (x, y) => {
    const t = mailTarget(x, y);
    document.querySelectorAll('[data-drop-target]').forEach((n) => { if (n !== t) delete n.dataset.dropTarget; });
    if (t) t.dataset.dropTarget = 'true';
    const el = document.elementFromPoint(x, y);
    const sp = el && el.closest && el.closest('[data-spring]');
    if (sp !== spring) { spring = sp; springAt = performance.now(); }
    else if (sp && performance.now() - springAt > 650) { spring = null; sp.click(); }
  };
  window.__pigeonMailDropAt = (x, y) => {
    const t = mailTarget(x, y);
    if (t) t.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }));
  };
  window.__pigeonMailDragEnd = () => {
    mailAccounts = null; spring = null;
    delete document.documentElement.dataset.mailDragging;
    document.querySelectorAll('[data-drop-target]').forEach((n) => delete n.dataset.dropTarget);
    send('mail-drag-end', '');
  };
"#;

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
/// take nothing. On a Gmail label of your own, the message gets the label.
pub fn drop_action(folder: &FolderInfo) -> Option<MailAction> {
    let f = &folder.folder;
    match f.role {
        Some(FolderRole::Drafts | FolderRole::Starred | FolderRole::Important) => None,
        _ if f.id.starts_with("CATEGORY_") => None,
        Some(FolderRole::Trash) => Some(MailAction::Trash),
        Some(FolderRole::Archive) => Some(MailAction::Archive),
        // Your own Gmail labels: add the label, the message stays put.
        None if f.kind == FolderKind::Label => Some(MailAction::AddLabel(f.id.clone())),
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
        let mut label = folder("Label_1", None);
        label.folder.kind = FolderKind::Label;
        assert_eq!(drop_action(&label), Some(MailAction::AddLabel("Label_1".into())));
        let mut inbox = folder("INBOX", Some(FolderRole::Inbox));
        inbox.folder.kind = FolderKind::Label;
        assert_eq!(drop_action(&inbox), Some(MailAction::MoveTo("INBOX".into())));
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
