use dioxus::prelude::*;
use pigeon_core::model::{FolderKind, FolderRole, ProviderKind};
use pigeon_engine::service::{AccountNav, Cmd, Mailbox};

use super::Icon;
use crate::format;
use crate::send_cmd;
use crate::state::{AppState, Mode, Theme};

const UNIFIED: [(FolderRole, &str, &str); 4] = [
    (FolderRole::Inbox, "Inbox", "inbox"),
    (FolderRole::Sent, "Sent", "send"),
    (FolderRole::Drafts, "Drafts", "file"),
    (FolderRole::Trash, "Trash", "trash"),
];

fn role_icon(role: Option<FolderRole>, kind: FolderKind) -> &'static str {
    match role {
        Some(FolderRole::Inbox) => "inbox",
        Some(FolderRole::Sent) => "send",
        Some(FolderRole::Drafts) => "file",
        Some(FolderRole::Trash) => "trash",
        Some(FolderRole::Spam) => "alert",
        Some(FolderRole::Archive) => "archive",
        Some(FolderRole::Starred) => "flag",
        Some(FolderRole::Important) => "tag",
        None if kind == FolderKind::Label => "tag",
        None => "folder",
    }
}

/// Folders worth showing under an account: inbox, archive, then user
/// folders/labels (Gmail's CATEGORY_* stay hidden).
fn visible_folders(a: &AccountNav) -> Vec<&pigeon_core::model::FolderInfo> {
    let mut out: Vec<_> = a
        .folders
        .iter()
        .filter(|f| match f.folder.role {
            Some(r) => matches!(r, FolderRole::Inbox | FolderRole::Archive | FolderRole::Starred),
            None => !f.folder.id.starts_with("CATEGORY_") && f.folder.parent_id.is_none(),
        })
        .collect();
    out.sort_by_key(|f| match f.folder.role {
        Some(FolderRole::Inbox) => 0,
        Some(FolderRole::Starred) => 1,
        Some(FolderRole::Archive) => 2,
        _ => 3,
    });
    out
}

#[component]
pub fn Sidebar() -> Element {
    let s = use_context::<AppState>();
    let mut mode = s.mode;
    let many = s.accounts.read().len() > 4;

    rsx! {
        aside { class: "sidebar",
            div { class: "sidebar__top" }
            div { class: "sidebar__controls",
                button { class: "search-field", r#type: "button",
                    onclick: move |_| { let mut p = s.palette_open; p.set(true); },
                    Icon { name: "search", class: "icon--sm" }
                    span { class: "spacer", style: "text-align:left", "Search" }
                    kbd { class: "kbd", "⌘K" }
                }
                div { class: "segmented segmented--block", role: "tablist",
                    button { role: "tab", "aria-selected": "{*mode.read() == Mode::Mail}", onclick: move |_| mode.set(Mode::Mail), "Mail" }
                    button { role: "tab", "aria-selected": "{*mode.read() == Mode::Calendar}", onclick: move |_| mode.set(Mode::Calendar), "Calendar" }
                }
            }
            match *mode.read() {
                Mode::Mail => rsx! { MailNav { many } },
                Mode::Calendar => rsx! { super::calendar::CalendarSidebar {} },
            }
            if *mode.read() == Mode::Mail {
                NextUp {}
            }
            SyncFooter {}
        }
    }
}

#[component]
fn MailNav(many: bool) -> Element {
    let s = use_context::<AppState>();
    let mut mailbox = s.mailbox;
    let accounts = s.accounts.read().clone();
    let current = mailbox.read().clone();

    // Account reordering: index being dragged, and the section hovered.
    let drag = use_signal(|| None::<usize>);
    let over = use_signal(|| None::<usize>);
    use_hook(|| {
        // WebKit only starts an HTML5 drag with data set, and only fires
        // `drop` when `dragover` was cancelled synchronously; do both in JS.
        document::eval(
            "document.addEventListener('dragstart', e => {\
               const h = e.target.closest && e.target.closest('[data-account-drag]');\
               if (h) { e.dataTransfer.setData('text/plain', h.dataset.accountDrag); e.dataTransfer.effectAllowed = 'move'; }\
             }, true);\
             document.addEventListener('dragover', e => {\
               if (e.target.closest && e.target.closest('[data-account-drop]')) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; }\
             }, true);",
        );
    });

    let unified_unread = |role: FolderRole| -> u32 {
        accounts.iter().flat_map(|a| &a.folders).filter(|f| f.folder.role == Some(role)).map(|f| f.unread).sum()
    };

    rsx! {
        nav { class: "sidebar__nav", "aria-label": "Mailboxes",
            for (role, label, icon) in UNIFIED {
                {
                    let count = if role == FolderRole::Inbox { unified_unread(role) } else { 0 };
                    let active = current == Mailbox::Unified(role);
                    rsx! {
                        a { class: "nav-item", href: "#",
                            "aria-current": if active { "page" } else { "false" },
                            onclick: move |e| { e.prevent_default(); mailbox.set(Mailbox::Unified(role)); },
                            Icon { name: icon }
                            span { class: "nav-item__label", "{label}" }
                            if count > 0 { span { class: "count", "{count}" } }
                        }
                    }
                }
            }
            for (i, a) in accounts.iter().enumerate() {
                AccountSection { key: "{a.account.id}", nav: a.clone(), first: i == 0, many, index: i, count: accounts.len(), drag, over }
            }
        }
    }
}

/// Drop the dragged account onto section `target` and save the new order.
fn finish_drag(s: AppState, mut drag: Signal<Option<usize>>, mut over: Signal<Option<usize>>, target: usize) {
    let from = *drag.peek();
    drag.set(None);
    over.set(None);
    let Some(from) = from.filter(|f| *f != target) else { return };
    let mut accounts = s.accounts;
    let ids: Vec<String> = {
        let mut list = accounts.write();
        crate::state::move_item(&mut list, from, target);
        list.iter().map(|a| a.account.id.clone()).collect()
    };
    crate::prefs::update(|p| p.account_order = ids);
}

#[component]
fn AccountSection(
    nav: AccountNav,
    first: bool,
    many: bool,
    index: usize,
    count: usize,
    drag: Signal<Option<usize>>,
    over: Signal<Option<usize>>,
) -> Element {
    let s = use_context::<AppState>();
    let (mut drag_w, mut over_w) = (drag, over);
    let draggable = count > 1;
    // Line above when moving up, below when moving down.
    let drop_side = match (*drag.read(), *over.read()) {
        (Some(from), Some(o)) if o == index && from != index => Some(if from > index { "before" } else { "after" }),
        _ => None,
    };
    let mut mailbox = s.mailbox;
    let mut collapsed_set = s.collapsed_accounts;
    let id = nav.account.id.clone();
    let collapsed = collapsed_set.read().contains(&id);
    let name = nav.account.email.clone();
    let meta = match nav.account.provider {
        ProviderKind::Microsoft => "M365",
        ProviderKind::Google => {
            if nav.account.email.to_lowercase().ends_with("@gmail.com") {
                "Gmail"
            } else {
                "Workspace"
            }
        }
        ProviderKind::ICloud => "iCloud",
    };
    let letter = format::initials(&nav.account.email).chars().next().unwrap_or('?');
    let current = mailbox.read().clone();
    let toggle_id = id.clone();

    rsx! {
        div { class: "nav-account", "data-account-drop": "{index}",
            "data-drop": drop_side,
            "data-dragging": (*drag.read() == Some(index)).then_some("true"),
            ondragover: move |e| { e.prevent_default(); if *over_w.peek() != Some(index) { over_w.set(Some(index)); } },
            ondrop: move |e| { e.prevent_default(); finish_drag(s, drag, over, index); },
        div { class: "nav-section", "data-hue": "{nav.hue}",
            style: if first { "margin-top:18px" } else { "" },
            draggable: if draggable { "true" } else { "false" },
            "data-account-drag": "{index}",
            title: if draggable { "Drag to reorder" } else { "" },
            ondragstart: move |_| drag_w.set(Some(index)),
            ondragend: move |_| { drag_w.set(None); over_w.set(None); },
            onclick: move |_| {
                let mut set = collapsed_set.write();
                if !set.remove(&toggle_id) { set.insert(toggle_id.clone()); }
            },
            if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
            span { dir: "auto", style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap", "{name}" }
            span { class: "nav-section__meta", "{meta}" }
            if draggable { Icon { name: "grip", class: "icon--xs nav-section__grip" } }
        }
        if !collapsed {
            for f in visible_folders(&nav) {
                {
                    let target = Mailbox::Folder { account_id: id.clone(), folder_id: f.folder.id.clone() };
                    let active = current == target;
                    let icon = role_icon(f.folder.role, f.folder.kind);
                    let unread = f.unread;
                    let label = f.folder.name.clone();
                    rsx! {
                        a { class: "nav-item nav-item--child", href: "#", key: "{f.folder.id}",
                            "aria-current": if active { "page" } else { "false" },
                            onclick: move |e| { e.prevent_default(); mailbox.set(target.clone()); },
                            Icon { name: icon, class: "icon--sm" }
                            span { class: "nav-item__label", dir: "auto", "{label}" }
                            if unread > 0 { span { class: "count", "{unread}" } }
                        }
                    }
                }
            }
        }
        }
    }
}

#[component]
fn NextUp() -> Element {
    let s = use_context::<AppState>();
    let events = s.next_up.read().clone();
    let now = pigeon_core::now_ms();
    let Some(row) = events.into_iter().find(|e| e.event.end_at > now) else { return rsx! {} };
    let e = row.event.clone();
    let hue = s.hue(&row.account_id);
    let soon = format::soon(e.start_at, now);
    let joinable = e.online_url.is_some() && e.start_at - now < 10 * 60_000;
    let url = e.online_url.clone().unwrap_or_default();
    let service = if url.contains("zoom.us") {
        "Zoom"
    } else if url.contains("meet.google") {
        "Google Meet"
    } else if url.contains("teams.microsoft") {
        "Teams"
    } else {
        "meeting"
    };
    rsx! {
        div { class: "next-up",
            span { class: "t-label", style: "padding:0 4px", "Next up" }
            div { class: "next-up__card",
                div { class: "next-up__item", "data-hue": "{hue}",
                    span { class: "next-up__bar" }
                    div {
                        div { class: "next-up__title", dir: "auto", "{e.title}" }
                        div { class: "next-up__when",
                            "{format::hhmm(e.start_at)}–{format::hhmm(e.end_at)}"
                            if !soon.is_empty() { " · " span { class: "next-up__soon", "{soon}" } }
                        }
                    }
                }
                if joinable {
                    a { class: "btn btn--primary", href: "#", style: "height:30px",
                        onclick: move |ev| { ev.prevent_default(); let _ = open::that_detached(&url); },
                        Icon { name: "video", class: "icon--sm" }
                        "Join {service}"
                    }
                }
            }
        }
    }
}

#[component]
fn SyncFooter() -> Element {
    let s = use_context::<AppState>();
    let st = s.sync.read().clone();
    let (state, icon, text) = if !st.needs_sign_in.is_empty() {
        ("error", "lock", format!("Sign in again: {}", st.needs_sign_in.join(", ")))
    } else if st.syncing {
        ("syncing", "sync", "Syncing…".to_string())
    } else if let Some(err) = &st.error {
        ("error", "alert", format!("Sync problem · {err}"))
    } else if let Some(t) = st.last_ok {
        ("synced", "cloud-check", format!("Synced · {}", format::hhmm(t)))
    } else {
        ("syncing", "sync", "Connecting…".to_string())
    };
    let mut theme = s.theme;
    let theme_label = match *theme.read() {
        Theme::System => "Theme: system",
        Theme::Dark => "Theme: dark",
        Theme::Light => "Theme: light",
    };
    rsx! {
        div { class: "sidebar__footer",
            button { class: "icon-btn icon-btn--sm", "aria-label": "{theme_label}", title: "{theme_label}",
                onclick: move |_| {
                    let next = match *theme.read() { Theme::System => Theme::Dark, Theme::Dark => Theme::Light, Theme::Light => Theme::System };
                    theme.set(next);
                },
                Icon { name: "sun", class: "icon--sm" }
            }
            div { class: "sync-status", "data-state": "{state}", title: "{text}",
                onclick: move |_| send_cmd(Cmd::SyncNow),
                Icon { name: icon, class: "icon--sm" }
                span { style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap", "{text}" }
            }
        }
    }
}
