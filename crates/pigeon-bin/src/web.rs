//! Opening a link in the user's browser, exactly as written.
//!
//! macOS re-escapes a whole URL when any character in it isn't allowed
//! (`|`, `[`, `]`, a space, non-ASCII), `%` included: `%2F` then reaches the
//! site as `%252F` and the site sees a different link than the one clicked.
//! Both `/usr/bin/open` and `NSURL URLWithString:` do it. So only the
//! disallowed characters are escaped first, and macOS gets a valid URL it
//! leaves alone.

/// Open `url` in its default app (the browser, for a web link). False when
/// macOS couldn't.
pub fn open(url: &str) -> bool {
    imp::open(&escape_invalid(url))
}

/// `url` with each character a URL can't hold percent-encoded, and every
/// escape already in it kept as is.
fn escape_invalid(url: &str) -> String {
    // Square brackets are fine in the host (an IPv6 address), not after it.
    let host_end = url.find("://").map_or(0, |i| url[i + 3..].find('/').map_or(url.len(), |j| i + 3 + j));
    let bytes = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    for (i, &b) in bytes.iter().enumerate() {
        let keep = match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' | b'.' | b'_' | b'~' | b':' | b'/' | b'?' | b'#' | b'@' => true,
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' => true,
            b'[' | b']' => i < host_end,
            b'%' => bytes.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)),
            _ => false,
        };
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    pub fn open(url: &str) -> bool {
        match NSURL::URLWithString(&NSString::from_str(url)) {
            Some(u) => NSWorkspace::sharedWorkspace().openURL(&u),
            None => {
                tracing::warn!("could not read a link to open");
                false
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn open(url: &str) -> bool {
        open::that_detached(url).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::escape_invalid;

    #[test]
    fn escapes_only_what_a_url_cannot_hold() {
        let ok = "https://x.io/c?u=https%3A%2F%2Fy.io%2Fa&b=1#top";
        assert_eq!(escape_invalid(ok), ok);
        assert_eq!(escape_invalid("https://x.io/a?q=a%2Fb&r=x|y&s=[1]"), "https://x.io/a?q=a%2Fb&r=x%7Cy&s=%5B1%5D");
        assert_eq!(escape_invalid("https://x.io/a b/שלום"), "https://x.io/a%20b/%D7%A9%D7%9C%D7%95%D7%9D");
        assert_eq!(escape_invalid("https://x.io/100%"), "https://x.io/100%25");
        assert_eq!(escape_invalid("http://[::1]:8080/a"), "http://[::1]:8080/a");
        assert_eq!(
            escape_invalid("zoommtg://zoom.us/join?confno=1&pwd=a.b"),
            "zoommtg://zoom.us/join?confno=1&pwd=a.b"
        );
    }
}
