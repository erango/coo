//! Sender photos: a small on-disk cache keyed by a hash of the address, and
//! the Gravatar lookup (opt-in: it sends a hash of the address to Gravatar).
//! When Gravatar has none, a company sender gets its brand's logo (BIMI: the
//! logo a domain publishes in DNS for mail, what Gmail shows for verified
//! brands). Provider sources (Google People, Microsoft Graph user photos)
//! need extra OAuth scopes and plug into the same cache when enabled.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::http::client;
use crate::{Error, Result};

/// How long a fetched photo (or a "no photo") is trusted.
pub const TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// Avatars render at 40 px at most; 96 covers 2x.
const SIZE: u32 = 96;

/// Lowercase hex SHA-256 of the trimmed, lowercased address (Gravatar's key,
/// and the cache file name, so addresses never appear on disk).
pub fn address_hash(email: &str) -> String {
    let digest = Sha256::digest(email.trim().to_lowercase().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn gravatar_url(email: &str) -> String {
    format!("https://gravatar.com/avatar/{}?s={SIZE}&d=404", address_hash(email))
}

fn photo_path(dir: &Path, email: &str) -> PathBuf {
    dir.join(format!("{}.img", address_hash(email)))
}

fn none_path(dir: &Path, email: &str) -> PathBuf {
    dir.join(format!("{}.none", address_hash(email)))
}

fn fresh(p: &Path, now: SystemTime) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        // Written after `now` (clock skew, same instant) counts as fresh.
        .is_ok_and(|t| now.duration_since(t).map_or(true, |age| age < TTL))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cached {
    Photo(Vec<u8>),
    /// Looked up recently and there was none.
    None,
    /// Not cached (or expired): fetch.
    Miss,
}

pub fn cached(dir: &Path, email: &str, now: SystemTime) -> Cached {
    let p = photo_path(dir, email);
    if fresh(&p, now)
        && let Ok(bytes) = std::fs::read(&p)
    {
        return Cached::Photo(bytes);
    }
    if fresh(&none_path(dir, email), now) { Cached::None } else { Cached::Miss }
}

pub fn store(dir: &Path, email: &str, photo: Option<&[u8]>) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    match photo {
        Some(bytes) => {
            std::fs::write(photo_path(dir, email), bytes)?;
            let _ = std::fs::remove_file(none_path(dir, email));
        }
        None => std::fs::write(none_path(dir, email), b"")?,
    }
    Ok(())
}

/// `data:` URI for an image (the webview can't load files from the cache).
pub fn data_uri(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(256)];
    let svg = head.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"));
    let mime = if svg {
        "image/svg+xml"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Gravatar's photo for an address, or None when it has none.
pub async fn gravatar(email: &str) -> Result<Option<Vec<u8>>> {
    let resp = client().get(gravatar_url(email)).send().await?;
    match resp.status().as_u16() {
        200 => Ok(Some(resp.bytes().await?.to_vec())),
        404 => Ok(None),
        s => Err(Error::Other(format!("gravatar: HTTP {s}"))),
    }
}

/// Mail domains whose senders are people, not one brand: no BIMI lookup.
const FREE_MAIL: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "msn.com",
    "yahoo.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "aol.com",
    "proton.me",
    "protonmail.com",
    "gmx.com",
    "gmx.net",
    "yandex.com",
    "mail.ru",
    "walla.co.il",
    "walla.com",
    "zoho.com",
    "fastmail.com",
    "hey.com",
];

/// The domains to try for `email`'s BIMI record: its own, then its
/// organizational domain (mail.shop.example.com → example.com; a.co.il keeps
/// three labels). None for free-mail senders.
pub fn bimi_domains(email: &str) -> Vec<String> {
    let Some((_, domain)) = email.trim().to_lowercase().rsplit_once('@').map(|(a, d)| (a.to_string(), d.to_string()))
    else {
        return Vec::new();
    };
    if domain.is_empty() || FREE_MAIL.contains(&domain.as_str()) {
        return Vec::new();
    }
    let labels: Vec<&str> = domain.split('.').collect();
    let keep = match labels.as_slice() {
        [.., second, tld] if tld.len() == 2 && matches!(*second, "co" | "com" | "org" | "net" | "ac" | "gov") => 3,
        _ => 2,
    };
    let mut out = vec![domain.clone()];
    if labels.len() > keep {
        out.push(labels[labels.len() - keep..].join("."));
    }
    out
}

/// The logo URL in a BIMI TXT record ("v=BIMI1; l=https://…svg; a=…").
pub fn bimi_logo_url(record: &str) -> Option<String> {
    let record = record.replace('"', "");
    if !record.trim_start().starts_with("v=BIMI1") {
        return None;
    }
    record
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("l="))
        .map(str::trim)
        .find(|u| u.starts_with("https://"))
        .map(String::from)
}

/// A brand logo for a company sender (BIMI), or None.
pub async fn bimi(email: &str) -> Result<Option<Vec<u8>>> {
    #[derive(serde::Deserialize)]
    struct Answer {
        data: String,
    }
    #[derive(serde::Deserialize)]
    struct Doh {
        #[serde(rename = "Answer", default)]
        answer: Vec<Answer>,
    }
    for domain in bimi_domains(email) {
        let resp: Doh = client()
            .get("https://cloudflare-dns.com/dns-query")
            .query(&[("name", format!("default._bimi.{domain}")), ("type", "TXT".into())])
            .header("accept", "application/dns-json")
            .send()
            .await?
            .json()
            .await?;
        let Some(url) = resp.answer.iter().find_map(|a| bimi_logo_url(&a.data)) else { continue };
        let logo = client().get(&url).send().await?;
        if !logo.status().is_success() {
            return Ok(None);
        }
        let bytes = logo.bytes().await?;
        // A logo is a small SVG; anything big isn't one.
        return Ok((bytes.len() < 128 * 1024).then(|| bytes.to_vec()));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bimi_lookups() {
        assert_eq!(bimi_domains("news@mail.shop.example.com"), ["mail.shop.example.com", "example.com"]);
        assert_eq!(bimi_domains("a@bank.co.il"), ["bank.co.il"]);
        assert_eq!(bimi_domains("a@mail.bank.co.il"), ["mail.bank.co.il", "bank.co.il"]);
        assert!(bimi_domains("dana@gmail.com").is_empty(), "people's mail: no brand logo");
        assert_eq!(
            bimi_logo_url("\"v=BIMI1; l=https://ex.com/logo.svg; a=https://ex.com/vmc.pem\"").as_deref(),
            Some("https://ex.com/logo.svg")
        );
        assert_eq!(bimi_logo_url("v=spf1 include:x"), None);
        assert_eq!(bimi_logo_url("v=BIMI1; l=http://insecure/logo.svg"), None);
        assert!(data_uri(b"<?xml version=\"1.0\"?><svg xmlns=").starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn hashes_match_gravatar() {
        // Gravatar's documented example: "MyEmailAddress@example.com ".
        assert_eq!(
            address_hash(" MyEmailAddress@example.com "),
            "84059b07d4be67b806386c0aad8070a23f18836bbaae342275dc0a83414c32ee"
        );
        assert!(gravatar_url("a@b.c").ends_with("?s=96&d=404"));
        assert!(!gravatar_url("Dana@X.io").contains("dana"), "no address in the URL");
    }

    #[test]
    fn cache_round_trip_and_expiry() {
        let dir = std::env::temp_dir().join(format!("coo-photos-{}", crate::auth::random_token(6)));
        let now = SystemTime::now();
        assert_eq!(cached(&dir, "a@x.io", now), Cached::Miss);
        store(&dir, "a@x.io", Some(b"\x89PNG..")).unwrap();
        assert_eq!(cached(&dir, "A@x.io", now), Cached::Photo(b"\x89PNG..".to_vec()));
        store(&dir, "b@x.io", None).unwrap();
        assert_eq!(cached(&dir, "b@x.io", now), Cached::None);
        assert_eq!(cached(&dir, "a@x.io", now + TTL + Duration::from_secs(1)), Cached::Miss);
        assert!(data_uri(b"\x89PNGxx").starts_with("data:image/png;base64,"));
        assert!(data_uri(b"\xff\xd8\xff").starts_with("data:image/jpeg;base64,"));
        std::fs::remove_dir_all(dir).ok();
    }
}
