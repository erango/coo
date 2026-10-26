//! Search pane: replaces the message list while a search is open. Results
//! come from the local index as you type; when nothing matches locally the
//! servers are searched automatically (after a pause in typing), and
//! "Search servers" fetches older matches on demand.
//!
//! Autocomplete: typing the start of an operator (`f`, `is`…) offers the
//! operators it could be; after `op:`, its values (contacts for from:/to:,
//! folders, accounts, dates). ↑↓ move the highlight, Tab inserts the
//! highlighted (or first) suggestion, Enter inserts only a highlighted one
//! and otherwise just closes the list (results are already live), Esc closes
//! the list, then search. Between words the operator list opens on ↓. An
//! empty box shows tips instead of results.
//!
//! Scope: search starts in the folder you're in (⌘F, /), shown as a chip
//! before the query; the chip's menu widens it to the account or all mail
//! (⌥⌘F), as do ⇧↵ and Backspace in an empty box. Matches the scope leaves
//! out are counted below the results ("14 more in other folders").

use std::time::Duration;

use dioxus::prelude::*;
use pigeon_core::model::Address;
use pigeon_core::search_complete::{self as complete, Completion, OPERATORS, Op, Suggestion, Target};
use pigeon_engine::service::{Cmd, Elsewhere, ListItem, Mailbox, SearchScope, ServerSearch, hue_for};

use super::Icon;
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, SearchState, next_token};

const DEBOUNCE: Duration = Duration::from_millis(120);
const LIMIT: u32 = 200;
const CONTACT_DEBOUNCE: Duration = Duration::from_millis(90);
const CONTACT_LIMIT: u32 = 6;
/// Folder/account lists get long with many accounts: typing narrows them.
const MAX_VALUES: usize = 8;

/// The folder (or unified view) the list shows.
pub fn here(s: &AppState) -> SearchScope {
    s.mailbox.read().search_scope()
}

/// Open the search pane in `scope`, optionally pre-filled.
pub fn open(s: AppState, query: &str, scope: SearchScope) {
    let mut search = s.search;
    let token = next_token();
    search.set(Some(SearchState {
        query: query.to_string(),
        token,
        items: Vec::new(),
        server: ServerSearch::Idle,
        scope: scope.clone(),
        elsewhere: Elsewhere::default(),
        caret: Some((query.to_string(), query.len())),
    }));
    if !query.trim().is_empty() {
        send_cmd(Cmd::Search { query: query.to_string(), scope, token, limit: LIMIT });
    }
    // Caret at the end, also for a pre-filled query (WebKit leaves it at 0).
    crate::bridge::set_input("search-input", query, complete::byte_to_utf16(query, query.len()));
}

/// Search the current folder (the list's search box, ⌘F, /).
pub fn open_here(s: AppState) {
    focus_in(s, here(&s));
}

/// Search in `scope`: keeps the query when search is already open.
pub fn focus_in(s: AppState, scope: SearchScope) {
    let open_query = s.search.read().as_ref().map(|st| (st.query.clone(), st.caret()));
    match open_query {
        Some((query, caret)) => {
            set_scope(s, scope);
            crate::bridge::set_input("search-input", &query, complete::byte_to_utf16(&query, caret));
        }
        None => open(s, "", scope),
    }
}

/// Debug: open pre-filled in the current folder, with a `|` marking the caret.
pub fn open_at(s: AppState, marked: &str) {
    let caret = marked.find('|').unwrap_or(marked.len());
    let query = marked.replacen('|', "", 1);
    open(s, &query, here(&s));
    set_query_at(s, query, caret);
}

/// Change where the open search looks, and search again right away.
pub fn set_scope(s: AppState, scope: SearchScope) {
    let mut search = s.search;
    let token = next_token();
    let query = {
        let mut guard = search.write();
        let Some(st) = guard.as_mut() else { return };
        if st.scope == scope {
            return;
        }
        st.scope = scope.clone();
        st.token = token;
        st.server = ServerSearch::Idle;
        st.elsewhere = Elsewhere::default();
        st.query.clone()
    };
    if !query.trim().is_empty() {
        send_cmd(Cmd::Search { query, scope, token, limit: LIMIT });
    }
}

/// The scope chip's name for `scope`.
pub fn scope_label(s: &AppState, scope: &SearchScope) -> String {
    match scope {
        SearchScope::All => "All mail".into(),
        SearchScope::Account(id) => s
            .accounts
            .read()
            .iter()
            .find(|a| &a.account.id == id)
            .map(|a| s.account_label(&a.account.id, &a.account.email))
            .unwrap_or_default(),
        SearchScope::Role(role) => super::list::role_title(*role).into(),
        SearchScope::Folder { account_id, folder_id } => {
            super::list::title(s, &Mailbox::Folder { account_id: account_id.clone(), folder_id: folder_id.clone() }).0
        }
    }
}

/// "Sent 6 · Archive 7": where the out-of-scope matches are.
fn elsewhere_line(e: &Elsewhere) -> String {
    e.folders
        .iter()
        .map(|f| {
            let name: &str = match f.role {
                Some(r) => super::list::role_title(r),
                None => &f.name,
            };
            format!("{name} {}", f.count)
        })
        .collect::<Vec<_>>()
        .join(" · ")
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
            st.elsewhere = Elsewhere::default();
        }
    }
    spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        let current = search.read().as_ref().map(|st| (st.token, st.scope.clone()));
        if let Some((current, scope)) = current
            && current == token
            && !query.trim().is_empty()
        {
            send_cmd(Cmd::Search { query, scope, token, limit: LIMIT });
        }
    });
}

/// Replace the query and put the caret at `caret` (bytes), as a pick does.
fn set_query_at(s: AppState, query: String, caret: usize) {
    let mut search = s.search;
    if let Some(st) = search.write().as_mut() {
        st.caret = Some((query.clone(), caret));
    }
    crate::bridge::set_input("search-input", &query, complete::byte_to_utf16(&query, caret));
    set_query(s, query);
}

/// The search box reported its caret: `[text, UTF-16 offset]` as JSON.
pub fn on_caret(s: AppState, v: &str) {
    let Ok((text, units)) = serde_json::from_str::<(String, usize)>(v) else { return };
    let at = complete::utf16_to_byte(&text, units);
    let mut search = s.search;
    if let Some(st) = search.write().as_mut() {
        st.caret = Some((text, at));
    }
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
                || complete::has_pending_operator(&st.query)
            {
                return;
            }
            st.server = ServerSearch::Loading;
            (st.query.clone(), st.scope.clone())
        };
        let (query, scope) = query;
        send_cmd(Cmd::SearchServers { query, scope, token });
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

/// A row in the suggestion list.
#[derive(Clone, PartialEq)]
struct Item {
    s: Suggestion,
    /// A person (from:/to:), shown with an avatar.
    person: bool,
}

/// What to offer for `c`, or a note when there's nothing to pick yet.
/// `contacts` is the latest contact answer for this box, if current.
fn suggestions(
    s: &AppState,
    query: &str,
    c: &Completion,
    contacts: Option<&[Address]>,
) -> (Vec<Item>, Option<&'static str>) {
    let plain = |v: Vec<Suggestion>| v.into_iter().map(|s| Item { s, person: false }).collect::<Vec<_>>();
    match &c.target {
        Target::Operator(p) => (plain(complete::operators(p)), None),
        Target::Value(Op::From | Op::To, p) => {
            let p = p.trim();
            if p.is_empty() {
                return (Vec::new(), Some("Type a name or address"));
            }
            let people = contacts
                .unwrap_or_default()
                .iter()
                .filter(|a| !a.email.eq_ignore_ascii_case(p))
                .map(|a| Item {
                    s: Suggestion {
                        insert: a.email.clone(),
                        label: a.name.clone().unwrap_or_else(|| a.email.clone()),
                        detail: a.email.clone(),
                    },
                    person: true,
                })
                .collect();
            (people, None)
        }
        Target::Value(Op::In, p) => {
            let accounts = s.accounts.read();
            let account = pigeon_core::search::parse(query).account;
            let folders = accounts.iter().flat_map(|a| a.folders.iter().map(|f| (a.account.id.as_str(), &f.folder)));
            let mut v = plain(complete::folder_values(folders, account.as_deref(), p));
            v.truncate(MAX_VALUES);
            (v, None)
        }
        Target::Value(Op::Account, p) => {
            let accounts: Vec<(String, String)> = s
                .accounts
                .read()
                .iter()
                .map(|a| (a.account.email.clone(), s.account_label(&a.account.id, &a.account.email)))
                .collect();
            let mut v = plain(complete::account_values(&accounts, p));
            v.truncate(MAX_VALUES);
            (v, None)
        }
        Target::Value(op, p) => (plain(complete::fixed_values(*op, p, chrono::Local::now().date_naive())), None),
    }
}

#[component]
pub fn SearchPane() -> Element {
    let s = use_context::<AppState>();
    // The (query, caret) the list was closed at (Esc, Enter, blur): it
    // stays closed until either changes.
    let mut dismissed = use_signal(|| None::<(String, usize)>);
    // Between words the operator list opens only on ↓, at this spot.
    let mut asked_ops = use_signal(|| None::<(String, usize)>);
    let mut highlight = use_signal(|| None::<usize>);
    let mut contact_token = use_signal(|| 0u64);
    let mut contact_query = use_signal(String::new);
    let mut scope_menu = use_signal(|| false);
    let here = use_memo(move || s.search.read().as_ref().map(|st| (st.query.clone(), st.caret())));
    // A new spot starts with nothing highlighted.
    use_effect(move || {
        here.read();
        highlight.set(None);
    });
    // from:/to: values: the composer's contact suggestions.
    use_effect(move || {
        let Some((q, caret)) = here() else { return };
        let p = match complete::at(&q, caret) {
            Some(Completion { target: Target::Value(Op::From | Op::To, p), .. }) => p.trim().to_string(),
            _ => String::new(),
        };
        if p == *contact_query.peek() {
            return;
        }
        contact_query.set(p.clone());
        if p.is_empty() {
            return;
        }
        let token = next_token();
        contact_token.set(token);
        spawn(async move {
            tokio::time::sleep(CONTACT_DEBOUNCE).await;
            if *contact_token.peek() == token {
                send_cmd(Cmd::SuggestContacts { query: p, token, limit: CONTACT_LIMIT });
            }
        });
    });

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
    let empty_box = st.query.is_empty();

    // Scope: the chip, its menu, and what it leaves out.
    let mailbox = s.mailbox.read().clone();
    let here_scope = mailbox.search_scope();
    let scope = st.scope.clone();
    let scope_name = scope_label(&s, &scope);
    let placeholder =
        if scope == SearchScope::All { "Search all mail".to_string() } else { format!("Search {scope_name}") };
    let mut scopes: Vec<(SearchScope, String, &'static str, &'static str)> =
        vec![(here_scope.clone(), scope_label(&s, &here_scope), "This folder", "⌘F")];
    if let Mailbox::Folder { account_id, .. } = &mailbox {
        let account = SearchScope::Account(account_id.clone());
        let name = scope_label(&s, &account);
        scopes.push((account, name, "Every folder in this account", ""));
    }
    scopes.push((SearchScope::All, "All mail".into(), "Every account", "⌥⌘F"));
    let menu_open = *scope_menu.read();
    let chev: &'static str = if menu_open { "chev-up" } else { "chev-down" };
    let elsewhere = (scope != SearchScope::All && has_query && st.elsewhere.total > 0).then(|| {
        let n = st.elsewhere.total;
        let other = if matches!(scope, SearchScope::Account(_)) { "accounts" } else { "folders" };
        let title = if count == 0 {
            format!("Nothing in {scope_name}, but {n} in other {other}")
        } else {
            format!("{n} more in other {other}")
        };
        (title, elsewhere_line(&st.elsewhere))
    });
    let server_scope = scope.clone();
    // (An `if` directly in a component prop trips the rsx macro.)
    let server_icon: &'static str = if st.server == ServerSearch::Loading { "sync" } else { "cloud" };

    // Autocomplete at the caret.
    let spot = (st.query.clone(), st.caret());
    let completion = complete::at(&spot.0, spot.1)
        .filter(|_| dismissed.read().as_ref() != Some(&spot))
        .filter(|c| c.target != Target::Operator(String::new()) || asked_ops.read().as_ref() == Some(&spot));
    let (items, note) = match &completion {
        Some(c) => {
            let contacts = s.contacts.read();
            let current = (contacts.0 == *contact_token.read()).then_some(contacts.1.as_slice());
            suggestions(&s, &st.query, c, current)
        }
        None => (Vec::new(), None),
    };
    let n = items.len();
    let showing = n > 0 || note.is_some();
    let at: Option<usize> = (*highlight.read()).map(|h| h.min(n.saturating_sub(1)));
    let pick = {
        let (query, completion, items) = (st.query.clone(), completion.clone(), items.clone());
        move |i: usize| {
            let (Some(c), Some(item)) = (&completion, items.get(i)) else { return };
            let (text, caret) = complete::apply(&query, c, &item.s.insert);
            set_query_at(s, text, caret);
        }
    };
    let (spot_key, spot_blur) = (spot.clone(), spot);
    let week_ago = (chrono::Local::now().date_naive() - chrono::Days::new(7)).format("%Y-%m-%d").to_string();
    let examples =
        ["from:dana has:attachment".to_string(), "is:unread in:inbox".into(), format!("after:{week_ago} is:flagged")];

    rsx! {
        div { class: "search-head",
            div { class: "search-bar",
                Icon { name: "search" }
                button { class: "scope-chip", r#type: "button", "aria-haspopup": "menu", "aria-expanded": "{menu_open}",
                    "aria-label": "Searching {scope_name}. Change",
                    // mousedown: keep focus (and the caret) in the box.
                    onmousedown: move |e| e.prevent_default(),
                    onclick: move |_| scope_menu.set(!menu_open),
                    span { dir: "auto", "{scope_name}" }
                    Icon { name: chev, class: "icon--xs" }
                }
                // No autocorrect or inline suggestions: macOS commits a pending
                // one when a suggestion is clicked, undoing the pick.
                input { id: "search-input", dir: "auto", value: "{st.query}", autocomplete: "off",
                    "autocorrect": "off", autocapitalize: "off", spellcheck: "false", "writingsuggestions": "false",
                    placeholder: "{placeholder}",
                    role: "combobox", "aria-expanded": "{showing}", "aria-autocomplete": "list",
                    "data-suggest-open": showing.then_some("true"),
                    oninput: move |e| set_query(s, e.value()),
                    onfocus: move |_| dismissed.set(None),
                    onblur: move |_| dismissed.set(Some(spot_blur.clone())),
                    onkeydown: {
                        let pick = pick.clone();
                        let here_scope = here_scope.clone();
                        let scoped = scope != SearchScope::All;
                        move |e: KeyboardEvent| match e.key() {
                            // ⇧↵: the same query in all mail.
                            Key::Enter if e.modifiers().shift() => {
                                e.prevent_default();
                                dismissed.set(Some(spot_key.clone()));
                                set_scope(s, SearchScope::All);
                            }
                            // Backspace in an empty box drops the folder chip.
                            Key::Backspace if empty_box && scoped => {
                                e.prevent_default();
                                set_scope(s, SearchScope::All);
                            }
                            // ⌘F here, ⌥⌘F everywhere (the bridge skips text fields).
                            _ if e.modifiers().meta() && e.code() == Code::KeyF && !e.modifiers().shift() => {
                                e.prevent_default();
                                let to = if e.modifiers().alt() { SearchScope::All } else { here_scope.clone() };
                                set_scope(s, to);
                            }
                            Key::ArrowDown if n > 0 => {
                                e.prevent_default();
                                highlight.set(Some(at.map_or(0, |h| (h + 1).min(n - 1))));
                            }
                            Key::ArrowDown if !showing => {
                                e.prevent_default();
                                dismissed.set(None);
                                asked_ops.set(Some(spot_key.clone()));
                            }
                            Key::ArrowUp if n > 0 => {
                                e.prevent_default();
                                // Up from the first row goes back to the text.
                                highlight.set(at.and_then(|h| h.checked_sub(1)));
                            }
                            Key::Tab if n > 0 => {
                                e.prevent_default();
                                pick(at.unwrap_or(0));
                            }
                            Key::Enter if at.is_some() => {
                                e.prevent_default();
                                pick(at.unwrap_or(0));
                            }
                            Key::Enter | Key::Escape if showing => {
                                e.prevent_default();
                                dismissed.set(Some(spot_key.clone()));
                            }
                            _ => {}
                        }
                    },
                }
                button { class: "icon-btn icon-btn--sm", "aria-label": "Close search (Esc)", onclick: move |_| close(s), Icon { name: "x" } }
                if menu_open {
                    div { class: "scope-menu-scrim", onclick: move |_| scope_menu.set(false) }
                    div { class: "scope-menu", role: "menu", "aria-label": "Search in",
                        div { class: "scope-menu__title", "Search in" }
                        for (option, name, detail, keys) in scopes {
                            {
                                let checked = option == scope;
                                rsx! {
                                    button { class: "scope-menu__item", r#type: "button", role: "menuitemradio", "aria-checked": "{checked}",
                                        key: "{detail}",
                                        onmousedown: move |e| e.prevent_default(),
                                        onclick: move |_| {
                                            scope_menu.set(false);
                                            set_scope(s, option.clone());
                                        },
                                        span { class: "scope-menu__check", if checked { Icon { name: "check", class: "icon--sm" } } }
                                        span { class: "scope-menu__text",
                                            span { class: "scope-menu__name", dir: "auto", "{name}" }
                                            span { class: "scope-menu__detail", "{detail}" }
                                        }
                                        if !keys.is_empty() { kbd { class: "kbd", "{keys}" } }
                                    }
                                }
                            }
                        }
                        div { class: "scope-menu__note",
                            "Backspace in an empty box searches all mail. "
                            code { "in:" }
                            " picks any folder."
                        }
                    }
                }
                if showing {
                    div { class: "suggest suggest--search", role: "listbox",
                        if let Some(note) = note { div { class: "suggest__empty", "{note}" } }
                        for (i, item) in items.into_iter().enumerate() {
                            {
                                let pick = pick.clone();
                                let selected = at == Some(i);
                                let hue = hue_for(&item.s.insert);
                                rsx! {
                                    div { class: if item.person { "suggest__item" } else { "suggest__item suggest__item--op" },
                                        key: "{item.s.insert}", role: "option", "aria-selected": "{selected}",
                                        onmouseenter: move |_| highlight.set(Some(i)),
                                        // mousedown, not click: keep focus in the box.
                                        onmousedown: move |e| {
                                            e.prevent_default();
                                            pick(i);
                                        },
                                        if item.person {
                                            span { class: "avatar avatar--28", "data-hue": "{hue}", "{format::initials(&item.s.label)}" }
                                            div { style: "min-width:0;display:flex;flex-direction:column",
                                                span { class: "suggest__name", dir: "auto", "{item.s.label}" }
                                                span { class: "suggest__addr", "{item.s.detail}" }
                                            }
                                        } else {
                                            span { class: "suggest__op", dir: "auto", "{item.s.label}" }
                                            span { class: "suggest__detail", dir: "auto", "{item.s.detail}" }
                                        }
                                    }
                                }
                            }
                        }
                        if n > 0 {
                            div { class: "suggest__hints",
                                span { kbd { class: "kbd", "↑↓" } " select" }
                                span { kbd { class: "kbd", "tab" } " insert" }
                                span { kbd { class: "kbd", "esc" } " close" }
                            }
                        }
                    }
                }
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
            // An empty box: what the operators are, and a few to try.
            if !has_query {
                div { class: "search-tips",
                    div { class: "search-tips__row",
                        span { class: "search-tips__label", "Try" }
                        for ex in examples {
                            {
                                let q = format!("{ex} ");
                                rsx! {
                                    button { class: "search-tips__example", r#type: "button", key: "{ex}",
                                        onclick: move |_| set_query_at(s, q.clone(), q.len()),
                                        "{ex}"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "search-tips__ops",
                        for (op, detail) in OPERATORS {
                            button { class: "search-tips__op", r#type: "button", key: "{op}",
                                onclick: move |_| {
                                    let q = if op.ends_with(':') { op.to_string() } else { format!("{op} ") };
                                    set_query_at(s, q.clone(), q.len());
                                },
                                span { class: "suggest__op", "{op}" }
                                span { class: "suggest__detail", "{detail}" }
                            }
                        }
                    }
                }
            }
            if let Some((title, line)) = elsewhere {
                div { class: "elsewhere",
                    div { class: "elsewhere__text",
                        span { class: "elsewhere__title", dir: "auto", "{title}" }
                        if !line.is_empty() { span { class: "elsewhere__folders", dir: "auto", "{line}" } }
                    }
                    button { class: "btn btn--secondary btn--sm", r#type: "button",
                        onclick: move |_| set_scope(s, SearchScope::All),
                        "Search all mail"
                        kbd { class: "kbd", "⇧↵" }
                    }
                }
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
                            onclick: move |_| {
                                send_cmd(Cmd::SearchServers { query: query_for_server.clone(), scope: server_scope.clone(), token })
                            },
                            "Search servers"
                        }
                    }
                    if st.server == ServerSearch::Loading { div { class: "progress", span {} } }
                }
            }
        }
    }
}
