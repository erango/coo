//! Saving a message's attachments: right-click one for Open / Save As… /
//! Save All Attachments…, and a "Save all…" link above two or more. The
//! dialogs are macOS's own (rfd); the engine downloads and copies.

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::send_cmd;

/// (attachment id, file name).
pub type File = (String, String);

/// Ask where to save one attachment, then save it.
pub fn save_as(account_id: String, message_id: String, file: File) {
    // Outlives the right-click menu that asked (it closes right away).
    dioxus::core::spawn_forever(async move {
        let dialog = rfd::AsyncFileDialog::new().set_title("Save Attachment").set_file_name(&file.1);
        let dialog = match super::dirs_downloads() {
            Some(d) => dialog.set_directory(d),
            None => dialog,
        };
        if let Some(to) = dialog.save_file().await {
            send_cmd(Cmd::SaveAttachments {
                account_id,
                message_id,
                files: vec![file],
                dest: to.path().to_path_buf(),
                into_folder: false,
                done: None,
            });
        }
    });
}

/// Ask for a folder, then save every attachment into it.
pub fn save_all(account_id: String, message_id: String, files: Vec<File>) {
    dioxus::core::spawn_forever(async move {
        let dialog = rfd::AsyncFileDialog::new().set_title("Save All Attachments");
        let dialog = match super::dirs_downloads() {
            Some(d) => dialog.set_directory(d),
            None => dialog,
        };
        if let Some(dir) = dialog.pick_folder().await {
            send_cmd(Cmd::SaveAttachments {
                account_id,
                message_id,
                files,
                dest: dir.path().to_path_buf(),
                into_folder: true,
                done: None,
            });
        }
    });
}

/// The attachments ticked in message `message_id`.
pub fn selected(s: &crate::state::AppState, message_id: &str) -> Vec<String> {
    let sel = s.att_sel.read();
    if sel.0 == message_id { sel.1.clone() } else { Vec::new() }
}

/// Tick exactly these attachments of `message_id` (none: clear).
pub fn select(s: crate::state::AppState, message_id: &str, ids: Vec<String>) {
    let mut sel = s.att_sel;
    sel.set((message_id.to_string(), ids));
}

/// A click on attachment `id`: toggle it, or with `range` (Shift) tick
/// everything from the last ticked one to it, in the order of `all`.
pub fn tick(s: crate::state::AppState, message_id: &str, all: &[String], id: &str, range: bool) {
    let mut now = selected(&s, message_id);
    let at = |x: &str| all.iter().position(|a| a == x);
    match (range, now.last().and_then(|l| at(l)), at(id)) {
        (true, Some(from), Some(to)) => {
            for a in &all[from.min(to)..=from.max(to)] {
                if !now.contains(a) {
                    now.push(a.clone());
                }
            }
        }
        _ => match now.iter().position(|a| a == id) {
            Some(i) => {
                now.remove(i);
            }
            None => now.push(id.to_string()),
        },
    }
    select(s, message_id, now);
}

/// The bridge's `attach-select` (a box dragged around attachments):
/// `{m: message id, ids: [...], add: bool}`.
pub fn select_from_bridge(s: crate::state::AppState, json: &str) {
    #[derive(serde::Deserialize)]
    struct Boxed {
        m: String,
        ids: Vec<String>,
        #[serde(default)]
        add: bool,
    }
    let Ok(b) = serde_json::from_str::<Boxed>(json) else { return };
    let mut ids = if b.add { selected(&s, &b.m) } else { Vec::new() };
    for id in b.ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    select(s, &b.m, ids);
}

/// The open right-click menu: pointer position and which attachment.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub x: f64,
    pub y: f64,
    pub file: File,
}

const WIDTH: f64 = 236.0;

#[component]
pub fn AttachmentMenu(account_id: String, message_id: String, all: Vec<File>, menu: Signal<Option<Menu>>) -> Element {
    let Some(m) = menu.read().clone() else { return rsx! {} };
    let mut close = move || menu.set(None);
    let height = if all.len() > 1 { 32.0 * 3.0 + 11.0 + 12.0 } else { 32.0 * 2.0 + 12.0 };
    // Kept inside the window, as the list's row menu is.
    let style = format!(
        "width:{WIDTH}px;left:max(8px, min({x}px, calc(100vw / var(--ui-zoom, 1) - {WIDTH}px - 8px)));\
         top:max(8px, min({y}px, calc(100vh / var(--ui-zoom, 1) - {height}px - 8px)))",
        x = m.x,
        y = m.y,
    );
    let (a1, a2, a3) = (account_id.clone(), account_id.clone(), account_id);
    let (m1, m2, m3) = (message_id.clone(), message_id.clone(), message_id);
    let (f1, f2) = (m.file.clone(), m.file.clone());
    let many = all.len() > 1;
    rsx! {
        div { class: "row-menu-scrim",
            onclick: move |_| close(),
            oncontextmenu: move |e| { e.prevent_default(); close() },
            div { class: "menu row-menu", role: "menu", "aria-label": "Attachment", style: "{style}",
                onclick: move |e| e.stop_propagation(),
                oncontextmenu: move |e| { e.prevent_default(); e.stop_propagation() },
                div { class: "menu-item", role: "menuitem",
                    onclick: move |_| {
                        close();
                        send_cmd(Cmd::OpenAttachment { account_id: a1.clone(), message_id: m1.clone(), attachment_id: f1.0.clone(), how: Default::default() });
                    },
                    Icon { name: "popout" }
                    span { "Open" }
                }
                div { class: "menu-item", role: "menuitem",
                    onclick: move |_| { close(); save_as(a2.clone(), m2.clone(), f2.clone()); },
                    Icon { name: "download" }
                    span { "Save As…" }
                }
                if many {
                    div { class: "menu-sep", role: "separator" }
                    div { class: "menu-item", role: "menuitem",
                        onclick: move |_| { close(); save_all(a3.clone(), m3.clone(), all.clone()); },
                        Icon { name: "folder" }
                        span { "Save All Attachments…" }
                    }
                }
            }
        }
    }
}
