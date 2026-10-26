use dioxus::prelude::*;
use pigeon_core::model::{FolderInfo, FolderKind, FolderRole, ProviderKind};
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

/// Standard folders in the order every account lists them.
const ROLE_ORDER: [FolderRole; 7] = [
    FolderRole::Inbox,
    FolderRole::Drafts,
    FolderRole::Sent,
    FolderRole::Archive,
    FolderRole::Starred,
    FolderRole::Spam,
    FolderRole::Trash,
];

/// Folders shown under an account: the standard ones in a fixed order, then
/// the user's own folders/labels by name. Hidden: Gmail's built-in labels
/// that aren't mailboxes (categories, chat, Important, the star/circle
/// "superstars"; user labels have `Label_…` ids), Exchange's chat history,
/// and an empty Outbox.
fn visible_folders(provider: ProviderKind, folders: &[FolderInfo]) -> Vec<&FolderInfo> {
    let custom_ok = |f: &FolderInfo| {
        if f.folder.parent_id.is_some() {
            return false;
        }
        match provider {
            ProviderKind::Google => f.folder.id.starts_with("Label_"),
            ProviderKind::Microsoft => match f.folder.name.to_lowercase().as_str() {
                "conversation history" => false,
                "outbox" => f.total > 0,
                _ => true,
            },
            ProviderKind::ICloud => true,
        }
    };
    let mut out: Vec<&FolderInfo> = folders
        .iter()
        .filter(|f| match f.folder.role {
            Some(r) => ROLE_ORDER.contains(&r),
            None => custom_ok(f),
        })
        .collect();
    out.sort_by_key(|f| match f.folder.role {
        Some(r) => (ROLE_ORDER.iter().position(|x| *x == r).unwrap_or(0), String::new()),
        None => (ROLE_ORDER.len(), f.folder.name.to_lowercase()),
    });
    // One entry per role (some servers expose duplicates, e.g. two Trash).
    let mut seen = Vec::new();
    out.retain(|f| {
        f.folder.role.is_none_or(|r| {
            if seen.contains(&r) {
                false
            } else {
                seen.push(r);
                true
            }
        })
    });
    out
}

#[component]
pub fn Sidebar() -> Element {
    let s = use_context::<AppState>();
    let mut mode = s.mode;
    let many = s.accounts.read().len() > 4;

    if *s.settings_shown.read() {
        return rsx! {
            aside { class: "sidebar", "aria-label": "Settings",
                div { class: "sidebar__top" }
                super::settings::SettingsSidebar {}
                SyncFooter {}
            }
        };
    }
    rsx! {
        aside { class: "sidebar",
            div { class: "sidebar__top" }
            div { class: "sidebar__controls",
                // Search lives in the list pane (⌘F); this is the palette.
                button { class: "search-field", r#type: "button", title: "Go to a folder, run a command, or search everywhere",
                    onclick: move |_| { let mut p = s.palette_open; p.set(true); },
                    Icon { name: "arrow-right", class: "icon--sm" }
                    span { class: "spacer", style: "text-align:left", "Go to or do…" }
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
               if (document.documentElement.dataset.mailDragging) return;\
               if (e.target.closest && e.target.closest('[data-account-drop]')) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; }\
             }, true);",
        );
        document::eval(super::mail_drag::JS);
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
            button { class: "nav-add-account", r#type: "button",
                onclick: move |_| super::add_account::open(s, ""),
                Icon { name: "plus", class: "icon--sm" }
                "Add account"
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
    let mailbox = s.mailbox;
    let mut collapsed_set = s.collapsed_accounts;
    let id = nav.account.id.clone();
    let collapsed = collapsed_set.read().contains(&id);
    let name = s.account_label(&nav.account.id, &nav.account.email);
    let address = crate::state::main_address(&nav).to_string();
    let meta = crate::state::provider_name(nav.account.provider, &nav.account.email);
    let letter = format::initials(&name).chars().next().unwrap_or('?');
    let current = mailbox.read().clone();
    let toggle_id = id.clone();
    let menu_id = id.clone();
    // Quiet: the default for new messages shows only in the name's tooltip.
    let tip = match (s.default_from().as_deref() == Some(id.as_str()), s.chosen_default().is_some()) {
        (true, true) => format!("{address} · Default for new messages"),
        (true, false) => format!("{address} · Default for new messages (top account)"),
        _ => address.clone(),
    };
    let menu_open = s
        .account_menu
        .read()
        .as_ref()
        .is_some_and(|m| m.account_id == id && m.target == super::account_menu::Target::Mail);

    rsx! {
        div { class: "nav-account", "data-account-drop": "{index}",
            "data-drop": drop_side,
            "data-dragging": (*drag.read() == Some(index)).then_some("true"),
            ondragover: move |e| { e.prevent_default(); if drag.peek().is_some() && *over_w.peek() != Some(index) { over_w.set(Some(index)); } },
            ondrop: move |e| { e.prevent_default(); if drag.peek().is_some() { finish_drag(s, drag, over, index); } },
        div { class: "nav-section", "data-hue": "{nav.hue}",
            style: if first { "margin-top:18px" } else { "" },
            draggable: if draggable { "true" } else { "false" },
            "data-account-drag": "{index}",
            // Hovering a mail drag over it opens a collapsed account.
            "data-spring": collapsed.then_some("true"),
            title: if draggable { "Drag to reorder" } else { "" },
            ondragstart: move |_| drag_w.set(Some(index)),
            ondragend: move |_| { drag_w.set(None); over_w.set(None); },
            "data-context": menu_open.then_some("true"),
            oncontextmenu: move |e| {
                e.prevent_default();
                let p = e.client_coordinates();
                super::account_menu::open(s, &menu_id, p.x, p.y);
            },
            onclick: move |_| {
                let mut set = collapsed_set.write();
                if !set.remove(&toggle_id) { set.insert(toggle_id.clone()); }
            },
            if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
            span { dir: "auto", style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap", title: "{tip}", "{name}" }
            span { class: "nav-section__meta", title: "{meta}", dangerous_inner_html: provider_mark(nav.account.provider, meta) }
            if draggable { Icon { name: "grip", class: "icon--xs nav-section__grip" } }
        }
        if !collapsed {
            {
                let all = visible_folders(nav.account.provider, &nav.folders);
                let pinned_keys = s.pinned_folders.read().clone();
                let open = s.folders_open.read().contains(&id);
                let is_pinned = |f: &FolderInfo| pinned_keys.contains(&pin_key(&id, &f.folder.id));
                let is_current = |f: &FolderInfo| {
                    current == Mailbox::Folder { account_id: id.clone(), folder_id: f.folder.id.clone() }
                };
                let custom: Vec<&FolderInfo> = all.iter().copied().filter(|f| f.folder.role.is_none()).collect();
                // Standard folders, then pinned ones (and, while the group is
                // closed, the folder you're in), then the group.
                let shown: Vec<&FolderInfo> = all
                    .iter()
                    .copied()
                    .filter(|f| f.folder.role.is_some() || is_pinned(f) || (!open && is_current(f)))
                    .collect();
                let rest: Vec<&FolderInfo> = custom.iter().copied().filter(|f| !is_pinned(f)).collect();
                let hidden_unread: u32 = if open { 0 } else { rest.iter().map(|f| f.unread).sum() };
                let group = if nav.account.provider == ProviderKind::Google { "Labels" } else { "Folders" };
                let toggle = id.clone();
                let chev = if open { "chev-down" } else { "chev-right" };
                rsx! {
                    for f in shown {
                        FolderRow { key: "{f.folder.id}", account_id: id.clone(), info: f.clone(), pinned: is_pinned(f), child: false }
                    }
                    if !rest.is_empty() {
                        button { class: "nav-item nav-item--child nav-group", r#type: "button", "aria-expanded": "{open}",
                            "data-spring": (!open).then_some("true"),
                            onclick: move |_| {
                                let mut o = s.folders_open;
                                let mut v = o.read().clone();
                                if !v.contains(&toggle) { v.push(toggle.clone()); } else { v.retain(|x| *x != toggle); }
                                let saved = v.clone();
                                o.set(v);
                                crate::prefs::update(move |p| p.folders_open = saved);
                            },
                            Icon { name: chev, class: "icon--sm" }
                            span { class: "nav-item__label", "{group}" }
                            if hidden_unread > 0 { span { class: "count", "{hidden_unread}" } }
                        }
                        if open {
                            for f in rest {
                                FolderRow { key: "g-{f.folder.id}", account_id: id.clone(), info: f.clone(), pinned: false, child: true }
                            }
                        }
                    }
                }
            }
        }
        }
    }
}

fn pin_key(account_id: &str, folder_id: &str) -> String {
    format!("{account_id}|{folder_id}")
}

/// One folder in the sidebar. Your own folders/labels get a pin button on
/// hover: pinned ones show outside the (usually closed) Folders group.
#[component]
fn FolderRow(account_id: String, info: FolderInfo, pinned: bool, child: bool) -> Element {
    let s = use_context::<AppState>();
    let mut mailbox = s.mailbox;
    let target = Mailbox::Folder { account_id: account_id.clone(), folder_id: info.folder.id.clone() };
    let active = *mailbox.read() == target;
    let icon = role_icon(info.folder.role, info.folder.kind);
    let unread = info.unread;
    let label = info.folder.name.clone();
    let custom = info.folder.role.is_none();
    let key = pin_key(&account_id, &info.folder.id);
    let pin_label = if pinned { "Unpin from the sidebar" } else { "Pin to the sidebar" };
    // A drop target for dragged messages (ui/mail_drag.rs).
    let droppable = super::mail_drag::drop_action(&info).is_some();
    let here = super::mail_drag::is_current(&mailbox.read(), &account_id, &info);
    let (drop_account, drop_info) = (account_id.clone(), info.clone());
    rsx! {
        a { class: "nav-item nav-item--child", "data-grouped": child.then_some("true"), href: "#",
            "data-folder-drop": droppable.then_some(account_id.clone()),
            "data-folder-current": here.then_some("true"),
            ondrop: move |e| { e.prevent_default(); e.stop_propagation(); super::mail_drag::drop_on(s, &drop_account, &drop_info); },
            "aria-current": if active { "page" } else { "false" },
            "data-pinned": pinned.then_some("true"),
            onclick: move |e| { e.prevent_default(); mailbox.set(target.clone()); },
            Icon { name: icon, class: "icon--sm" }
            span { class: "nav-item__label", dir: "auto", "{label}" }
            if unread > 0 { span { class: "count", "{unread}" } }
            if custom {
                button { class: "nav-item__pin", r#type: "button", "aria-label": "{pin_label}", title: "{pin_label}",
                    "aria-pressed": "{pinned}",
                    onclick: move |e| {
                        e.prevent_default();
                        e.stop_propagation();
                        let mut p = s.pinned_folders;
                        let mut v = p.read().clone();
                        if v.contains(&key) { v.retain(|k| *k != key); } else { v.push(key.clone()); }
                        let saved = v.clone();
                        p.set(v);
                        crate::prefs::update(move |pr| pr.pinned_folders = saved);
                    },
                    Icon { name: "pushpin", class: "icon--xs" }
                }
            }
        }
    }
}

#[component]
fn NextUp() -> Element {
    use pigeon_core::meeting::{JoinState, join_state, joinable};
    let s = use_context::<AppState>();
    let events = s.next_up.read().clone();
    let now = pigeon_core::now_ms();
    // A meeting you can join right now beats the next item on the agenda.
    let join = joinable(events.iter().map(|r| &r.event), now).map(|(e, l)| (e.id.clone(), l));
    let row = match &join {
        Some((id, _)) => events.iter().find(|r| &r.event.id == id).cloned(),
        None => events.iter().find(|r| r.event.end_at > now).cloned(),
    };
    let Some(row) = row else { return rsx! {} };
    let e = row.event.clone();
    let hue = s.hue(&row.account_id);
    let live = join_state(e.start_at, e.end_at, now) == JoinState::Live;
    let soon = if live { "now".to_string() } else { format::soon(e.start_at, now) };
    rsx! {
        div { class: "next-up",
            span { class: "t-label", style: "padding:0 4px", if live { "Now" } else { "Next up" } }
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
                if let Some((_, link)) = join {
                    a { class: "btn btn--primary", href: "#", style: "height:30px",
                        title: "{link.url}",
                        onclick: move |ev| { ev.prevent_default(); crate::meeting::join(&link.url); },
                        Icon { name: "video", class: "icon--sm" }
                        if live { "Join now" } else { "Join {link.service.label()}" }
                        kbd { class: "kbd", "⇧J" }
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
        let who: Vec<&str> =
            st.needs_sign_in.iter().map(|id| id.split_once(':').map_or(id.as_str(), |(_, e)| e)).collect();
        ("auth", "lock", format!("Sign in again · {}", who.join(", ")))
    } else if st.syncing {
        ("syncing", "sync", "Syncing…".to_string())
    } else if st.offline {
        ("offline", "offline", "Offline · showing saved mail".to_string())
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
            // Settings has its own Back button at the top while open.
            if !*s.settings_shown.read() {
                button { class: "icon-btn icon-btn--sm", "aria-label": "Settings", title: "Settings (⌘,)",
                    onclick: move |_| { let mut o = s.settings_open; o.set(true); },
                    Icon { name: "gear", class: "icon--sm" }
                }
            }
            if let Some(v) = s.update_ready.read().clone() {
                button { class: "update-pill", r#type: "button", title: "Restart Coo to update to {v}",
                    onclick: move |_| super::restart_to_update(),
                    Icon { name: "sync", class: "icon--xs" }
                    span { "Update ready · Restart" }
                }
            }
            // A button once there's nothing in flight: hover reads "Sync now".
            button { class: "sync-status", r#type: "button", "data-state": "{state}", title: "{text}",
                disabled: state == "syncing",
                onclick: move |_| send_cmd(Cmd::SyncNow),
                span { class: "sync-status__now", Icon { name: icon, class: "icon--sm" } span { class: "sync-status__text", "{text}" } }
                span { class: "sync-status__hover", "aria-hidden": "true", Icon { name: "sync", class: "icon--sm" } span { class: "sync-status__text", "Sync now" } }
            }
        }
    }
}

/// Small provider mark for account headers (label kept as the tooltip).
fn provider_mark(kind: ProviderKind, label: &str) -> String {
    let svg = provider_svg(kind);
    format!("<span class=\"provider-mark\" aria-label=\"{label}\">{svg}</span>")
}

/// The provider's logo, 12px by default (CSS can size it up).
pub(crate) fn provider_svg(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Microsoft => {
            r##"<svg viewBox="0 0 16 16" width="12" height="12"><rect x="1" y="1" width="6.5" height="6.5" fill="#F25022"/><rect x="8.5" y="1" width="6.5" height="6.5" fill="#7FBA00"/><rect x="1" y="8.5" width="6.5" height="6.5" fill="#00A4EF"/><rect x="8.5" y="8.5" width="6.5" height="6.5" fill="#FFB900"/></svg>"##
        }
        ProviderKind::Google => {
            r##"<svg viewBox="0 0 48 48" width="12" height="12"><path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"/><path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"/><path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"/><path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"/></svg>"##
        }
        ProviderKind::ICloud => {
            r##"<svg viewBox="0 0 24 16" width="15" height="10"><path fill="#3693F3" d="M19.3 6.6A6.5 6.5 0 0 0 6.9 4.5 4.6 4.6 0 0 0 0 8.8 4.4 4.4 0 0 0 4.4 13.2h14.3a3.4 3.4 0 0 0 .6-6.6z"/></svg>"##
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::Folder;

    fn f(id: &str, name: &str, role: Option<FolderRole>, kind: FolderKind, total: u32) -> FolderInfo {
        FolderInfo {
            folder: Folder { id: id.into(), name: name.into(), parent_id: None, role, kind },
            total,
            unread: 0,
        }
    }

    fn names(v: Vec<&FolderInfo>) -> Vec<String> {
        v.iter().map(|f| f.folder.name.clone()).collect()
    }

    #[test]
    fn gmail_lists_standard_labels_in_order_then_user_labels() {
        let l = FolderKind::Label;
        let folders = vec![
            f("TRASH", "Trash", Some(FolderRole::Trash), l, 3),
            f("Label_2", "zeta", None, l, 1),
            f("SENT", "Sent", Some(FolderRole::Sent), l, 9),
            f("CATEGORY_SOCIAL", "Social", None, l, 4),
            f("IMPORTANT", "Important", Some(FolderRole::Important), l, 4),
            f("SPAM", "Spam", Some(FolderRole::Spam), l, 1),
            f("BLUE_STAR", "BLUE_STAR", None, l, 0),
            f("CHAT", "Chat", None, l, 2),
            f("INBOX", "Inbox", Some(FolderRole::Inbox), l, 5),
            f("Label_1", "Alpha", None, l, 1),
            f("DRAFT", "Drafts", Some(FolderRole::Drafts), l, 0),
            f("STARRED", "Starred", Some(FolderRole::Starred), l, 0),
        ];
        assert_eq!(
            names(visible_folders(ProviderKind::Google, &folders)),
            ["Inbox", "Drafts", "Sent", "Starred", "Spam", "Trash", "Alpha", "zeta"]
        );
    }

    #[test]
    fn exchange_hides_chat_history_and_empty_outbox() {
        let k = FolderKind::Folder;
        let mut folders = vec![
            f("d", "Deleted Items", Some(FolderRole::Trash), k, 0),
            f("j", "Junk Email", Some(FolderRole::Spam), k, 0),
            f("c", "Conversation History", None, k, 12),
            f("o", "Outbox", None, k, 0),
            f("a", "Archive", Some(FolderRole::Archive), k, 3),
            f("q", "Q4 Planning", None, k, 2),
            f("s", "Sent Items", Some(FolderRole::Sent), k, 3),
            f("i", "Inbox", Some(FolderRole::Inbox), k, 3),
        ];
        assert_eq!(
            names(visible_folders(ProviderKind::Microsoft, &folders)),
            ["Inbox", "Sent Items", "Archive", "Junk Email", "Deleted Items", "Q4 Planning"]
        );
        folders[3].total = 1; // a stuck outgoing message: show the Outbox
        assert!(names(visible_folders(ProviderKind::Microsoft, &folders)).contains(&"Outbox".to_string()));
    }

    #[test]
    fn duplicate_roles_show_once_and_subfolders_stay_nested() {
        let k = FolderKind::Folder;
        let mut child = f("n", "Nested", None, k, 1);
        child.folder.parent_id = Some("q".into());
        let folders = vec![
            f("t1", "Trash", Some(FolderRole::Trash), k, 0),
            f("t2", "Deleted Messages", Some(FolderRole::Trash), k, 0),
            child,
        ];
        assert_eq!(names(visible_folders(ProviderKind::ICloud, &folders)), ["Trash"]);
    }
}
