//! Google Drive in the composer: the attach menu ("From this Mac…" /
//! "From Google Drive…"), the "waiting for your browser" bar, and the sheet
//! where each picked file becomes an attachment (or an export of a Google
//! file) or a link.

use dioxus::prelude::*;
use pigeon_core::compose::{AttachmentSource, DraftAttachment};
use pigeon_core::drive::{
    DriveChoice, DriveFile, LinkReason, MAX_MESSAGE_BYTES, chosen_bytes, default_choices, drive_attachment,
    export_formats, links_html,
};
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::Icon;
use super::compose::ComposeSlot;
use crate::send_cmd;
use crate::state::{AppState, DrivePicked};

/// Icon and tint for a file, by type.
pub fn file_kind(mime: &str, name: &str) -> (&'static str, &'static str) {
    let n = name.to_ascii_lowercase();
    match mime {
        m if m.contains("pdf") || n.ends_with(".pdf") => ("pdf", "pdf"),
        m if m.contains("sheet") || m.contains("excel") || m.contains("csv") || n.ends_with(".xlsx") => {
            ("sheet", "sheet")
        }
        m if m.contains("document") || m.contains("word") || n.ends_with(".docx") => ("doc", "file"),
        m if m.starts_with("image/") || m.ends_with(".drawing") => ("file", "image"),
        _ => ("file", "file"),
    }
}

/// What the size column says for a picked file.
fn size_label(f: &DriveFile) -> String {
    match (f.size_bytes, f.mime_type.as_str()) {
        (Some(n), _) => crate::format::size(n),
        (None, "application/vnd.google-apps.document") => "Google Docs".into(),
        (None, "application/vnd.google-apps.spreadsheet") => "Google Sheets".into(),
        (None, "application/vnd.google-apps.presentation") => "Google Slides".into(),
        (None, "application/vnd.google-apps.drawing") => "Google Drawings".into(),
        (None, m) if m.starts_with("application/vnd.google-apps.") => "Google file".into(),
        _ => String::new(),
    }
}

/// Bytes already attached to a draft (local files and sized Drive files).
pub fn attached_bytes(atts: &[DraftAttachment]) -> u64 {
    atts.iter()
        .map(|a| match &a.source {
            AttachmentSource::File { path } => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            AttachmentSource::Drive { size, .. } => size.unwrap_or(0),
            AttachmentSource::Message { .. } => 0,
        })
        .sum()
}

/// Google accounts that can attach from Drive: (account id, label).
fn google_accounts(s: &AppState) -> Vec<(String, String)> {
    s.accounts
        .read()
        .iter()
        .filter(|a| a.account.provider == ProviderKind::Google)
        .map(|a| (a.account.id.clone(), s.account_label(&a.account.id, &a.account.email)))
        .collect()
}

fn start_pick(s: AppState, draft_id: String, account_id: String) {
    let mut waiting = s.drive_waiting;
    waiting.set(Some(draft_id.clone()));
    let nickname = s.nicknames.read().get(&account_id).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    send_cmd(Cmd::DrivePick { draft_id, account_id, nickname });
}

/// Add local files to a draft's attachments, skipping ones already there
/// (and empty paths, which a drop of non-files can produce).
pub fn add_local_files(atts: &mut Vec<DraftAttachment>, paths: impl IntoIterator<Item = std::path::PathBuf>) {
    for path in paths {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let already = atts
            .iter()
            .any(|a| matches!(&a.source, AttachmentSource::File { path: p } if std::path::Path::new(p) == path));
        if !already {
            atts.push(DraftAttachment {
                name,
                content_type: None,
                source: AttachmentSource::File { path: path.to_string_lossy().into_owned() },
            });
        }
    }
}

/// The native open panel, any file type, several at once. Not a hidden
/// `<input type=file>`: Dioxus turns a missing `accept` into the filter
/// `*`, which macOS takes as the extension "*" and greys out every file.
fn pick_local_files(slot: ComposeSlot) {
    spawn(async move {
        let Some(files) = rfd::AsyncFileDialog::new().set_title("Attach").pick_files().await else { return };
        let mut draft = slot.draft;
        if let Some(d) = draft.write().as_mut() {
            add_local_files(&mut d.attachments, files.into_iter().map(|f| f.path().to_path_buf()));
        }
    });
}

/// Paperclip menu: files from this Mac (the native open panel) or from
/// Google Drive (picker in the browser).
#[component]
pub fn AttachMenu() -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    let mut open = use_signal(|| false);
    let draft_id = slot.draft.read().as_ref().map(|d| d.id.clone()).unwrap_or_default();
    let google = google_accounts(&s);
    let available = pigeon_engine::drive::drive_available();
    let is_open = *open.read();
    rsx! {
        span { class: "attach-menu-anchor",
            button { class: "icon-btn icon-btn--sm", r#type: "button", title: "Attach", "aria-label": "Attach",
                "aria-haspopup": "menu", "aria-expanded": "{is_open}",
                onclick: move |_| open.set(!is_open),
                Icon { name: "clip" }
            }
            if is_open {
                div { class: "attach-menu-scrim", onclick: move |_| open.set(false) }
                div { class: "menu attach-menu", role: "menu",
                    div { class: "menu-item", role: "menuitem",
                        onclick: move |_| { open.set(false); pick_local_files(slot); },
                        Icon { name: "upload", class: "icon--sm" }
                        "From this Mac…"
                    }
                    div { class: "menu-sep" }
                    if !available {
                        div { class: "menu-item attach-menu__off", "aria-disabled": "true",
                            Icon { name: "cloud", class: "icon--sm" }
                            div { class: "attach-menu__text",
                                span { "From Google Drive…" }
                                span { class: "attach-menu__hint", "{pigeon_engine::drive::NOT_IN_BUILD}" }
                            }
                        }
                    } else if google.is_empty() {
                        div { class: "menu-item attach-menu__off", "aria-disabled": "true",
                            Icon { name: "cloud", class: "icon--sm" }
                            div { class: "attach-menu__text",
                                span { "From Google Drive…" }
                                span { class: "attach-menu__hint", "Add a Google account to attach from Drive." }
                            }
                        }
                    } else if google.len() == 1 {
                        {
                            let (account_id, label) = google[0].clone();
                            let draft_id = draft_id.clone();
                            rsx! {
                                div { class: "menu-item", role: "menuitem", title: "{label}",
                                    onclick: move |_| { open.set(false); start_pick(s, draft_id.clone(), account_id.clone()); },
                                    Icon { name: "cloud", class: "icon--sm" }
                                    "From Google Drive…"
                                }
                            }
                        }
                    } else {
                        div { class: "menu__label", "From Google Drive" }
                        for (account_id, label) in google {
                            {
                                let draft_id = draft_id.clone();
                                rsx! {
                                    div { class: "menu-item", role: "menuitem", key: "{account_id}",
                                        onclick: move |_| { open.set(false); start_pick(s, draft_id.clone(), account_id.clone()); },
                                        Icon { name: "cloud", class: "icon--sm" }
                                        span { class: "attach-menu__account", "{label}…" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// "Waiting for Google Drive" while the picker is open in the browser.
#[component]
pub fn DriveWaiting() -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    let draft_id = slot.draft.read().as_ref().map(|d| d.id.clone());
    if draft_id.is_none() || *s.drive_waiting.read() != draft_id {
        return rsx! {};
    }
    let mut waiting = s.drive_waiting;
    rsx! {
        div { class: "drive-waiting", role: "status",
            Icon { name: "cloud", class: "icon--sm" }
            span { "Pick files in your browser. They’ll show up here." }
            span { class: "spacer" }
            button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| waiting.set(None), "Stop waiting" }
        }
    }
}

/// The sheet for files just picked for this composer's draft.
#[component]
pub fn DriveSheet(editor: String) -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    let draft_id = slot.draft.read().as_ref().map(|d| d.id.clone());
    let picked = s.drive_picked.read().clone().filter(|p| Some(&p.draft_id) == draft_id.as_ref());
    match picked {
        Some(p) => rsx! { PickedSheet { key: "{p.token}", picked: p, editor } },
        None => rsx! {},
    }
}

/// One choice button's label.
fn choice_label(f: &DriveFile, c: DriveChoice) -> &'static str {
    match c {
        DriveChoice::Attach => "Attach",
        DriveChoice::Export(i) => export_formats(&f.mime_type).get(i).map(|x| x.label).unwrap_or("Attach"),
        DriveChoice::Link => "Link",
    }
}

/// The choices a file offers: Attach/Link, or each export format and Link.
fn options(f: &DriveFile) -> Vec<DriveChoice> {
    let mut out: Vec<DriveChoice> = if f.is_native() {
        (0..export_formats(&f.mime_type).len()).map(DriveChoice::Export).collect()
    } else {
        vec![DriveChoice::Attach]
    };
    out.push(DriveChoice::Link);
    out
}

#[component]
fn PickedSheet(picked: DrivePicked, editor: String) -> Element {
    let s = use_context::<AppState>();
    let slot = use_context::<ComposeSlot>();
    let already = slot.draft.peek().as_ref().map(|d| attached_bytes(&d.attachments)).unwrap_or(0);
    let defaults = default_choices(&picked.files, already);
    let mut choices = use_signal(|| defaults.iter().map(|c| c.0).collect::<Vec<_>>());
    let reasons: Vec<Option<LinkReason>> = defaults.iter().map(|c| c.1).collect();
    let mut share = use_signal(|| false);
    let current = choices.read().clone();
    let total = already + chosen_bytes(&picked.files, &current);
    let any_link = current.contains(&DriveChoice::Link);
    let over = total > MAX_MESSAGE_BYTES;
    let n = picked.files.len();
    let title = if n == 1 { "1 file from Google Drive".to_string() } else { format!("{n} files from Google Drive") };
    let mut picked_state = s.drive_picked;

    let add = {
        let picked = picked.clone();
        move |_| {
            let choices = choices.read().clone();
            let mut draft = slot.draft;
            let mut links: Vec<&DriveFile> = Vec::new();
            if let Some(d) = draft.write().as_mut() {
                for (f, c) in picked.files.iter().zip(&choices) {
                    match drive_attachment(&picked.account_id, f, *c) {
                        Some(a) if !d.attachments.contains(&a) => d.attachments.push(a),
                        Some(_) => {}
                        None if *c == DriveChoice::Link => links.push(f),
                        None => {}
                    }
                }
            }
            if !links.is_empty() {
                let html = links_html(&links);
                if html.is_empty() {
                    s.local_toast("Google Drive didn't give a link for that file", true);
                } else {
                    crate::bridge::insert_html(&editor, &html);
                }
                if *share.read() {
                    let file_ids = links.iter().map(|f| f.id.clone()).collect();
                    send_cmd(Cmd::DriveShare { account_id: picked.account_id.clone(), file_ids });
                }
            }
            picked_state.set(None);
        }
    };

    rsx! {
        div { class: "drive-sheet popover", role: "dialog", "aria-label": "{title}",
            div { class: "drive-sheet__head",
                Icon { name: "cloud", class: "icon--sm" }
                span { class: "drive-sheet__title", "{title}" }
            }
            div { class: "drive-sheet__list",
                for (i, f) in picked.files.iter().cloned().enumerate() {
                    {
                        let (kind, icon) = file_kind(&f.mime_type, &f.name);
                        let opts = options(&f);
                        let chosen = current.get(i).copied().unwrap_or(DriveChoice::Link);
                        let note = match (reasons.get(i).copied().flatten(), chosen) {
                            (Some(LinkReason::TooBig), DriveChoice::Link) => Some("Too big to attach, so it goes in as a link"),
                            _ if chosen == DriveChoice::Link && f.url.is_none() => Some("No link available for this file"),
                            _ => None,
                        };
                        let size = size_label(&f);
                        rsx! {
                            div { class: "drive-row", key: "{f.id}",
                                span { class: "attachment__icon", "data-kind": "{kind}", Icon { name: icon, class: "icon--sm" } }
                                div { class: "drive-row__meta",
                                    div { class: "attachment__name", dir: "auto", title: "{f.name}", "{f.name}" }
                                    div { class: "attachment__size",
                                        "{size}"
                                        if let Some(n) = note { span { class: "drive-row__note", " · {n}" } }
                                    }
                                }
                                div { class: "segmented segmented--sm", role: "radiogroup", "aria-label": "Attach or link",
                                    for c in opts {
                                        button { r#type: "button", role: "radio", "aria-checked": "{chosen == c}",
                                            onclick: move |_| { if let Some(x) = choices.write().get_mut(i) { *x = c; } },
                                            "{choice_label(&f, c)}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if over {
                div { class: "drive-sheet__warn", role: "alert",
                    Icon { name: "alert", class: "icon--sm" }
                    "Attachments add up to more than 25 MB; many servers refuse that. Link the big ones instead."
                }
            }
            div { class: "drive-sheet__foot",
                if any_link {
                    label { class: "drive-sheet__share",
                        input { class: "check", r#type: "checkbox", checked: *share.read(),
                            onchange: move |e| share.set(e.checked()) }
                        "Let anyone with the link view"
                    }
                }
                span { class: "spacer" }
                button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: move |_| picked_state.set(None), "Cancel" }
                button { class: "btn btn--primary btn--sm", r#type: "button", onclick: add, "Add" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(mime: &str, size: Option<u64>) -> DriveFile {
        DriveFile { id: "x".into(), name: "x".into(), mime_type: mime.into(), size_bytes: size, url: None }
    }

    #[test]
    fn local_files_are_added_once() {
        let mut atts = Vec::new();
        add_local_files(&mut atts, ["/tmp/a/Report.pdf".into(), "/tmp/a/notes".into(), "".into()]);
        add_local_files(&mut atts, ["/tmp/a/Report.pdf".into(), "/tmp/b/Report.pdf".into()]);
        let got: Vec<_> = atts
            .iter()
            .map(|a| match &a.source {
                AttachmentSource::File { path } => (a.name.as_str(), path.as_str()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            got,
            [("Report.pdf", "/tmp/a/Report.pdf"), ("notes", "/tmp/a/notes"), ("Report.pdf", "/tmp/b/Report.pdf")]
        );
    }

    #[test]
    fn options_per_file_type() {
        let doc = f("application/vnd.google-apps.document", None);
        assert_eq!(options(&doc), [DriveChoice::Export(0), DriveChoice::Export(1), DriveChoice::Link]);
        assert_eq!(
            options(&doc).into_iter().map(|c| choice_label(&doc, c)).collect::<Vec<_>>(),
            ["PDF", "Word", "Link"]
        );
        let form = f("application/vnd.google-apps.form", None);
        assert_eq!(options(&form), [DriveChoice::Link], "nothing to export: link only");
        let pdf = f("application/pdf", Some(10));
        assert_eq!(options(&pdf), [DriveChoice::Attach, DriveChoice::Link]);
    }

    #[test]
    fn sizes_and_kinds() {
        assert_eq!(size_label(&f("application/vnd.google-apps.spreadsheet", None)), "Google Sheets");
        assert_eq!(size_label(&f("application/vnd.google-apps.form", None)), "Google file");
        assert_eq!(size_label(&f("application/pdf", Some(2048))), "2 KB");
        assert_eq!(file_kind("application/vnd.google-apps.spreadsheet", "Budget"), ("sheet", "sheet"));
        assert_eq!(file_kind("application/octet-stream", "a.PDF"), ("pdf", "pdf"));
        assert_eq!(file_kind("image/png", "p.png"), ("file", "image"));
        let drive = DraftAttachment {
            name: "a".into(),
            content_type: None,
            source: AttachmentSource::Drive {
                account_id: "g".into(),
                file_id: "f".into(),
                export_mime: None,
                size: Some(500),
            },
        };
        assert_eq!(attached_bytes(&[drive.clone(), drive]), 1000);
    }
}
