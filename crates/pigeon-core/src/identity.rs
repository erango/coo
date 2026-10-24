//! Who really sent a message: the facts behind a sender's display name and
//! the warnings worth showing, so a phishing mail can't hide behind a
//! friendly name. Pure functions; the reader's sender card renders them.

use serde::{Deserialize, Serialize};

use crate::model::Address;

/// Delivery facts from a message's headers (kept with its body).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Origin {
    /// `Reply-To`: where replies would actually go.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reply_to: Vec<Address>,
    /// `Delivered-To` / `X-Original-To`: the address it was delivered to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_to: Option<String>,
    /// SPF / DKIM / DMARC verdicts from the receiving server's
    /// `Authentication-Results`, lowercased (`pass`, `fail`, `softfail`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Auth {
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
    /// The From domain DMARC was evaluated against (`header.from=`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dmarc_from: Option<String>,
}

impl Auth {
    pub fn any_failed(&self) -> bool {
        [&self.spf, &self.dkim, &self.dmarc]
            .iter()
            .any(|v| matches!(v.as_deref(), Some("fail" | "softfail" | "permerror")))
    }
}

/// `Authentication-Results: mx.google.com; dkim=pass header.i=@x; spf=pass …;
/// dmarc=pass (p=NONE) header.from=x` → the three verdicts (first of each).
pub fn parse_auth_results(value: &str) -> Auth {
    let mut a = Auth::default();
    for part in value.split(';') {
        let part = part.trim();
        let Some((method, rest)) = part.split_once('=') else { continue };
        let verdict =
            rest.split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or_default().to_ascii_lowercase();
        if verdict.is_empty() {
            continue;
        }
        let slot = match method.trim().to_ascii_lowercase().as_str() {
            "spf" => &mut a.spf,
            "dkim" => &mut a.dkim,
            "dmarc" => &mut a.dmarc,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(verdict);
            if method.trim().eq_ignore_ascii_case("dmarc") {
                a.dmarc_from = rest
                    .split_whitespace()
                    .find_map(|t| t.strip_prefix("header.from="))
                    .map(|d| d.trim_end_matches(')').to_ascii_lowercase());
            }
        }
    }
    a
}

/// Origin from raw `(name, value)` header pairs (any provider).
pub fn origin_from_headers<'a>(headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> Origin {
    let mut o = Origin::default();
    for (name, value) in headers {
        match name.to_ascii_lowercase().as_str() {
            "reply-to" if o.reply_to.is_empty() => o.reply_to = crate::text::parse_address_list(value),
            "delivered-to" | "x-original-to" if o.delivered_to.is_none() => {
                let v = value.trim().trim_start_matches('<').trim_end_matches('>').to_string();
                if !v.is_empty() {
                    o.delivered_to = Some(v);
                }
            }
            "authentication-results" if o.auth.is_none() => {
                let a = parse_auth_results(value);
                if a != Auth::default() {
                    o.auth = Some(a);
                }
            }
            _ => {}
        }
    }
    o
}

/// The domain part of an address, lowercased.
pub fn domain_of(email: &str) -> String {
    email.rsplit_once('@').map(|(_, d)| d.trim().trim_end_matches('>').to_ascii_lowercase()).unwrap_or_default()
}

/// A domain for display: punycode (`xn--`) decoded to Unicode. Returns
/// (shown, was_punycode).
pub fn display_domain(domain: &str) -> (String, bool) {
    let puny = domain.split('.').any(|l| l.starts_with("xn--"));
    if !puny {
        return (domain.to_string(), false);
    }
    let (unicode, res) = idna::domain_to_unicode(domain);
    (if res.is_ok() { unicode } else { domain.to_string() }, true)
}

/// Well-known names phishers borrow, and domains that legitimately use them.
const BRANDS: &[(&str, &[&str])] = &[
    ("paypal", &["paypal.com", "paypal.me"]),
    ("microsoft", &["microsoft.com", "office.com", "outlook.com", "live.com", "microsoftonline.com", "office365.com"]),
    ("office 365", &["microsoft.com", "office.com", "office365.com", "microsoftonline.com"]),
    ("outlook", &["microsoft.com", "outlook.com"]),
    ("google", &["google.com", "gmail.com", "youtube.com", "googlemail.com"]),
    ("gmail", &["google.com", "gmail.com"]),
    ("apple", &["apple.com", "icloud.com", "me.com"]),
    ("icloud", &["apple.com", "icloud.com"]),
    ("amazon", &["amazon.com", "amazon.co.uk", "amazon.de", "amazonaws.com", "amazon.co.il"]),
    ("netflix", &["netflix.com"]),
    ("docusign", &["docusign.com", "docusign.net"]),
    ("dhl", &["dhl.com", "dhl.de", "dhl.co.il"]),
    ("fedex", &["fedex.com"]),
    ("ups", &["ups.com"]),
    ("linkedin", &["linkedin.com"]),
    ("facebook", &["facebook.com", "facebookmail.com", "meta.com"]),
    ("instagram", &["instagram.com", "facebookmail.com", "meta.com"]),
    ("whatsapp", &["whatsapp.com", "meta.com"]),
    ("dropbox", &["dropbox.com", "dropboxmail.com"]),
    ("adobe", &["adobe.com"]),
    ("github", &["github.com"]),
    ("stripe", &["stripe.com"]),
    ("zoom", &["zoom.us", "zoom.com"]),
];

/// The brand a display name claims, if its domain isn't one of that brand's.
pub fn brand_mismatch(display_name: &str, domain: &str) -> Option<&'static str> {
    let name = display_name.to_lowercase();
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let joined = words.join(" ");
    BRANDS.iter().find_map(|(brand, domains)| {
        let named = if brand.contains(' ') { joined.contains(brand) } else { words.contains(brand) };
        let legit = domains.iter().any(|d| domain == *d || domain.ends_with(&format!(".{d}")));
        (named && !legit).then_some(*brand)
    })
}

/// An email address written inside a display name ("support@bank.com"
/// <evil@x.io>) that isn't the real one.
pub fn name_shows_other_address(from: &Address) -> Option<String> {
    let name = from.name.as_deref()?;
    let real = from.email.to_ascii_lowercase();
    name.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '"' | '\'' | ',' | ';'))
        .filter(|t| t.contains('@') && t.contains('.'))
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase())
        .find(|t| *t != real)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Replies would go somewhere other than the sender's address.
    ReplyToDiffers(String),
    /// The display name contains a different email address.
    NameShowsOtherAddress(String),
    /// The domain is punycode: shown here decoded.
    InternationalDomain(String),
    /// The name claims a brand the domain doesn't belong to.
    BrandMismatch(&'static str),
    /// SPF, DKIM or DMARC failed.
    AuthFailed,
    /// No earlier mail from this address in this account.
    FirstTime,
}

/// Everything suspicious about a sender, most serious first. `seen` is how
/// many messages from this address the account already has (None: unknown).
pub fn warnings(from: &Address, origin: Option<&Origin>, seen: Option<u32>) -> Vec<Warning> {
    let mut w = Vec::new();
    let domain = domain_of(&from.email);
    if let Some(a) = origin.and_then(|o| o.auth.as_ref())
        && a.any_failed()
    {
        w.push(Warning::AuthFailed);
    }
    if let Some(addr) = name_shows_other_address(from) {
        w.push(Warning::NameShowsOtherAddress(addr));
    }
    if let Some(brand) = from.name.as_deref().and_then(|n| brand_mismatch(n, &domain)) {
        w.push(Warning::BrandMismatch(brand));
    }
    if let Some(o) = origin
        && let Some(r) = o.reply_to.iter().find(|r| !r.email.eq_ignore_ascii_case(&from.email))
    {
        w.push(Warning::ReplyToDiffers(r.email.clone()));
    }
    let (shown, puny) = display_domain(&domain);
    if puny {
        w.push(Warning::InternationalDomain(shown));
    }
    if seen == Some(0) {
        w.push(Warning::FirstTime);
    }
    w
}

/// Whether the sender can be shown as verified: the receiving server checked
/// SPF, DKIM and DMARC and they all passed (DMARC pass means the From domain
/// itself is authenticated), DMARC looked at this very From domain, and
/// nothing else about the sender looks off. This proves the mail came from
/// the domain shown, not who owns that domain.
pub fn is_verified(from: &Address, origin: Option<&Origin>, warnings: &[Warning]) -> bool {
    let Some(a) = origin.and_then(|o| o.auth.as_ref()) else { return false };
    let pass = |v: &Option<String>| v.as_deref() == Some("pass");
    let aligned = a.dmarc_from.as_deref().is_none_or(|d| d == domain_of(&from.email));
    pass(&a.spf)
        && pass(&a.dkim)
        && pass(&a.dmarc)
        && aligned
        && warnings.iter().all(|w| matches!(w, Warning::FirstTime))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: Option<&str>, email: &str) -> Address {
        Address { name: name.map(String::from), email: email.into() }
    }

    #[test]
    fn parses_authentication_results() {
        let v = "mx.google.com; dkim=pass header.i=@x.io header.s=s1; spf=softfail (google.com: domain of transitioning) \
                 smtp.mailfrom=y@x.io; dmarc=fail (p=REJECT sp=REJECT dis=NONE) header.from=x.io";
        let r = parse_auth_results(v);
        assert_eq!(r.dkim.as_deref(), Some("pass"));
        assert_eq!(r.spf.as_deref(), Some("softfail"));
        assert_eq!(r.dmarc.as_deref(), Some("fail"));
        assert!(r.any_failed());
        assert!(!parse_auth_results("mx; dkim=pass; spf=pass; dmarc=pass").any_failed());
        assert_eq!(r.dmarc_from.as_deref(), Some("x.io"));
    }

    #[test]
    fn verified_only_when_everything_passes() {
        let from = a(Some("Shop"), "news@mail.shop.com");
        let origin = |v: &str| Origin { auth: Some(parse_auth_results(v)), ..Default::default() };
        let all = origin("mx; dkim=pass header.i=@shop.com; spf=pass; dmarc=pass (p=REJECT) header.from=mail.shop.com");
        assert!(is_verified(&from, Some(&all), &[]));
        assert!(is_verified(&from, Some(&all), &[Warning::FirstTime]), "new senders can still be verified");
        assert!(!is_verified(&from, Some(&all), &[Warning::BrandMismatch("paypal")]));
        let softfail = origin("mx; dkim=pass; spf=softfail; dmarc=pass header.from=mail.shop.com");
        assert!(!is_verified(&from, Some(&softfail), &[]));
        let other = origin("mx; dkim=pass; spf=pass; dmarc=pass header.from=evil.io");
        assert!(!is_verified(&from, Some(&other), &[]), "DMARC must be about this From domain");
        assert!(!is_verified(&from, Some(&origin("mx; dkim=pass; spf=pass")), &[]), "no DMARC, no badge");
        assert!(!is_verified(&from, None, &[]));
    }

    #[test]
    fn origin_from_raw_headers() {
        let o = origin_from_headers([
            ("Delivered-To", "me@gmail.com"),
            ("Reply-To", "\"Billing\" <billing@other.io>"),
            ("Authentication-Results", "mx.google.com; spf=pass smtp.mailfrom=x.io"),
            ("Authentication-Results", "later; spf=fail"),
        ]);
        assert_eq!(o.delivered_to.as_deref(), Some("me@gmail.com"));
        assert_eq!(o.reply_to[0].email, "billing@other.io");
        assert_eq!(o.auth.unwrap().spf.as_deref(), Some("pass"), "the first (receiving server's) result wins");
    }

    #[test]
    fn spoof_checks() {
        let fake = a(Some("PayPal Support"), "service@paypa1-secure.com");
        assert_eq!(brand_mismatch("PayPal Support", "paypa1-secure.com"), Some("paypal"));
        assert_eq!(brand_mismatch("PayPal", "mail.paypal.com"), None);
        assert_eq!(brand_mismatch("Upstairs neighbors", "x.io"), None, "whole words only");
        assert_eq!(brand_mismatch("Office 365 Admin", "x.io"), Some("office 365"));
        let shown = a(Some("support@bank.com"), "evil@x.io");
        assert_eq!(name_shows_other_address(&shown).as_deref(), Some("support@bank.com"));
        assert_eq!(name_shows_other_address(&a(Some("dana@x.io"), "dana@x.io")), None);
        let ws = warnings(&fake, None, Some(0));
        assert!(ws.contains(&Warning::BrandMismatch("paypal")) && ws.contains(&Warning::FirstTime));
        assert!(warnings(&a(Some("Dana"), "dana@x.io"), None, Some(12)).is_empty());
    }

    #[test]
    fn reply_to_and_idn() {
        let o = Origin { reply_to: vec![a(None, "elsewhere@y.io")], ..Default::default() };
        assert_eq!(warnings(&a(None, "dana@x.io"), Some(&o), None), [Warning::ReplyToDiffers("elsewhere@y.io".into())]);
        let same = Origin { reply_to: vec![a(None, "DANA@x.io")], ..Default::default() };
        assert!(warnings(&a(None, "dana@x.io"), Some(&same), None).is_empty());
        // аpple.com with a Cyrillic "а".
        let (shown, puny) = display_domain("xn--pple-43d.com");
        assert!(puny && shown == "аpple.com", "{shown}");
        assert_eq!(display_domain("apple.com"), ("apple.com".to_string(), false));
        assert!(
            warnings(&a(None, "id@xn--pple-43d.com"), None, None)
                .contains(&Warning::InternationalDomain("аpple.com".into()))
        );
    }
}
