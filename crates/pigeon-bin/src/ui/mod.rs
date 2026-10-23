mod calendar;
mod compose;
mod list;
pub mod mailframe;
mod onboarding;
mod reader;
mod sidebar;
mod toasts;

use dioxus::prelude::*;
use pigeon_core::compose::ReplyKind;
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, ListItem};

use crate::bridge::{self, Msg};
use crate::send_cmd;
use crate::state::{self, AppState, Mode, Theme};

const ICONS: &str = include_str!("../../../../design/icons.svg");
const DAY_MS: i64 = 86_400_000;

#[component]
pub fn Icon(name: &'static str, #[props(default)] class: &'static str) -> Element {
    rsx! {
        svg { class: "icon {class}", "aria-hidden": "true",
            r#use { href: "#i-{name}" }
        }
    }
}

#[component]
pub fn App() -> Element {
    let s = use_context_provider(AppState::new);

    use_hook(move || {
        spawn(state::pump(s));
        spawn(bridge_loop(s));
    });

    use_effect(move || {
        bridge::set_theme(match *s.theme.read() {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::System => "system",
        })
    });

    // Tell the engine what to list whenever the view changes.
    use_effect(move || {
        let mailbox = s.mailbox.read().clone();
        let threaded = *s.threaded.read();
        send_cmd(Cmd::ShowMailbox { mailbox, threaded });
    });

    use_effect(move || {
        if *s.mode.read() == Mode::Calendar {
            let from = *s.week_start.read();
            send_cmd(Cmd::ShowCalendar { from, to: from + 7 * DAY_MS });
        }
    });

    let body = if !*s.loaded.read() {
        rsx! { div { class: "app" } }
    } else if s.accounts.read().is_empty() {
        rsx! { onboarding::Onboarding {} }
    } else {
        rsx! {
            div { class: "app",
                sidebar::Sidebar {}
                match *s.mode.read() {
                    Mode::Mail => rsx! {
                        list::MailList {}
                        reader::Reader {}
                    },
                    Mode::Calendar => rsx! { calendar::CalendarView {} },
                }
            }
            compose::ComposerOverlay {}
        }
    };

    rsx! {
        div { style: "display:none", dangerous_inner_html: ICONS }
        {body}
    }
}

async fn bridge_loop(s: AppState) {
    let mut eval = document::eval(bridge::JS);
    while let Ok(msg) = eval.recv::<Msg>().await {
        match msg.k.as_str() {
            "os" => {
                let mut os = s.os_dark;
                os.set(msg.v == "dark");
            }
            "link" => {
                let v = msg.v.to_ascii_lowercase();
                if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("mailto:") {
                    let _ = open::that_detached(&msg.v);
                }
            }
            "key" => on_key(s, &msg.v),
            _ => {}
        }
    }
}

/// The selected list item, if any.
pub fn selected_item(s: &AppState) -> Option<ListItem> {
    let key = s.selected.read().clone()?;
    s.list.read().iter().find(|i| AppState::key(i) == key).cloned()
}

/// Select a list item and open it in the reader.
pub fn select(s: AppState, item: &ListItem) {
    let mut sel = s.selected;
    sel.set(Some(AppState::key(item)));
    match item {
        ListItem::Thread(t) => {
            send_cmd(Cmd::OpenThread { account_id: t.account_id.clone(), thread_id: t.thread_id.clone() })
        }
        ListItem::Message(m) => {
            send_cmd(Cmd::OpenMessage { account_id: m.account_id.clone(), message_id: m.header.id.clone() })
        }
    }
}

fn step(s: AppState, delta: isize) {
    let list = s.list.read().clone();
    if list.is_empty() {
        return;
    }
    let current = s.selected.read().clone();
    let idx = current.and_then(|k| list.iter().position(|i| AppState::key(i) == k));
    let next = match idx {
        Some(i) => (i as isize + delta).clamp(0, list.len() as isize - 1) as usize,
        None => 0,
    };
    select(s, &list[next]);
}

/// Run a mail action on a list item; advances the selection when the item
/// leaves the list.
pub fn act(s: AppState, item: &ListItem, action: MailAction) {
    let header = item.header();
    let leaves = matches!(action, MailAction::Archive | MailAction::Trash | MailAction::MoveTo(_));
    if leaves && s.selected.read().as_deref() == Some(AppState::key(item).as_str()) {
        let list = s.list.read().clone();
        let pos = list.iter().position(|i| AppState::key(i) == AppState::key(item));
        let next = pos.and_then(|p| list.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| list.get(q)))).cloned();
        match next {
            Some(n) => select(s, &n),
            None => {
                let (mut sel, mut reader) = (s.selected, s.reader);
                sel.set(None);
                reader.set(None);
            }
        }
    }
    let subject = if header.subject.is_empty() { "(no subject)".to_string() } else { header.subject.clone() };
    let label = match &action {
        MailAction::Archive => format!("Archived “{subject}”"),
        MailAction::Trash => format!("Deleted “{subject}”"),
        MailAction::Flag => format!("Flagged “{subject}”"),
        MailAction::Unflag => format!("Unflagged “{subject}”"),
        MailAction::MoveTo(_) => format!("Moved “{subject}”"),
        _ => String::new(),
    };
    let thread_id = match item {
        ListItem::Thread(t) => Some(t.thread_id.clone()),
        ListItem::Message(_) => None,
    };
    send_cmd(Cmd::Act {
        account_id: item.account_id().to_string(),
        ids: vec![header.id.clone()],
        thread_id,
        action,
        label,
    });
}

pub fn reply(item: &ListItem, kind: ReplyKind) {
    send_cmd(Cmd::StartReply { account_id: item.account_id().to_string(), message_id: item.header().id.clone(), kind });
}

fn on_key(s: AppState, key: &str) {
    let (mut mode, mut draft) = (s.mode, s.draft);
    match key {
        "meta+1" => return mode.set(Mode::Mail),
        "meta+2" => return mode.set(Mode::Calendar),
        "meta+r" => return send_cmd(Cmd::SyncNow),
        "meta+n" | "c" => return send_cmd(Cmd::NewDraft),
        "Escape" => {
            if draft.read().is_some() {
                if let Some(d) = draft.read().clone() {
                    send_cmd(Cmd::SaveDraft(d));
                }
                draft.set(None);
            }
            return;
        }
        "meta+Enter" => {
            compose::send_current(s);
            return;
        }
        "z" | "meta+z" => {
            let undo = s.toasts.read().iter().rev().find_map(|t| t.undo.clone());
            if let Some((account_id, handle)) = undo {
                let mut toasts = s.toasts;
                toasts.write().retain(|t| t.undo.as_ref().map(|u| u.1) != Some(handle));
                send_cmd(Cmd::Undo { account_id, handle });
            }
            return;
        }
        _ => {}
    }
    if *s.mode.read() != Mode::Mail {
        return;
    }
    match key {
        "j" | "ArrowDown" => step(s, 1),
        "k" | "ArrowUp" => step(s, -1),
        _ => {
            let Some(item) = selected_item(&s) else { return };
            let h = item.header();
            let unread = match &item {
                ListItem::Thread(t) => t.unread_count > 0,
                ListItem::Message(m) => !m.header.is_read,
            };
            match key {
                "e" => act(s, &item, MailAction::Archive),
                "#" | "x" => act(s, &item, MailAction::Trash),
                "s" => act(s, &item, if h.is_flagged { MailAction::Unflag } else { MailAction::Flag }),
                "u" => act(s, &item, if unread { MailAction::MarkRead } else { MailAction::MarkUnread }),
                "r" => reply(&item, ReplyKind::Reply),
                "a" => reply(&item, ReplyKind::ReplyAll),
                "f" => reply(&item, ReplyKind::Forward),
                _ => {}
            }
        }
    }
}
