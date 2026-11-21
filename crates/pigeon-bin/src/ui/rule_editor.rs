//! The rule editor sheet: When (sender, recipient, subject, words,
//! attachment, account) + Then (archive, read, label or folder, star or
//! flag, trash, no notification, Needs you), with what it already matches
//! and where it will run.

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_core::rules::{Place, Rule, Runs, Then, When, Why};
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

/// The sheet's subject: a rule to edit or a new one (possibly filled in
/// from a message or a filter made elsewhere).
#[derive(Debug, Clone, PartialEq)]
pub struct Editing {
    pub rule: Rule,
    pub is_new: bool,
}

pub fn open(mut s: AppState, rule: Rule, is_new: bool) {
    s.rule_matches.set(None);
    s.rule_editor.set(Some(Editing { rule, is_new }));
}

/// "a, b ,c" → ["a", "b", "c"].
pub fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect()
}

/// The provider of a rule's account (None: all accounts).
pub fn provider_of(s: &AppState, account: Option<&str>) -> Option<ProviderKind> {
    let id = account?;
    s.accounts.read().iter().find(|a| a.account.id == id).map(|a| a.account.provider)
}

/// Where `rule` runs, as the app knows it.
pub fn runs(s: &AppState, rule: &Rule) -> Runs {
    let provider = provider_of(s, rule.account_id.as_deref());
    let may = match provider {
        Some(ProviderKind::Google) => true,
        Some(ProviderKind::Microsoft) => {
            rule.account_id.as_ref().is_some_and(|id| s.outlook_rules_allowed.read().contains(id))
        }
        _ => false,
    };
    rule.runs(provider, may)
}

/// An action checkbox: label, checked, what it flips, a note.
type Toggle = (&'static str, bool, fn(&mut Then), Option<&'static str>);

/// Matches asked for so far; each answer carries its question's key.
static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[component]
pub fn RuleEditor() -> Element {
    let s = use_context::<AppState>();
    let Some(editing) = s.rule_editor.read().clone() else { return rsx! {} };
    rsx! { Sheet { key: "{editing.rule.id}", editing } }
}

#[component]
fn Sheet(editing: Editing) -> Element {
    let mut s = use_context::<AppState>();
    let r0 = editing.rule.clone();
    let mut name = use_signal(|| r0.name.clone());
    let mut account = use_signal(|| r0.account_id.clone().unwrap_or_default());
    let mut from = use_signal(|| r0.when.from.join(", "));
    let mut to = use_signal(|| r0.when.to.join(", "));
    let mut subject = use_signal(|| r0.when.subject.join(", "));
    let mut words = use_signal(|| r0.when.words.join(", "));
    let mut attach = use_signal(|| r0.when.has_attachment);
    let mut then = use_signal(|| r0.then.clone());
    let mut apply_now = use_signal(|| editing.is_new);
    let mut problem = use_signal(|| None::<&'static str>);

    let build = move || {
        let acct = account.read().clone();
        Rule {
            name: name.read().clone(),
            account_id: (!acct.is_empty()).then_some(acct),
            when: When {
                from: split_list(&from.read()),
                to: split_list(&to.read()),
                subject: split_list(&subject.read()),
                words: split_list(&words.read()),
                has_attachment: *attach.read(),
            },
            then: then.read().clone(),
            ..r0.clone()
        }
    };
    let rule = build();

    // Ask what the conditions match whenever they change.
    let ask = build.clone();
    let key = use_memo(move || {
        let r = ask();
        let k = ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if !r.when.is_empty() {
            send_cmd(Cmd::RuleMatches { key: k, rule: Rule { then: Then::default(), ..r } });
        }
        k
    });
    let matches = s.rule_matches.read().clone().filter(|m| m.0 == key());

    let provider = provider_of(&s, rule.account_id.as_deref());
    let gmail = provider == Some(ProviderKind::Google);
    let where_ = runs(&s, &rule);
    let accounts: Vec<(String, String, ProviderKind)> = s
        .accounts
        .read()
        .iter()
        .map(|a| (a.account.id.clone(), s.account_label(&a.account.id, &a.account.email), a.account.provider))
        .collect();
    let places: Vec<(String, String)> = s
        .accounts
        .read()
        .iter()
        .find(|a| Some(a.account.id.as_str()) == rule.account_id.as_deref())
        .map(|a| {
            a.folders
                .iter()
                .filter(|f| f.folder.role.is_none())
                .map(|f| (f.folder.id.clone(), f.folder.name.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut close = move || s.rule_editor.set(None);
    let save_build = build.clone();
    let save = move |_| {
        let r = save_build();
        if let Some(why) = r.check() {
            problem.set(Some(why));
            return;
        }
        send_cmd(Cmd::SaveRule { apply_now: *apply_now.peek() && r.then.moves_mail(), rule: r });
        s.rule_editor.set(None);
    };
    let delete_id = rule.id.clone();
    let where_icon: &'static str = if matches!(where_, Runs::Server(_)) { "cloud-check" } else { "pigeon" };
    let inbox_n = matches.as_ref().map(|m| m.2).unwrap_or(0);
    let can_apply = rule.then.moves_mail() && inbox_n > 0;

    // One action row: label, checked, toggle, note.
    let toggles: Vec<Toggle> = vec![
        ("Skip the Inbox (archive)", rule.then.archive, |t| t.archive = !t.archive, None),
        ("Mark as read", rule.then.read, |t| t.read = !t.read, None),
        (
            if gmail { "Star" } else { "Flag" },
            rule.then.star,
            |t| t.star = !t.star,
            (provider == Some(ProviderKind::Microsoft)).then_some("Coo only"),
        ),
        ("Move to Trash", rule.then.trash, |t| t.trash = !t.trash, None),
        ("Don’t notify me", rule.then.quiet, |t| t.quiet = !t.quiet, Some("Coo only")),
        ("Send to Needs you", rule.then.needs_you, |t| t.needs_you = !t.needs_you, Some("Coo only")),
    ];

    rsx! {
        div { class: "update-scrim", onclick: move |_| close(),
            div { class: "dialog rule-sheet", role: "dialog", "aria-label": if editing.is_new { "New rule" } else { "Edit rule" },
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| if e.key() == Key::Escape { close() },
                div { class: "rule-sheet__head",
                    label { class: "rule-sheet__name",
                        span { "Rule name" }
                        input { r#type: "text", value: "{name}", placeholder: "Named after the sender if left empty",
                            oninput: move |e| name.set(e.value()) }
                    }
                    label { class: "rule-sheet__account",
                        span { "Account" }
                        select { class: "input", value: "{account}", onchange: move |e| {
                                account.set(e.value());
                                // Folders belong to one account.
                                then.with_mut(|t| t.label = None);
                            },
                            option { value: "", selected: account.read().is_empty(), "All accounts" }
                            for (id, label, _) in accounts.iter() {
                                option { key: "{id}", value: "{id}", selected: *account.read() == *id, "{label}" }
                            }
                        }
                    }
                }
                div { class: "rule-sheet__cols",
                    div { class: "rule-sheet__col",
                        h3 { class: "rule-sheet__h", "When new mail" }
                        Cond { label: "is from", hint: "dana@acme.com, @acme.com", value: from, on_input: move |v| from.set(v) }
                        Cond { label: "is to or cc", hint: "team@acme.com", value: to, on_input: move |v| to.set(v) }
                        Cond { label: "subject has", hint: "invoice, receipt", value: subject, on_input: move |v| subject.set(v) }
                        Cond { label: "has the words", hint: "unsubscribe", value: words, on_input: move |v| words.set(v) }
                        label { class: "rule-check",
                            input { r#type: "checkbox", checked: *attach.read(), onchange: move |e| attach.set(e.checked()) }
                            "Has an attachment"
                        }
                        p { class: "rule-sheet__note", "Commas mean “any of these”. Every filled line must match. @acme.com covers everyone there." }
                        h3 { class: "rule-sheet__h", "Already matches" }
                        if rule.when.is_empty() {
                            p { class: "rule-sheet__note", "Fill in a condition to see what it catches." }
                        } else if let Some((_, total, inbox, samples)) = matches.clone() {
                            p { class: "rule-sheet__count",
                                b { "{total} messages" } " in the last 3 months"
                                if inbox > 0 { ", {inbox} still in Inbox" }
                                "."
                            }
                            if !samples.is_empty() {
                                div { class: "rule-sheet__samples",
                                    for m in samples {
                                        div { class: "rule-sheet__sample", key: "{m.account_id}{m.header.id}",
                                            span { class: "rule-sheet__from", dir: "auto", {m.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default()} }
                                            span { class: "rule-sheet__subj", dir: "auto", "{m.header.subject}" }
                                        }
                                    }
                                }
                            }
                        } else {
                            p { class: "rule-sheet__note", "Counting…" }
                        }
                    }
                    div { class: "rule-sheet__col",
                        h3 { class: "rule-sheet__h", "Then" }
                        for (label, on, flip, note) in toggles {
                            label { class: "rule-check", key: "{label}",
                                input { r#type: "checkbox", checked: on, onchange: move |_| then.with_mut(flip) }
                                span { class: "rule-check__label", "{label}" }
                                if let Some(n) = note { span { class: "rule-check__note", "{n}" } }
                            }
                        }
                        label { class: "rule-check rule-check--place",
                            span { class: "rule-check__label", if gmail { "Add label" } else { "Move to folder" } }
                            select { class: "input", disabled: rule.account_id.is_none() || provider == Some(ProviderKind::ICloud) && places.is_empty(),
                                title: if rule.account_id.is_none() { "Choose an account first: folders belong to one account" } else { "" },
                                onchange: move |e| {
                                    let id = e.value();
                                    let place = places.iter().find(|(i, _)| *i == id).map(|(i, n)| Place { id: i.clone(), name: n.clone() });
                                    then.with_mut(|t| t.label = place);
                                },
                                option { value: "", selected: rule.then.label.is_none(), "None" }
                                for (id, n) in places.iter() {
                                    option { key: "{id}", value: "{id}", selected: rule.then.label.as_ref().is_some_and(|l| l.id == *id), "{n}" }
                                }
                            }
                        }
                        div { class: "rule-where", "data-server": matches!(where_, Runs::Server(_)).then_some("true"),
                            div { class: "rule-where__title",
                                Icon { name: where_icon, class: "icon--sm" }
                                {if matches!(where_, Runs::Server(_)) { format!("Saved as {}", where_.label().to_lowercase()) } else { "Runs in Coo".to_string() }}
                            }
                            p { "{where_.explain()}" }
                            if where_ == Runs::Coo(Why::Permission) {
                                {
                                    let id = rule.account_id.clone().unwrap_or_default();
                                    rsx! {
                                        button { class: "btn btn--secondary btn--sm", r#type: "button",
                                            onclick: move |_| send_cmd(Cmd::AllowOutlookRules { account_id: id.clone() }),
                                            "Allow Outlook rules…"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(p) = *problem.read() {
                    p { class: "rule-sheet__problem", role: "alert", "{p}" }
                }
                div { class: "rule-sheet__foot",
                    if can_apply {
                        label { class: "rule-check",
                            input { r#type: "checkbox", checked: *apply_now.read(), onchange: move |e| apply_now.set(e.checked()) }
                            "Also do this now to the {inbox_n} matching messages in Inbox"
                        }
                    } else {
                        span { class: "rule-sheet__note", if rule.then.moves_mail() || rule.then.is_empty() { "" } else { "This rule only affects new mail." } }
                    }
                    span { class: "rule-sheet__spacer" }
                    if !editing.is_new {
                        button { class: "btn btn--ghost btn--sm rule-sheet__delete", r#type: "button",
                            onclick: move |_| { send_cmd(Cmd::DeleteRule { id: delete_id.clone() }); s.rule_editor.set(None); },
                            "Delete rule"
                        }
                    }
                    button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| close(), "Cancel" }
                    button { class: "btn btn--primary btn--sm", r#type: "button", onclick: save, "Save rule" }
                }
            }
        }
    }
}

#[component]
fn Cond(label: &'static str, hint: &'static str, value: Signal<String>, on_input: EventHandler<String>) -> Element {
    rsx! {
        label { class: "rule-cond",
            span { class: "rule-cond__label", "{label}" }
            input { class: "input", r#type: "text", value: "{value}", placeholder: "{hint}", dir: "auto",
                oninput: move |e| on_input.call(e.value()) }
        }
    }
}
