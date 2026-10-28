use dioxus::prelude::*;
use pigeon_core::model::FolderRole;
use pigeon_engine::MailAction;
use pigeon_engine::service::{ListItem, Mailbox};

use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::{Icon, act, vlist};
use crate::format;
use crate::send_cmd;
use crate::shortcuts::{Do, hint, tip};
use crate::state::{AppState, OutState, Outgoing};

/// Accounts a mailbox shows mail from.
fn scope_accounts(mb: &Mailbox, all: &[String]) -> Vec<String> {
    match mb {
        Mailbox::Unified(_) => all.to_vec(),
        Mailbox::Folder { account_id, .. } => vec![account_id.clone()],
    }
}

/// Outgoing rows belong in the inbox/sent/drafts views of their account.
pub fn shows_outgoing(mb: &Mailbox, role_of_folder: Option<FolderRole>, draft_account: &str) -> bool {
    match mb {
        Mailbox::Unified(r) => matches!(r, FolderRole::Inbox | FolderRole::Sent | FolderRole::Drafts),
        Mailbox::Folder { account_id, .. } => {
            account_id == draft_account
                && matches!(role_of_folder, Some(FolderRole::Inbox | FolderRole::Sent | FolderRole::Drafts))
        }
    }
}

/// A role folder's name in unified views.
pub fn role_title(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Inbox => "Inbox",
        FolderRole::Sent => "Sent",
        FolderRole::Drafts => "Drafts",
        FolderRole::Trash => "Trash",
        FolderRole::Archive => "Archive",
        FolderRole::Spam => "Spam",
        FolderRole::Starred => "Starred",
        FolderRole::Important => "Important",
    }
}

/// A mailbox's (title, subtitle): folder name and whose it is.
pub fn title(s: &AppState, mb: &Mailbox) -> (String, String) {
    match mb {
        Mailbox::Unified(role) => (role_title(*role).to_string(), "All accounts".to_string()),
        Mailbox::Folder { account_id, folder_id } => {
            let accounts = s.accounts.read();
            let acct = accounts.iter().find(|a| &a.account.id == account_id);
            let name = acct
                .and_then(|a| a.folders.iter().find(|f| &f.folder.id == folder_id))
                .map(|f| f.folder.name.clone())
                .unwrap_or_default();
            (name, acct.map(|a| s.account_label(&a.account.id, &a.account.email)).unwrap_or_default())
        }
    }
}

/// What the list drew last, and the motion in flight (see list_motion).
#[derive(Default)]
struct Motion {
    view: String,
    keys: Vec<String>,
    items: Vec<ListItem>,
    /// Display order of live rows and ghosts.
    order: Vec<super::list_motion::Entry>,
    /// (index in `items` before the change, the removed row).
    ghosts: Vec<(usize, ListItem)>,
    entering: std::collections::HashSet<String>,
    generation: u64,
}

/// The outgoing rows drawn last and their motion (see `plan_outgoing`).
#[derive(Default)]
struct OutMotion {
    view: String,
    keys: Vec<String>,
    rows: Vec<Outgoing>,
    order: Vec<super::list_motion::Entry>,
    ghosts: Vec<(usize, Outgoing)>,
    entering: std::collections::HashSet<String>,
    generation: u64,
}

/// A ghost gets its own key: the same draft may be back (retry) meanwhile.
fn out_key(id: &str, motion: Option<&str>) -> String {
    if motion == Some("leaving") { format!("out-ghost-{id}") } else { format!("out-{id}") }
}

/// Live rows interleaved with collapsing ghosts, as (item, is_ghost).
fn merged_items(
    items: &[ListItem],
    order: &[super::list_motion::Entry],
    ghosts: &[(usize, ListItem)],
) -> Vec<(ListItem, bool)> {
    use super::list_motion::Entry;
    if ghosts.is_empty() || order.is_empty() {
        return items.iter().map(|i| (i.clone(), false)).collect();
    }
    order
        .iter()
        .filter_map(|e| match *e {
            Entry::Live(i) => items.get(i).map(|i| (i.clone(), false)),
            Entry::Ghost(g) => ghosts.iter().find(|(j, _)| *j == g).map(|(_, i)| (i.clone(), true)),
        })
        .collect()
}

#[component]
pub fn MailList() -> Element {
    let s = use_context::<AppState>();
    let mut threaded = s.threaded;
    let items = s.list.read().clone();
    // Search happens in place: the folder keeps its title.
    let (heading, mut sub) = title(&s, &s.mailbox.read());
    let searching = s.search.read().as_ref().map(|st| (st.query.trim().is_empty(), st.items.len()));
    match searching {
        Some((false, 1)) => sub = "1 result".into(),
        Some((false, n)) => sub = format!("{n} results"),
        _ => {}
    }
    let now = chrono::Local::now();
    let many = s.accounts.read().len() > 4;

    let mailbox = s.mailbox.read().clone();
    let account_ids: Vec<String> = s.accounts.read().iter().map(|a| a.account.id.clone()).collect();
    let in_scope = scope_accounts(&mailbox, &account_ids);
    let cold = s.accounts.read().iter().any(|a| in_scope.contains(&a.account.id) && !a.first_sync_done);
    let sync = s.sync.read().clone();
    let expired: Vec<String> = sync.needs_sign_in.iter().filter(|a| in_scope.contains(a)).cloned().collect();
    let offline = sync.offline;
    let folder_role = match &mailbox {
        Mailbox::Folder { account_id, folder_id } => s
            .accounts
            .read()
            .iter()
            .find(|a| &a.account.id == account_id)
            .and_then(|a| a.folders.iter().find(|f| &f.folder.id == folder_id))
            .and_then(|f| f.folder.role),
        Mailbox::Unified(_) => None,
    };
    let outgoing: Vec<Outgoing> = s
        .outgoing
        .read()
        .iter()
        .filter(|o| shows_outgoing(&mailbox, folder_role, &o.draft.from.account_id))
        .cloned()
        .collect();

    // Virtualized: only the slots in (and near) the viewport are rendered,
    // between spacers that keep the scroll height exact.
    let mut scroll_top = use_signal(|| 0.0f64);
    let mut viewport = use_signal(|| 1200.0f64);
    let mut asked_for = use_signal(|| 0usize);
    // Outgoing rows animate in and out like mail rows (diffed the same way,
    // during render).
    let out_motion = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(OutMotion::default())));
    let mut out_tick = use_signal(|| 0u64);
    let _ = out_tick.read();
    let out_view = format!("{mailbox:?}|{}", s.search.read().is_some());
    {
        let mut m = out_motion.borrow_mut();
        let keys: Vec<String> = outgoing.iter().map(|o| o.draft.id.clone()).collect();
        if m.keys != keys || m.view != out_view {
            let plan = super::list_motion::plan_outgoing(&m.keys, &keys, m.view == out_view);
            m.generation += 1;
            let animating = plan.animates();
            m.ghosts = plan.ghosts.iter().map(|&g| (g, m.rows[g].clone())).collect();
            m.order = super::list_motion::merge(&m.keys, &keys, &plan.ghosts);
            m.entering = plan.entering.into_iter().collect();
            m.rows = outgoing.clone();
            m.keys = keys;
            m.view = out_view;
            if animating {
                let (generation, cell) = (m.generation, out_motion.clone());
                spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(super::list_motion::CLEAR_AFTER_MS)).await;
                    let mut m = cell.borrow_mut();
                    if m.generation == generation {
                        m.entering.clear();
                        m.ghosts.clear();
                        m.order = (0..m.rows.len()).map(super::list_motion::Entry::Live).collect();
                        drop(m);
                        out_tick += 1;
                    }
                });
            }
        }
    }
    let out_display: Vec<(Outgoing, Option<&'static str>)> = {
        use super::list_motion::Entry;
        let m = out_motion.borrow();
        m.order
            .iter()
            .filter_map(|e| match *e {
                Entry::Live(i) => {
                    m.rows.get(i).map(|o| (o.clone(), m.entering.contains(&o.draft.id).then_some("entering")))
                }
                Entry::Ghost(g) => m.ghosts.iter().find(|(j, _)| *j == g).map(|(_, o)| (o.clone(), Some("leaving"))),
            })
            .collect()
    };
    // Outgoing (sending/failed) rows sit above the virtualized rows.
    let style = *s.list_style.read();
    let metrics = style.metrics();
    let avatars = *s.list_avatars.read();
    let out_h = out_display
        .iter()
        .map(|(o, _)| {
            metrics.row_h + if matches!(o.state, OutState::Sending | OutState::Sent(_)) { SEND_BAND_EXTRA } else { 0.0 }
        })
        .sum::<f64>();

    // Insert/remove motion. Diffed here, during render, against the last
    // list this component drew (a non-reactive cell), so a removed row is
    // replaced by its collapsing ghost in the same frame it leaves `items`.
    let motion = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(Motion::default())));
    let mut motion_tick = use_signal(|| 0u64);
    let _ = motion_tick.read();
    let keys: Vec<String> = items.iter().map(AppState::key).collect();
    let view = format!("{mailbox:?}|{}|{}", *threaded.read(), s.search.read().is_some());
    {
        let mut m = motion.borrow_mut();
        if m.keys != keys || m.view != view {
            let same_view = m.view == view;
            let list_top = (*scroll_top.peek() - out_h).max(0.0);
            let at_top = list_top < metrics.row_h / 2.0;
            let old_layout = vlist::layout(m.items.iter().map(|i| format::group(i.header().received_at, now)), metrics);
            let first_visible = vlist::first_visible_item(&old_layout, list_top).unwrap_or(0);
            let plan = super::list_motion::plan(&m.keys, &keys, same_view, at_top, first_visible);
            let ghosts: Vec<(usize, ListItem)> = plan.ghosts.iter().map(|&g| (g, m.items[g].clone())).collect();
            let order = super::list_motion::merge(&m.keys, &keys, &plan.ghosts);
            // Scrolled down: keep the anchor row where it is on screen.
            if let Some(anchor) = plan.anchor.as_deref() {
                let old_top = m
                    .items
                    .iter()
                    .position(|i| AppState::key(i) == anchor)
                    .and_then(|i| vlist::item_span(&old_layout, i));
                let merged = merged_items(&items, &order, &ghosts);
                let new_layout =
                    vlist::layout(merged.iter().map(|(i, _)| format::group(i.header().received_at, now)), metrics);
                let new_top = merged
                    .iter()
                    .position(|(i, _)| AppState::key(i) == anchor)
                    .and_then(|i| vlist::item_span(&new_layout, i));
                if let (Some((o, _)), Some((n, _))) = (old_top, new_top) {
                    let delta = n - o;
                    if delta.abs() > 0.5 {
                        document::eval(&format!(
                            "requestAnimationFrame(()=>{{const l=document.querySelector('.pane--list .list[role=listbox]'); if(l) l.scrollTop+={delta};}})"
                        ));
                    }
                }
            }
            m.generation += 1;
            let animating = plan.animates();
            m.entering = plan.entering.into_iter().collect();
            m.ghosts = ghosts;
            m.order = order;
            m.items = items.clone();
            m.keys = keys.clone();
            m.view = view;
            if animating {
                let (generation, cell) = (m.generation, motion.clone());
                spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(super::list_motion::CLEAR_AFTER_MS)).await;
                    let mut m = cell.borrow_mut();
                    if m.generation == generation {
                        m.entering.clear();
                        m.ghosts.clear();
                        m.order = (0..m.items.len()).map(super::list_motion::Entry::Live).collect();
                        drop(m);
                        motion_tick += 1;
                    }
                });
            }
        }
    }
    let (display, entering) = {
        let m = motion.borrow();
        (merged_items(&items, &m.order, &m.ghosts), m.entering.clone())
    };

    let layout = vlist::layout(display.iter().map(|(i, _)| format::group(i.header().received_at, now)), metrics);
    let (start, end, top_pad, bottom_pad) =
        vlist::window(&layout, (*scroll_top.read() - out_h).max(0.0), *viewport.read());
    let rows: Vec<Element> = layout.slots[start..end]
        .iter()
        .enumerate()
        .map(|(n, (slot, _))| match *slot {
            vlist::Slot::Group(g) => rsx! { div { class: "list-group", key: "g-{g}", "{g}" } },
            vlist::Slot::Item(i) => {
                let (item, ghost) = &display[i];
                let key = AppState::key(item);
                let motion = if *ghost {
                    Some("leaving")
                } else if entering.contains(&key) {
                    Some("entering")
                } else {
                    None
                };
                let rkey = if *ghost { format!("ghost-{key}") } else { key };
                let edge = layout.edge(start + n);
                rsx! { Row { key: "{rkey}", item: item.clone(), many, motion, edge, avatar: avatars } }
            }
        })
        .collect();
    let total = layout.total + out_h;
    let count = items.len();

    // Keep the keyboard selection visible (it may not be rendered at all).
    let selected_index =
        s.selected.read().as_ref().and_then(|k| display.iter().position(|(i, g)| !g && AppState::key(i) == *k));
    let span = selected_index.and_then(|i| vlist::item_span(&layout, i)).map(|(t, b)| (t + out_h, b + out_h));
    use_effect(use_reactive!(|span| {
        if let Some((top, bottom)) = span
            && let Some(y) = vlist::scroll_into_view(*scroll_top.peek(), *viewport.peek(), top, bottom, metrics.group_h)
        {
            document::eval(&format!(
                "const l=document.querySelector('.pane--list .list[role=listbox]'); if(l) l.scrollTop={y};"
            ));
        }
    }));
    let on_scroll = move |e: Event<ScrollData>| {
        let (top, h) = (e.data().scroll_top(), e.data().client_height() as f64);
        scroll_top.set(top);
        if h > 0.0 {
            viewport.set(h);
        }
        // Near the end of what's loaded: ask the engine for the next page,
        // once per list length.
        if top + h > total - vlist::LOAD_AHEAD && count != *asked_for.peek() {
            asked_for.set(count);
            crate::send_cmd(pigeon_engine::service::Cmd::LoadMore { have: count as u32 });
        }
    };

    rsx! {
        section { class: "pane pane--list", "aria-label": "{heading}", "data-list-style": style.as_pref(),
            header { class: "pane-header",
                h1 { class: "pane-header__title", dir: "auto", "{heading}" }
                span { class: "pane-header__sub", "{sub}" }
                span { class: "spacer" }
                div { class: "segmented segmented--sm", role: "tablist", "aria-label": "Grouping",
                    button { role: "tab", "aria-selected": "{*threaded.read()}", onclick: move |_| threaded.set(true), "Threads" }
                    button { role: "tab", "aria-selected": "{!*threaded.read()}", onclick: move |_| threaded.set(false), "Messages" }
                }
                button { class: "icon-btn", r#type: "button", "aria-label": "New message", title: "New message (⌘N)",
                    onclick: move |_| super::new_message(s),
                    Icon { name: "edit" }
                }
            }
            if searching.is_none() {
                {
                    let search_key = if s.reader.read().is_some() { hint(Do::Search) } else { "⌘F".to_string() };
                    rsx! {
                button { class: "list-search", r#type: "button", "aria-label": "Search {heading}",
                    onclick: move |_| super::search::open_here(s),
                    Icon { name: "search", class: "icon--sm" }
                    span { class: "list-search__placeholder", dir: "auto", "Search {heading}" }
                    // ⌘F finds in an open conversation instead.
                    if !search_key.is_empty() { kbd { class: "kbd", "{search_key}" } }
                }
                    }
                }
            }
            if searching.is_some() {
                super::search::SearchPane {}
            } else if items.is_empty() && out_display.is_empty() && cold {
                div { class: "list", role: "listbox", "aria-busy": "true",
                    for i in 0..9 {
                        div { class: "row row--skeleton", key: "sk{i}",
                            span { class: "skeleton" } span { class: "skeleton" } span { class: "skeleton" }
                        }
                    }
                }
            } else if items.is_empty() && out_display.is_empty() && expired.is_empty() {
                Empty { syncing: s.sync.read().syncing }
            } else {
                div { class: "list", role: "listbox", "data-auth": (!expired.is_empty()).then_some("expired"),
                    "data-style": style.as_pref(),
                    "data-avatars": avatars.then_some("true"),
                    onscroll: on_scroll,
                    for (n, (o, motion)) in out_display.iter().enumerate() {
                        OutgoingRow {
                            edge: match (n == 0, n + 1 == out_display.len()) {
                                (true, true) => Some("only"),
                                (true, false) => Some("first"),
                                (false, true) => Some("last"),
                                _ => None,
                            },
                            key: "{out_key(&o.draft.id, *motion)}",
                            out: o.clone(),
                            many,
                            motion: *motion,
                        }
                    }
                    div { style: "height:{top_pad}px", "aria-hidden": "true" }
                    {rows.into_iter()}
                    div { style: "height:{bottom_pad}px", "aria-hidden": "true" }
                }
            }
            if let Some(acct) = expired.first() {
                AuthCard { account_id: acct.clone() }
            }
            if offline {
                div { class: "list-footer",
                    Icon { name: "offline", class: "icon--sm" }
                    span { class: "t-caption", "Offline · changes sync when you’re back" }
                }
            }
        }
    }
}

/// How much taller a sending row is than the style's row: room for its
/// "Sending…" band (app.css `.row__send-band`).
const SEND_BAND_EXTRA: f64 = 30.0;

const SLEEPING_BIRD: &str = r##"<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong)" stroke-width="1"/><path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"/><path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent)" stroke-width="3" stroke-linecap="round"/><path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning)"/><path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"/><path d="M28 45V54M35 45V54" stroke="var(--warning)" stroke-width="2.4" stroke-linecap="round"/>"##;
/// Same bird, awake (onboarding).
pub const AWAKE_BIRD: &str = r##"<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong)" stroke-width="1"/><path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"/><path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent)" stroke-width="3" stroke-linecap="round"/><path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning)"/><circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"/><path d="M28 45V54M35 45V54" stroke="var(--warning)" stroke-width="2.4" stroke-linecap="round"/>"##;

/// What an empty mailbox says: only the inbox gets the sleeping pigeon.
fn empty_copy(role: Option<FolderRole>) -> (&'static str, &'static str) {
    match role {
        Some(FolderRole::Inbox) => ("All caught up", "Nothing left here. Enjoy the quiet."),
        Some(FolderRole::Sent) => ("Nothing sent yet", "Messages you send show up here."),
        Some(FolderRole::Drafts) => ("No drafts", "Unsent messages are kept here as you write."),
        Some(FolderRole::Trash) => ("Trash is empty", "Delete a message here to remove it for good."),
        Some(FolderRole::Spam) => ("No junk", "Nothing suspicious got through."),
        Some(FolderRole::Starred) => ("Nothing flagged", "Flag a message with F to keep it here."),
        _ => ("No messages here", "This folder is empty."),
    }
}

#[component]
fn Empty(syncing: bool) -> Element {
    let s = use_context::<AppState>();
    let role = match &*s.mailbox.read() {
        Mailbox::Unified(r) => Some(*r),
        Mailbox::Folder { account_id, folder_id } => s
            .accounts
            .read()
            .iter()
            .find(|a| &a.account.id == account_id)
            .and_then(|a| a.folders.iter().find(|f| &f.folder.id == folder_id))
            .and_then(|f| f.folder.role),
    };
    let inbox = role == Some(FolderRole::Inbox);
    let (title, body) = empty_copy(role);
    // Inbox zero shows what's next today, as in the design.
    let now = pigeon_core::now_ms();
    let next = if inbox && !syncing {
        s.next_up_shown()
            .iter()
            .find(|e| e.event.end_at > now && !e.event.all_day && format::same_day(e.event.start_at, now))
            .cloned()
    } else {
        None
    };
    rsx! {
        div { class: "empty-state",
            if inbox || syncing {
                div { class: "perch",
                    // The app icon's bird, asleep (closed eye); collar and feet follow the theme.
                    svg { view_box: "4 4 60 56", "aria-hidden": "true", dangerous_inner_html: SLEEPING_BIRD }
                    if !syncing {
                        span { class: "perch__z", "z" }
                        span { class: "perch__z", "z" }
                    }
                }
            }
            if syncing {
                div { class: "empty-state__title", "Fetching your mail…" }
                div { class: "empty-state__body", "Recent messages show up first; older mail fills in behind." }
            } else {
                div { class: "empty-state__title", "{title}" }
                div { class: "empty-state__body", "{body}" }
            }
            if let Some(e) = next {
                div { class: "empty-state__next",
                    div { class: "next-up__item", "data-hue": "{s.hue(&e.account_id)}", style: "flex:1;min-width:0",
                        span { class: "next-up__bar" }
                        div {
                            div { class: "next-up__title", dir: "auto", "{e.event.title}" }
                            div { class: "next-up__when", "Next: {format::hhmm(e.event.start_at)}–{format::hhmm(e.event.end_at)}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Row(
    item: ListItem,
    many: bool,
    #[props(default)] motion: Option<&'static str>,
    /// Place in its date group (day bands round the ends).
    #[props(default)]
    edge: Option<&'static str>,
    /// Draw the sender avatar column.
    #[props(default)]
    avatar: bool,
) -> Element {
    let s = use_context::<AppState>();
    let key = AppState::key(&item);
    let open = s.selected.read().as_deref() == Some(key.as_str());
    let selected = open || s.multi.read().contains(&key);
    let hue = s.hue(item.account_id());
    let h = item.header().clone();
    let (who, count, unread) = match &item {
        ListItem::Thread(t) => {
            let who = if t.participants.is_empty() { "(unknown)".to_string() } else { t.participants.join(", ") };
            (who, (t.message_count > 1).then_some(t.message_count), t.unread_count > 0)
        }
        ListItem::Message(m) => {
            (m.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_default(), None, !m.header.is_read)
        }
    };
    let who = if h.is_draft { "Draft".to_string() } else { who };
    let letter = s
        .accounts
        .read()
        .iter()
        .find(|a| a.account.id == item.account_id())
        .and_then(|a| a.account.email.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?');
    let time = format::row_time(h.received_at, chrono::Local::now());
    let subject = if h.subject.is_empty() { "(no subject)".to_string() } else { h.subject.clone() };
    let (i1, i2, i3, i4, i5) = (item.clone(), item.clone(), item.clone(), item.clone(), item.clone());
    let (i6, i7, i8) = (item.clone(), item.clone(), item.clone());
    // Drafts stay put; rows on their way out can't be picked up.
    let can_drag = !h.is_draft && motion.is_none();
    let (drag_accounts, drag_label) = super::mail_drag::row_attrs(&s, &item, &subject);
    // Ringed while its right-click menu is open.
    let menu_on = s.row_menu.read().as_ref().is_some_and(|m| !m.bulk && AppState::key(&m.item) == key);
    let flagged = h.is_flagged;
    let flag_label = tip(if flagged { "Unflag" } else { "Flag" }, Do::Flag);
    let read_label = if unread { tip("Mark read", Do::MarkRead) } else { tip("Mark unread", Do::MarkUnread) };
    // Inline invite only when the body is already loaded (never fetch for the list).
    let invite_key = (item.account_id().to_string(), h.id.clone());
    let row_invite = s
        .bodies
        .read()
        .get(&invite_key)
        .and_then(|b| b.invite.clone())
        .filter(|i| i.method == pigeon_core::model::InviteMethod::Request);
    let row_event = s.invites.read().get(&invite_key).and_then(|v| v.event.clone());

    rsx! {
        div { class: "row", role: "option", "data-hue": "{hue}",
            "data-entering": (motion == Some("entering")).then_some("true"),
            "data-leaving": (motion == Some("leaving")).then_some("true"),
            "aria-hidden": (motion == Some("leaving")).then_some("true"),
            "data-unread": unread.then_some("true"),
            "data-flagged": flagged.then_some("true"),
            "data-state": h.is_draft.then_some("draft"),
            "data-stale": s.sync.read().needs_sign_in.iter().any(|a| a == item.account_id()).then_some("true"),
            "aria-selected": "{selected}",
            "data-focused": open.then_some("true"),
            "data-context": menu_on.then_some("true"),
            "data-edge": edge,
            draggable: if can_drag { "true" } else { "false" },
            "data-mail-drag": can_drag.then_some(drag_accounts),
            "data-drag-label": can_drag.then_some(drag_label),
            ondragstart: move |_| super::mail_drag::start(s, &i8),
            onclick: move |e| super::multi::click(s, &i1, super::multi::How::of(&e)),
            // Its own window (the first click has opened it here already).
            ondoubleclick: move |e| {
                if super::multi::How::of(&e) == super::multi::How::Plain && motion != Some("leaving") {
                    super::reader_window::open(s, i6.clone());
                }
            },
            oncontextmenu: move |e| {
                e.prevent_default();
                if motion != Some("leaving") {
                    let p = e.client_coordinates();
                    super::row_menu::open(s, &i7, p.x, p.y);
                }
            },
            if avatar {
                match h.from.clone() {
                    Some(from) => rsx! {
                        super::avatar::Avatar { email: from.email.clone(), label: from.display().to_string(), class: "avatar--36 row__avatar" }
                    },
                    None => rsx! { span { class: "avatar avatar--36 row__avatar", "?" } },
                }
            }
            div { class: "row__line",
                if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
                match h.from.clone().filter(|_| !h.is_draft) {
                    Some(from) => rsx! {
                        super::sender_card::Person { account_id: item.account_id().to_string(), message_id: Some(h.id.clone()),
                            addr: from, sender: true, click_through: true, class: "row__who",
                            span { dir: "auto", "{who}" }
                        }
                    },
                    None => rsx! { span { class: "row__who", dir: "auto", "{who}" } },
                }
                if let Some(n) = count { span { class: "row__count", "{n}" } }
                span { class: "row__meta",
                    if flagged { Icon { name: "flag", class: "row__flag" } }
                    if h.has_attachments { Icon { name: "clip" } }
                    time { class: "row__time", "{time}" }
                }
                // A quick second click on a button isn't a double-click on the row.
                span { class: "row__actions", ondoubleclick: move |e| e.stop_propagation(),
                    button { class: "icon-btn icon-btn--sm", "aria-label": tip("Archive", Do::Archive),
                        onclick: move |e| { e.stop_propagation(); act(s, &i2, MailAction::Archive) },
                        Icon { name: "archive" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": tip("Delete", Do::Delete),
                        onclick: move |e| { e.stop_propagation(); act(s, &i3, MailAction::Trash) },
                        Icon { name: "trash" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": flag_label,
                        onclick: move |e| {
                            e.stop_propagation();
                            act(s, &i4, if flagged { MailAction::Unflag } else { MailAction::Flag })
                        },
                        Icon { name: "flag" }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": read_label,
                        onclick: move |e| {
                            e.stop_propagation();
                            act(s, &i5, if unread { MailAction::MarkRead } else { MailAction::MarkUnread })
                        },
                        Icon { name: "mailopen" }
                    }
                }
            }
            div { class: "row__subject", dir: "auto", "{subject}" }
            match row_invite {
                Some(inv) => rsx! { super::invite::RowInvite { invite: inv, event: row_event } },
                // The text gets its own shrinkable box: the snippet row is a flex
                // container, and RTL text there overflowed past the edge.
                None => rsx! { div { class: "row__snippet", dir: "auto", span { class: "row__snippet-text", dir: "auto", "{h.snippet}" } } },
            }
        }
    }
}

#[component]
fn OutgoingRow(
    out: Outgoing,
    many: bool,
    #[props(default)] motion: Option<&'static str>,
    #[props(default)] edge: Option<&'static str>,
) -> Element {
    let s = use_context::<AppState>();
    let d = out.draft.clone();
    let hue = s.hue(&d.from.account_id);
    let to = d.to.iter().chain(&d.cc).map(|a| a.display().to_string()).collect::<Vec<_>>().join(", ");
    let subject = if d.subject.is_empty() { "(no subject)".to_string() } else { d.subject.clone() };
    let state = match &out.state {
        OutState::Sending | OutState::Sent(_) => "sending",
        OutState::Failed(_) => "failed",
        OutState::Queued(_) => "queued",
    };
    let letter = d.from.address.email.chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or('?');
    let (retry, open, cancel) = (d.clone(), d.clone(), d.id.clone());
    rsx! {
        div { class: "row", role: "option", "data-hue": "{hue}", "data-state": "{state}", "data-edge": edge,
            "data-entering": (motion == Some("entering")).then_some("true"),
            "data-leaving": (motion == Some("leaving")).then_some("true"),
            "aria-hidden": (motion == Some("leaving")).then_some("true"),
            onclick: move |_| {
                if motion != Some("leaving") {
                    let mut dr = s.draft;
                    dr.set(Some(open.clone()));
                }
            },
            if *s.list_avatars.read() {
                span { class: "avatar avatar--36 avatar--self row__avatar", "{format::initials(&d.from.address.display().to_string())}" }
            }
            div { class: "row__line",
                if many { span { class: "pip", "{letter}" } } else { span { class: "dot" } }
                span { class: "row__who", dir: "auto", "To: {to}" }
                span { class: "row__meta",
                    if let OutState::Queued(at) = out.state { time { class: "row__time", "{format::hhmm(at)}" } }
                }
            }
            div { class: "row__subject", dir: "auto", "{subject}" }
            match out.state.clone() {
                OutState::Sending | OutState::Sent(_) => rsx! {
                    div { class: "row__send-band",
                        span { "Sending…" }
                        span { class: "progress progress--indeterminate", role: "progressbar", "aria-label": "Sending", span {} }
                    }
                },
                OutState::Failed(err) => rsx! {
                    div { class: "row__snippet", title: "{err}",
                        "Failed to send · "
                        a { href: "#", onclick: move |e| {
                                e.prevent_default();
                                e.stop_propagation();
                                s.track_outgoing(retry.clone(), OutState::Sending);
                                send_cmd(Cmd::Send(retry.clone()));
                            },
                            "Retry"
                        }
                        span { class: "t-3", style: "font-weight:400", " · Saved to Drafts" }
                    }
                },
                OutState::Queued(at) => rsx! {
                    div { class: "row__snippet",
                        "Scheduled · {format::long_time(at, chrono::Local::now())} · "
                        a { href: "#", onclick: move |e| {
                                e.prevent_default();
                                e.stop_propagation();
                                send_cmd(Cmd::CancelScheduled { draft_id: cancel.clone() });
                            },
                            "Cancel"
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn AuthCard(account_id: String) -> Element {
    let s = use_context::<AppState>();
    let acct = s.accounts.read().iter().find(|a| a.account.id == account_id).map(|a| a.account.clone());
    let Some(acct) = acct else { return rsx! {} };
    let provider = acct.provider;
    let label = match provider {
        ProviderKind::Microsoft => "Microsoft 365",
        ProviderKind::Google => "Google",
        ProviderKind::ICloud => "iCloud",
    };
    let email = acct.email.clone();
    rsx! {
        div { class: "auth-card", role: "alertdialog",
            div { class: "auth-card__icon", Icon { name: "lock" } }
            div { class: "auth-card__title", "Sign in to {acct.email} again" }
            div { class: "auth-card__body",
                "Your {label} sign-in expired. Mail stays on this Mac; new mail and your changes wait until you sign in."
            }
            button { class: "btn btn--primary", r#type: "button",
                onclick: move |_| {
                    if provider == ProviderKind::ICloud {
                        super::add_account::open_icloud(s, &email);
                    } else {
                        let mut si = s.signing_in;
                        si.set(true);
                        send_cmd(Cmd::Login { provider, login_hint: Some(email.clone()) });
                    }
                },
                "Sign in again"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_rows_show_in_inbox_sent_drafts_of_their_account() {
        assert!(shows_outgoing(&Mailbox::Unified(FolderRole::Inbox), None, "a"));
        assert!(!shows_outgoing(&Mailbox::Unified(FolderRole::Trash), None, "a"));
        let f = Mailbox::Folder { account_id: "a".into(), folder_id: "x".into() };
        assert!(shows_outgoing(&f, Some(FolderRole::Sent), "a"));
        assert!(!shows_outgoing(&f, Some(FolderRole::Sent), "b"));
        assert!(!shows_outgoing(&f, None, "a"));
    }

    #[test]
    fn scope_is_all_accounts_for_unified() {
        let all = vec!["a".to_string(), "b".to_string()];
        assert_eq!(scope_accounts(&Mailbox::Unified(FolderRole::Inbox), &all), all);
        assert_eq!(scope_accounts(&Mailbox::Folder { account_id: "b".into(), folder_id: "f".into() }, &all), ["b"]);
    }
}
