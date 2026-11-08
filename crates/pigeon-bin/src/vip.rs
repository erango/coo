//! Who may notify about new mail. Each account notifies about all its mail,
//! only mail from VIPs, or nothing. Senders (an address, or `@domain` for
//! everyone there) can be VIPs or muted, the same for every account; a
//! person's own setting wins over their domain's. Muting only stops alerts:
//! the mail still arrives and counts as unread.

use dioxus::prelude::*;

use crate::prefs::Prefs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountMode {
    All,
    VipOnly,
    Off,
}

impl AccountMode {
    pub const ALL: [AccountMode; 3] = [AccountMode::All, AccountMode::VipOnly, AccountMode::Off];

    pub fn label(self) -> &'static str {
        match self {
            AccountMode::All => "All mail",
            AccountMode::VipOnly => "Only VIPs",
            AccountMode::Off => "Off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Vip,
    Muted,
}

/// Bumped on every change, so views showing VIP and muted marks redraw.
pub static REV: GlobalSignal<u64> = Signal::global(|| 0);

pub fn account_mode(p: &Prefs, account_id: &str) -> AccountMode {
    if p.notify_accounts_muted.iter().any(|a| a == account_id) {
        AccountMode::Off
    } else if p.notify_accounts_vip_only.iter().any(|a| a == account_id) {
        AccountMode::VipOnly
    } else {
        AccountMode::All
    }
}

pub fn set_account_mode(p: &mut Prefs, account_id: &str, mode: AccountMode) {
    p.notify_accounts_muted.retain(|a| a != account_id);
    p.notify_accounts_vip_only.retain(|a| a != account_id);
    match mode {
        AccountMode::All => {}
        AccountMode::VipOnly => p.notify_accounts_vip_only.push(account_id.to_string()),
        AccountMode::Off => p.notify_accounts_muted.push(account_id.to_string()),
    }
}

/// How a typed address or domain is stored: lowercased, a bare domain
/// (`linkedin.com`) as `@linkedin.com`. None when it's neither.
pub fn key(input: &str) -> Option<String> {
    let k = input.trim().trim_matches(|c| c == '<' || c == '>').to_lowercase();
    let ok_domain =
        |d: &str| d.contains('.') && !d.starts_with('.') && !d.ends_with('.') && !d.contains(char::is_whitespace);
    if let Some(d) = k.strip_prefix('@') {
        return ok_domain(d).then(|| format!("@{d}"));
    }
    match k.split_once('@') {
        Some((local, d)) => (!local.is_empty() && ok_domain(d) && !local.contains(char::is_whitespace)).then_some(k),
        None => ok_domain(&k).then(|| format!("@{k}")),
    }
}

/// The rule stored for exactly this key (an address or `@domain`).
pub fn rule_of(p: &Prefs, key: &str) -> Option<Rule> {
    if p.notify_vips.iter().any(|k| k == key) {
        Some(Rule::Vip)
    } else if p.notify_muted_senders.iter().any(|k| k == key) {
        Some(Rule::Muted)
    } else {
        None
    }
}

/// The rule that applies to mail from `email`: its own, else its domain's
/// (or a parent domain's: `@linkedin.com` covers `e.linkedin.com`).
pub fn rule_for(p: &Prefs, email: &str) -> Option<Rule> {
    let email = email.trim().to_lowercase();
    if let Some(r) = rule_of(p, &email) {
        return Some(r);
    }
    let mut domain = email.rsplit_once('@')?.1;
    loop {
        if let Some(r) = rule_of(p, &format!("@{domain}")) {
            return Some(r);
        }
        match domain.split_once('.') {
            Some((_, rest)) if rest.contains('.') => domain = rest,
            _ => return None,
        }
    }
}

pub fn set_rule(p: &mut Prefs, key: &str, rule: Option<Rule>) {
    p.notify_vips.retain(|k| k != key);
    p.notify_muted_senders.retain(|k| k != key);
    match rule {
        Some(Rule::Vip) => p.notify_vips.push(key.to_string()),
        Some(Rule::Muted) => p.notify_muted_senders.push(key.to_string()),
        None => {}
    }
}

/// Change prefs through `f` and redraw the marks.
pub fn update(f: impl FnOnce(&mut Prefs)) {
    crate::prefs::update(f);
    *REV.write() += 1;
}

/// The rule for mail from `email` under the current settings, for marks in
/// the list. Reading it subscribes the view to changes.
pub fn mark(email: &str) -> Option<Rule> {
    let _ = REV.read();
    crate::prefs::with(|p| rule_for(p, email))
}

/// Whether new mail in `account_id` from `from` alerts: None stays quiet,
/// Some(true) is a VIP's.
pub fn alerts(p: &Prefs, account_id: &str, from: Option<&str>) -> Option<bool> {
    let rule = from.and_then(|f| rule_for(p, f));
    match (account_mode(p, account_id), rule) {
        (AccountMode::Off, _) | (_, Some(Rule::Muted)) => None,
        (_, Some(Rule::Vip)) => Some(true),
        (AccountMode::VipOnly, _) => None,
        (AccountMode::All, None) => Some(false),
    }
}

/// The domain part of an address, for "Mute everyone at …".
pub fn domain_of(email: &str) -> Option<String> {
    let d = email.trim().rsplit_once('@')?.1.to_lowercase();
    d.contains('.').then_some(d)
}

/// Domains of big mail providers: muting "everyone at gmail.com" is never
/// what's meant, so the menus don't offer it.
pub fn is_shared_domain(domain: &str) -> bool {
    const SHARED: &[&str] = &[
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
        "walla.co.il",
        "gmx.com",
    ];
    SHARED.contains(&domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_addresses_or_domains() {
        assert_eq!(key(" Dana@Halyard.co ").as_deref(), Some("dana@halyard.co"));
        assert_eq!(key("linkedin.com").as_deref(), Some("@linkedin.com"));
        assert_eq!(key("@LinkedIn.com").as_deref(), Some("@linkedin.com"));
        assert_eq!(key("<a@b.io>").as_deref(), Some("a@b.io"));
        for bad in ["", "dana", "@", "a@b", "a b@c.io", "@.com"] {
            assert_eq!(key(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_persons_rule_wins_over_their_domain() {
        let mut p = Prefs::default();
        set_rule(&mut p, "@linkedin.com", Some(Rule::Muted));
        set_rule(&mut p, "boss@linkedin.com", Some(Rule::Vip));
        assert_eq!(rule_for(&p, "Boss@LinkedIn.com"), Some(Rule::Vip));
        assert_eq!(rule_for(&p, "news@linkedin.com"), Some(Rule::Muted));
        assert_eq!(rule_for(&p, "x@e.linkedin.com"), Some(Rule::Muted), "subdomains too");
        assert_eq!(rule_for(&p, "x@notlinkedin.com"), None);
        // Setting again moves it, never in both lists.
        set_rule(&mut p, "@linkedin.com", Some(Rule::Vip));
        assert_eq!(p.notify_muted_senders, Vec::<String>::new());
    }

    #[test]
    fn accounts_and_senders_decide_together() {
        let mut p = Prefs::default();
        set_rule(&mut p, "vip@x.io", Some(Rule::Vip));
        set_rule(&mut p, "@spam.io", Some(Rule::Muted));
        assert_eq!(alerts(&p, "a", Some("ann@y.io")), Some(false));
        assert_eq!(alerts(&p, "a", Some("vip@x.io")), Some(true));
        assert_eq!(alerts(&p, "a", Some("bob@spam.io")), None);
        assert_eq!(alerts(&p, "a", None), Some(false));
        set_account_mode(&mut p, "a", AccountMode::VipOnly);
        assert_eq!(account_mode(&p, "a"), AccountMode::VipOnly);
        assert_eq!(alerts(&p, "a", Some("ann@y.io")), None);
        assert_eq!(alerts(&p, "a", Some("vip@x.io")), Some(true));
        set_account_mode(&mut p, "a", AccountMode::Off);
        assert_eq!(alerts(&p, "a", Some("vip@x.io")), None, "Off means off");
        assert_eq!(p.notify_accounts_vip_only, Vec::<String>::new());
        set_account_mode(&mut p, "a", AccountMode::All);
        assert_eq!(account_mode(&p, "a"), AccountMode::All);
    }
}
