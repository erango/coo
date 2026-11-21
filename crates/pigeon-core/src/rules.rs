//! Mail rules: "when new mail is from …, archive it". A rule belongs to one
//! account or to all of them. When the account's server can run it (a
//! Gmail filter, an Outlook inbox rule), it's saved there and keeps working
//! with Coo closed; otherwise Coo applies it as mail lands, and mail that
//! came in while Coo was closed is handled on the next sync.
//!
//! "Don't notify me" (a muted sender) is a rule too: it only stops alerts.

use serde::{Deserialize, Serialize};

use crate::model::{MessageRow, ProviderKind};
use crate::watches::{GRACE_MS, RECENT_MS};

/// What new mail must look like. Each list matches any of its values; the
/// lists that are given must all match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct When {
    /// Senders: an address (`dana@acme.com`), a domain (`@acme.com`, which
    /// also covers its subdomains) or part of an address or name.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub from: Vec<String>,
    /// Recipients (To or Cc), matched like `from`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub to: Vec<String>,
    /// Text the subject contains, any case.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subject: Vec<String>,
    /// Words in the subject or the message.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub words: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub has_attachment: bool,
}

/// A folder (Outlook) or label (Gmail) a rule files mail into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
}

/// What a rule does with a match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Then {
    /// Skip the inbox.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub archive: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub read: bool,
    /// Gmail: add this label. Outlook and iCloud: move to this folder.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<Place>,
    /// Star (Gmail) or flag.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub star: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub trash: bool,
    /// No notification (the mail still arrives and counts as unread).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub quiet: bool,
    /// Put it in Needs you.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub needs_you: bool,
}

impl Then {
    pub fn is_empty(&self) -> bool {
        *self == Then::default()
    }

    /// Does something to the message itself (not only alerts or Needs you),
    /// so it can be done to mail that's already here.
    pub fn moves_mail(&self) -> bool {
        self.archive || self.read || self.label.is_some() || self.star || self.trash
    }

    /// Only stops notifications: a muted sender.
    pub fn only_quiet(&self) -> bool {
        *self == Then { quiet: true, ..Default::default() }
    }

    /// The first action only Coo can do for this provider, by its label.
    pub fn coo_only(&self, provider: ProviderKind) -> Option<&'static str> {
        if self.quiet {
            Some("Don’t notify me")
        } else if self.needs_you {
            Some("Send to Needs you")
        } else if self.star && provider == ProviderKind::Microsoft {
            // Outlook rules can't flag.
            Some("Flag")
        } else {
            None
        }
    }
}

/// A saved rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// One account, or None for all of them.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub account_id: Option<String>,
    pub when: When,
    pub then: Then,
    pub enabled: bool,
    /// Who made it: empty for the user, else the assistant's name.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub created_by: String,
    pub created_at: i64,
    /// The Gmail filter or Outlook rule it's saved as, when it runs there.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub server_id: Option<String>,
    /// Why the last save to the server failed (it then runs in Coo).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub problem: Option<String>,
    #[serde(default)]
    pub hits: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_hit_at: Option<i64>,
}

impl Rule {
    pub fn new(name: &str, account_id: Option<String>, when: When, then: Then, now: i64) -> Self {
        Rule {
            id: format!("r{}", crate::auth::random_token(4).to_lowercase()),
            name: name.trim().to_string(),
            account_id,
            when,
            then,
            enabled: true,
            created_by: String::new(),
            created_at: now,
            server_id: None,
            problem: None,
            hits: 0,
            last_hit_at: None,
        }
    }

    /// What's wrong with it, in a sentence, or None when it can be saved.
    pub fn check(&self) -> Option<&'static str> {
        if self.when.is_empty() {
            Some("Add at least one condition: a rule with none would catch all mail.")
        } else if self.then.is_empty() {
            Some("Choose what the rule does.")
        } else if self.then.archive && self.then.trash {
            Some("Choose Archive or Trash, not both.")
        } else {
            None
        }
    }

    pub fn applies_to(&self, account_id: &str) -> bool {
        self.account_id.as_deref().is_none_or(|a| a == account_id)
    }

    /// Whether Coo should apply this rule to `m`, stored just now: it's on,
    /// for this account, and `m` is new incoming mail it matches. `mine`:
    /// the user's own addresses, lowercased.
    pub fn catches(&self, m: &MessageRow, mine: &[String], now: i64) -> bool {
        let since = self.created_at.max(now - RECENT_MS) - GRACE_MS;
        let from = m.header.from.as_ref().map(|a| a.email.to_lowercase()).unwrap_or_default();
        self.enabled
            && self.applies_to(&m.account_id)
            && !m.header.is_draft
            && m.header.received_at >= since
            && !mine.contains(&from)
            && self.when.matches(m)
    }

    /// Where it runs for an account of `provider`. `may_save`: the account
    /// lets Coo save rules on its server (Outlook asks for a permission).
    pub fn runs(&self, provider: Option<ProviderKind>, may_save: bool) -> Runs {
        let Some(provider) = provider.filter(|_| self.account_id.is_some()) else { return Runs::Coo(Why::AllAccounts) };
        if provider == ProviderKind::ICloud {
            return Runs::Coo(Why::ICloud);
        }
        if let Some(action) = self.then.coo_only(provider) {
            return Runs::Coo(Why::CooOnly(action));
        }
        if !may_save {
            return Runs::Coo(Why::Permission);
        }
        Runs::Server(provider)
    }
}

/// Where a rule runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    /// As a Gmail filter or Outlook rule.
    Server(ProviderKind),
    Coo(Why),
}

/// Why a rule runs in Coo rather than on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    AllAccounts,
    ICloud,
    /// An action only Coo can do, by its label.
    CooOnly(&'static str),
    /// Outlook hasn't allowed Coo to save rules yet.
    Permission,
}

impl Runs {
    /// "Gmail filter", "Outlook rule" or "In Coo".
    pub fn label(self) -> &'static str {
        match self {
            Runs::Server(ProviderKind::Google) => "Gmail filter",
            Runs::Server(_) => "Outlook rule",
            Runs::Coo(_) => "In Coo",
        }
    }

    /// One or two sentences on what that means.
    pub fn explain(self) -> String {
        const LATER: &str =
            "Coo applies it while it’s open; mail that came in while Coo was closed is handled on the next sync.";
        match self {
            Runs::Server(ProviderKind::Google) => {
                "Saved as a Gmail filter: Gmail runs it on every device, even with Coo closed.".into()
            }
            Runs::Server(_) => {
                "Saved as an Outlook rule: Outlook runs it on every device, even with Coo closed.".into()
            }
            Runs::Coo(Why::AllAccounts) => format!("It covers all your accounts, so it runs in Coo. {LATER}"),
            Runs::Coo(Why::ICloud) => format!("iCloud doesn’t let apps save rules, so it runs in Coo. {LATER}"),
            Runs::Coo(Why::CooOnly(a)) => format!("“{a}” is something only Coo can do, so it runs in Coo. {LATER}"),
            Runs::Coo(Why::Permission) => {
                format!("Outlook hasn’t allowed Coo to save rules yet, so it runs in Coo. {LATER}")
            }
        }
    }
}

/// Lowercased value of a sender or recipient condition.
fn norm(v: &str) -> String {
    v.trim().trim_matches(|c| c == '<' || c == '>').to_lowercase()
}

/// Whether one address (or its display name) matches a `from`/`to` value:
/// `@acme.com` the domain or a subdomain, a full address exactly, anything
/// else as part of the address or name.
pub fn address_matches(value: &str, email: &str, name: &str) -> bool {
    let v = norm(value);
    if v.is_empty() {
        return false;
    }
    let email = email.trim().to_lowercase();
    if let Some(domain) = v.strip_prefix('@') {
        let Some((_, d)) = email.rsplit_once('@') else { return false };
        return d == domain || d.ends_with(&format!(".{domain}"));
    }
    if v.contains('@') {
        return email == v;
    }
    email.contains(&v) || name.to_lowercase().contains(&v)
}

impl When {
    pub fn is_empty(&self) -> bool {
        self.from.iter().chain(&self.to).chain(&self.subject).chain(&self.words).all(|v| v.trim().is_empty())
            && !self.has_attachment
    }

    pub fn matches(&self, m: &MessageRow) -> bool {
        let h = &m.header;
        let any = |list: &[String], f: &dyn Fn(&str) -> bool| {
            let list: Vec<&String> = list.iter().filter(|v| !v.trim().is_empty()).collect();
            list.is_empty() || list.iter().any(|v| f(v))
        };
        let subject = h.subject.to_lowercase();
        let text = format!("{subject} {}", h.snippet.to_lowercase());
        let from = h.from.as_ref();
        any(&self.from, &|v| from.is_some_and(|a| address_matches(v, &a.email, a.name.as_deref().unwrap_or_default())))
            && any(&self.to, &|v| {
                h.to.iter().chain(&h.cc).any(|a| address_matches(v, &a.email, a.name.as_deref().unwrap_or_default()))
            })
            && any(&self.subject, &|v| subject.contains(&v.trim().to_lowercase()))
            && any(&self.words, &|v| text.contains(&v.trim().to_lowercase()))
            && (!self.has_attachment || h.has_attachments)
    }

    /// "From @rescana.com · Subject has “invoice” or “receipt”".
    pub fn summary(&self) -> String {
        let quoted = |l: &[String]| l.iter().map(|v| format!("“{}”", v.trim())).collect::<Vec<_>>().join(" or ");
        let mut parts = Vec::new();
        if !self.from.is_empty() {
            parts.push(format!("From {}", self.from.join(", ")));
        }
        if !self.to.is_empty() {
            parts.push(format!("To {}", self.to.join(", ")));
        }
        if !self.subject.is_empty() {
            parts.push(format!("Subject has {}", quoted(&self.subject)));
        }
        if !self.words.is_empty() {
            parts.push(format!("Has {}", quoted(&self.words)));
        }
        if self.has_attachment {
            parts.push("Has attachment".into());
        }
        parts.join(" · ")
    }
}

impl Then {
    /// "Skip inbox, mark read, label “Vendors”". `provider` picks the words
    /// (label or folder, star or flag); None for all accounts.
    pub fn summary(&self, provider: Option<ProviderKind>) -> String {
        let gmail = provider == Some(ProviderKind::Google);
        let mut parts: Vec<String> = Vec::new();
        if self.trash {
            parts.push("move to Trash".into());
        }
        if self.archive {
            parts.push("skip inbox".into());
        }
        if self.read {
            parts.push("mark read".into());
        }
        if let Some(p) = &self.label {
            parts.push(if gmail { format!("label “{}”", p.name) } else { format!("move to “{}”", p.name) });
        }
        if self.star {
            parts.push(if gmail { "star".into() } else { "flag".into() });
        }
        if self.quiet {
            parts.push("no notification".into());
        }
        if self.needs_you {
            parts.push("send to Needs you".into());
        }
        let s = parts.join(", ");
        let mut chars = s.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => s,
        }
    }
}

/// One thing a rule did (Settings › Rules › Activity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logged {
    pub id: i64,
    pub rule_id: String,
    pub rule_name: String,
    pub account_id: String,
    pub message_id: String,
    pub thread_id: String,
    /// The sender's name or address and the subject, as they were.
    pub from: String,
    pub subject: String,
    /// What was done ("Skip inbox, mark read").
    pub what: String,
    /// Where it ran: "Gmail filter", "Outlook rule" or "In Coo".
    pub how: String,
    /// What was done, to undo it.
    pub then: Then,
    pub at: i64,
    pub undone_at: Option<i64>,
}

/// A Gmail filter or Outlook rule as the server has it. `rule` is set when
/// Coo can show and edit it as one of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRule {
    pub id: String,
    /// The Outlook rule's name; Gmail filters have none.
    pub name: String,
    pub when: When,
    pub then: Then,
    /// Something Coo can't edit (forwarding, other conditions): shown, but
    /// "Edit in Coo" would lose it.
    pub extra: bool,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Address, MessageHeader};

    fn row(from: &str, subject: &str, at: i64) -> MessageRow {
        MessageRow {
            account_id: "google:me@x.io".into(),
            header: MessageHeader {
                id: "m".into(),
                thread_id: "t".into(),
                internet_message_id: None,
                subject: subject.into(),
                from: Some(Address::new(Some("Rescana Alerts"), from)),
                to: vec![Address::new(None, "me@x.io")],
                cc: vec![],
                snippet: "Please complete the Bezeq questionnaire".into(),
                received_at: at,
                is_read: false,
                is_flagged: false,
                has_attachments: false,
                is_draft: false,
                folder_ids: vec![],
            },
        }
    }

    #[test]
    fn senders_match_by_domain_address_or_part() {
        assert!(address_matches("@rescana.com", "noreply@Rescana.com", ""));
        assert!(address_matches("@rescana.com", "a@mail.rescana.com", ""), "subdomains too");
        assert!(!address_matches("@rescana.com", "a@notrescana.com", ""));
        assert!(address_matches("<A@B.io>", "a@b.io", ""));
        assert!(!address_matches("a@b.io", "aa@b.io", ""), "a full address is exact");
        assert!(address_matches("rescana", "x@y.io", "Rescana Alerts"), "or part of the name");
        assert!(!address_matches(" ", "x@y.io", ""));
    }

    #[test]
    fn every_given_list_must_match() {
        let m = row("noreply@rescana.com", "Reminder: questionnaire due", 1_000);
        let w = When { from: vec!["@rescana.com".into()], ..Default::default() };
        assert!(w.matches(&m));
        let w = When { from: vec!["@rescana.com".into()], subject: vec!["invoice".into()], ..Default::default() };
        assert!(!w.matches(&m));
        let w = When { subject: vec!["invoice".into(), "Questionnaire".into()], ..Default::default() };
        assert!(w.matches(&m), "any value of a list");
        assert!(When { words: vec!["bezeq".into()], ..Default::default() }.matches(&m), "words: the snippet too");
        assert!(When { to: vec!["me@x.io".into()], ..Default::default() }.matches(&m));
        assert!(!When { has_attachment: true, ..Default::default() }.matches(&m));
        assert!(When::default().is_empty() && When { from: vec![" ".into()], ..Default::default() }.is_empty());
    }

    #[test]
    fn rules_catch_only_new_incoming_mail() {
        let when = When { from: vec!["@rescana.com".into()], ..Default::default() };
        let then = Then { archive: true, ..Default::default() };
        let mut r = Rule::new("Rescana", Some("google:me@x.io".into()), when, then, 1_000_000);
        assert!(r.id.starts_with('r'));
        assert!(r.catches(&row("a@rescana.com", "Hi", 1_000_000), &[], 1_000_000));
        assert!(!r.catches(&row("a@rescana.com", "Hi", 1_000_000 - GRACE_MS - 1), &[], 1_000_000), "history");
        assert!(!r.catches(&row("me@x.io", "Hi", 1_000_000), &["me@x.io".into()], 1_000_000));
        r.account_id = Some("microsoft:me@y.io".into());
        assert!(!r.catches(&row("a@rescana.com", "Hi", 1_000_000), &[], 1_000_000), "another account");
        r.account_id = None;
        r.enabled = false;
        assert!(!r.catches(&row("a@rescana.com", "Hi", 1_000_000), &[], 1_000_000));
    }

    #[test]
    fn where_a_rule_runs() {
        let when = When { from: vec!["@x.io".into()], ..Default::default() };
        let mut r = Rule::new("x", Some("a".into()), when, Then { archive: true, ..Default::default() }, 0);
        assert_eq!(r.runs(Some(ProviderKind::Google), true), Runs::Server(ProviderKind::Google));
        assert_eq!(r.runs(Some(ProviderKind::Microsoft), false), Runs::Coo(Why::Permission));
        assert_eq!(r.runs(Some(ProviderKind::ICloud), true), Runs::Coo(Why::ICloud));
        r.then.star = true;
        assert_eq!(r.runs(Some(ProviderKind::Google), true), Runs::Server(ProviderKind::Google));
        assert_eq!(r.runs(Some(ProviderKind::Microsoft), true), Runs::Coo(Why::CooOnly("Flag")));
        r.then.needs_you = true;
        assert_eq!(r.runs(Some(ProviderKind::Google), true), Runs::Coo(Why::CooOnly("Send to Needs you")));
        r.account_id = None;
        assert_eq!(r.runs(Some(ProviderKind::Google), true), Runs::Coo(Why::AllAccounts));
    }

    #[test]
    fn checks_and_summaries() {
        let mut r = Rule::new("x", None, When::default(), Then::default(), 0);
        assert!(r.check().is_some());
        r.when.from = vec!["@rescana.com".into()];
        assert!(r.check().is_some(), "no action");
        r.then = Then { archive: true, read: true, ..Default::default() };
        assert_eq!(r.check(), None);
        assert_eq!(r.when.summary(), "From @rescana.com");
        assert_eq!(r.then.summary(Some(ProviderKind::Google)), "Skip inbox, mark read");
        r.then.label = Some(Place { id: "L1".into(), name: "Vendors".into() });
        assert_eq!(r.then.summary(Some(ProviderKind::Google)), "Skip inbox, mark read, label “Vendors”");
        assert!(Then { quiet: true, ..Default::default() }.only_quiet());
        r.then.trash = true;
        assert!(r.check().is_some());
    }
}
