//! Right-click menus on the sidebars' accounts and calendars.
//!
//! Mail sidebar, on an account header: a new message from that account,
//! making it the default for new messages, muting its notifications, copying
//! its address, and its settings. Calendar sidebar: on a calendar, a new
//! event in it and making it the default for new events; on an account
//! header, its calendars that take new events, to pick the default from.
//! Gmail labels: on the Labels group, a new label; on one of your labels, a
//! new label or deleting it.
//! Same popover as the list rows' menu (`row_menu`): at the pointer, kept
//! inside the window; Esc, a click outside or picking an item closes it.

use dioxus::prelude::*;
use pigeon_core::model::Calendar;
use pigeon_engine::service::Cmd;

use super::Icon;
use super::event_editor::{account_calendar, chosen_calendar};
use crate::send_cmd;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
pub struct AccountMenu {
    pub account_id: String,
    pub target: Target,
    /// Pointer position, viewport px.
    pub x: f64,
    pub y: f64,
    /// Keyboard-highlighted entry (index into `entries`).
    pub active: Option<usize>,
}

/// What was right-clicked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Target {
    /// An account header in the mail sidebar.
    #[default]
    Mail,
    /// An account header in the calendar sidebar.
    CalAccount,
    /// A calendar (its id) in the calendar sidebar.
    Calendar(String),
    /// A Gmail account's Labels group.
    Labels,
    /// One of your Gmail labels (its id).
    Label(String),
    /// A Trash folder: these accounts' (all of them for the unified one).
    Trash(Vec<String>),
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
    /// A new event in the right-clicked calendar.
    NewEventHere,
    /// A new event on the account (its default calendar).
    NewEvent,
    SetDefaultCal,
    /// The right-clicked calendar is the chosen default: checked.
    IsDefaultCal,
    /// Back to the automatic calendar.
    ClearDefaultCal,
    /// A calendar that can't take new events: a note, not clickable.
    ReadOnly,
    /// An account none of whose calendars take new events.
    NoWritable,
    /// A heading over the account's calendars.
    DefaultFor,
    /// Make the account's n-th writable calendar the default.
    PickCal(usize),
    /// The n-th writable calendar is the chosen default: checked.
    PickedCal(usize),
    CalSettings,
    NewLabel,
    DeleteLabel,
    EmptyTrash,
    Sep,
}

impl Entry {
    /// (label, icon). The menu's heading names the account (or calendar),
    /// so "New message" is from it. `PickCal` lines show the calendar's
    /// name instead.
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
            Entry::NewEventHere => ("New event in this calendar", "plus"),
            Entry::NewEvent => ("New event", "plus"),
            Entry::SetDefaultCal => ("Set as default for new events", "cal"),
            Entry::IsDefaultCal => ("Default for new events", "check"),
            Entry::ClearDefaultCal => ("Use the automatic choice", "undo"),
            Entry::ReadOnly => ("Read-only: no new events", "lock"),
            Entry::NoWritable => ("No calendars that take events", "lock"),
            Entry::DefaultFor => ("Default for new events", ""),
            Entry::PickCal(_) => ("", "cal"),
            Entry::PickedCal(_) => ("", "check"),
            Entry::CalSettings => ("Calendar settings…", "gear"),
            Entry::NewLabel => ("New label…", "plus"),
            Entry::DeleteLabel => ("Delete label…", "trash"),
            Entry::EmptyTrash => ("Empty Trash…", "trash"),
            Entry::Sep => ("", ""),
        }
    }

    /// Keyboard and pointer can pick it.
    fn selectable(self) -> bool {
        !matches!(
            self,
            Entry::Sep
                | Entry::IsDefault
                | Entry::IsDefaultCal
                | Entry::PickedCal(_)
                | Entry::ReadOnly
                | Entry::NoWritable
                | Entry::DefaultFor
        )
    }

    /// Shown checked.
    fn checked(self) -> bool {
        matches!(self, Entry::IsDefault | Entry::IsDefaultCal | Entry::PickedCal(_))
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

/// The menu for one calendar: `writable` = it can take new events, `chosen`
/// = it's the default the user picked (as for mail, the automatic one can
/// still be pinned).
pub fn calendar_entries(writable: bool, chosen: bool) -> Vec<Entry> {
    let mut v = if !writable {
        vec![Entry::ReadOnly]
    } else if chosen {
        vec![Entry::NewEventHere, Entry::Sep, Entry::IsDefaultCal, Entry::ClearDefaultCal]
    } else {
        vec![Entry::NewEventHere, Entry::Sep, Entry::SetDefaultCal]
    };
    v.extend([Entry::Sep, Entry::CalSettings]);
    v
}

/// The calendar sidebar's menu for an account with `writable` calendars that
/// take new events, `chosen` = which of them is the picked default.
pub fn cal_account_entries(writable: usize, chosen: Option<usize>) -> Vec<Entry> {
    let mut v = if writable == 0 {
        vec![Entry::NoWritable]
    } else {
        let mut v = vec![Entry::NewEvent, Entry::Sep, Entry::DefaultFor];
        v.extend((0..writable).map(|i| if chosen == Some(i) { Entry::PickedCal(i) } else { Entry::PickCal(i) }));
        if chosen.is_some() {
            v.push(Entry::ClearDefaultCal);
        }
        v
    };
    v.extend([Entry::Sep, Entry::CalSettings]);
    v
}

/// Next keyboard-highlighted entry, skipping separators, notes and checked
/// lines, stopping at the ends.
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

/// The menu's height in px: design `.menu` padding, 32 px items, 11 px
/// separators, 28 px headings (one always: the account or calendar name).
pub fn height(entries: &[Entry]) -> f64 {
    let px = |e: &Entry| match e {
        Entry::Sep => 11.0,
        Entry::DefaultFor => 28.0,
        _ => 32.0,
    };
    12.0 + 28.0 + entries.iter().map(px).sum::<f64>()
}

const WIDTH: f64 = 264.0;

/// The account's calendars that take new events, in sidebar order.
fn writable(s: &AppState, account_id: &str) -> Vec<Calendar> {
    s.calendar.read().calendars.iter().filter(|(a, c)| a == account_id && c.can_edit).map(|(_, c)| c.clone()).collect()
}

/// The picked default calendar (account id, calendar id), while usable.
fn chosen_cal(s: &AppState) -> Option<(String, String)> {
    chosen_calendar(&s.calendar.read().calendars, &s.default_calendar.read())
}

fn entries_for(s: &AppState, m: &AccountMenu) -> Vec<Entry> {
    match &m.target {
        Target::Mail => {
            let chosen = s.chosen_default().as_deref() == Some(m.account_id.as_str());
            let muted = crate::prefs::get().notify_accounts_muted.contains(&m.account_id);
            entries(chosen, muted)
        }
        Target::Calendar(id) => {
            let can = writable(s, &m.account_id).iter().any(|c| &c.id == id);
            let chosen = chosen_cal(s).is_some_and(|(a, c)| a == m.account_id && &c == id);
            calendar_entries(can, chosen)
        }
        Target::CalAccount => {
            let cals = writable(s, &m.account_id);
            let chosen = chosen_cal(s)
                .filter(|(a, _)| *a == m.account_id)
                .and_then(|(_, c)| cals.iter().position(|x| x.id == c));
            cal_account_entries(cals.len(), chosen)
        }
        Target::Labels => vec![Entry::NewLabel],
        Target::Label(_) => vec![Entry::DeleteLabel],
        Target::Trash(_) => vec![Entry::EmptyTrash],
    }
}

/// Right-click on an account header in the mail sidebar.
pub fn open(s: AppState, account_id: &str, x: f64, y: f64) {
    open_for(s, account_id, Target::Mail, x, y);
}

/// Right-click on something in a sidebar.
pub fn open_for(s: AppState, account_id: &str, target: Target, x: f64, y: f64) {
    let mut m = s.account_menu;
    m.set(Some(AccountMenu { account_id: account_id.to_string(), target, x, y, active: None }));
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
        Entry::NewEventHere | Entry::SetDefaultCal => {
            let Target::Calendar(cal) = &m.target else { return };
            if entry == Entry::SetDefaultCal {
                s.set_default_calendar(Some((&id, cal)));
            } else {
                super::event_editor::open_new_now_in(s, Some((id.clone(), cal.clone())));
            }
        }
        Entry::NewEvent => {
            let target = account_calendar(&s.calendar.peek().calendars, &id, &s.default_calendar.peek());
            super::event_editor::open_new_now_in(s, target);
        }
        Entry::PickCal(i) => {
            if let Some(c) = writable(&s, &id).get(i) {
                s.set_default_calendar(Some((&id, &c.id)));
            }
        }
        Entry::ClearDefaultCal => s.set_default_calendar(None),
        Entry::CalSettings => super::open_settings_at(s, "Calendar"),
        Entry::NewLabel => super::sidebar::start_new_label(s, &id),
        Entry::EmptyTrash => {
            let Target::Trash(ids) = &m.target else { return };
            super::empty_trash(ids.clone());
        }
        Entry::DeleteLabel => {
            let Target::Label(label) = &m.target else { return };
            super::sidebar::delete_label(s, &id, label);
        }
        Entry::IsDefault
        | Entry::IsDefaultCal
        | Entry::PickedCal(_)
        | Entry::ReadOnly
        | Entry::NoWritable
        | Entry::DefaultFor
        | Entry::Sep => {}
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
    let account = s.account_label(&m.account_id, &email);
    // The heading names what was clicked; its tooltip, where that lives.
    let (name, tip) = match &m.target {
        Target::Labels => ("Labels".to_string(), account.clone()),
        Target::Trash(ids) if ids.len() > 1 => ("Trash".to_string(), "All accounts".to_string()),
        Target::Trash(_) => ("Trash".to_string(), account.clone()),
        Target::Label(id) => {
            let name = s
                .accounts
                .read()
                .iter()
                .find(|a| a.account.id == m.account_id)
                .and_then(|a| a.folders.iter().find(|f| &f.folder.id == id))
                .map(|f| f.folder.name.clone());
            (name.unwrap_or_default(), account.clone())
        }
        Target::Calendar(id) => {
            let cal = s
                .calendar
                .read()
                .calendars
                .iter()
                .find(|(a, c)| a == &m.account_id && &c.id == id)
                .map(|(_, c)| c.name.clone());
            (cal.unwrap_or_default(), account.clone())
        }
        _ => (account, email),
    };
    let cals = writable(&s, &m.account_id);
    let list = entries_for(&s, &m);
    let h = height(&list);
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
                div { class: "menu__label account-menu__label", dir: "auto", title: "{tip}", "{name}" }
                for (i, entry) in list.iter().copied().enumerate() {
                    if entry == Entry::Sep {
                        div { class: "menu-sep", key: "{i}", role: "separator" }
                    } else if entry == Entry::DefaultFor {
                        div { class: "menu__label account-menu__label", key: "{i}", "{entry.look().0}" }
                    } else {
                        {
                            let (label, icon) = entry.look();
                            let label = match entry {
                                Entry::PickCal(n) | Entry::PickedCal(n) => cals.get(n).map(|c| c.name.clone()).unwrap_or_default(),
                                _ => label.to_string(),
                            };
                            let m = m.clone();
                            let on = entry.selectable();
                            rsx! {
                                div { class: "menu-item", key: "{i}",
                                    role: if entry.checked() { "menuitemcheckbox" } else { "menuitem" },
                                    "aria-checked": entry.checked().then_some("true"),
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
                                    span { dir: "auto", "{label}" }
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

    #[test]
    fn calendar_menu_offers_the_default_only_where_events_can_go() {
        assert_eq!(
            calendar_entries(true, false),
            [Entry::NewEventHere, Entry::Sep, Entry::SetDefaultCal, Entry::Sep, Entry::CalSettings]
        );
        let chosen = calendar_entries(true, true);
        assert!(chosen.contains(&Entry::IsDefaultCal) && chosen.contains(&Entry::ClearDefaultCal));
        assert!(!chosen.contains(&Entry::SetDefaultCal));
        let read_only = calendar_entries(false, false);
        assert_eq!(read_only, [Entry::ReadOnly, Entry::Sep, Entry::CalSettings]);
        assert_eq!(step(&read_only, None, true), Some(2), "the note isn't selectable");
    }

    #[test]
    fn account_menu_in_the_calendar_lists_its_writable_calendars() {
        let e = cal_account_entries(3, Some(1));
        assert_eq!(
            e,
            [
                Entry::NewEvent,
                Entry::Sep,
                Entry::DefaultFor,
                Entry::PickCal(0),
                Entry::PickedCal(1),
                Entry::PickCal(2),
                Entry::ClearDefaultCal,
                Entry::Sep,
                Entry::CalSettings
            ]
        );
        assert_eq!(step(&e, Some(0), true), Some(3), "skips the separator and the heading");
        assert_eq!(step(&e, Some(3), true), Some(5), "skips the checked calendar");
        assert!(!cal_account_entries(2, None).contains(&Entry::ClearDefaultCal));
        assert_eq!(cal_account_entries(0, None), [Entry::NoWritable, Entry::Sep, Entry::CalSettings]);
        assert_eq!(height(&[Entry::NewEvent, Entry::Sep, Entry::DefaultFor]), 12.0 + 28.0 + 32.0 + 11.0 + 28.0);
    }
}
