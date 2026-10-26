//! Who handles mail for an address: the "Add account" sheet asks for an
//! address and suggests the sign-in. Known consumer domains are answered
//! from the address alone; anything else from its domain's MX hosts (the
//! engine does the DNS lookup and hands the records here).

use crate::model::ProviderKind;

/// Where an address's mail lives, as far as Coo can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailHost {
    Gmail,
    GoogleWorkspace,
    OutlookCom,
    Microsoft365,
    ICloud,
    /// A custom domain on iCloud+ (MX at mail.icloud.com).
    ICloudCustomDomain,
    Unknown,
}

impl MailHost {
    /// The sign-in that fits (None when Coo couldn't tell).
    pub fn provider(self) -> Option<ProviderKind> {
        match self {
            MailHost::Gmail | MailHost::GoogleWorkspace => Some(ProviderKind::Google),
            MailHost::OutlookCom | MailHost::Microsoft365 => Some(ProviderKind::Microsoft),
            MailHost::ICloud | MailHost::ICloudCustomDomain => Some(ProviderKind::ICloud),
            MailHost::Unknown => None,
        }
    }

    /// The product name shown on the "detected" card.
    pub fn name(self) -> &'static str {
        match self {
            MailHost::Gmail => "Gmail",
            MailHost::GoogleWorkspace => "Google Workspace",
            MailHost::OutlookCom => "Outlook.com",
            MailHost::Microsoft365 => "Microsoft 365",
            MailHost::ICloud => "iCloud Mail",
            MailHost::ICloudCustomDomain => "iCloud+ custom domain",
            MailHost::Unknown => "Unknown",
        }
    }

    /// A free consumer mailbox at Google or Microsoft: its address may well
    /// be someone's Apple ID too, so the sheet offers iCloud as well.
    pub fn may_be_apple_id(self) -> bool {
        matches!(self, MailHost::Gmail | MailHost::OutlookCom)
    }
}

/// The company behind a sign-in, as the sheet says it ("Continue with Google").
pub fn provider_label(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::Google => "Google",
        ProviderKind::Microsoft => "Microsoft",
        ProviderKind::ICloud => "iCloud",
    }
}

/// What detection found for one address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// The address's domain, lowercased.
    pub domain: String,
    pub host: MailHost,
}

/// The lowercased domain of a full `name@domain.tld` address.
pub fn domain_of(email: &str) -> Option<String> {
    let email = email.trim();
    if !crate::compose::is_plain_address(email) {
        return None;
    }
    let (_, domain) = email.rsplit_once('@')?;
    Some(domain.trim_end_matches('.').to_ascii_lowercase())
}

/// `name.<tld>` or `name.co.<cc>` / `name.com.<cc>` (hotmail.co.uk, live.fr),
/// not a subdomain someone else owns (live.acme.com).
fn is_brand_domain(domain: &str, brand: &str) -> bool {
    let Some(rest) = domain.strip_prefix(brand).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    let labels: Vec<&str> = rest.split('.').collect();
    match labels[..] {
        [tld] => !tld.is_empty(),
        [second, cc] => matches!(second, "co" | "com") && cc.len() == 2,
        _ => false,
    }
}

/// Consumer domains Coo knows without asking DNS.
pub fn by_domain(domain: &str) -> Option<MailHost> {
    let d = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    Some(match d.as_str() {
        "gmail.com" | "googlemail.com" => MailHost::Gmail,
        "icloud.com" | "me.com" | "mac.com" => MailHost::ICloud,
        "msn.com" | "outlook.com" => MailHost::OutlookCom,
        _ if ["hotmail", "live", "outlook"].iter().any(|b| is_brand_domain(&d, b)) => MailHost::OutlookCom,
        _ => return None,
    })
}

/// One MX host name (trailing dot and case ignored).
fn by_mx_host(host: &str) -> MailHost {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let under = |suffix: &str| h == suffix || h.ends_with(&format!(".{suffix}"));
    // aspmx.l.google.com, smtp.google.com, aspmx.googlemail.com …
    if under("google.com") || under("googlemail.com") {
        MailHost::GoogleWorkspace
    } else if under("outlook.com") || under("mx.microsoft") {
        // *.mail.protection.outlook.com, and Exchange Online's newer
        // DNSSEC hosts under mx.microsoft.
        MailHost::Microsoft365
    } else if under("mail.icloud.com") {
        MailHost::ICloudCustomDomain
    } else {
        MailHost::Unknown
    }
}

/// Classify a domain by its MX records, `(preference, host)`: the most
/// preferred host decides (a filtering service in front means Unknown).
pub fn by_mx<S: AsRef<str>>(records: &[(u16, S)]) -> MailHost {
    records.iter().min_by_key(|(pref, _)| *pref).map(|(_, h)| by_mx_host(h.as_ref())).unwrap_or(MailHost::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_of_a_full_address_only() {
        assert_eq!(domain_of(" Dana@Acme.co.IL ").as_deref(), Some("acme.co.il"));
        assert_eq!(domain_of("dana@acme"), None);
        assert_eq!(domain_of("dana"), None);
        assert_eq!(domain_of("@acme.com"), None);
    }

    #[test]
    fn consumer_domains() {
        assert_eq!(by_domain("gmail.com"), Some(MailHost::Gmail));
        assert_eq!(by_domain("GoogleMail.com"), Some(MailHost::Gmail));
        assert_eq!(by_domain("outlook.com"), Some(MailHost::OutlookCom));
        assert_eq!(by_domain("hotmail.com"), Some(MailHost::OutlookCom));
        assert_eq!(by_domain("hotmail.co.uk"), Some(MailHost::OutlookCom));
        assert_eq!(by_domain("live.fr"), Some(MailHost::OutlookCom));
        assert_eq!(by_domain("msn.com"), Some(MailHost::OutlookCom));
        assert_eq!(by_domain("icloud.com"), Some(MailHost::ICloud));
        assert_eq!(by_domain("me.com"), Some(MailHost::ICloud));
        assert_eq!(by_domain("mac.com"), Some(MailHost::ICloud));
        assert_eq!(by_domain("live.acme.com"), None, "someone else's subdomain");
        assert_eq!(by_domain("hotmail.example.org.il"), None);
        assert_eq!(by_domain("acme.co.il"), None);
    }

    #[test]
    fn mx_hosts() {
        assert_eq!(by_mx(&[(1, "aspmx.l.google.com."), (5, "alt1.aspmx.l.google.com.")]), MailHost::GoogleWorkspace);
        assert_eq!(by_mx(&[(1, "smtp.google.com")]), MailHost::GoogleWorkspace);
        assert_eq!(by_mx(&[(10, "aspmx.googlemail.com")]), MailHost::GoogleWorkspace);
        assert_eq!(by_mx(&[(0, "acme-co-il.mail.protection.outlook.com.")]), MailHost::Microsoft365);
        assert_eq!(by_mx(&[(0, "acme-com.o-v1.mx.microsoft.")]), MailHost::Microsoft365);
        assert_eq!(
            by_mx(&[(10, "mx01.mail.icloud.com."), (10, "mx02.mail.icloud.com.")]),
            MailHost::ICloudCustomDomain
        );
        assert_eq!(by_mx(&[(10, "mx1.fastmail.com")]), MailHost::Unknown);
        assert_eq!(by_mx::<&str>(&[]), MailHost::Unknown);
        // Not fooled by look-alikes.
        assert_eq!(by_mx(&[(10, "google.com.evil.example")]), MailHost::Unknown);
        assert_eq!(by_mx(&[(10, "notoutlook.com")]), MailHost::Unknown);
    }

    #[test]
    fn most_preferred_mx_decides() {
        let filtered = [(20, "aspmx.l.google.com"), (10, "mx.mimecast.com")];
        assert_eq!(by_mx(&filtered), MailHost::Unknown);
        let backup = [(20, "backup.example.net"), (10, "aspmx.l.google.com")];
        assert_eq!(by_mx(&backup), MailHost::GoogleWorkspace);
    }

    #[test]
    fn hosts_map_to_sign_ins() {
        assert_eq!(MailHost::GoogleWorkspace.provider(), Some(ProviderKind::Google));
        assert_eq!(MailHost::OutlookCom.provider(), Some(ProviderKind::Microsoft));
        assert_eq!(MailHost::ICloudCustomDomain.provider(), Some(ProviderKind::ICloud));
        assert_eq!(MailHost::Unknown.provider(), None);
        assert!(MailHost::Gmail.may_be_apple_id() && !MailHost::GoogleWorkspace.may_be_apple_id());
    }
}
