//! User-level mail actions, resolved into provider-shaped ops.

use pigeon_core::model::{FolderRole, MailOp, ProviderKind, SpamReport};
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
    /// Report as spam/junk (where the provider takes reports) and move to
    /// the spam folder.
    Spam,
    /// Out of spam, back to the inbox, reported as not junk where possible.
    NotSpam,
    /// Report phishing and move to the spam folder (providers whose
    /// [`SpamSupport::phishing`](pigeon_core::model::SpamSupport) is set).
    Phishing,
}

impl MailAction {
    /// Takes the message out of the list being viewed.
    pub fn leaves(&self) -> bool {
        matches!(
            self,
            MailAction::Archive
                | MailAction::Trash
                | MailAction::MoveTo(_)
                | MailAction::Spam
                | MailAction::NotSpam
                | MailAction::Phishing
        )
    }
}

/// The spam actions to offer for one account: `Spam` or, while viewing its
/// spam folder, `NotSpam`; plus `Phishing` where the provider has it
/// (outside the spam folder: a report also moves there, and a move into the
/// folder it's already in is a needless server round trip). Nothing
/// without a spam folder to move to.
pub fn spam_actions(provider: ProviderKind, has_spam_folder: bool, in_spam: bool) -> Vec<MailAction> {
    if !has_spam_folder {
        return Vec::new();
    }
    let mut v = vec![if in_spam { MailAction::NotSpam } else { MailAction::Spam }];
    if provider.spam_support().phishing && !in_spam {
        v.push(MailAction::Phishing);
    }
    v
}

/// Coo's Gmail access can trash but not delete (that takes Gmail's
/// full-access permission); Gmail empties Trash itself after 30 days.
pub const GMAIL_PURGE: &str = "Gmail deletes messages in Trash for good after 30 days";

/// Whether Delete while viewing `from_folder` deletes for good.
pub fn purges(provider: ProviderKind, from_folder: Option<&str>, trash: Option<&str>) -> bool {
    provider != ProviderKind::Google && from_folder.is_some() && from_folder == trash
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
    // Gmail labels that aren't places (leaving them keeps the message there).
    let sticky = || {
        [FolderRole::Starred, FolderRole::Important, FolderRole::Sent, FolderRole::Drafts]
            .into_iter()
            .filter_map(&role)
            .collect::<Vec<_>>()
    };
    let report = |ids: Vec<String>, report: SpamReport, to: FolderRole| -> Result<MailOp> {
        Ok(MailOp::Report { ids, report, folder_id: need(to)? })
    };
    // Delete in Trash means delete for good.
    if *action == MailAction::Trash && from_folder.is_some_and(|f| role(FolderRole::Trash).as_deref() == Some(f)) {
        if provider == ProviderKind::Google {
            return Err(Error::Other(GMAIL_PURGE.into()));
        }
        return Ok(MailOp::Purge { ids });
    }
    Ok(match (action, provider) {
        (MailAction::Spam, ProviderKind::Microsoft) => report(ids, SpamReport::Junk, FolderRole::Spam)?,
        (MailAction::NotSpam, ProviderKind::Microsoft) => report(ids, SpamReport::NotJunk, FolderRole::Inbox)?,
        (MailAction::Phishing, ProviderKind::Microsoft) => report(ids, SpamReport::Phishing, FolderRole::Spam)?,
        (MailAction::Spam, ProviderKind::ICloud) => MailOp::MoveTo { ids, folder_id: need(FolderRole::Spam)? },
        (MailAction::NotSpam, ProviderKind::ICloud) => MailOp::MoveTo { ids, folder_id: need(FolderRole::Inbox)? },
        // The SPAM label is Gmail's spam report; it also leaves the inbox
        // and the place-like label it was listed under.
        (MailAction::Spam, ProviderKind::Google) => {
            let spam = need(FolderRole::Spam)?;
            let mut remove = vec![need(FolderRole::Inbox)?];
            let sticky = sticky();
            if let Some(f) = from_folder.filter(|f| *f != spam && !remove.iter().chain(&sticky).any(|s| s == f)) {
                remove.push(f.to_string());
            }
            MailOp::Relabel { ids, add: vec![spam], remove }
        }
        (MailAction::NotSpam, ProviderKind::Google) => {
            MailOp::Relabel { ids, add: vec![need(FolderRole::Inbox)?], remove: vec![need(FolderRole::Spam)?] }
        }
        (MailAction::Phishing, _) => {
            return Err(Error::Other("this account's provider has no way to report phishing".into()));
        }
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
            let sticky = sticky();
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
    fn delete_in_trash_is_for_good() {
        for p in [ProviderKind::Microsoft, ProviderKind::ICloud] {
            assert_eq!(
                resolve(p, &MailAction::Trash, ids(), Some("TRASH"), roles).unwrap(),
                MailOp::Purge { ids: ids() }
            );
            assert!(purges(p, Some("TRASH"), Some("TRASH")));
            assert!(!purges(p, Some("INBOX"), Some("TRASH")));
            assert!(!purges(p, None, None));
        }
        assert!(resolve(ProviderKind::Google, &MailAction::Trash, ids(), Some("TRASH"), roles).is_err());
        assert!(!purges(ProviderKind::Google, Some("TRASH"), Some("TRASH")));
        // Elsewhere it's still a move to Trash.
        assert_eq!(
            resolve(ProviderKind::ICloud, &MailAction::Trash, ids(), Some("INBOX"), roles).unwrap(),
            MailOp::MoveTo { ids: ids(), folder_id: "TRASH".into() }
        );
    }

    fn spam_roles(r: FolderRole) -> Option<String> {
        match r {
            FolderRole::Spam => Some("SPAM".into()),
            r => roles(r),
        }
    }

    #[test]
    fn gmail_spam_is_the_spam_label() {
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::Spam, ids(), Some("INBOX"), spam_roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["SPAM".into()], remove: vec!["INBOX".into()] }
        );
        // Listed under a user label: leaves it too; under Starred: stays starred.
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::Spam, ids(), Some("L1"), spam_roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["SPAM".into()], remove: vec!["INBOX".into(), "L1".into()] }
        );
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::Spam, ids(), Some("STARRED"), spam_roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["SPAM".into()], remove: vec!["INBOX".into()] }
        );
        assert_eq!(
            resolve(ProviderKind::Google, &MailAction::NotSpam, ids(), Some("SPAM"), spam_roles).unwrap(),
            MailOp::Relabel { ids: ids(), add: vec!["INBOX".into()], remove: vec!["SPAM".into()] }
        );
        assert!(resolve(ProviderKind::Google, &MailAction::Phishing, ids(), None, spam_roles).is_err());
    }

    #[test]
    fn exchange_reports_and_icloud_moves() {
        let report = |report, folder_id: &str| MailOp::Report { ids: ids(), report, folder_id: folder_id.into() };
        let ms = |a: &MailAction| resolve(ProviderKind::Microsoft, a, ids(), Some("INBOX"), spam_roles).unwrap();
        assert_eq!(ms(&MailAction::Spam), report(SpamReport::Junk, "SPAM"));
        assert_eq!(ms(&MailAction::Phishing), report(SpamReport::Phishing, "SPAM"));
        assert_eq!(ms(&MailAction::NotSpam), report(SpamReport::NotJunk, "INBOX"));
        let ic = |a: &MailAction| resolve(ProviderKind::ICloud, a, ids(), Some("INBOX"), spam_roles);
        assert_eq!(ic(&MailAction::Spam).unwrap(), MailOp::MoveTo { ids: ids(), folder_id: "SPAM".into() });
        assert_eq!(ic(&MailAction::NotSpam).unwrap(), MailOp::MoveTo { ids: ids(), folder_id: "INBOX".into() });
        assert!(ic(&MailAction::Phishing).is_err());
        assert!(resolve(ProviderKind::Microsoft, &MailAction::Spam, ids(), None, roles).is_err(), "no junk folder");
    }

    #[test]
    fn spam_actions_follow_provider_support() {
        use MailAction::*;
        assert_eq!(spam_actions(ProviderKind::Microsoft, true, false), [Spam, Phishing]);
        assert_eq!(spam_actions(ProviderKind::Microsoft, true, true), [NotSpam]);
        assert_eq!(spam_actions(ProviderKind::Google, true, false), [Spam]);
        assert_eq!(spam_actions(ProviderKind::Google, true, true), [NotSpam]);
        assert_eq!(spam_actions(ProviderKind::ICloud, true, false), [Spam]);
        assert!(spam_actions(ProviderKind::Microsoft, false, false).is_empty());
        assert!(ProviderKind::Google.spam_support().reports && !ProviderKind::ICloud.spam_support().reports);
        assert!(Spam.leaves() && NotSpam.leaves() && Phishing.leaves() && !Flag.leaves());
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
