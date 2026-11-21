//! Mail rules (`pigeon_core::rules`): saving them as Gmail filters or
//! Outlook rules where the server can run them, applying the rest as mail
//! lands, doing a rule to mail that's already here, and undoing what a rule
//! did.

use pigeon_core::auth::{self, MS_RULES_SCOPE, OAuthApp, ms_has_scope};
use pigeon_core::model::{Account, FolderRole, MessageRow, ProviderKind};
use pigeon_core::rules::{Logged, Rule, Runs, ServerRule, Then};
use pigeon_core::watches::Watch;
use pigeon_core::{Error, Result, now_ms};

use crate::actions::MailAction;
use crate::{Backend, Pigeon};

/// "Already matches" looks this far back.
pub const MATCH_WINDOW_MS: i64 = 90 * 24 * 3_600_000;
/// At most this many recent messages are checked for a rule's matches.
const MATCH_SCAN: u32 = 20_000;

/// What a rule matches among the mail that's here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Matches {
    /// Matches in the last 90 days.
    pub total: usize,
    /// Of those, still in the inbox: (account id, message id).
    pub inbox: Vec<(String, String)>,
    /// The newest few, for a preview.
    pub samples: Vec<MessageRow>,
}

impl Pigeon {
    /// Whether Coo may save rules on this account's server.
    pub fn may_save_rules(&self, account: &Account) -> bool {
        match account.provider {
            ProviderKind::Google => true,
            ProviderKind::Microsoft => self
                .store()
                .account_scopes(&account.id)
                .ok()
                .flatten()
                .is_some_and(|s| ms_has_scope(&s, MS_RULES_SCOPE)),
            ProviderKind::ICloud => false,
        }
    }

    /// Microsoft accounts that allow Coo to save Outlook rules.
    pub fn outlook_rule_accounts(&self) -> Vec<String> {
        self.accounts()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.provider == ProviderKind::Microsoft && self.may_save_rules(a))
            .map(|a| a.id)
            .collect()
    }

    /// Where `rule` runs now.
    pub fn rule_runs(&self, rule: &Rule) -> Runs {
        let account = rule.account_id.as_deref().and_then(|id| self.account(id).ok());
        let may = account.as_ref().is_some_and(|a| self.may_save_rules(a));
        rule.runs(account.map(|a| a.provider), may)
    }

    /// Save a rule: on the server when it can run there (replacing the
    /// copy saved before, or `rule.server_id` when Coo takes over a filter
    /// made elsewhere), else in Coo. A failed server save leaves it running
    /// in Coo with `problem` set. Returns the rule as saved.
    pub async fn save_rule(&self, mut rule: Rule) -> Result<Rule> {
        if let Some(why) = rule.check() {
            return Err(Error::Other(why.into()));
        }
        rule.name = rule.name.trim().to_string();
        if rule.name.is_empty() {
            rule.name = default_name(&rule);
        }
        let old = self.store().rule(&rule.id)?;
        let replaced = old.as_ref().and_then(|o| o.server_id.clone()).or(rule.server_id.take());
        rule.problem = None;
        let runs = self.rule_runs(&rule);
        if let (Runs::Server(_), true) = (runs, rule.enabled) {
            match self.put_server_rule(&rule).await {
                Ok(id) => rule.server_id = Some(id),
                Err(e) => {
                    tracing::warn!(error = %e, "could not save a rule on the server");
                    rule.problem = Some(e.to_string());
                }
            }
        }
        if let Some(old_id) = replaced.filter(|o| rule.server_id.as_ref() != Some(o)) {
            let account = self.account(
                rule.account_id.as_deref().or(old.as_ref().and_then(|o| o.account_id.as_deref())).unwrap_or_default(),
            );
            if let Ok(account) = account
                && let Err(e) = self.delete_server_rule(&account, &old_id).await
            {
                tracing::warn!(error = %e, "could not remove the rule's old server copy");
            }
        }
        {
            let store = self.store();
            store.save_rule(&rule)?;
            // "Send to Needs you" catches through a watch of the same id.
            if rule.then.needs_you {
                let mut w = store
                    .watch(&rule.id)?
                    .unwrap_or_else(|| Watch::new(&rule.name, Default::default(), rule.created_at));
                w.id = rule.id.clone();
                w.name = rule.name.clone();
                w.rule = true;
                w.paused = !rule.enabled;
                w.created_by = rule.created_by.clone();
                store.save_watch(&w)?;
            } else {
                store.delete_watch(&rule.id)?;
            }
        }
        Ok(rule)
    }

    /// Delete a rule and its server copy.
    pub async fn delete_rule(&self, id: &str) -> Result<()> {
        let Some(rule) = self.store().rule(id)? else { return Ok(()) };
        if let (Some(server), Some(account)) = (&rule.server_id, &rule.account_id) {
            let account = self.account(account)?;
            self.delete_server_rule(&account, server).await?;
        }
        let store = self.store();
        store.delete_rule(id)?;
        store.delete_watch(id)?;
        Ok(())
    }

    async fn put_server_rule(&self, rule: &Rule) -> Result<String> {
        let account = self.account(rule.account_id.as_deref().unwrap_or_default())?;
        match &*self.backend(&account)? {
            Backend::Google(g) => {
                g.create_gmail_filter(&pigeon_google::filters::to_gmail_filter(&rule.when, &rule.then)).await
            }
            Backend::Microsoft(m) => {
                let archive = self.store().role_folder(&account.id, FolderRole::Archive)?;
                if rule.then.archive && rule.then.label.is_none() && archive.is_none() {
                    return Err(Error::Other("this account has no Archive folder".into()));
                }
                let sequence = self.store().rules()?.len() as u32 + 1;
                let body = pigeon_graph::rules::to_outlook_rule(
                    &rule.name,
                    &rule.when,
                    &rule.then,
                    archive.as_deref(),
                    sequence,
                );
                m.create_outlook_rule(&body).await
            }
            Backend::ICloud(_) => Err(Error::Other("iCloud can't save rules".into())),
        }
    }

    async fn delete_server_rule(&self, account: &Account, id: &str) -> Result<()> {
        match &*self.backend(account)? {
            Backend::Google(g) => g.delete_gmail_filter(id).await,
            Backend::Microsoft(m) => m.delete_outlook_rule(id).await,
            Backend::ICloud(_) => Ok(()),
        }
    }

    /// The account's Gmail filters or Outlook rules that Coo didn't make.
    pub async fn server_rules(&self, account: &Account) -> Result<Vec<ServerRule>> {
        let ours: Vec<String> = self.store().rules()?.into_iter().filter_map(|r| r.server_id).collect();
        let folders: Vec<_> = self.store().folders(&account.id)?.into_iter().map(|f| f.folder).collect();
        let all: Vec<ServerRule> = match &*self.backend(account)? {
            Backend::Google(g) => g
                .gmail_filters()
                .await?
                .iter()
                .filter_map(|v| pigeon_google::filters::from_gmail_filter(v, &folders))
                .collect(),
            Backend::Microsoft(m) => {
                if !self.may_save_rules(account) {
                    return Ok(Vec::new());
                }
                let archive = self.store().role_folder(&account.id, FolderRole::Archive)?;
                m.outlook_rules()
                    .await?
                    .iter()
                    .filter_map(|v| pigeon_graph::rules::from_outlook_rule(v, &folders, archive.as_deref()))
                    .collect()
            }
            Backend::ICloud(_) => Vec::new(),
        };
        Ok(all.into_iter().filter(|r| !ours.contains(&r.id)).collect())
    }

    /// Ask Microsoft (in the browser) to let Coo save Outlook rules for an
    /// account that's already signed in.
    pub async fn allow_outlook_rules(&self, account_id: &str, on_url: impl Fn(&str)) -> Result<()> {
        let account = self.account(account_id)?;
        if account.provider != ProviderKind::Microsoft {
            return Err(Error::Other("Outlook rules need a Microsoft account".into()));
        }
        let granted = self.store().account_scopes(&account.id)?.unwrap_or_default();
        let app = OAuthApp::from_env(ProviderKind::Microsoft)?.microsoft_add_scopes(
            &account.email,
            &granted,
            &[MS_RULES_SCOPE],
        );
        let token = auth::authorize(&app, &on_url).await?;
        let (email, _) = pigeon_graph::whoami(&token.access_token).await?;
        if !email.eq_ignore_ascii_case(&account.email) {
            return Err(Error::Auth(format!(
                "That was a different Microsoft account. Choose {} to save its rules in Outlook.",
                account.email
            )));
        }
        let scopes = token.scope.clone().unwrap_or_default();
        if !ms_has_scope(&scopes, MS_RULES_SCOPE) {
            return Err(Error::Auth("Outlook didn't allow Coo to save rules. Try again and accept.".into()));
        }
        let refresh =
            token.refresh_token.as_deref().ok_or_else(|| Error::Auth("Microsoft returned no refresh token".into()))?;
        self.tokens.set(&account.id, refresh)?;
        self.store().set_account_scopes(&account.id, &scopes)?;
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        // Rules that waited for this move to Outlook.
        let waiting: Vec<Rule> = self
            .store()
            .rules()?
            .into_iter()
            .filter(|r| r.account_id.as_deref() == Some(account.id.as_str()) && r.server_id.is_none())
            .collect();
        for r in waiting {
            if let Err(e) = self.save_rule(r).await {
                tracing::warn!(error = %e, "could not move a rule to Outlook");
            }
        }
        Ok(())
    }

    /// What `rule` matches among the last 90 days of mail. `mine`: the
    /// user's addresses, lowercased (their own mail never matches).
    pub fn rule_matches(&self, rule: &Rule, mine: &[String]) -> Result<Matches> {
        let rows = self.store().messages_since(rule.account_id.as_deref(), now_ms() - MATCH_WINDOW_MS, MATCH_SCAN)?;
        let mut out = Matches::default();
        for (m, in_inbox) in rows {
            let from = m.header.from.as_ref().map(|a| a.email.to_lowercase()).unwrap_or_default();
            if mine.contains(&from) || !rule.applies_to(&m.account_id) || !rule.when.matches(&m) {
                continue;
            }
            out.total += 1;
            if in_inbox {
                out.inbox.push((m.account_id.clone(), m.header.id.clone()));
            }
            if out.samples.len() < 3 {
                out.samples.push(m);
            }
        }
        Ok(out)
    }

    /// Do a rule's actions to messages of one account (queued; flush after).
    /// Reads and flags go first: an Outlook move can change ids.
    pub fn perform(&self, account: &Account, ids: &[String], then: &Then) -> Result<()> {
        let inbox = self.store().role_folder(&account.id, FolderRole::Inbox)?;
        let from = inbox.as_deref();
        let gmail = account.provider == ProviderKind::Google;
        if then.read {
            self.act(account, ids, from, &MailAction::MarkRead)?;
        }
        if then.star {
            self.act(account, ids, from, &MailAction::Flag)?;
        }
        if then.trash {
            self.act(account, ids, from, &MailAction::Trash)?;
            return Ok(());
        }
        match (&then.label, gmail) {
            (Some(l), true) => {
                self.act(account, ids, from, &MailAction::AddLabel(l.id.clone()))?;
            }
            (Some(l), false) => {
                self.act(account, ids, from, &MailAction::MoveTo(l.id.clone()))?;
                return Ok(());
            }
            (None, _) => {}
        }
        if then.archive {
            self.act(account, ids, from, &MailAction::Archive)?;
        }
        Ok(())
    }

    /// Put back what a rule did to messages of one account (queued).
    pub fn unperform(&self, account: &Account, ids: &[String], then: &Then) -> Result<()> {
        let inbox = self.store().role_folder(&account.id, FolderRole::Inbox)?;
        let gmail = account.provider == ProviderKind::Google;
        if (then.trash || then.archive || (then.label.is_some() && !gmail))
            && let Some(inbox) = &inbox
        {
            self.act(account, ids, None, &MailAction::MoveTo(inbox.clone()))?;
        }
        if let (Some(l), true) = (&then.label, gmail) {
            self.act(account, ids, None, &MailAction::Relabel { add: vec![], remove: vec![l.id.clone()] })?;
        }
        if then.read {
            self.act(account, ids, None, &MailAction::MarkUnread)?;
        }
        if then.star {
            self.act(account, ids, None, &MailAction::Unflag)?;
        }
        Ok(())
    }

    /// Activity entries for a rule's action on `m`.
    pub fn rule_logged(rule: &Rule, m: &MessageRow, provider: Option<ProviderKind>, runs: Runs, now: i64) -> Logged {
        Logged {
            id: 0,
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            account_id: m.account_id.clone(),
            message_id: m.header.id.clone(),
            thread_id: m.header.thread_id.clone(),
            from: m.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default(),
            subject: m.header.subject.clone(),
            what: rule.then.summary(provider),
            how: runs.label().into(),
            then: rule.then.clone(),
            at: now,
            undone_at: None,
        }
    }
}

/// "Rescana" for a rule on @rescana.com, else the first condition.
pub fn default_name(rule: &Rule) -> String {
    let w = &rule.when;
    let first = w.from.first().or(w.subject.first()).or(w.words.first()).or(w.to.first());
    match first {
        Some(v) => {
            let v = v.trim().trim_start_matches('@');
            let v = v.split('@').next_back().unwrap_or(v);
            let base = v.split('.').next().unwrap_or(v);
            let mut c = base.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => "Rule".into(),
            }
        }
        None if w.has_attachment => "With attachments".into(),
        None => "Rule".into(),
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::rules::When;

    use super::*;

    #[test]
    fn names_from_the_first_condition() {
        let r = |when: When| Rule::new("", None, when, Then::default(), 0);
        assert_eq!(default_name(&r(When { from: vec!["@rescana.com".into()], ..Default::default() })), "Rescana");
        assert_eq!(default_name(&r(When { from: vec!["dana@halyard.co".into()], ..Default::default() })), "Halyard");
        assert_eq!(default_name(&r(When { subject: vec!["invoice".into()], ..Default::default() })), "Invoice");
    }
}
