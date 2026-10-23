//! Refresh-token persistence: macOS Keychain, or a 0600 JSON file for dev
//! builds (`PIGEON_TOKEN_STORE=file`) where every rebuild changes the code
//! signature and would trigger Keychain access prompts.
//!
//! The `coo` helper (CLI / MCP server) doesn't read the Keychain itself: it
//! asks the running app through the sign-in broker (`broker.rs`), so agents
//! never trigger an "allow access" prompt nobody can see.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum TokenStore {
    #[cfg(target_os = "macos")]
    Keychain,
    File(PathBuf),
    /// Ask the running app (the helper's mode when the app uses the
    /// Keychain). `fallback` is tried when the app can't be reached: the
    /// Keychain for an interactive CLI, nothing for the MCP server.
    Brokered {
        client: crate::broker::Client,
        fallback: Option<Box<TokenStore>>,
    },
}

/// What to tell a headless caller whose Keychain read needs a person.
pub const KEYCHAIN_APPROVAL_NEEDED: &str = crate::broker::APP_NOT_RUNNING;

/// Keeps Keychain reads from showing an "allow access" dialog while held;
/// they fail with [`KEYCHAIN_APPROVAL_NEEDED`] instead. For headless callers:
/// the MCP server runs as an agent's subprocess, where nobody sees a dialog
/// and the request would hang. No-op elsewhere and for the file store.
pub struct HeadlessGuard {
    #[cfg(target_os = "macos")]
    _lock: Option<security_framework::os::macos::keychain::KeychainUserInteractionLock>,
}

/// Turn off Keychain UI for this process until the guard drops.
pub fn headless() -> HeadlessGuard {
    HeadlessGuard {
        #[cfg(target_os = "macos")]
        _lock: security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()
            .map_err(|e| tracing::warn!(error = %e, "could not disable Keychain UI"))
            .ok(),
    }
}

impl TokenStore {
    /// `keychain` or `file`, for diagnostics and generated agent configs.
    pub fn kind(&self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => "keychain",
            TokenStore::File(_) => "file",
            // The app behind the broker keeps them in the Keychain.
            TokenStore::Brokered { .. } => "keychain",
        }
    }

    /// The helper's store: the app's sign-ins through the broker when the
    /// app uses the Keychain, the shared file otherwise. `interactive`
    /// allows falling back to reading the Keychain directly (a prompt a
    /// person can answer) when the app can't be reached.
    pub fn for_helper(data_dir: &std::path::Path, interactive: bool) -> Self {
        match TokenStore::from_env(data_dir) {
            TokenStore::File(p) => TokenStore::File(p),
            local => TokenStore::Brokered {
                client: crate::broker::Client::new(data_dir, true),
                fallback: interactive.then(|| Box::new(local)),
            },
        }
    }

    /// The broker client, when this store asks the app.
    pub fn broker(&self) -> Option<&crate::broker::Client> {
        match self {
            TokenStore::Brokered { client, .. } => Some(client),
            _ => None,
        }
    }

    /// Where reads go when the broker is out of reach.
    pub fn fallback(&self) -> Option<&TokenStore> {
        match self {
            TokenStore::Brokered { fallback, .. } => fallback.as_deref(),
            other => Some(other),
        }
    }

    /// Whether a sign-in is stored (never returns the secret).
    pub fn has(&self, account_id: &str) -> Result<bool> {
        if let TokenStore::Brokered { client, fallback } = self {
            return match client.call(&crate::broker::Request::Has { account: account_id.to_string() }) {
                Ok(r) if r.ok => Ok(r.has.unwrap_or(false)),
                Ok(r) => Err(r.into_error(account_id)),
                Err(_) => match fallback {
                    Some(f) => f.has(account_id),
                    None => Err(Error::TokenStore(crate::broker::APP_NOT_RUNNING.into())),
                },
            };
        }
        Ok(self.get(account_id)?.is_some())
    }

    pub fn from_env(data_dir: &std::path::Path) -> Self {
        let file = TokenStore::File(data_dir.join("tokens.json"));
        #[cfg(target_os = "macos")]
        if std::env::var("PIGEON_TOKEN_STORE").as_deref() != Ok("file") {
            return TokenStore::Keychain;
        }
        file
    }

    /// The stored secret. Through the broker this is only ever a password
    /// (iCloud); OAuth refresh tokens stay in the app.
    pub fn get(&self, account_id: &str) -> Result<Option<String>> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => keychain::get(account_id),
            TokenStore::File(path) => Ok(read_file(path)?.remove(account_id)),
            TokenStore::Brokered { client, fallback } => {
                match client.call(&crate::broker::Request::Secret { account: account_id.to_string() }) {
                    Ok(r) if r.ok => Ok(r.token),
                    Ok(r) if r.failure == Some(crate::broker::Failure::UnknownAccount) => Ok(None),
                    Ok(r) => Err(r.into_error(account_id)),
                    Err(_) => match fallback {
                        Some(f) => f.get(account_id),
                        None => Err(Error::TokenStore(crate::broker::APP_NOT_RUNNING.into())),
                    },
                }
            }
        }
    }

    pub fn set(&self, account_id: &str, token: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => keychain::set(account_id, token),
            TokenStore::Brokered { fallback: Some(f), .. } => f.set(account_id, token),
            TokenStore::Brokered { fallback: None, .. } => {
                Err(Error::TokenStore("sign in from the Coo app (Settings › Accounts)".into()))
            }
            TokenStore::File(path) => {
                let mut map = read_file(path)?;
                map.insert(account_id.to_string(), token.to_string());
                write_file(path, &map)
            }
        }
    }

    pub fn delete(&self, account_id: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => keychain::delete(account_id),
            TokenStore::Brokered { fallback: Some(f), .. } => f.delete(account_id),
            TokenStore::Brokered { fallback: None, .. } => {
                Err(Error::TokenStore("sign out from the Coo app (Settings › Accounts)".into()))
            }
            TokenStore::File(path) => {
                let mut map = read_file(path)?;
                if map.remove(account_id).is_some() {
                    write_file(path, &map)?;
                }
                Ok(())
            }
        }
    }
}

fn read_file(path: &PathBuf) -> Result<BTreeMap<String, String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.into()),
    }
}

fn write_file(path: &PathBuf, map: &BTreeMap<String, String>) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(map)?)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path).map_err(|e| Error::TokenStore(e.to_string()))
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords;

    use crate::paths::APP_ID;
    use crate::{Error, Result};

    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
    /// The read needed a dialog, and UI is off (see `headless`).
    const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

    /// The Keychain service; tests use their own (`PIGEON_KEYCHAIN_SERVICE`)
    /// so they never touch real sign-ins.
    fn service() -> String {
        std::env::var("PIGEON_KEYCHAIN_SERVICE").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| APP_ID.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>> {
        match passwords::get_generic_password(&service(), account) {
            Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|e| Error::TokenStore(e.to_string())),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) if e.code() == ERR_SEC_INTERACTION_NOT_ALLOWED => {
                Err(Error::TokenStore(super::KEYCHAIN_APPROVAL_NEEDED.to_string()))
            }
            Err(e) => Err(Error::TokenStore(e.to_string())),
        }
    }

    pub fn set(account: &str, token: &str) -> Result<()> {
        passwords::set_generic_password(&service(), account, token.as_bytes())
            .map_err(|e| Error::TokenStore(e.to_string()))
    }

    pub fn delete(account: &str) -> Result<()> {
        match passwords::delete_generic_password(&service(), account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(Error::TokenStore(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pigeon-tokens-{}", crate::auth::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let store = TokenStore::File(dir.join("tokens.json"));
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "t1").unwrap();
        store.set("b", "t2").unwrap();
        store.set("a", "t3").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("t3"));
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("b").unwrap().as_deref(), Some("t2"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("tokens.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
