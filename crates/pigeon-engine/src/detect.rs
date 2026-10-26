//! "Add account" provider detection: known consumer domains straight
//! away, otherwise the domain's MX records through the system resolver
//! (its configured DNS servers; never DNS-over-HTTPS to a third party),
//! classified by `pigeon_core::provider_detect`.

use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use pigeon_core::provider_detect::{self, Detection, MailHost};

/// Past this the sheet stops waiting and lets the user pick.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// Who handles mail for `email`; `Unknown` when the address has no domain,
/// DNS fails, or it takes longer than `TIMEOUT`.
pub async fn detect(email: &str) -> Detection {
    let Some(domain) = provider_detect::domain_of(email) else {
        return Detection { domain: String::new(), host: MailHost::Unknown };
    };
    if let Some(host) = provider_detect::by_domain(&domain) {
        return Detection { domain, host };
    }
    let host = match tokio::time::timeout(TIMEOUT, mx_hosts(&domain)).await {
        Ok(Ok(records)) => provider_detect::by_mx(&records),
        // Domains are the user's own business: debug only.
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "MX lookup failed");
            MailHost::Unknown
        }
        Err(_) => {
            tracing::debug!("MX lookup timed out");
            MailHost::Unknown
        }
    };
    Detection { domain, host }
}

/// `(preference, exchange)` for each MX record of `domain`.
async fn mx_hosts(domain: &str) -> Result<Vec<(u16, String)>, String> {
    let mut builder = TokioResolver::builder_tokio().map_err(|e| e.to_string())?;
    builder.options_mut().timeout = TIMEOUT;
    builder.options_mut().attempts = 1;
    let resolver = builder.build().map_err(|e| e.to_string())?;
    // Fully qualified: no search-domain suffixes.
    let lookup = resolver.mx_lookup(format!("{domain}.")).await.map_err(|e| e.to_string())?;
    Ok(lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::MX(mx) => Some((mx.preference, mx.exchange.to_ascii())),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn consumer_domains_need_no_dns() {
        let d = detect("Someone@Gmail.com").await;
        assert_eq!((d.domain.as_str(), d.host), ("gmail.com", MailHost::Gmail));
        assert_eq!(detect("not an address").await.host, MailHost::Unknown);
    }

    /// Real DNS; run by hand: `cargo test -p pigeon-engine live_mx -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_mx() {
        for email in ["a@google.com", "a@microsoft.com", "a@apple.com", "a@fastmail.com"] {
            println!("{email}: {:?}", detect(email).await);
        }
    }
}
