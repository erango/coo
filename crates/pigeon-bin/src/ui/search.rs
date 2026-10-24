//! Search pane: replaces the message list while a search is open. Results
//! come from the local index as you type; when nothing matches locally the
//! servers are searched automatically (after a pause in typing), and
//! "Search servers" fetches older matches on demand.

use std::time::Duration;

use dioxus::prelude::*;
use pigeon_engine::service::{Cmd, ListItem, ServerSearch};

use super::Icon;
use crate::send_cmd;
use crate::state::{AppState, SearchState, next_token};

const DEBOUNCE: Duration = Duration::from_millis(120);
const LIMIT: u32 = 200;

/// Open (or focus) the search pane, optionally pre-filled.
pub fn open(s: AppState, query: &str) {
    let mut search = s.search;
    let token = next_token();
    search.set(Some(SearchState { query: query.to_string(), token, items: Vec::new(), server: ServerSearch::Idle }));
    if !query.trim().is_empty() {
        send_cmd(Cmd::Search { query: query.to_string(), token, limit: LIMIT });
    }
    crate::bridge::focus("search-input");
}

pub fn close(s: AppState) {
    let mut search = s.search;
    search.set(None);
}

/// Update the query and search after a short pause in typing.
fn set_query(s: AppState, query: String) {
    let mut search = s.search;
    let token = next_token();
    if let Some(st) = search.write().as_mut() {
        st.query = query.clone();
        st.token = token;
        st.server = ServerSearch::Idle;
        if query.trim().is_empty() {
            st.items.clear();
        }
    }
    spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        let current = search.read().as_ref().map(|st| st.token);
        if current == Some(token) && !query.trim().is_empty() {
            send_cmd(Cmd::Search { query, token, limit: LIMIT });
        }
    });
}

/// How long typing must pause before an empty local result goes to the
/// servers: each server search costs API quota on every account.
const AUTO_SERVER_DELAY: Duration = Duration::from_millis(700);

/// No local matches: search the servers once typing settles, unless the
/// query changed or a server search already ran for it.
pub fn auto_server_search(s: AppState, token: u64) {
    let mut search = s.search;
    spawn(async move {
        tokio::time::sleep(AUTO_SERVER_DELAY).await;
        let query = {
            let mut guard = search.write();
            let Some(st) = guard.as_mut() else { return };
            if st.token != token
                || !st.items.is_empty()
                || st.server != ServerSearch::Idle
                || st.query.trim().is_empty()
            {
                return;
            }
            st.server = ServerSearch::Loading;
            st.query.clone()
        };
        send_cmd(Cmd::SearchServers { query, token });
    });
}

/// Toggle an operator token (e.g. `is:unread`) in the query.
fn toggle(s: AppState, op: &str) {
    let Some(q) = s.search.read().as_ref().map(|st| st.query.clone()) else { return };
    let has = q.split_whitespace().any(|t| t.eq_ignore_ascii_case(op));
    let next = if has {
        q.split_whitespace().filter(|t| !t.eq_ignore_ascii_case(op)).collect::<Vec<_>>().join(" ")
    } else {
        format!("{} {op}", q.trim()).trim().to_string()
    };
    set_query(s, next);
}

#[component]
pub fn SearchPane() -> Element {
    let s = use_context::<AppState>();
    let Some(st) = s.search.read().clone() else { return rsx! {} };
    let accounts = s.accounts.read().clone();
    let many = accounts.len() > 4;
    let words: Vec<String> = st.query.split_whitespace().map(|w| w.to_lowercase()).collect();
    let on = |op: &str| words.iter().any(|w| w == op);
    let chips = [("is:unread", "Unread"), ("is:flagged", "Flagged"), ("has:attachment", "Has attachment")];
    let count = st.items.len();
    let query_for_server = st.query.clone();
    let token = st.token;
    let has_query = !st.query.trim().is_empty();
    // (An `if` directly in a component prop trips the rsx macro.)
    let server_icon: &'static str = if st.server == ServerSearch::Loading { "sync" } else { "cloud" };

    rsx! {
        div { class: "search-head",
            div { class: "search-bar",
                Icon { name: "search" }
                input { id: "search-input", dir: "auto", value: "{st.query}", autocomplete: "off",
                    placeholder: "Search mail · from: to: has:attachment is:unread in: before: after:",
                    oninput: move |e| set_query(s, e.value()),
                }
                if has_query { span { class: "search-bar__stats", if count == 1 { "1 result" } else { "{count} results" } } }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Close search (Esc)", onclick: move |_| close(s), Icon { name: "x" } }
            }
            div { class: "search-filters",
                for (op, label) in chips {
                    button { class: "chip", r#type: "button", "aria-pressed": "{on(op)}", onclick: move |_| toggle(s, op), "{label}" }
                }
                for a in accounts {
                    {
                        let needle = a.account.email.to_lowercase();
                        let op = format!("account:{needle}");
                        let pressed = on(&op);
                        rsx! {
                            button { class: "chip", r#type: "button", "data-hue": "{a.hue}", "aria-pressed": "{pressed}", key: "{a.account.id}",
                                onclick: move |_| toggle(s, &op),
                                span { class: "dot dot--sm" }
                                "{s.account_label(&a.account.id, &a.account.email)}"
                            }
                        }
                    }
                }
            }
        }
        div { class: "list", role: "listbox",
            for row in st.items.iter() {
                super::list::Row { key: "{row.account_id}|{row.header.id}", item: ListItem::Message(row.clone()), many }
            }
            if has_query {
                div { class: "server-more", "data-state": match st.server { ServerSearch::Loading => "loading", _ => "idle" },
                    div { class: "server-more__msg",
                        Icon { name: server_icon }
                        match st.server {
                            ServerSearch::Idle => rsx! { span { "Showing mail stored on this Mac. " b { "Search the servers" } " for older messages." } },
                            ServerSearch::Loading => rsx! { span { "Searching every account…" } },
                            ServerSearch::Done(0) => rsx! { span { "No matches on the servers either." } },
                            ServerSearch::Done(1) => rsx! { span { b { "1" } " match came back from the servers." } },
                            ServerSearch::Done(n) => rsx! { span { b { "{n}" } " matches came back from the servers." } },
                        }
                    }
                    if st.server == ServerSearch::Idle {
                        button { class: "btn btn--secondary btn--sm", r#type: "button", style: "align-self:flex-start",
                            onclick: move |_| send_cmd(Cmd::SearchServers { query: query_for_server.clone(), token }),
                            "Search servers"
                        }
                    }
                    if st.server == ServerSearch::Loading { div { class: "progress", span {} } }
                }
            }
        }
    }
}
