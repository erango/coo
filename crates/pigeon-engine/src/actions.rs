//! User-level mail actions, resolved into provider-shaped ops.

use pigeon_core::model::{FolderRole, MailOp, ProviderKind};
use pigeon_core::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailAction {
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    Archive,
    Trash,
    /// Move to a folder (Exchange) or label (Gmail).
    MoveTo(String),
    /// Add a label without leaving the current one (Gmail only).
    AddLabel(String),
}

/// Translate `action` for this provider. `from_folder` is the folder being
/// viewed (Gmail "move" removes that label); `role` looks up role folders.
pub fn resolve(
    provider: ProviderKind,
    action: &MailAction,
    ids: Vec<String>,
    from_folder: Option<&str>,
    role: impl Fn(FolderRole) -> Option<String>,
) -> Result<MailOp> {
    let need = |r: FolderRole| role(r).ok_or_else(|| Error::Other(format!("account has no {} folder", r.as_str())));
    Ok(match (action, provider) {
        (MailAction::MarkRead, _) => MailOp::SetRead { ids, read: true },
        (MailAction::MarkUnread, _) => MailOp::SetRead { ids, read: false },
        (MailAction::Flag, _) => MailOp::SetFlagged { ids, flagged: true },
        (MailAction::Unflag, _) => MailOp::SetFlagged { ids, flagged: false },
        (MailAction::Archive, ProviderKind::Microsoft | ProviderKind::ICloud) => {
            MailOp::MoveTo { ids, folder_id: need(FolderRole::Archive)? }
        }
        (MailAction::Archive, ProviderKind::Google) => {
            MailOp::Relabel { ids, add: vec![], remove: vec![need(FolderRole::Inbox)?] }
        }
        (MailAction::Trash, ProviderKind::Microsoft | ProviderKind::ICloud) => {
            MailOp::MoveTo { ids, folder_id: need(FolderRole::Trash)? }
        }
        (MailAction::Trash, ProviderKind::Google) => MailOp::Trash { ids },
        (MailAction::MoveTo(folder_id), ProviderKind::Microsoft | ProviderKind::ICloud) => {
            MailOp::MoveTo { ids, folder_id: folder_id.clone() }
        }
        (MailAction::MoveTo(folder_id), ProviderKind::Google) => {
            // Leaving a place-like label; virtual ones (starred, sent...) stay.
            let sticky = [FolderRole::Starred, FolderRole::Important, FolderRole::Sent, FolderRole::Drafts]
                .into_iter()
                .filter_map(&role)
                .collect::<Vec<_>>();
            let remove = from_folder
                .filter(|f| *f != folder_id && !sticky.iter().any(|s| s == f))
                .map(|f| vec![f.to_string()])
                .unwrap_or_default();
            MailOp::Relabel { ids, add: vec![folder_id.clone()], remove }
        }
        (MailAction::AddLabel(label), ProviderKind::Google) => {
            MailOp::Relabel { ids, add: vec![label.clone()], remove: vec![] }
        }
        (MailAction::AddLabel(_), _) => return Err(Error::Other("labels are a Gmail feature; use Move".into())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(r: FolderRole) -> Option<String> {
        match r {
            FolderRole::Inbox => Some("INBOX".into()),
            FolderRole::Archive => Some("arch".into()),
            FolderRole::Trash => Some("TRASH".into()),
            FolderRole::Starred => Some("STARRED".into()),
            _ => None,
        }
    }

    fn ids() -> Vec<String> {
        vec!["m".into()]
    }

    #[test]
    fn add_label_is_gmail_only_and_keeps_current() {
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::AddLabel("L1".into()), ids(), Some("INBOX"), roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["L1".into()], remove: vec![] }
        );
        assert!(resolve(ProviderKind::Microsoft, &MailAction::AddLabel("x".into()), ids(), None, roles).is_err());
    }

    #[test]
    fn archive_and_trash_per_provider() {
        assert_eq!(
            resolve(ProviderKind::Microsoft, &MailAction::Archive, ids(), None, roles).unwrap(),
            MailOp::MoveTo { ids: ids(), folder_id: "arch".into() }
        );
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::Archive, ids(), None, roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec![], remove: vec!["INBOX".into()] }
        );
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::Trash, ids(), None, roles).unwrap(),
            MailOp::Trash { ids: ids() }
        );
        assert!(resolve(ProviderKind::Microsoft, &MailAction::Archive, ids(), None, |_| None).is_err());
    }

    #[test]
    fn gmail_move_swaps_labels_but_keeps_virtual_ones() {
        let mv = MailAction::MoveTo("L1".into());
        assert_eq!(
            resolve(ProviderKind::Google, &mv, ids(), Some("INBOX"), roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["L1".into()], remove: vec!["INBOX".into()] }
        );
        assert_eq!(
            resolve(ProviderKind::Google, &mv, ids(), Some("STARRED"), roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["L1".into()], remove: vec![] }
        );
    }
}
