//! Settings › Rules: every mail rule with where it runs, the Gmail filters
//! and Outlook rules made elsewhere, and Activity (what rules did, with
//! Undo).

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_core::rules::{Logged, Rule, Runs, ServerRule, When};
use pigeon_engine::service::Cmd;

use super::Icon;
use super::rule_editor::{open, provider_of, runs};
use super::settings::{Group, Row};
use crate::send_cmd;
use crate::state::AppState;

fn seen(r: &Rule) -> String {
    match (r.hits, r.last_hit_at) {
        (0, _) => "Nothing yet".into(),
        (n, Some(at)) => format!("{n} handled · last {}", crate::format::row_time(at, chrono::Local::now())),
        (n, None) => format!("{n} handled"),
    }
}

fn new_rule() -> Rule {
    Rule::new("", None, When::default(), Default::default(), pigeon_core::now_ms())
}

#[component]
pub fn Rules() -> Element {
    let s = use_context::<AppState>();
    let mut activity = use_signal(|| false);
    use_hook(|| send_cmd(Cmd::LoadServerRules));
    if *activity.read() {
        return rsx! { Activity { on_back: move |_| activity.set(false) } };
    }
    let rules = s.rules.read().clone();
    let mut p = use_signal(crate::prefs::get);
    let waiting: Vec<(String, String)> = s
        .accounts
        .read()
        .iter()
        .filter(|a| {
            a.account.provider == ProviderKind::Microsoft && !s.outlook_rules_allowed.read().contains(&a.account.id)
        })
        .map(|a| (a.account.id.clone(), a.account.email.clone()))
        .collect();
    let elsewhere = s.server_rules.read().clone();
    rsx! {
        div { class: "settings__title-row",
            div { class: "settings__title", "Rules" }
            button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| activity.set(true), "Activity" }
            button { class: "btn btn--primary btn--sm", r#type: "button", onclick: move |_| open(s, new_rule(), true),
                Icon { name: "plus", class: "icon--sm" } "New rule" }
        }
        p { class: "settings__lede",
            "New mail that matches a rule is handled as it arrives. Rules that Gmail or Outlook can run themselves are saved there, so they keep working when Coo is closed."
        }
        for (id, email) in waiting {
            div { class: "rule-banner", role: "note", key: "{id}",
                Icon { name: "alert", class: "icon--sm" }
                span { "To save rules in Outlook for {email}, Microsoft asks you to allow one more permission. Until then, its rules run in Coo." }
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| send_cmd(Cmd::AllowOutlookRules { account_id: id.clone() }), "Allow…" }
            }
        }
        Group { title: "",
            if rules.is_empty() {
                Row { label: "No rules yet", hint: "Make one here, or right-click a message › Always for <sender>." }
            }
            for r in rules.iter() {
                RuleRow { key: "{r.id}", rule: r.clone() }
            }
        }
        if let Some(list) = elsewhere {
            if list.iter().any(|(_, l)| !l.is_empty()) {
                Group { title: "Made in Gmail or Outlook",
                    p { class: "rules-note", "Already on the server. Coo shows them so you know why mail moves." }
                    for (account, items) in list {
                        for r in items {
                            ServerRow { key: "{account}{r.id}", account: account.clone(), rule: r }
                        }
                    }
                }
            }
        }
        Group { title: "Suggestions",
            Row { label: "Suggest rules", hint: "After you archive, delete or mark read a sender’s mail three times in a week without opening it, Coo offers to do it for you.",
                input { class: "switch", r#type: "checkbox", checked: p.read().rule_suggestions, "aria-label": "Suggest rules",
                    onchange: move |e| { let on = e.checked(); crate::prefs::update(move |x| x.rule_suggestions = on); p.set(crate::prefs::get()); } }
            }
            if !p.read().rule_nudge_dismissed.is_empty() {
                Row { label: "Senders you said no to", hint: "{p.read().rule_nudge_dismissed.join(\", \")}",
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: move |_| { crate::prefs::update(|x| x.rule_nudge_dismissed.clear()); p.set(crate::prefs::get()); },
                        "Suggest again" }
                }
            }
        }
    }
}

#[component]
fn RuleRow(rule: Rule) -> Element {
    let s = use_context::<AppState>();
    let provider = provider_of(&s, rule.account_id.as_deref());
    let where_ = runs(&s, &rule);
    let account = match &rule.account_id {
        Some(id) => s
            .accounts
            .read()
            .iter()
            .find(|a| a.account.id == *id)
            .map(|a| s.account_label(&a.account.id, &a.account.email))
            .unwrap_or_else(|| id.clone()),
        None => "All accounts".into(),
    };
    let label =
        if rule.problem.is_some() { "In Coo for now".to_string() } else { format!("{} · {account}", where_.label()) };
    let summary = format!("{} · {}", rule.when.summary(), rule.then.summary(provider));
    let by = if rule.created_by.is_empty() { String::new() } else { format!("added by {}", rule.created_by) };
    let (r1, r2) = (rule.clone(), rule.clone());
    rsx! {
        div { class: "srow rule-row", "data-off": (!rule.enabled).then_some("true"),
            input { class: "switch", r#type: "checkbox", checked: rule.enabled, "aria-label": "Rule on",
                onchange: move |e| send_cmd(Cmd::SaveRule { rule: Rule { enabled: e.checked(), ..r1.clone() }, apply_now: false }) }
            div { class: "srow__text",
                span { class: "srow__label", dir: "auto", "{rule.name}"
                    if !by.is_empty() { span { class: "watch-row__by", "{by}" } }
                }
                span { class: "srow__hint", dir: "auto", "{summary}" }
                if let Some(p) = &rule.problem {
                    span { class: "srow__hint rule-row__problem", "Couldn’t save it on the server: {p}" }
                }
            }
            span { class: "rule-chip", "data-server": matches!(where_, Runs::Server(_)).then_some("true"), title: "{where_.explain()}", "{label}" }
            span { class: "rule-row__seen", "{seen(&rule)}" }
            button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Edit {rule.name}", title: "Edit",
                onclick: move |_| open(s, r2.clone(), false),
                Icon { name: "edit", class: "icon--sm" }
            }
        }
    }
}

#[component]
fn ServerRow(account: String, rule: ServerRule) -> Element {
    let s = use_context::<AppState>();
    let provider = provider_of(&s, Some(&account));
    let kind = if provider == Some(ProviderKind::Google) { "Gmail filter" } else { "Outlook rule" };
    let email =
        s.accounts.read().iter().find(|a| a.account.id == account).map(|a| a.account.email.clone()).unwrap_or_default();
    let title = if rule.name.is_empty() { rule.when.summary() } else { rule.name.clone() };
    let mut what = rule.then.summary(provider);
    if rule.extra {
        what = if what.is_empty() { "Other actions".into() } else { format!("{what}, and more") };
    }
    let take = rule.clone();
    rsx! {
        div { class: "srow rule-row rule-row--elsewhere",
            div { class: "srow__text",
                span { class: "srow__label", dir: "auto", "{title}" }
                span { class: "srow__hint", dir: "auto", "{rule.when.summary()} · {what}" }
            }
            span { class: "rule-chip", "data-server": "true", "{kind} · {email}" }
            if !rule.extra && !rule.when.is_empty() && !rule.then.is_empty() {
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    title: "Coo takes it over: it’s replaced by a rule you can edit here",
                    onclick: move |_| {
                        let mut r = Rule::new(&take.name, Some(account.clone()), take.when.clone(), take.then.clone(), pigeon_core::now_ms());
                        r.server_id = Some(take.id.clone());
                        r.enabled = take.enabled;
                        open(s, r, true);
                    },
                    "Edit in Coo"
                }
            }
        }
    }
}

#[component]
fn Activity(on_back: EventHandler<()>) -> Element {
    let s = use_context::<AppState>();
    let log = s.rule_log.read().clone();
    let mut filter = use_signal(String::new);
    let names: Vec<(String, String)> = {
        let mut v: Vec<(String, String)> = Vec::new();
        for e in &log {
            if !v.iter().any(|(id, _)| *id == e.rule_id) {
                v.push((e.rule_id.clone(), e.rule_name.clone()));
            }
        }
        v
    };
    let shown: Vec<Logged> =
        log.into_iter().filter(|e| filter.read().is_empty() || e.rule_id == *filter.read()).collect();
    let open_ids: Vec<i64> = shown.iter().filter(|e| e.undone_at.is_none()).map(|e| e.id).collect();
    let today = chrono::Local::now().date_naive();
    let day = |at: i64| {
        let d = chrono::DateTime::from_timestamp_millis(at).map(|t| t.with_timezone(&chrono::Local).date_naive());
        match d {
            Some(d) if d == today => "Today".to_string(),
            Some(d) if Some(d) == today.pred_opt() => "Yesterday".to_string(),
            Some(d) => d.format("%A, %B %-d").to_string(),
            None => String::new(),
        }
    };
    let mut groups: Vec<(String, Vec<Logged>)> = Vec::new();
    for e in shown {
        let d = day(e.at);
        match groups.last_mut() {
            Some((g, v)) if *g == d => v.push(e),
            _ => groups.push((d, vec![e])),
        }
    }
    rsx! {
        div { class: "settings__title-row",
            button { class: "icon-btn", r#type: "button", "aria-label": "Back to Rules", onclick: move |_| on_back.call(()),
                Icon { name: "chev-left" } }
            div { class: "settings__title", "Rule activity" }
            select { class: "input rules-filter", "aria-label": "Rule", onchange: move |e| filter.set(e.value()),
                option { value: "", "All rules" }
                for (id, n) in names {
                    option { key: "{id}", value: "{id}", selected: *filter.read() == id, "{n}" }
                }
            }
        }
        p { class: "settings__lede",
            "What rules did in the last 30 days. Undo puts a message back. Mail handled by a Gmail filter or Outlook rule shows here when Coo sees it arrive already moved."
        }
        if groups.is_empty() {
            Group { title: "", Row { label: "Nothing yet", hint: "Rules haven’t handled any mail lately." } }
        }
        for (d, items) in groups {
            Group { key: "{d}", title: "{d}",
                for e in items {
                    LogRow { key: "{e.id}", entry: e }
                }
            }
        }
        if !filter.read().is_empty() && open_ids.len() > 1 {
            div { class: "rules-undo-all",
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| send_cmd(Cmd::UndoRuleLog { ids: open_ids.clone() }),
                    "Undo all of these" }
            }
        }
    }
}

#[component]
fn LogRow(entry: Logged) -> Element {
    let id = entry.id;
    rsx! {
        div { class: "srow rule-log", "data-undone": entry.undone_at.is_some().then_some("true"),
            div { class: "srow__text",
                span { class: "srow__label", dir: "auto", b { "{entry.from}" } " · {entry.subject}" }
                span { class: "srow__hint", "{entry.what} · {entry.rule_name} · {entry.how}" }
            }
            if entry.undone_at.is_some() {
                span { class: "rule-row__seen", "Undone" }
            } else if entry.then.moves_mail() {
                button { class: "btn btn--secondary btn--sm", r#type: "button",
                    onclick: move |_| send_cmd(Cmd::UndoRuleLog { ids: vec![id] }), "Undo" }
            }
        }
    }
}
