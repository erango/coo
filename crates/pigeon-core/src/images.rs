//! Saving an image out of a message (right-click › Save Image…).

use base64::Engine as _;

/// The bytes and media type of an inline `data:` image.
pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.split(';').next().filter(|m| !m.is_empty()).unwrap_or("application/octet-stream");
    let bytes = if meta.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?
    } else {
        percent_decode(data)
    };
    Some((mime.to_string(), bytes))
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// A file extension for an image media type.
pub fn image_ext(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "image/heic" => "heic",
        "image/bmp" => "bmp",
        _ => "png",
    }
}

/// The name offered in the save dialog: the URL's last path segment when it
/// looks like a file name, else "image" with an extension for its type.
pub fn image_file_name(url: &str) -> String {
    if let Some((mime, _)) = url.starts_with("data:").then(|| decode_data_url(url)).flatten() {
        return format!("image.{}", image_ext(&mime));
    }
    let name = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_string)))
        .map(|n| percent_decode(&n))
        .and_then(|b| String::from_utf8(b).ok())
        .map(|n| n.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0')).collect::<String>())
        .unwrap_or_default();
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && (1..=5).contains(&ext.len()) => name,
        _ if !name.is_empty() && name.len() <= 80 => format!("{name}.png"),
        _ => "image.png".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_inline_images() {
        assert_eq!(image_file_name("https://x.com/a/b/logo.png?w=2"), "logo.png");
        assert_eq!(image_file_name("https://x.com/a/My%20Pic.jpeg"), "My Pic.jpeg");
        assert_eq!(image_file_name("https://x.com/track"), "track.png");
        assert_eq!(image_file_name("https://x.com/"), "image.png");
        assert_eq!(image_file_name("data:image/jpeg;base64,/9j/"), "image.jpg");
        let (mime, bytes) = decode_data_url("data:image/png;base64,iVBORw==").unwrap();
        assert_eq!((mime.as_str(), &bytes[..4]), ("image/png", &[0x89, b'P', b'N', b'G'][..]));
        let (_, svg) = decode_data_url("data:image/svg+xml,%3Csvg%3E").unwrap();
        assert_eq!(svg, b"<svg>");
        assert!(decode_data_url("https://x.com/a.png").is_none());
    }
}
