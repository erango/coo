//! Spam / junk actions: which ones to offer for what's selected (per the
//! account's provider and whether we're looking at its spam folder), the
//! toast wording, and the `!` toggle.

use dioxus::prelude::*;
use pigeon_core::model::{FolderRole, ProviderKind};
use pigeon_engine::MailAction;
use pigeon_engine::service::{AccountNav, ListItem, Mailbox};

use super::{act, multi, selected_item};
use crate::state::AppState;

/// `mailbox` is this account's spam folder (or the unified Spam).
fn in_spam(mailbox: &Mailbox, account: &AccountNav) -> bool {
    super::viewing(mailbox, account, FolderRole::Spam)
}

/// The spam actions every one of `account_ids` supports, in menu order
/// (`Spam` or `NotSpam`, then `Phishing`).
pub fn offered(accounts: &[AccountNav], mailbox: &Mailbox, account_ids: &[&str]) -> Vec<MailAction> {
    let mut out: Option<Vec<MailAction>> = None;
    for id in account_ids {
        let Some(a) = accounts.iter().find(|a| a.account.id == *id) else { return Vec::new() };
        let has_spam = a.folders.iter().any(|f| f.folder.role == Some(FolderRole::Spam));
        let these = pigeon_engine::spam_actions(a.account.provider, has_spam, in_spam(mailbox, a));
        out = Some(match out {
            None => these,
            Some(prev) => prev.into_iter().filter(|x| these.contains(x)).collect(),
        });
    }
    out.unwrap_or_default()
}

/// Every one of `account_ids` reports to its provider (not just a move).
fn reports(accounts: &[AccountNav], account_ids: &[&str]) -> bool {
    let provider = |id: &&str| accounts.iter().find(|a| a.account.id == *id).map(|a| a.account.provider);
    account_ids.iter().all(|id| provider(id).is_some_and(|p: ProviderKind| p.spam_support().reports))
}

/// Toast for a spam action on `what` ("“Subject”", "3 messages"). Reporting
/// phishing can't be taken back on the server, so its toast says Undo only
/// moves the message back.
pub fn label(action: &MailAction, reports: bool, what: &str) -> Option<String> {
    Some(match action {
        MailAction::Spam if reports => format!("Reported {what} as spam"),
        MailAction::Spam => format!("Moved {what} to Junk"),
        MailAction::NotSpam if reports => format!("Reported {what} as not spam, moved to Inbox"),
        MailAction::NotSpam => format!("Moved {what} to Inbox"),
        MailAction::Phishing => format!("Reported {what} as phishing. Undo only moves it back"),
        _ => return None,
    })
}

/// [`label`] for these items' accounts.
pub fn label_for(s: &AppState, items: &[ListItem], action: &MailAction, what: &str) -> Option<String> {
    let ids: Vec<&str> = items.iter().map(|i| i.account_id()).collect();
    label(action, reports(&s.accounts.read(), &ids), what)
}

/// What's offered for these list items.
pub fn for_items(s: &AppState, items: &[ListItem]) -> Vec<MailAction> {
    if items.is_empty() {
        return Vec::new();
    }
    let ids: Vec<&str> = items.iter().map(|i| i.account_id()).collect();
    offered(&s.accounts.read(), &s.mailbox.read(), &ids)
}

/// `!`: spam, or not spam in the spam folder, for the selection or the open
/// message.
pub fn toggle(s: AppState) {
    let many = multi::items(&s);
    let items = if many.is_empty() { selected_item(&s).into_iter().collect() } else { many.clone() };
    let Some(action) = for_items(&s, &items).into_iter().find(|a| *a != MailAction::Phishing) else {
        if !items.is_empty() {
            s.local_toast("This account has no junk folder", false);
        }
        return;
    };
    if many.is_empty() {
        act(s, &items[0], action);
    } else {
        multi::act_many(s, &items, action);
    }
}

/// Message › Report Phishing.
pub fn report_phishing(s: AppState) {
    let many = multi::items(&s);
    let items = if many.is_empty() { selected_item(&s).into_iter().collect() } else { many.clone() };
    if items.is_empty() {
        return;
    }
    if !for_items(&s, &items).contains(&MailAction::Phishing) {
        return s.local_toast("Reporting phishing isn't available for this account", false);
    }
    if many.is_empty() {
        act(s, &items[0], MailAction::Phishing);
    } else {
        multi::act_many(s, &items, MailAction::Phishing);
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Account, Folder, FolderInfo, FolderKind};

    use super::*;

    fn nav(provider: ProviderKind, id: &str, spam: bool) -> AccountNav {
        let f = |fid: &str, role| FolderInfo {
            folder: Folder {
                id: fid.into(),
                name: fid.into(),
                parent_id: None,
                role: Some(role),
                kind: FolderKind::Folder,
            },
            total: 0,
            unread: 0,
        };
        let mut folders = vec![f("in", FolderRole::Inbox)];
        if spam {
            folders.push(f("junk", FolderRole::Spam));
        }
        AccountNav {
            account: Account { id: id.into(), provider, email: format!("{id}@x.io"), display_name: String::new() },
            hue: 1,
            folders,
            first_sync_done: true,
            main_address: None,
        }
    }

    #[test]
    fn offers_what_every_selected_account_supports() {
        use MailAction::*;
        let accts = [
            nav(ProviderKind::Microsoft, "ms", true),
            nav(ProviderKind::Google, "g", true),
            nav(ProviderKind::ICloud, "ic", false),
        ];
        let inbox = Mailbox::Unified(FolderRole::Inbox);
        assert_eq!(offered(&accts, &inbox, &["ms"]), [Spam, Phishing]);
        assert_eq!(offered(&accts, &inbox, &["g"]), [Spam]);
        assert_eq!(offered(&accts, &inbox, &["ms", "g"]), [Spam], "no phishing for the Gmail half");
        assert!(offered(&accts, &inbox, &["ic"]).is_empty(), "no junk folder");
        assert!(offered(&accts, &inbox, &["ms", "ic"]).is_empty());
        assert!(offered(&accts, &inbox, &["gone"]).is_empty());

        let junk = Mailbox::Folder { account_id: "ms".into(), folder_id: "junk".into() };
        assert_eq!(offered(&accts, &junk, &["ms"]), [NotSpam]);
        let other = Mailbox::Folder { account_id: "ms".into(), folder_id: "in".into() };
        assert_eq!(offered(&accts, &other, &["ms"]), [Spam, Phishing]);
        assert_eq!(offered(&accts, &Mailbox::Unified(FolderRole::Spam), &["g"]), [NotSpam]);
    }

    #[test]
    fn toast_wording_says_what_happened() {
        assert_eq!(label(&MailAction::Spam, true, "“Hi”").unwrap(), "Reported “Hi” as spam");
        assert_eq!(label(&MailAction::Spam, false, "“Hi”").unwrap(), "Moved “Hi” to Junk");
        assert!(label(&MailAction::Phishing, true, "“Hi”").unwrap().contains("Undo only moves it back"));
        assert_eq!(label(&MailAction::Archive, true, "x"), None);
        let accts = [nav(ProviderKind::Google, "g", true), nav(ProviderKind::ICloud, "ic", true)];
        assert!(reports(&accts, &["g"]) && !reports(&accts, &["g", "ic"]));
    }
}
