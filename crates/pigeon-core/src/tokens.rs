//! Refresh-token persistence: macOS Keychain, or a 0600 JSON file for dev
//! builds (`PIGEON_TOKEN_STORE=file`) where every rebuild changes the code
//! signature and would trigger Keychain access prompts.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum TokenStore {
    #[cfg(target_os = "macos")]
    Keychain,
    File(PathBuf),
}

/// What to tell a headless caller whose Keychain read needs a person.
pub const KEYCHAIN_APPROVAL_NEEDED: &str = "macOS needs you to allow this program to read Coo's \
     sign-ins from the Keychain. Run `coo doctor` once in Terminal and choose \"Always Allow\" (again after \
     updating Pigeon), or use the file token store (PIGEON_TOKEN_STORE=file) for both the app and the agent";

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
        }
    }

    pub fn from_env(data_dir: &std::path::Path) -> Self {
        let file = TokenStore::File(data_dir.join("tokens.json"));
        #[cfg(target_os = "macos")]
        if std::env::var("PIGEON_TOKEN_STORE").as_deref() != Ok("file") {
            return TokenStore::Keychain;
        }
        file
    }

    pub fn get(&self, account_id: &str) -> Result<Option<String>> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => keychain::get(account_id),
            TokenStore::File(path) => Ok(read_file(path)?.remove(account_id)),
        }
    }

    pub fn set(&self, account_id: &str, token: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStore::Keychain => keychain::set(account_id, token),
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

    pub fn get(account: &str) -> Result<Option<String>> {
        match passwords::get_generic_password(APP_ID, account) {
            Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|e| Error::TokenStore(e.to_string())),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) if e.code() == ERR_SEC_INTERACTION_NOT_ALLOWED => {
                Err(Error::TokenStore(super::KEYCHAIN_APPROVAL_NEEDED.to_string()))
            }
            Err(e) => Err(Error::TokenStore(e.to_string())),
        }
    }

    pub fn set(account: &str, token: &str) -> Result<()> {
        passwords::set_generic_password(APP_ID, account, token.as_bytes()).map_err(|e| Error::TokenStore(e.to_string()))
    }

    pub fn delete(account: &str) -> Result<()> {
        match passwords::delete_generic_password(APP_ID, account) {
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
