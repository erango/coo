//! Google Drive API v3, for files the user picked (`drive.file` scope):
//! metadata, download, export of Google-native files, and "anyone with the
//! link can view" sharing. Bytes stay in memory; nothing is written to disk.

use std::sync::Arc;

use pigeon_core::auth::TokenSource;
use pigeon_core::drive::{EXPORT_LIMIT_BYTES, MAX_MESSAGE_BYTES};
use pigeon_core::http::{client, send};
use pigeon_core::{Error, Result};
use serde::Deserialize;

const DRIVE: &str = "https://www.googleapis.com/drive/v3";

pub const NEEDS_CONNECT: &str = "Coo can't open Google Drive files for this account yet. Choose “From Google Drive…” \
                                 again and allow access in your browser.";
const EXPORT_TOO_BIG: &str = "This Google file is too big to attach (Google exports files up to 10 MB). \
                              Insert it as a link instead.";
const GONE: &str = "A Google Drive file isn't available anymore (deleted, or access removed). \
                    Remove it from the message and pick it again.";

/// A file's metadata, as `files.get` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// Bytes, as a decimal string; absent for Google-native files.
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub web_view_link: Option<String>,
}

impl FileMeta {
    pub fn size_bytes(&self) -> Option<u64> {
        self.size.as_deref().and_then(|s| s.parse().ok())
    }
}

/// Why a Drive call failed, from its status and error body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The sign-in lacks the Drive scope (or it was revoked).
    NeedsConnect,
    /// Export over Google's 10 MB limit.
    ExportTooBig,
    /// Deleted, or not shared with Coo (`drive.file` only sees picked files).
    Gone,
    Other,
}

pub fn classify(status: u16, body: &str) -> Problem {
    let b = body.to_ascii_lowercase();
    match status {
        403 if b.contains("exportsizelimitexceeded") || b.contains("too large to be exported") => Problem::ExportTooBig,
        401 | 403
            if b.contains("insufficientpermissions")
                || b.contains("access_token_scope_insufficient")
                || b.contains("insufficient authentication scopes") =>
        {
            Problem::NeedsConnect
        }
        404 => Problem::Gone,
        _ => Problem::Other,
    }
}

/// Whether `e` means the account needs to grant Drive access first.
pub fn needs_connect(e: &Error) -> bool {
    match e {
        Error::Api { status, body, .. } => classify(*status, body) == Problem::NeedsConnect,
        Error::Auth(m) | Error::Other(m) => m == NEEDS_CONNECT,
        _ => false,
    }
}

/// A clear error for the user in place of the raw API one.
fn friendly(e: Error) -> Error {
    let Error::Api { status, body, .. } = &e else { return e };
    match classify(*status, body) {
        Problem::NeedsConnect => Error::Auth(NEEDS_CONNECT.into()),
        Problem::ExportTooBig => Error::Other(EXPORT_TOO_BIG.into()),
        Problem::Gone => Error::Other(GONE.into()),
        // File ids stay out of error reports (they're logged at error level).
        Problem::Other => Error::Api { status: *status, url: format!("{DRIVE}/files/…"), body: body.clone() },
    }
}

fn too_big(size: u64) -> Error {
    Error::Other(format!(
        "A Google Drive file is too big to attach ({} MB; the limit is 25 MB). Insert it as a link instead.",
        size.div_ceil(1024 * 1024)
    ))
}

/// The Drive client for one Google account's sign-in.
pub struct Drive {
    tokens: Arc<TokenSource>,
}

impl Drive {
    pub fn new(tokens: Arc<TokenSource>) -> Self {
        Drive { tokens }
    }

    async fn call(&self, make: impl Fn(&str) -> reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let token = self.tokens.access_token().await?;
        let result = send(|| make(&token)).await;
        if matches!(result, Err(ref e) if e.status() == Some(401)) {
            self.tokens.invalidate().await;
        }
        result.map_err(friendly)
    }

    pub async fn metadata(&self, file_id: &str) -> Result<FileMeta> {
        let url = format!("{DRIVE}/files/{file_id}");
        let resp = self
            .call(|t| {
                client()
                    .get(&url)
                    .bearer_auth(t)
                    .query(&[("fields", "id,name,mimeType,size,webViewLink"), ("supportsAllDrives", "true")])
            })
            .await?;
        Ok(resp.json().await?)
    }

    /// The file's bytes: downloaded as is, or exported to `export_mime`
    /// (Google Docs/Sheets/Slides/Drawings).
    pub async fn content(&self, file_id: &str, export_mime: Option<&str>) -> Result<Vec<u8>> {
        let (resp, limit) = match export_mime {
            Some(mime) => {
                let url = format!("{DRIVE}/files/{file_id}/export");
                let resp = self.call(|t| client().get(&url).bearer_auth(t).query(&[("mimeType", mime)])).await?;
                (resp, EXPORT_LIMIT_BYTES)
            }
            None => {
                let meta = self.metadata(file_id).await?;
                if let Some(size) = meta.size_bytes().filter(|s| *s > MAX_MESSAGE_BYTES) {
                    return Err(too_big(size));
                }
                let url = format!("{DRIVE}/files/{file_id}");
                let resp = self
                    .call(|t| {
                        client().get(&url).bearer_auth(t).query(&[("alt", "media"), ("supportsAllDrives", "true")])
                    })
                    .await?;
                (resp, MAX_MESSAGE_BYTES)
            }
        };
        read_capped(resp, limit, export_mime.is_some()).await
    }

    /// Let anyone with the link view the file.
    pub async fn share_with_anyone(&self, file_id: &str) -> Result<()> {
        let url = format!("{DRIVE}/files/{file_id}/permissions");
        let body = serde_json::json!({ "type": "anyone", "role": "reader" });
        self.call(|t| {
            client()
                .post(&url)
                .bearer_auth(t)
                .query(&[("supportsAllDrives", "true"), ("sendNotificationEmail", "false")])
                .json(&body)
        })
        .await?;
        Ok(())
    }
}

/// Read a response body into memory, refusing more than `limit` bytes.
async fn read_capped(mut resp: reqwest::Response, limit: u64, export: bool) -> Result<Vec<u8>> {
    let over = |n: u64| if export { Error::Other(EXPORT_TOO_BIG.into()) } else { too_big(n) };
    if let Some(n) = resp.content_length().filter(|n| *n > limit) {
        return Err(over(n));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        out.extend_from_slice(&chunk);
        if out.len() as u64 > limit {
            return Err(over(out.len() as u64));
        }
    }
    Ok(out)
}

/// The scopes an access token carries (Google's tokeninfo), for accounts
/// that signed in before Coo recorded them. The token goes in the POST
/// body, never the URL.
pub async fn granted_scopes(access_token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Info {
        #[serde(default)]
        scope: String,
    }
    let resp =
        send(|| client().post("https://oauth2.googleapis.com/tokeninfo").form(&[("access_token", access_token)]))
            .await?;
    Ok(resp.json::<Info>().await?.scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_fixture_parses() {
        let m: FileMeta = serde_json::from_str(
            r#"{"id":"1AbC","name":"Q4 plan.pdf","mimeType":"application/pdf","size":"1048576",
                "webViewLink":"https://drive.google.com/file/d/1AbC/view?usp=drivesdk"}"#,
        )
        .unwrap();
        assert_eq!(m.size_bytes(), Some(1_048_576));
        assert!(m.web_view_link.unwrap().starts_with("https://drive.google.com/"));
        let native: FileMeta =
            serde_json::from_str(r#"{"id":"2","name":"Budget","mimeType":"application/vnd.google-apps.spreadsheet"}"#)
                .unwrap();
        assert_eq!(native.size_bytes(), None);
    }

    #[test]
    fn errors_are_classified() {
        let scope = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes.",
            "errors":[{"reason":"insufficientPermissions"}],"status":"PERMISSION_DENIED",
            "details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
        assert_eq!(classify(403, scope), Problem::NeedsConnect);
        let export = r#"{"error":{"errors":[{"domain":"global","reason":"exportSizeLimitExceeded",
            "message":"This file is too large to be exported."}],"code":403}}"#;
        assert_eq!(classify(403, export), Problem::ExportTooBig);
        assert_eq!(classify(404, r#"{"error":{"code":404,"message":"File not found: x."}}"#), Problem::Gone);
        assert_eq!(classify(403, r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#), Problem::Other);
        assert_eq!(classify(500, ""), Problem::Other);

        let api = |status, body: &str| Error::Api { status, url: String::new(), body: body.into() };
        assert!(needs_connect(&api(403, scope)));
        assert!(!needs_connect(&api(403, export)));
        let e = friendly(api(403, export));
        assert!(e.to_string().contains("10 MB") && e.to_string().contains("link"), "{e}");
        assert!(needs_connect(&friendly(api(403, scope))));
        assert!(friendly(api(404, "")).to_string().contains("isn't available"));
        assert_eq!(friendly(api(500, "boom")).status(), Some(500));
        let secret = Error::Api { status: 500, url: format!("{DRIVE}/files/SECRETID/export"), body: "x".into() };
        assert!(!friendly(secret).to_string().contains("SECRETID"));
        assert!(too_big(30 * 1024 * 1024 + 1).to_string().contains("31 MB"));
    }
}
