//! Google Drive attachments: what the user picked, the formats a Google
//! file can be exported to, and the per-file "attach or link" choice. Pure;
//! the Drive API client lives in `pigeon-google`, the picker page too.

use serde::{Deserialize, Serialize};

use crate::compose::{AttachmentSource, DraftAttachment};
use crate::text::escape_html;

/// The OAuth scope that lets Coo open only the files the user picks.
pub const DRIVE_FILE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

/// Gmail's limit for a whole message; Coo keeps attachments under it.
pub const MAX_MESSAGE_BYTES: u64 = 25 * 1024 * 1024;

/// Google refuses to export files bigger than this.
pub const EXPORT_LIMIT_BYTES: u64 = 10 * 1024 * 1024;

/// A file picked in the Google Picker (as the picker page posts it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// Google-native files (Docs, Sheets…) have no size.
    #[serde(default, deserialize_with = "lenient_size")]
    pub size_bytes: Option<u64>,
    /// The file's "open in Drive" link (`webViewLink`).
    #[serde(default)]
    pub url: Option<String>,
}

/// The Picker reports sizes as numbers or strings depending on the file.
fn lenient_size<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(match Option::<serde_json::Value>::deserialize(d)? {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

impl DriveFile {
    /// A Google Docs/Sheets/Slides/Drawings (etc.) file: no bytes of its
    /// own, only exports.
    pub fn is_native(&self) -> bool {
        is_native(&self.mime_type)
    }
}

pub fn is_native(mime: &str) -> bool {
    mime.starts_with("application/vnd.google-apps.")
}

/// One way to export a Google-native file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportFormat {
    /// Button label ("PDF", "Word"…).
    pub label: &'static str,
    pub mime: &'static str,
    /// File extension, without the dot.
    pub ext: &'static str,
}

const PDF: ExportFormat = ExportFormat { label: "PDF", mime: "application/pdf", ext: "pdf" };
const DOCX: ExportFormat = ExportFormat {
    label: "Word",
    mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ext: "docx",
};
const XLSX: ExportFormat = ExportFormat {
    label: "Excel",
    mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ext: "xlsx",
};
const PPTX: ExportFormat = ExportFormat {
    label: "PowerPoint",
    mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ext: "pptx",
};
const PNG: ExportFormat = ExportFormat { label: "PNG", mime: "image/png", ext: "png" };

/// Export formats for a Google-native type, PDF first. Empty for anything
/// that can't be exported (Forms, Sites, folders, shortcuts…): link only.
pub fn export_formats(mime: &str) -> &'static [ExportFormat] {
    match mime {
        "application/vnd.google-apps.document" => &[PDF, DOCX],
        "application/vnd.google-apps.spreadsheet" => &[PDF, XLSX],
        "application/vnd.google-apps.presentation" => &[PDF, PPTX],
        "application/vnd.google-apps.drawing" => &[PDF, PNG],
        _ => &[],
    }
}

/// The export format for an export mime type (any native source).
pub fn export_format(export_mime: &str) -> Option<ExportFormat> {
    [PDF, DOCX, XLSX, PPTX, PNG].into_iter().find(|f| f.mime == export_mime)
}

/// The attachment name for an exported file: "Budget" → "Budget.xlsx".
pub fn export_name(name: &str, ext: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() { "Untitled" } else { name };
    let dotted = format!(".{ext}");
    if name.to_ascii_lowercase().ends_with(&dotted) { name.to_string() } else { format!("{name}{dotted}") }
}

/// What to do with one picked file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveChoice {
    /// Attach the file's bytes (non-native files).
    Attach,
    /// Attach an export (native files); index into `export_formats`.
    Export(usize),
    /// Insert a link into the message.
    Link,
}

/// Why a file defaults to a link even though it could be attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkReason {
    /// Attaching it would take the message over the size limit.
    TooBig,
}

/// Starting choices for picked files: regular files are attached while the
/// message stays under the limit (`already` bytes are attached), Google
/// files and anything that doesn't fit become links.
pub fn default_choices(files: &[DriveFile], already: u64) -> Vec<(DriveChoice, Option<LinkReason>)> {
    let mut total = already;
    files
        .iter()
        .map(|f| {
            if f.is_native() {
                return (DriveChoice::Link, None);
            }
            let size = f.size_bytes.unwrap_or(0);
            if total.saturating_add(size) <= MAX_MESSAGE_BYTES {
                total += size;
                (DriveChoice::Attach, None)
            } else {
                (DriveChoice::Link, Some(LinkReason::TooBig))
            }
        })
        .collect()
}

/// Bytes the chosen attachments add (exports count as unknown, 0).
pub fn chosen_bytes(files: &[DriveFile], choices: &[DriveChoice]) -> u64 {
    files.iter().zip(choices).filter(|(_, c)| **c == DriveChoice::Attach).map(|(f, _)| f.size_bytes.unwrap_or(0)).sum()
}

/// The draft attachment for a file attached (or exported) from the Google
/// account `account_id`. None for links.
pub fn drive_attachment(account_id: &str, file: &DriveFile, choice: DriveChoice) -> Option<DraftAttachment> {
    let (name, content_type, export_mime, size) = match choice {
        DriveChoice::Link => return None,
        DriveChoice::Attach if file.is_native() => return None,
        DriveChoice::Attach => (file.name.clone(), file.mime_type.clone(), None, file.size_bytes),
        DriveChoice::Export(i) => {
            let f = export_formats(&file.mime_type).get(i)?;
            (export_name(&file.name, f.ext), f.mime.to_string(), Some(f.mime.to_string()), None)
        }
    };
    Some(DraftAttachment {
        name,
        content_type: Some(content_type),
        source: AttachmentSource::Drive {
            account_id: account_id.to_string(),
            file_id: file.id.clone(),
            export_mime,
            size,
        },
    })
}

/// The link inserted for a file: its name, pointing at Drive.
pub fn link_html(file: &DriveFile) -> Option<String> {
    let url = file.url.as_deref().filter(|u| u.starts_with("https://"))?;
    Some(format!("<a href=\"{}\">{}</a>", escape_html(url), escape_html(&file.name)))
}

/// Several links, one per line.
pub fn links_html(files: &[&DriveFile]) -> String {
    files.iter().filter_map(|f| link_html(f)).collect::<Vec<_>>().join("<br>")
}

/// Whether a granted-scope string (space-separated, as the token endpoint
/// returns it) includes `scope`.
pub fn has_scope(granted: &str, scope: &str) -> bool {
    granted.split_whitespace().any(|s| s == scope)
}

/// The Google Cloud project number: the numeric prefix of an OAuth client
/// id (`123456-abc.apps.googleusercontent.com`). The Picker calls it app id.
pub fn app_id(client_id: &str) -> Option<&str> {
    let (num, _) = client_id.split_once('-')?;
    (!num.is_empty() && num.bytes().all(|b| b.is_ascii_digit())).then_some(num)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, mime: &str, size: Option<u64>) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: format!("{id} name"),
            mime_type: mime.into(),
            size_bytes: size,
            url: Some(format!("https://drive.google.com/file/d/{id}/view")),
        }
    }

    const MB: u64 = 1024 * 1024;

    #[test]
    fn picker_docs_parse_with_string_or_number_sizes() {
        let v: Vec<DriveFile> = serde_json::from_str(
            r#"[{"id":"a","name":"Budget","mimeType":"application/vnd.google-apps.spreadsheet","url":"https://docs.google.com/x"},
                {"id":"b","name":"photo.jpg","mimeType":"image/jpeg","sizeBytes":"2048","url":"https://drive.google.com/b"},
                {"id":"c","name":"c.pdf","mimeType":"application/pdf","sizeBytes":4096,"url":null}]"#,
        )
        .unwrap();
        assert_eq!(v[0].size_bytes, None);
        assert!(v[0].is_native() && !v[1].is_native());
        assert_eq!((v[1].size_bytes, v[2].size_bytes), (Some(2048), Some(4096)));
        assert_eq!(v[2].url, None);
    }

    #[test]
    fn export_choices_per_google_type() {
        let labels = |m: &str| export_formats(m).iter().map(|f| f.label).collect::<Vec<_>>();
        assert_eq!(labels("application/vnd.google-apps.document"), ["PDF", "Word"]);
        assert_eq!(labels("application/vnd.google-apps.spreadsheet"), ["PDF", "Excel"]);
        assert_eq!(labels("application/vnd.google-apps.presentation"), ["PDF", "PowerPoint"]);
        assert_eq!(labels("application/vnd.google-apps.drawing"), ["PDF", "PNG"]);
        assert!(export_formats("application/vnd.google-apps.form").is_empty());
        assert!(export_formats("application/pdf").is_empty());
        let xlsx = export_formats("application/vnd.google-apps.spreadsheet")[1];
        assert_eq!(export_format(xlsx.mime), Some(xlsx));
        assert_eq!(export_format("text/plain"), None);
    }

    #[test]
    fn export_names_get_the_extension_once() {
        assert_eq!(export_name("Budget", "pdf"), "Budget.pdf");
        assert_eq!(export_name("Budget 2026", "xlsx"), "Budget 2026.xlsx");
        assert_eq!(export_name("Deck.PPTX", "pptx"), "Deck.PPTX");
        assert_eq!(export_name("  ", "docx"), "Untitled.docx");
        assert_eq!(export_name("תקציב", "pdf"), "תקציב.pdf");
    }

    #[test]
    fn defaults_attach_small_files_and_link_the_rest() {
        let files = vec![
            file("doc", "application/vnd.google-apps.document", None),
            file("small", "application/pdf", Some(3 * MB)),
            file("big", "video/mp4", Some(30 * MB)),
            file("mid", "image/png", Some(10 * MB)),
            file("tipping", "image/png", Some(10 * MB)),
        ];
        let got = default_choices(&files, 4 * MB);
        assert_eq!(
            got,
            vec![
                (DriveChoice::Link, None),
                (DriveChoice::Attach, None),
                (DriveChoice::Link, Some(LinkReason::TooBig)),
                (DriveChoice::Attach, None),
                // 4 + 3 + 10 + 10 = 27 MB: over the limit.
                (DriveChoice::Link, Some(LinkReason::TooBig)),
            ]
        );
        let choices: Vec<DriveChoice> = got.iter().map(|c| c.0).collect();
        assert_eq!(chosen_bytes(&files, &choices), 13 * MB);
        // Unknown size counts as small.
        assert_eq!(default_choices(&[file("x", "text/plain", None)], 0)[0].0, DriveChoice::Attach);
    }

    #[test]
    fn attachments_from_choices() {
        let pdf = file("p", "application/pdf", Some(2048));
        let a = drive_attachment("google:me@x.io", &pdf, DriveChoice::Attach).unwrap();
        assert_eq!((a.name.as_str(), a.content_type.as_deref()), ("p name", Some("application/pdf")));
        assert_eq!(
            a.source,
            AttachmentSource::Drive {
                account_id: "google:me@x.io".into(),
                file_id: "p".into(),
                export_mime: None,
                size: Some(2048)
            }
        );
        assert_eq!(drive_attachment("g", &pdf, DriveChoice::Link), None);

        let mut sheet = file("s", "application/vnd.google-apps.spreadsheet", None);
        sheet.name = "Budget".into();
        let x = drive_attachment("g", &sheet, DriveChoice::Export(1)).unwrap();
        assert_eq!(x.name, "Budget.xlsx");
        let AttachmentSource::Drive { export_mime, .. } = &x.source else { panic!() };
        assert_eq!(export_mime.as_deref(), x.content_type.as_deref());
        assert_eq!(drive_attachment("g", &sheet, DriveChoice::Export(0)).unwrap().name, "Budget.pdf");
        assert_eq!(drive_attachment("g", &sheet, DriveChoice::Export(5)), None);
        assert_eq!(drive_attachment("g", &sheet, DriveChoice::Attach), None, "native files only export");
    }

    #[test]
    fn links_are_escaped_and_https_only() {
        let mut f = file("a", "application/pdf", None);
        f.name = "Q4 <plan> & \"notes\"".into();
        f.url = Some("https://drive.google.com/file/d/a/view?usp=drivesdk&x=\"y\"".into());
        assert_eq!(
            link_html(&f).unwrap(),
            "<a href=\"https://drive.google.com/file/d/a/view?usp=drivesdk&amp;x=&quot;y&quot;\">Q4 &lt;plan&gt; &amp; &quot;notes&quot;</a>"
        );
        f.url = Some("javascript:alert(1)".into());
        assert_eq!(link_html(&f), None);
        let (a, b) = (file("a", "x", None), file("b", "x", None));
        assert_eq!(links_html(&[&a, &b]).matches("<br>").count(), 1);
    }

    #[test]
    fn scopes_and_app_id() {
        let granted = "openid https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/drive.file";
        assert!(has_scope(granted, DRIVE_FILE_SCOPE));
        assert!(!has_scope("openid https://www.googleapis.com/auth/drive.file.x", DRIVE_FILE_SCOPE));
        assert_eq!(app_id("123456789012-abcdef.apps.googleusercontent.com"), Some("123456789012"));
        assert_eq!(app_id("abc-def.apps.googleusercontent.com"), None);
        assert_eq!(app_id("nodash"), None);
    }
}
