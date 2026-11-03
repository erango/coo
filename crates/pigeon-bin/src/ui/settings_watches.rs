//! Settings › Watches: saved rules for mail you care about. A match goes to
//! Needs you (and a notification when asked), where you or a connected
//! assistant picks it up.

use dioxus::prelude::*;
use pigeon_core::watches::{Filters, Watch};
use pigeon_engine::service::Cmd;

use super::settings::{Group, Row};
use crate::send_cmd;
use crate::state::AppState;

/// "a, b ,c" → ["a", "b", "c"].
fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect()
}

/// What a watch waits for, in a few words per filter.
fn describe(s: &AppState, f: &Filters) -> Vec<String> {
    let mut out = Vec::new();
    for a in &f.accounts {
        let label = s
            .accounts
            .read()
            .iter()
            .find(|n| n.account.id == *a)
            .map(|n| s.account_label(&n.account.id, &n.account.email))
            .unwrap_or_else(|| a.clone());
        out.push(label);
    }
    out.extend(f.from.iter().map(|v| format!("from {v}")));
    out.extend(f.subjects.iter().map(|v| format!("subject has “{v}”")));
    if !f.threads.is_empty() {
        out.push(if f.threads.len() == 1 {
            "one conversation".into()
        } else {
            format!("{} conversations", f.threads.len())
        });
    }
    if out.is_empty() {
        out.push("any new mail".into());
    }
    out
}

fn seen(w: &Watch) -> String {
    match (w.hits, w.last_hit_at) {
        (0, _) => "Waiting".into(),
        (n, Some(at)) => format!("{n} caught · last {}", crate::format::row_time(at, chrono::Local::now())),
        (n, None) => format!("{n} caught"),
    }
}

#[component]
pub fn Watches() -> Element {
    let s = use_context::<AppState>();
    let watches = s.watches.read().clone();
    let mut adding = use_signal(|| false);
    rsx! {
        div { class: "settings__title", "Watches" }
        p { class: "settings__lede",
            "Coo keeps an eye out for mail you care about, even with no assistant open. Matches go to Needs you, where you or any connected assistant can pick them up."
        }
        Group { title: "",
            if watches.is_empty() && !*adding.read() {
                Row { label: "No watches yet", hint: "Add one here, or ask your assistant to watch for something (“tell me when Lena replies”).",
                    button { class: "btn btn--primary btn--sm", r#type: "button", onclick: move |_| adding.set(true), "New watch" }
                }
            }
            for w in watches.iter() {
                WatchRow { key: "{w.id}", watch: w.clone() }
            }
            if !watches.is_empty() && !*adding.read() {
                div { class: "srow",
                    button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| adding.set(true), "New watch" }
                }
            }
        }
        if *adding.read() {
            NewWatch { on_done: move |_| adding.set(false) }
        }
    }
}

#[component]
fn WatchRow(watch: Watch) -> Element {
    let s = use_context::<AppState>();
    let chips = describe(&s, &watch.filters);
    let by = if watch.created_by.is_empty() { String::new() } else { format!("added by {}", watch.created_by) };
    let (w1, w2, w3) = (watch.clone(), watch.clone(), watch.id.clone());
    rsx! {
        div { class: "srow watch-row", "data-paused": watch.paused.then_some("true"),
            div { class: "srow__text",
                span { class: "srow__label", dir: "auto", "{watch.name}"
                    if !by.is_empty() { span { class: "watch-row__by", "{by}" } }
                }
                span { class: "watch-row__chips",
                    for c in chips { span { class: "watch-chip", dir: "auto", "{c}" } }
                }
                if !watch.note.is_empty() {
                    span { class: "srow__hint", dir: "auto", "For your assistant: {watch.note}" }
                }
                span { class: "srow__hint", "{seen(&watch)}" }
            }
            div { class: "srow__control watch-row__controls",
                label { class: "watch-row__toggle",
                    input { class: "switch", r#type: "checkbox", checked: watch.notify, "aria-label": "Notify me",
                        onchange: move |e| send_cmd(Cmd::SaveWatch(Watch { notify: e.checked(), ..w1.clone() })) }
                    "Notify"
                }
                label { class: "watch-row__toggle",
                    input { class: "switch", r#type: "checkbox", checked: !watch.paused, "aria-label": "Watching",
                        onchange: move |e| send_cmd(Cmd::SaveWatch(Watch { paused: !e.checked(), ..w2.clone() })) }
                    "On"
                }
                button { class: "icon-btn icon-btn--sm", r#type: "button", "aria-label": "Remove this watch", title: "Remove",
                    onclick: move |_| send_cmd(Cmd::DeleteWatch { id: w3.clone() }),
                    super::Icon { name: "trash", class: "icon--sm" }
                }
            }
        }
    }
}

#[component]
fn NewWatch(on_done: EventHandler<()>) -> Element {
    let s = use_context::<AppState>();
    let mut name = use_signal(String::new);
    let mut account = use_signal(String::new);
    let mut from = use_signal(String::new);
    let mut subject = use_signal(String::new);
    let mut note = use_signal(String::new);
    let mut notify = use_signal(|| true);
    let accounts: Vec<(String, String)> = s
        .accounts
        .read()
        .iter()
        .map(|a| (a.account.id.clone(), s.account_label(&a.account.id, &a.account.email)))
        .collect();
    let filters = move || Filters {
        accounts: if account.read().is_empty() { vec![] } else { vec![account.read().clone()] },
        from: split_list(&from.read()),
        subjects: split_list(&subject.read()),
        ..Default::default()
    };
    let ready = !name.read().trim().is_empty() && !filters().is_empty();
    rsx! {
        Group { title: "New watch",
            Row { label: "Name", hint: "How it shows in Needs you.",
                input { class: "input", r#type: "text", placeholder: "Supplier changes", value: "{name}", "aria-label": "Watch name",
                    oninput: move |e| name.set(e.value()) }
            }
            Row { label: "Account",
                select { class: "select", "aria-label": "Account", value: "{account}",
                    onchange: move |e| account.set(e.value()),
                    option { value: "", "Any account" }
                    for (id, label) in accounts {
                        option { key: "{id}", value: "{id}", "{label}" }
                    }
                }
            }
            Row { label: "From", hint: "Addresses or parts of them, separated by commas (@acme.com).",
                input { class: "input", r#type: "text", placeholder: "@northsidemills.com", value: "{from}", "aria-label": "From",
                    oninput: move |e| from.set(e.value()) }
            }
            Row { label: "Subject has", hint: "Words, separated by commas; any one of them matches.",
                input { class: "input", r#type: "text", placeholder: "delivery, invoice", value: "{subject}", "aria-label": "Subject has",
                    oninput: move |e| subject.set(e.value()) }
            }
            Row { label: "For your assistant", hint: "Optional: what an assistant should do with a match.",
                input { class: "input", r#type: "text", placeholder: "Check Friday orders and tell me what's short", value: "{note}",
                    "aria-label": "Note for your assistant", oninput: move |e| note.set(e.value()) }
            }
            Row { label: "Notify me", hint: "Also show a notification for each match.",
                input { class: "switch", r#type: "checkbox", checked: *notify.read(), "aria-label": "Notify me",
                    onchange: move |e| notify.set(e.checked()) }
            }
            div { class: "srow watch-new__actions",
                button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| on_done.call(()), "Cancel" }
                button { class: "btn btn--primary btn--sm", r#type: "button", disabled: !ready,
                    title: if ready { "" } else { "Give it a name and at least one condition" },
                    onclick: move |_| {
                        let mut w = Watch::new(&name.read(), filters(), pigeon_core::now_ms());
                        w.note = note.read().trim().to_string();
                        w.notify = *notify.read();
                        send_cmd(Cmd::SaveWatch(w));
                        on_done.call(());
                    },
                    "Add watch"
                }
            }
        }
    }
}

/// Watch one conversation (the row menu's "Watch this conversation").
pub fn watch_thread(account_id: &str, thread_id: &str, subject: &str) {
    let name = if subject.trim().is_empty() { "A conversation".to_string() } else { subject.trim().to_string() };
    let mut w = Watch::new(
        &name,
        Filters { threads: vec![thread_id.to_string()], ..Default::default() },
        pigeon_core::now_ms(),
    );
    w.filters.accounts = vec![account_id.to_string()];
    w.notify = true;
    send_cmd(Cmd::SaveWatch(w));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_split_on_commas() {
        assert_eq!(split_list(" @acme.com, bob@x.io ,,"), vec!["@acme.com", "bob@x.io"]);
        assert!(split_list("  ").is_empty());
    }
}
