//! Google People API, for sender photos: the photo of each address in the
//! account's other contacts (people it mailed with), its own contacts, and
//! its Workspace directory. Only addresses and photo URLs are read.

use std::sync::Arc;

use pigeon_core::Result;
use pigeon_core::auth::TokenSource;
use pigeon_core::http::{client, get_json};
use serde::Deserialize;

const PEOPLE: &str = "https://people.googleapis.com/v1";
const PAGE_SIZE: &str = "1000";
/// Pages fetched per source at most (a runaway pager stops here).
const MAX_PAGES: usize = 50;
/// Avatars render at 40 px at most; 96 covers 2x.
const PHOTO_SIZE: u32 = 96;

/// (lowercased address, photo URL) pairs.
pub type Photos = Vec<(String, String)>;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    /// `otherContacts.list` and `people.listDirectoryPeople`.
    #[serde(default)]
    other_contacts: Vec<Person>,
    #[serde(default)]
    people: Vec<Person>,
    /// `people.connections.list`.
    #[serde(default)]
    connections: Vec<Person>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Person {
    #[serde(default)]
    email_addresses: Vec<Email>,
    #[serde(default)]
    photos: Vec<Photo>,
}

#[derive(Debug, Default, Deserialize)]
struct Email {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Default, Deserialize)]
struct Photo {
    #[serde(default)]
    url: String,
    /// A generated placeholder (a letter on a color), not a real photo.
    #[serde(default)]
    default: bool,
}

/// The photo URL at the size avatars use: Google's image URLs take a size
/// suffix (`=s100`, `=s100-c`…) or, in older ones, an `sz=` parameter.
pub fn sized(url: &str) -> String {
    let size = format!("s{PHOTO_SIZE}");
    if let Some((base, query)) = url.split_once('?') {
        let rest: Vec<&str> = query.split('&').filter(|p| !p.starts_with("sz=")).collect();
        let mut q = rest.join("&");
        if !q.is_empty() {
            q.push('&');
        }
        return format!("{base}?{q}sz={PHOTO_SIZE}");
    }
    // `…/photo.jpg` style paths carry no suffix; googleusercontent ids do.
    match url.rsplit_once('=') {
        Some((base, opts)) if !opts.contains('/') && opts.starts_with('s') => format!("{base}={size}"),
        _ if url.contains("googleusercontent.com") => format!("{url}={size}"),
        _ => url.to_string(),
    }
}

/// (lowercased address, photo URL) for every person on a page with a real
/// photo; each of a person's addresses gets their photo.
fn photos_of(people: &[Person]) -> Photos {
    let mut out = Vec::new();
    for p in people {
        let Some(photo) = p.photos.iter().find(|ph| !ph.default && ph.url.starts_with("https://")) else { continue };
        for e in &p.email_addresses {
            let email = e.value.trim().to_lowercase();
            if email.contains('@') {
                out.push((email, sized(&photo.url)));
            }
        }
    }
    out
}

/// A page's photos and its next page token.
fn page_photos(page: Page) -> (Photos, Option<String>) {
    let mut all = page.other_contacts;
    all.extend(page.people);
    all.extend(page.connections);
    (photos_of(&all), page.next_page_token.filter(|t| !t.is_empty()))
}

/// Which list to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// People the account mailed with (`contacts.other.readonly`).
    OtherContacts,
    /// The account's own contacts (`contacts.readonly`).
    Contacts,
    /// Its Workspace domain's people (`directory.readonly`).
    Directory,
}

impl Source {
    fn request(self) -> (String, Vec<(&'static str, &'static str)>) {
        match self {
            Source::OtherContacts => (
                format!("{PEOPLE}/otherContacts"),
                vec![("readMask", "emailAddresses,photos"), ("pageSize", PAGE_SIZE)],
            ),
            Source::Contacts => (
                format!("{PEOPLE}/people/me/connections"),
                vec![("personFields", "emailAddresses,photos"), ("pageSize", PAGE_SIZE)],
            ),
            Source::Directory => (
                format!("{PEOPLE}/people:listDirectoryPeople"),
                vec![
                    ("readMask", "emailAddresses,photos"),
                    ("sources", "DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE"),
                    ("pageSize", PAGE_SIZE),
                ],
            ),
        }
    }
}

/// The People client for one Google account's sign-in.
pub struct People {
    tokens: Arc<TokenSource>,
}

impl People {
    pub fn new(tokens: Arc<TokenSource>) -> Self {
        People { tokens }
    }

    /// Every (address, photo URL) in `source`. A personal account has no
    /// directory: that answers empty rather than failing.
    pub async fn photos(&self, source: Source) -> Result<Photos> {
        let (url, query) = source.request();
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let access = self.tokens.access_token().await?;
            let mut q: Vec<(&str, String)> = query.iter().map(|(k, v)| (*k, v.to_string())).collect();
            if let Some(t) = &token {
                q.push(("pageToken", t.clone()));
            }
            let result: Result<Page> = get_json(|| client().get(&url).bearer_auth(&access).query(&q)).await;
            let page = match result {
                Ok(v) => v,
                Err(e) if source == Source::Directory && matches!(e.status(), Some(400 | 403)) => return Ok(out),
                Err(e) => {
                    if e.status() == Some(401) {
                        self.tokens.invalidate().await;
                    }
                    return Err(e);
                }
            };
            let (photos, next) = page_photos(page);
            out.extend(photos);
            match next {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_page(json: &str) -> serde_json::Result<(Photos, Option<String>)> {
        serde_json::from_str(json).map(page_photos)
    }

    #[test]
    fn photos_skip_placeholders_and_cover_every_address() {
        let json = r#"{
          "otherContacts": [
            {"emailAddresses": [{"value": "Dana@Example.com"}, {"value": "dana.c@work.io"}],
             "photos": [{"url": "https://lh3.googleusercontent.com/cm/abc=s100", "default": false}]},
            {"emailAddresses": [{"value": "noface@example.com"}],
             "photos": [{"url": "https://lh3.googleusercontent.com/cm/letter=s100", "default": true}]},
            {"emailAddresses": [{"value": "nophoto@example.com"}]},
            {"photos": [{"url": "https://lh3.googleusercontent.com/cm/x=s100"}]}
          ],
          "nextPageToken": "p2"
        }"#;
        let (photos, next) = parse_page(json).unwrap();
        assert_eq!(
            photos,
            [
                ("dana@example.com".to_string(), "https://lh3.googleusercontent.com/cm/abc=s96".to_string()),
                ("dana.c@work.io".to_string(), "https://lh3.googleusercontent.com/cm/abc=s96".to_string()),
            ]
        );
        assert_eq!(next.as_deref(), Some("p2"));
    }

    #[test]
    fn connections_and_directory_pages_parse_and_end() {
        let (c, next) = parse_page(
            r#"{"connections": [{"emailAddresses": [{"value": "a@x.io"}], "photos": [{"url": "https://lh3.googleusercontent.com/a/b"}]}], "nextPageToken": ""}"#,
        )
        .unwrap();
        assert_eq!(c, [("a@x.io".to_string(), "https://lh3.googleusercontent.com/a/b=s96".to_string())]);
        assert_eq!(next, None);
        let (d, _) = parse_page(r#"{"people": [{"emailAddresses": [{"value": "b@x.io"}], "photos": [{"url": "https://lh3.googleusercontent.com/a-/c=s100-c"}]}]}"#).unwrap();
        assert_eq!(d[0].1, "https://lh3.googleusercontent.com/a-/c=s96");
        assert_eq!(parse_page("{}").unwrap(), (Vec::new(), None));
    }

    #[test]
    fn sizes() {
        assert_eq!(
            sized("https://lh3.googleusercontent.com/cm/abc=s100"),
            "https://lh3.googleusercontent.com/cm/abc=s96"
        );
        assert_eq!(sized("https://lh3.googleusercontent.com/cm/abc"), "https://lh3.googleusercontent.com/cm/abc=s96");
        assert_eq!(sized("https://example.com/p.jpg?sz=50&x=1"), "https://example.com/p.jpg?x=1&sz=96");
        assert_eq!(sized("https://example.com/photo.jpg"), "https://example.com/photo.jpg");
    }
}
