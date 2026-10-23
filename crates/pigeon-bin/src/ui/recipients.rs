//! Recipient field: address chips plus autocomplete from local mail history.
//!
//! Chips are the field's value (handed back via `on_change`); the text box
//! holds only the address being typed. Comma, semicolon, or a space after a
//! valid address turns it into a chip; Enter/Tab picks the highlighted
//! suggestion; Backspace on an empty box removes the last chip; pasting a
//! list makes several chips at once.

use std::time::Duration;

use dioxus::prelude::*;
use pigeon_core::model::Address;
use pigeon_core::text::parse_address_list;
use pigeon_engine::service::{Cmd, hue_for};

use super::Icon;
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, next_token};

const DEBOUNCE: Duration = Duration::from_millis(90);
const LIMIT: u32 = 6;

/// `local@domain.tld`, no spaces.
pub fn is_valid(email: &str) -> bool {
    let e = email.trim();
    let Some((local, domain)) = e.rsplit_once('@') else { return false };
    !local.is_empty()
        && !e.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

/// Split typed/pasted text into addresses to commit and the text left over.
/// With `force`, a trailing non-empty fragment is committed too (blur).
pub fn split_input(text: &str, force: bool) -> (Vec<Address>, String) {
    let ends_with_sep = text.ends_with([',', ';', '\n']) || (text.ends_with(' ') && is_valid(text.trim()));
    let parts: Vec<&str> = text.split([',', ';', '\n']).collect();
    let (done, rest) =
        if ends_with_sep || force { (&parts[..], "") } else { (&parts[..parts.len() - 1], parts[parts.len() - 1]) };
    let mut out = Vec::new();
    for p in done {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        match parse_address_list(p).into_iter().next() {
            Some(a) => out.push(a),
            // Keep junk visible as an invalid chip rather than dropping it.
            None => out.push(Address::new(None, p)),
        }
    }
    (out, rest.trim_start().to_string())
}

fn add_unique(list: &mut Vec<Address>, new: Vec<Address>) {
    for a in new {
        if !list.iter().any(|x| x.email.eq_ignore_ascii_case(&a.email)) {
            list.push(a);
        }
    }
}

#[component]
pub fn RecipientInput(
    id: String,
    value: Vec<Address>,
    on_change: EventHandler<Vec<Address>>,
    #[props(default)] placeholder: String,
    /// Pre-typed text (debug screenshots).
    #[props(default)]
    initial_text: String,
) -> Element {
    let s = use_context::<AppState>();
    let mut text = use_signal(|| initial_text.clone());
    let mut cursor = use_signal(|| 0usize);
    let mut my_token = use_signal(|| 0u64);
    let mut open = use_signal(|| !initial_text.is_empty());

    let mut request = move |q: String| {
        let token = next_token();
        my_token.set(token);
        cursor.set(0);
        if q.trim().is_empty() {
            open.set(false);
            return;
        }
        open.set(true);
        spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;
            if *my_token.read() == token {
                send_cmd(Cmd::SuggestContacts { query: q, token, limit: LIMIT });
            }
        });
    };
    use_hook(move || {
        if !initial_text.is_empty() {
            request(initial_text.clone());
        }
    });

    let suggestions: Vec<Address> = {
        let c = s.contacts.read();
        if c.0 == *my_token.read() && *open.read() && !text.read().trim().is_empty() {
            c.1.iter().filter(|a| !value.iter().any(|v| v.email.eq_ignore_ascii_case(&a.email))).cloned().collect()
        } else {
            Vec::new()
        }
    };
    let showing = !suggestions.is_empty();
    let at = (*cursor.read()).min(suggestions.len().saturating_sub(1));
    let query = text.read().trim().to_lowercase();

    let commit = {
        let value = value.clone();
        move |new: Vec<Address>| {
            if new.is_empty() {
                return;
            }
            let mut next = value.clone();
            add_unique(&mut next, new);
            on_change.call(next);
        }
    };
    let (commit_input, commit_key, commit_blur, commit_pick) = (commit.clone(), commit.clone(), commit.clone(), commit);
    let (v_key, v_remove) = (value.clone(), value.clone());
    let picks = suggestions.clone();

    rsx! {
        div { class: "recipients", style: "position:relative",
            for (i, a) in value.iter().enumerate() {
                {
                    let invalid = !is_valid(&a.email);
                    let label = a.name.clone().unwrap_or_else(|| a.email.clone());
                    let hue = hue_for(&a.email);
                    let remove = v_remove.clone();
                    rsx! {
                        span { class: "recipient", key: "{a.email}-{i}", title: "{a.email}", dir: "auto",
                            "aria-invalid": invalid.then_some("true"),
                            if !invalid { span { class: "avatar avatar--20", "data-hue": "{hue}", "{format::initials(&label)}" } }
                            "{label}"
                            button { class: "icon-btn icon-btn--xs", r#type: "button", "aria-label": "Remove {a.email}",
                                onclick: move |_| {
                                    let mut next = remove.clone();
                                    next.remove(i);
                                    on_change.call(next);
                                },
                                Icon { name: "x", class: "icon--xs" }
                            }
                        }
                    }
                }
            }
            input { class: "input--bare", id: "{id}", dir: "auto", autocomplete: "off", value: "{text}",
                placeholder: if value.is_empty() { placeholder.clone() } else { String::new() },
                "data-suggest-open": showing.then_some("true"),
                oninput: move |e| {
                    let (done, rest) = split_input(&e.value(), false);
                    commit_input(done);
                    text.set(rest.clone());
                    request(rest);
                },
                onkeydown: move |e| {
                    let n = picks.len();
                    match e.key() {
                        Key::ArrowDown if n > 0 => { e.prevent_default(); cursor.set((at + 1).min(n - 1)); }
                        Key::ArrowUp if n > 0 => { e.prevent_default(); cursor.set(at.saturating_sub(1)); }
                        Key::Enter | Key::Tab if n > 0 => {
                            e.prevent_default();
                            commit_key(vec![picks[at].clone()]);
                            text.set(String::new());
                            open.set(false);
                        }
                        Key::Enter | Key::Tab if is_valid(&text.read()) => {
                            e.prevent_default();
                            let (done, _) = split_input(&text.read(), true);
                            commit_key(done);
                            text.set(String::new());
                        }
                        Key::Escape if n > 0 => { e.prevent_default(); open.set(false); }
                        Key::Backspace if text.read().is_empty() && !v_key.is_empty() => {
                            let mut next = v_key.clone();
                            next.pop();
                            on_change.call(next);
                        }
                        _ => {}
                    }
                },
                onblur: move |_| {
                    open.set(false);
                    let (done, rest) = split_input(&text.read(), true);
                    commit_blur(done);
                    text.set(rest);
                },
            }
            if showing {
                div { class: "suggest", style: "left:0", role: "listbox",
                    for (i, a) in suggestions.into_iter().enumerate() {
                        {
                            let name = a.name.clone().unwrap_or_else(|| a.email.clone());
                            let hue = hue_for(&a.email);
                            let pick = commit_pick.clone();
                            let chosen = a.clone();
                            let highlight = highlight_prefix(&name, &query);
                            rsx! {
                                div { class: "suggest__item", key: "{a.email}", role: "option", "aria-selected": "{i == at}",
                                    onmouseenter: move |_| cursor.set(i),
                                    // mousedown, not click: keep focus in the box (no blur commit).
                                    onmousedown: move |e| {
                                        e.prevent_default();
                                        pick(vec![chosen.clone()]);
                                        text.set(String::new());
                                        open.set(false);
                                    },
                                    span { class: "avatar avatar--28", "data-hue": "{hue}", "{format::initials(&name)}" }
                                    div { style: "min-width:0;display:flex;flex-direction:column",
                                        span { class: "suggest__name", dir: "auto",
                                            "{highlight.0}"
                                            b { "{highlight.1}" }
                                            "{highlight.2}"
                                        }
                                        span { class: "suggest__addr", "{a.email}" }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "suggest__hints",
                        span { kbd { class: "kbd", "↑↓" } " select" }
                        span { kbd { class: "kbd", "↵" } " add" }
                        span { kbd { class: "kbd", "esc" } " close" }
                    }
                }
            }
        }
    }
}

/// Split `name` into (before, match, after) around the first word that
/// starts with `query` (case-insensitive), for bolding.
pub fn highlight_prefix(name: &str, query: &str) -> (String, String, String) {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (name.to_string(), String::new(), String::new());
    }
    let lower = name.to_lowercase();
    let mut start = None;
    for (i, _) in lower.char_indices() {
        let word_start = i == 0 || lower[..i].ends_with(char::is_whitespace);
        if word_start && lower[i..].starts_with(&q) {
            start = Some(i);
            break;
        }
    }
    // Lowercasing can change byte lengths; only highlight when it didn't.
    match start {
        Some(i) if lower.len() == name.len() && name.is_char_boundary(i + q.len()) => {
            (name[..i].to_string(), name[i..i + q.len()].to_string(), name[i + q.len()..].to_string())
        }
        _ => (name.to_string(), String::new(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity() {
        assert!(is_valid("a@b.co"));
        assert!(!is_valid("a@b"));
        assert!(!is_valid("dana cohen"));
        assert!(!is_valid("@b.co"));
        assert!(!is_valid("a@.co"));
    }

    #[test]
    fn splitting_typed_and_pasted_text() {
        let (done, rest) = split_input("Dana <dana@x.co.il>, bob@y.org; partial", false);
        assert_eq!(done, vec![Address::new(Some("Dana"), "dana@x.co.il"), Address::new(None, "bob@y.org")]);
        assert_eq!(rest, "partial");
        // A space after a valid address commits it; a space inside a name doesn't.
        assert_eq!(split_input("bob@y.org ", false).0.len(), 1);
        let (done, rest) = split_input("Dana ", false);
        assert!(done.is_empty());
        assert_eq!(rest, "Dana ");
        // Blur commits what's left, even if it's not an address (shown invalid).
        let (done, rest) = split_input("not an address", true);
        assert_eq!((done.len(), rest.as_str()), (1, ""));
        assert!(!is_valid(&done[0].email));
        assert_eq!(split_input("", true).0.len(), 0);
    }

    #[test]
    fn dedupes_case_insensitively() {
        let mut list = vec![Address::new(None, "A@x.com")];
        add_unique(&mut list, vec![Address::new(None, "a@x.com"), Address::new(None, "b@x.com")]);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn highlights_word_prefix() {
        assert_eq!(highlight_prefix("Dana Cohen", "co"), ("Dana ".into(), "Co".into(), "hen".into()));
        assert_eq!(highlight_prefix("Dana Cohen", "x"), ("Dana Cohen".into(), String::new(), String::new()));
        assert_eq!(highlight_prefix("דנה כהן", "כה"), ("דנה ".into(), "כה".into(), "ן".into()));
    }
}
