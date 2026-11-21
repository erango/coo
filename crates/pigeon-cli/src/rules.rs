//! Mail rules for agents (`coo rule-add`, `coo rules`, `coo rule-remove`
//! and the MCP add_rule / list_rules / remove_rule tools): the same rules
//! as Settings › Rules in Coo, saved as Gmail filters or Outlook rules
//! where the server can run them.

use pigeon_core::rules::{Place, Rule, Then, When};
use pigeon_core::{Error, Result};
use pigeon_engine::Pigeon;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ops::Writes;

/// What a new rule should do, as an agent asks for it.
#[derive(Debug, Default, Clone)]
pub struct Spec {
    pub name: String,
    /// One account (id, address or part of it); None for all of them.
    pub account: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub subject: Vec<String>,
    pub words: Vec<String>,
    pub has_attachment: bool,
    /// Actions: archive, read, star (or flag), trash, quiet, needs_you;
    /// `label` is a folder or label name.
    pub actions: Vec<String>,
    pub label: Option<String>,
    /// Also do it now to matching mail in the inbox.
    pub apply_now: bool,
}

#[derive(Debug, Serialize)]
pub struct RuleOut {
    pub id: String,
    pub name: String,
    pub account: Option<String>,
    pub when: String,
    pub then: String,
    pub enabled: bool,
    /// "Gmail filter", "Outlook rule" or "In Coo".
    pub runs: String,
    pub runs_note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    pub handled: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_by: String,
}

fn out(app: &Pigeon, r: &Rule) -> RuleOut {
    let runs = app.rule_runs(r);
    let provider = r.account_id.as_deref().and_then(|id| app.account(id).ok()).map(|a| a.provider);
    RuleOut {
        id: r.id.clone(),
        name: r.name.clone(),
        account: r.account_id.clone(),
        when: r.when.summary(),
        then: r.then.summary(provider),
        enabled: r.enabled,
        runs: if r.problem.is_some() { "In Coo".into() } else { runs.label().into() },
        runs_note: runs.explain(),
        problem: r.problem.clone(),
        handled: r.hits,
        created_by: r.created_by.clone(),
    }
}

/// Build the rule a spec describes (no I/O beyond the local store).
fn build(app: &Pigeon, spec: &Spec, by: &str) -> Result<Rule> {
    let account = spec.account.as_deref().map(|a| app.find_account(a)).transpose()?;
    let mut then = Then::default();
    for a in &spec.actions {
        match a.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "archive" | "skip_inbox" => then.archive = true,
            "read" | "mark_read" => then.read = true,
            "star" | "flag" => then.star = true,
            "trash" | "delete" => then.trash = true,
            "quiet" | "mute" | "no_notification" => then.quiet = true,
            "needs_you" => then.needs_you = true,
            "" => {}
            other => {
                return Err(Error::Other(format!(
                    "unknown action {other:?}: use archive, read, star, trash, quiet, needs_you (and label for a folder)"
                )));
            }
        }
    }
    if let Some(label) = spec.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        let account = account
            .as_ref()
            .ok_or_else(|| Error::Other("a label or folder belongs to one account: give the account too".into()))?;
        let folders = app.store().folders(&account.id)?;
        let f = folders
            .iter()
            .find(|f| f.folder.name.eq_ignore_ascii_case(label) || f.folder.id == label)
            .ok_or_else(|| Error::Other(format!("{} has no folder or label named {label:?}", account.email)))?;
        then.label = Some(Place { id: f.folder.id.clone(), name: f.folder.name.clone() });
    }
    let when = When {
        from: spec.from.clone(),
        to: spec.to.clone(),
        subject: spec.subject.clone(),
        words: spec.words.clone(),
        has_attachment: spec.has_attachment,
    };
    let mut rule = Rule::new(&spec.name, account.map(|a| a.id), when, then, pigeon_core::now_ms());
    rule.created_by = by.to_string();
    if let Some(why) = rule.check() {
        return Err(Error::Other(why.into()));
    }
    Ok(rule)
}

/// Save a rule; with `apply_now`, also do it to matching inbox mail.
pub async fn add(app: &Pigeon, w: Writes, spec: &Spec, by: &str) -> Result<Value> {
    w.check()?;
    let rule = app.save_rule(build(app, spec, by)?).await?;
    let mut applied = 0;
    if spec.apply_now && rule.then.moves_mail() {
        let mine: Vec<String> = app.accounts()?.into_iter().map(|a| a.email.to_lowercase()).collect();
        let m = app.rule_matches(&rule, &mine)?;
        let mut by_account: Vec<(String, Vec<String>)> = Vec::new();
        for (a, id) in m.inbox {
            match by_account.iter_mut().find(|(x, _)| *x == a) {
                Some((_, v)) => v.push(id),
                None => by_account.push((a, vec![id])),
            }
        }
        let now = pigeon_core::now_ms();
        for (account_id, ids) in by_account {
            let account = app.account(&account_id)?;
            app.perform(&account, &ids, &rule.then)?;
            {
                let store = app.store();
                let runs = pigeon_core::rules::Runs::Coo(pigeon_core::rules::Why::AllAccounts);
                for id in &ids {
                    if let Some(header) = store.message(&account_id, id)? {
                        let row = pigeon_core::model::MessageRow { account_id: account_id.clone(), header };
                        store.log_rule(&Pigeon::rule_logged(&rule, &row, Some(account.provider), runs, now))?;
                    }
                }
            }
            app.flush(&account).await?;
            applied += ids.len();
        }
    }
    Ok(json!({ "rule": out(app, &rule), "applied_now": applied }))
}

pub fn list(app: &Pigeon) -> Result<Vec<RuleOut>> {
    // Not under the store lock: `out` takes it again.
    let rules = app.store().rules()?;
    Ok(rules.iter().map(|r| out(app, r)).collect())
}

pub async fn remove(app: &Pigeon, w: Writes, id: &str) -> Result<Value> {
    w.check()?;
    if app.store().rule(id)?.is_none() {
        return Err(Error::Other(format!("no rule {id:?}: list_rules has the ids")));
    }
    app.delete_rule(id).await?;
    Ok(json!({ "removed": id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_and_conditions_are_checked() {
        let dir = std::env::temp_dir().join(format!("coo-rules-{}", pigeon_core::auth::random_token(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let app = Pigeon::open_with(dir.clone(), pigeon_core::tokens::TokenStore::File(dir.join("t.json"))).unwrap();
        let spec = Spec {
            from: vec!["@rescana.com".into()],
            actions: vec!["archive".into(), "Mark read".into()],
            ..Default::default()
        };
        let r = build(&app, &spec, "Claude").unwrap();
        assert!(r.then.archive && r.then.read && r.account_id.is_none());
        assert_eq!(r.created_by, "Claude");
        let bad = Spec { actions: vec!["explode".into()], ..spec.clone() };
        assert!(build(&app, &bad, "").is_err());
        let empty = Spec { from: vec![], ..spec.clone() };
        assert!(build(&app, &empty, "").is_err(), "no condition");
        let label = Spec { label: Some("Vendors".into()), ..spec };
        assert!(build(&app, &label, "").is_err(), "a label needs an account");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
