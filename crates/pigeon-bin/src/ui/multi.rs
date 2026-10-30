//! Multi-selection in the mail list: ⌘-click adds or removes a row,
//! ⇧-click selects a range from the last clicked row, and dragging from the
//! list's background draws a selection rectangle (⌘ or ⇧ adds to what's
//! selected). With two or more rows
//! selected the reading pane shows what's selected and bulk actions.

use dioxus::prelude::*;
use pigeon_engine::MailAction;
use pigeon_engine::service::{ActItem, Cmd, ListItem};

use super::{Icon, select};
use crate::send_cmd;
use crate::shortcuts::{Do, hint};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Plain click: just this row.
    Plain,
    /// ⌘-click: add or remove this row.
    Toggle,
    /// ⇧-click: everything from the anchor to this row.
    Range,
}

impl How {
    pub fn of(e: &MouseEvent) -> How {
        let m = e.modifiers();
        if m.shift() {
            How::Range
        } else if m.meta() {
            How::Toggle
        } else {
            How::Plain
        }
    }
}

/// The selection after clicking `clicked`: (selected keys in list order,
/// new anchor). `open` is the row open in the reader, which a first ⌘-click
/// or ⇧-click extends from.
pub fn next_selection(
    list: &[String],
    open: Option<&str>,
    multi: &[String],
    anchor: Option<&str>,
    clicked: &str,
    how: How,
) -> (Vec<String>, Option<String>) {
    let current: Vec<&str> =
        if multi.is_empty() { open.into_iter().collect() } else { multi.iter().map(String::as_str).collect() };
    let in_order = |keep: &dyn Fn(&str) -> bool| list.iter().filter(|k| keep(k)).cloned().collect::<Vec<_>>();
    match how {
        How::Plain => (vec![clicked.to_string()], Some(clicked.to_string())),
        How::Toggle => {
            let was = current.contains(&clicked);
            let sel = in_order(&|k| if k == clicked { !was } else { current.contains(&k) });
            (sel, Some(clicked.to_string()))
        }
        How::Range => {
            let from = anchor.or(open).unwrap_or(clicked);
            let pos = |k: &str| list.iter().position(|x| x == k);
            match (pos(from), pos(clicked)) {
                (Some(a), Some(b)) => {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    (list[lo..=hi].to_vec(), Some(from.to_string()))
                }
                _ => (vec![clicked.to_string()], Some(clicked.to_string())),
            }
        }
    }
}

/// ⌘A in the list: select every row in the view. In a folder (not a
/// search) the rest of it loads and joins the selection as it arrives.
pub fn select_all(s: AppState) {
    let keys: Vec<String> = s.visible_items().iter().map(AppState::key).collect();
    if keys.len() < 2 {
        return;
    }
    let (mut m, mut all) = (s.multi, s.select_all);
    m.set(keys);
    close_reader(s);
    if s.search.peek().is_none() {
        all.set(true);
        send_cmd(Cmd::LoadAll);
    }
}

/// The rows a selection rectangle from `y0` to `y1` touches (in the
/// virtualized layout's coordinates), in list order.
pub fn band_hits(layout: &super::vlist::Layout, keys: &[String], y0: f64, y1: f64) -> Vec<String> {
    let (lo, hi) = (y0.min(y1), y0.max(y1));
    layout
        .slots
        .iter()
        .filter_map(|(slot, top)| match *slot {
            super::vlist::Slot::Item(i) if *top < hi && top + layout.metrics.row_h > lo => keys.get(i).cloned(),
            _ => None,
        })
        .collect()
}

thread_local! {
    /// What was selected when the rectangle started (kept when it adds).
    static BAND_BASE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The bridge's "list-band" message, `y0|y1|add|first`: select the rows
/// the selection rectangle touches.
pub fn on_band(s: AppState, v: &str) {
    let mut parts = v.split('|');
    let mut num = || parts.next().and_then(|p| p.parse::<f64>().ok());
    let (Some(y0), Some(y1), Some(add), Some(first)) = (num(), num(), num(), num()) else { return };
    if first == 1.0 {
        let base = if add == 1.0 {
            let multi = s.multi.peek().clone();
            if multi.is_empty() { s.selected.peek().clone().into_iter().collect() } else { multi }
        } else {
            Vec::new()
        };
        BAND_BASE.with(|b| *b.borrow_mut() = base);
    }
    let list = s.visible_items();
    let keys: Vec<String> = list.iter().map(AppState::key).collect();
    let now = chrono::Local::now();
    let metrics = s.list_style.peek().metrics(*s.list_lines.peek());
    let layout = super::vlist::layout(list.iter().map(|i| crate::format::group(i.header().received_at, now)), metrics);
    let hits = band_hits(&layout, &keys, y0, y1);
    let base = BAND_BASE.with(|b| b.borrow().clone());
    let sel: Vec<String> = keys.iter().filter(|k| hits.contains(k) || base.contains(k)).cloned().collect();
    let (mut m, mut a, mut all) = (s.multi, s.anchor, s.select_all);
    all.set(false);
    a.set(hits.first().cloned());
    match sel.as_slice() {
        [] => {
            m.set(Vec::new());
            close_reader(s);
        }
        [one] => {
            m.set(Vec::new());
            if s.selected.peek().as_deref() != Some(one.as_str())
                && let Some(i) = list.iter().find(|i| AppState::key(i) == *one)
            {
                select(s, i);
            }
        }
        _ => {
            if *s.multi.peek() != sel {
                m.set(sel);
            }
            close_reader(s);
        }
    }
}

/// Apply a click on a list row.
pub fn click(s: AppState, item: &ListItem, how: How) {
    let mut all = s.select_all;
    all.set(false);
    let list = s.visible_items();
    let keys: Vec<String> = list.iter().map(AppState::key).collect();
    let (open, multi, anchor) = (s.selected.read().clone(), s.multi.read().clone(), s.anchor.read().clone());
    let (sel, anchor) = next_selection(&keys, open.as_deref(), &multi, anchor.as_deref(), &AppState::key(item), how);
    let (mut m, mut a) = (s.multi, s.anchor);
    a.set(anchor);
    match sel.as_slice() {
        [] => {
            m.set(Vec::new());
            close_reader(s);
        }
        [one] => {
            m.set(Vec::new());
            if let Some(i) = list.iter().find(|i| AppState::key(i) == *one) {
                select(s, i);
            }
        }
        _ => {
            m.set(sel);
            close_reader(s);
        }
    }
}

fn close_reader(s: AppState) {
    let (mut sel, mut reader) = (s.selected, s.reader);
    if sel.peek().is_some() {
        sel.set(None);
    }
    if reader.peek().is_some() {
        reader.set(None);
    }
}

/// Drop the multi-selection.
pub fn clear(s: AppState) {
    let (mut m, mut all) = (s.multi, s.select_all);
    m.set(Vec::new());
    all.set(false);
}

/// The selected rows still in the list (a sync may have removed some).
pub fn items(s: &AppState) -> Vec<ListItem> {
    let multi = s.multi.read();
    if multi.len() < 2 {
        return Vec::new();
    }
    s.visible_items().into_iter().filter(|i| multi.contains(&AppState::key(i))).collect()
}

/// "3 conversations" / "3 messages".
fn noun(items: &[ListItem]) -> String {
    let threads = items.iter().all(|i| matches!(i, ListItem::Thread(_)));
    let n = items.len();
    match (threads, n) {
        (true, 1) => "1 conversation".into(),
        (true, _) => format!("{n} conversations"),
        (false, 1) => "1 message".into(),
        (false, _) => format!("{n} messages"),
    }
}

fn unread(i: &ListItem) -> bool {
    match i {
        ListItem::Thread(t) => t.unread_count > 0,
        ListItem::Message(m) => !m.header.is_read,
    }
}

/// Flag toggles to Flag unless every selected row is flagged already; read
/// state to Read unless every row is read already.
pub fn toggle_flag(items: &[ListItem]) -> MailAction {
    if items.iter().all(|i| i.header().is_flagged) { MailAction::Unflag } else { MailAction::Flag }
}

pub fn toggle_read(items: &[ListItem]) -> MailAction {
    if items.iter().any(unread) { MailAction::MarkRead } else { MailAction::MarkUnread }
}

/// Act on every selected row; leaving actions also end the selection.
pub fn act_many(s: AppState, items: &[ListItem], action: MailAction) {
    if items.is_empty() {
        return;
    }
    let what = noun(items);
    let accounts: Vec<&str> = items.iter().map(|i| i.account_id()).collect();
    let in_trash = action == MailAction::Trash && super::delete_in_trash(&s, &accounts);
    let label = match &action {
        MailAction::Archive => format!("Archived {what}"),
        MailAction::Trash if in_trash => format!("Deleted {what} for good"),
        MailAction::Trash => format!("Deleted {what}"),
        MailAction::Flag => format!("Flagged {what}"),
        MailAction::Unflag => format!("Unflagged {what}"),
        MailAction::MoveTo(_) => format!("Moved {what}"),
        MailAction::AddLabel(_) => format!("Labeled {what}"),
        MailAction::Spam | MailAction::NotSpam | MailAction::Phishing => {
            super::spam::label_for(&s, items, &action, &what).unwrap_or_default()
        }
        _ => String::new(),
    };
    if action == MailAction::Archive {
        super::milestone::archived(items);
    }
    if action.leaves() {
        clear(s);
    }
    let items = items
        .iter()
        .map(|i| ActItem {
            account_id: i.account_id().to_string(),
            ids: vec![i.header().id.clone()],
            thread_id: match i {
                ListItem::Thread(t) => Some(t.thread_id.clone()),
                ListItem::Message(_) => None,
            },
        })
        .collect();
    send_cmd(Cmd::ActMany { items, action, label });
}

/// Reading pane while several rows are selected.
#[component]
pub fn MultiPane() -> Element {
    let s = use_context::<AppState>();
    let items = items(&s);
    let what = noun(&items);
    let (flag, read) = (toggle_flag(&items), toggle_read(&items));
    let flag_label = if flag == MailAction::Unflag { "Unflag" } else { "Flag" };
    let read_label = if read == MailAction::MarkRead { "Mark read" } else { "Mark unread" };
    let read_key = hint(if read == MailAction::MarkRead { Do::MarkRead } else { Do::MarkUnread });
    let (i1, i2, i3, i4, i5) = (items.clone(), items.clone(), items.clone(), items.clone(), items.clone());
    // Spam or Not spam (in the spam folder), when every selected account can.
    let spam = super::spam::for_items(&s, &items).into_iter().find(|a| *a != MailAction::Phishing);
    rsx! {
        main { class: "pane pane--reader",
            div { class: "reader-toolbar", super::compose::DockedDraftChip {} }
            div { class: "multi-pane",
                div { class: "multi-pane__stack", "aria-hidden": "true",
                    span {} span {} span { "{items.len()}" }
                }
                div { class: "multi-pane__title", "{what} selected" }
                div { class: "multi-pane__actions",
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| act_many(s, &i1, MailAction::Archive),
                        Icon { name: "archive", class: "icon--sm" } "Archive" kbd { class: "kbd", {hint(Do::Archive)} } }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| act_many(s, &i2, MailAction::Trash),
                        Icon { name: "trash", class: "icon--sm" } "Delete" kbd { class: "kbd", {hint(Do::Delete)} } }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| super::move_picker::open(s, false),
                        Icon { name: "folder", class: "icon--sm" } "Move" kbd { class: "kbd", {hint(Do::Move)} } }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| act_many(s, &i3, read.clone()),
                        Icon { name: "mailopen", class: "icon--sm" } "{read_label}" kbd { class: "kbd", {read_key} } }
                    button { class: "btn btn--secondary", r#type: "button", onclick: move |_| act_many(s, &i4, flag.clone()),
                        Icon { name: "flag", class: "icon--sm" } "{flag_label}" kbd { class: "kbd", {hint(Do::Flag)} } }
                    if let Some(spam) = spam {
                        {
                            let (icon, text) =
                                if spam == MailAction::NotSpam { ("inbox", "Not spam") } else { ("alert", "Spam") };
                            rsx! {
                                button { class: "btn btn--secondary", r#type: "button", onclick: move |_| act_many(s, &i5, spam.clone()),
                                    Icon { name: icon, class: "icon--sm" }
                                    "{text}"
                                    kbd { class: "kbd", {hint(Do::Spam)} } }
                            }
                        }
                    }
                }
                button { class: "link-btn multi-pane__clear", r#type: "button", onclick: move |_| clear(s),
                    "Clear selection" kbd { class: "kbd", "Esc" } }
                div { class: "multi-pane__hint", "⌘-click adds or removes a message · ⇧-click selects a range · drag from empty space to select an area" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_rectangle_takes_the_rows_it_touches() {
        let m = super::super::vlist::Metrics { row_h: 68.0, group_h: 26.0 };
        // "Today" header (0..26), rows a (26..94), b (94..162), then
        // "Older" (162..188) and c (188..256).
        let layout = super::super::vlist::layout(["Today", "Today", "Older"], m);
        let k: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(band_hits(&layout, &k, 0.0, 20.0), Vec::<String>::new());
        assert_eq!(band_hits(&layout, &k, 90.0, 120.0), ["a", "b"]);
        assert_eq!(band_hits(&layout, &k, 300.0, 150.0), ["b", "c"]);
        assert_eq!(band_hits(&layout, &k, 165.0, 185.0), Vec::<String>::new());
    }

    fn keys() -> Vec<String> {
        ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect()
    }
    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_click_selects_one() {
        assert_eq!(
            next_selection(&keys(), Some("a"), &v(&["a", "b"]), Some("a"), "c", How::Plain),
            (v(&["c"]), Some("c".into()))
        );
    }

    #[test]
    fn cmd_click_extends_from_the_open_row_and_toggles() {
        let (sel, _) = next_selection(&keys(), Some("b"), &[], None, "d", How::Toggle);
        assert_eq!(sel, v(&["b", "d"]));
        let (sel, _) = next_selection(&keys(), None, &v(&["b", "d"]), Some("d"), "a", How::Toggle);
        assert_eq!(sel, v(&["a", "b", "d"]), "kept in list order");
        let (sel, _) = next_selection(&keys(), None, &v(&["a", "b", "d"]), Some("a"), "b", How::Toggle);
        assert_eq!(sel, v(&["a", "d"]));
    }

    #[test]
    fn shift_click_selects_a_range_from_the_anchor() {
        let (sel, anchor) = next_selection(&keys(), Some("b"), &[], None, "d", How::Range);
        assert_eq!((sel, anchor.as_deref()), (v(&["b", "c", "d"]), Some("b")));
        // A second ⇧-click re-ranges from the same anchor, upwards too.
        let (sel, _) = next_selection(&keys(), None, &v(&["b", "c", "d"]), Some("b"), "a", How::Range);
        assert_eq!(sel, v(&["a", "b"]));
        // Nothing open yet: the clicked row alone.
        let (sel, _) = next_selection(&keys(), None, &[], None, "c", How::Range);
        assert_eq!(sel, v(&["c"]));
    }
}
