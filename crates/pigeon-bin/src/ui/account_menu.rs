//! Right-click menu on a sidebar account header: a new message from that
//! account, making it the default for new messages, muting its
//! notifications, copying its address, and its settings. Same popover as
//! the list rows' menu (`row_menu`): at the pointer, kept inside the window;
//! Esc, a click outside or picking an item closes it.

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
pub struct AccountMenu {
    pub account_id: String,
    /// Pointer position, viewport px.
    pub x: f64,
    pub y: f64,
    /// Keyboard-highlighted entry (index into `entries`).
    pub active: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    NewFrom,
    SetDefault,
    /// Already the chosen default: shown checked, not clickable.
    IsDefault,
    /// Back to automatic (the top account).
    ClearDefault,
    Mute,
    Unmute,
    CopyAddress,
    Settings,
    Sep,
}

impl Entry {
    /// (label, icon). The menu's heading names the account, so "New
    /// message" is from it.
    fn look(self) -> (&'static str, &'static str) {
        match self {
            Entry::NewFrom => ("New message", "edit"),
            Entry::SetDefault => ("Set as default for new messages", "send"),
            Entry::IsDefault => ("Default for new messages", "check"),
            Entry::ClearDefault => ("Use the top account instead", "undo"),
            Entry::Mute => ("Mute notifications", "slash"),
            Entry::Unmute => ("Unmute notifications", "bell"),
            Entry::CopyAddress => ("Copy address", "copy"),
            Entry::Settings => ("Account settings…", "gear"),
            Entry::Sep => ("", ""),
        }
    }

    /// Keyboard and pointer can pick it.
    fn selectable(self) -> bool {
        !matches!(self, Entry::Sep | Entry::IsDefault)
    }
}

/// The menu for an account: `chosen` = the user picked it as the default
/// (the automatic top-account default can still be pinned explicitly).
pub fn entries(chosen: bool, muted: bool) -> Vec<Entry> {
    let mut v = vec![Entry::NewFrom, Entry::Sep];
    if chosen {
        v.extend([Entry::IsDefault, Entry::ClearDefault]);
    } else {
        v.push(Entry::SetDefault);
    }
    v.push(if muted { Entry::Unmute } else { Entry::Mute });
    v.extend([Entry::CopyAddress, Entry::Sep, Entry::Settings]);
    v
}

/// Next keyboard-highlighted entry, skipping separators and the checked
/// default line, stopping at the ends.
pub fn step(entries: &[Entry], active: Option<usize>, down: bool) -> Option<usize> {
    let ok = |i: &usize| entries[*i].selectable();
    let next = match (active, down) {
        (None, true) => (0..entries.len()).find(ok),
        (None, false) => (0..entries.len()).rev().find(ok),
        (Some(a), true) => (a + 1..entries.len()).find(ok),
        (Some(a), false) => (0..a).rev().find(ok),
    };
    next.or(active)
}

const WIDTH: f64 = 264.0;

fn entries_for(s: &AppState, m: &AccountMenu) -> Vec<Entry> {
    let chosen = s.chosen_default().as_deref() == Some(m.account_id.as_str());
    let muted = crate::prefs::get().notify_accounts_muted.contains(&m.account_id);
    entries(chosen, muted)
}

/// Right-click on an account header.
pub fn open(s: AppState, account_id: &str, x: f64, y: f64) {
    let mut m = s.account_menu;
    m.set(Some(AccountMenu { account_id: account_id.to_string(), x, y, active: None }));
}

pub fn close(s: AppState) {
    let mut m = s.account_menu;
    m.set(None);
}

fn run(s: AppState, m: &AccountMenu, entry: Entry) {
    close(s);
    let id = m.account_id.clone();
    match entry {
        Entry::NewFrom => send_cmd(Cmd::NewDraft { from_account: Some(id) }),
        Entry::SetDefault => s.set_default_account(Some(&id)),
        Entry::ClearDefault => s.set_default_account(None),
        Entry::Mute | Entry::Unmute => {
            let mute = entry == Entry::Mute;
            crate::prefs::update(move |p| {
                p.notify_accounts_muted.retain(|a| *a != id);
                if mute {
                    p.notify_accounts_muted.push(id);
                }
            });
        }
        Entry::CopyAddress => {
            let email =
                s.accounts.peek().iter().find(|a| a.account.id == m.account_id).map(|a| a.account.email.clone());
            if let Some(email) = email {
                let js =
                    format!("navigator.clipboard.writeText({})", serde_json::to_string(&email).unwrap_or_default());
                document::eval(&js);
                s.local_toast("Address copied", false);
            }
        }
        Entry::Settings => super::settings::show_account(s, &id),
        Entry::IsDefault | Entry::Sep => {}
    }
}

/// Keys while the menu is open: it takes them all, like a native menu.
pub fn on_key(s: AppState, key: &str) {
    let Some(m) = s.account_menu.read().clone() else { return };
    let list = entries_for(&s, &m);
    match key {
        "Escape" => close(s),
        "ArrowDown" | "j" | "ArrowUp" | "k" => {
            let active = step(&list, m.active, matches!(key, "ArrowDown" | "j"));
            let mut am = s.account_menu;
            if let Some(open) = am.write().as_mut() {
                open.active = active;
            }
        }
        "Enter" => {
            if let Some(&e) = m.active.and_then(|i| list.get(i)) {
                run(s, &m, e);
            }
        }
        _ => {}
    }
}

#[component]
pub fn AccountMenuView() -> Element {
    let s = use_context::<AppState>();
    let Some(m) = s.account_menu.read().clone() else { return rsx! {} };
    let Some(email) = s.accounts.read().iter().find(|a| a.account.id == m.account_id).map(|a| a.account.email.clone())
    else {
        return rsx! {};
    };
    let name = s.account_label(&m.account_id, &email);
    let list = entries_for(&s, &m);
    let seps = list.iter().filter(|e| **e == Entry::Sep).count() as f64;
    // Design `.menu` padding, 32 px items, 11 px separators, the heading.
    let h = 12.0 + 32.0 * (list.len() as f64 - seps) + 11.0 * seps + 28.0;
    // Kept inside the window: CSS zoom doesn't rescale viewport units.
    let style = format!(
        "width:{WIDTH}px;left:max(8px, min({x}px, calc(100vw / var(--ui-zoom, 1) - {WIDTH}px - 8px)));\
         top:max(8px, min({y}px, calc(100vh / var(--ui-zoom, 1) - {h}px - 8px)))",
        x = m.x,
        y = m.y,
    );
    rsx! {
        div { class: "row-menu-scrim",
            onclick: move |_| close(s),
            oncontextmenu: move |e| { e.prevent_default(); close(s) },
            div { class: "menu row-menu account-menu", role: "menu", style: "{style}", "aria-label": "{name}",
                onclick: move |e| e.stop_propagation(),
                oncontextmenu: move |e| { e.prevent_default(); e.stop_propagation() },
                div { class: "menu__label account-menu__label", dir: "auto", title: "{email}", "{name}" }
                for (i, entry) in list.iter().copied().enumerate() {
                    if entry == Entry::Sep {
                        div { class: "menu-sep", key: "{i}", role: "separator" }
                    } else {
                        {
                            let (label, icon) = entry.look();
                            let m = m.clone();
                            let on = entry.selectable();
                            rsx! {
                                div { class: "menu-item", key: "{i}",
                                    role: if entry == Entry::IsDefault { "menuitemcheckbox" } else { "menuitem" },
                                    "aria-checked": (entry == Entry::IsDefault).then_some("true"),
                                    "aria-disabled": (!on).then_some("true"),
                                    "data-active": "{m.active == Some(i)}",
                                    onmouseenter: move |_| {
                                        if on {
                                            let mut am = s.account_menu;
                                            if let Some(open) = am.write().as_mut() { open.active = Some(i); }
                                        }
                                    },
                                    onclick: move |_| if on { run(s, &m, entry) },
                                    Icon { name: icon }
                                    span { "{label}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_line_follows_the_choice() {
        assert_eq!(
            entries(false, false),
            [
                Entry::NewFrom,
                Entry::Sep,
                Entry::SetDefault,
                Entry::Mute,
                Entry::CopyAddress,
                Entry::Sep,
                Entry::Settings
            ]
        );
        let chosen = entries(true, true);
        assert!(chosen.contains(&Entry::IsDefault) && chosen.contains(&Entry::ClearDefault));
        assert!(!chosen.contains(&Entry::SetDefault));
        assert!(chosen.contains(&Entry::Unmute) && !chosen.contains(&Entry::Mute));
    }

    #[test]
    fn keyboard_skips_separators_and_the_checked_default() {
        let e = entries(true, false);
        // NewFrom, Sep, IsDefault, ClearDefault, …
        assert_eq!(step(&e, None, true), Some(0));
        assert_eq!(step(&e, Some(0), true), Some(3), "skips the separator and the checked line");
        assert_eq!(step(&e, Some(3), false), Some(0));
        assert_eq!(step(&e, None, false), Some(e.len() - 1));
        assert_eq!(step(&e, Some(e.len() - 1), true), Some(e.len() - 1));
    }
}
