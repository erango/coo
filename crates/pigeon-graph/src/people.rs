//! Sender photos from Microsoft 365: the user's Outlook contacts (an index
//! of address → contact id, read about daily; `Contacts.Read`) and, on
//! demand, a coworker's profile photo (`User.ReadBasic.All`). Photos are
//! binary and need the account's token. Requests that carry an address
//! skip the request log (`http::send` logs paths).

use pigeon_core::http::client;
use pigeon_core::{Error, Result};
use serde::Deserialize;

use crate::{BASE, Graph};

/// A photo bigger than this isn't a sender photo.
const MAX_PHOTO: usize = 512 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Contact {
    id: String,
    #[serde(default)]
    email_addresses: Vec<EmailAddress>,
}

#[derive(Deserialize)]
struct EmailAddress {
    address: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Organization {
    #[serde(default)]
    verified_domains: Vec<Domain>,
}

#[derive(Deserialize)]
struct Domain {
    name: String,
}

/// Contacts as (lowercased address, contact id), one per address (the
/// first contact that has it).
pub(crate) fn contact_index(contacts: Vec<Contact>) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for c in contacts {
        for a in c.email_addresses {
            let Some(addr) = a.address.map(|a| a.trim().to_lowercase()).filter(|a| a.contains('@')) else { continue };
            if seen.insert(addr.clone()) {
                out.push((addr, c.id.clone()));
            }
        }
    }
    out
}

/// The organization's verified domains, lowercased.
fn org_domains(orgs: Vec<Organization>) -> Vec<String> {
    orgs.into_iter().flat_map(|o| o.verified_domains).map(|d| d.name.to_lowercase()).collect()
}

impl Graph {
    /// The user's Outlook contacts as (address, contact id).
    pub async fn contact_index(&self) -> Result<Vec<(String, String)>> {
        let contacts: Vec<Contact> =
            self.get_all(&format!("{BASE}/me/contacts?$select=id,emailAddresses&$top=500")).await?;
        Ok(contact_index(contacts))
    }

    /// A contact's photo, or None when it has none.
    pub async fn contact_photo(&self, contact_id: &str) -> Result<Option<Vec<u8>>> {
        let id = url::form_urlencoded::byte_serialize(contact_id.as_bytes()).collect::<String>();
        self.photo(&format!("{BASE}/me/contacts/{id}/photo/$value")).await
    }

    /// A coworker's profile photo (96 px), or None when they have none or
    /// the account can't see it.
    pub async fn user_photo(&self, email: &str) -> Result<Option<Vec<u8>>> {
        let who = url::form_urlencoded::byte_serialize(email.trim().to_lowercase().as_bytes()).collect::<String>();
        self.photo(&format!("{BASE}/users/{who}/photos/96x96/$value")).await
    }

    /// The domains of the user's organization (where coworkers' photos can
    /// come from).
    pub async fn org_domains(&self) -> Result<Vec<String>> {
        let orgs: Vec<Organization> = self.get_all(&format!("{BASE}/organization?$select=verifiedDomains")).await?;
        Ok(org_domains(orgs))
    }

    async fn photo(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let token = self.tokens.access_token().await?;
        let resp = client().get(url).bearer_auth(&token).send().await?;
        match resp.status().as_u16() {
            200 => {
                let bytes = resp.bytes().await?;
                Ok((!bytes.is_empty() && bytes.len() < MAX_PHOTO).then(|| bytes.to_vec()))
            }
            // No photo, no such person, or not allowed to see them.
            400 | 403 | 404 => Ok(None),
            401 => {
                self.tokens.invalidate().await;
                Err(Error::Other("Graph photo: HTTP 401".into()))
            }
            s => Err(Error::Other(format!("Graph photo: HTTP {s}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_become_an_address_index() {
        let page: crate::types::Page<Contact> = serde_json::from_str(
            r#"{"value":[
                {"id":"c1","emailAddresses":[{"name":"Ava","address":"Ava@Chen.me"},{"address":"ava@work.io"}]},
                {"id":"c2","emailAddresses":[{"address":"ava@chen.me"},{"address":"not-an-address"}]},
                {"id":"c3"}
            ],"@odata.nextLink":"https://graph.microsoft.com/v1.0/me/contacts?$skip=3"}"#,
        )
        .unwrap();
        assert!(page.next_link.is_some());
        assert_eq!(
            contact_index(page.value),
            [("ava@chen.me".to_string(), "c1".to_string()), ("ava@work.io".to_string(), "c1".to_string())]
        );
    }

    #[test]
    fn organization_domains() {
        let page: crate::types::Page<Organization> = serde_json::from_str(
            r#"{"value":[{"verifiedDomains":[{"name":"Halyard.co","isDefault":true},{"name":"halyard.onmicrosoft.com"}]}]}"#,
        )
        .unwrap();
        assert_eq!(org_domains(page.value), ["halyard.co", "halyard.onmicrosoft.com"]);
    }
}
