use std::path::PathBuf;

use crate::{Error, Result};

/// Bundle id; also the Keychain service name.
pub const APP_ID: &str = "il.co.frost-media.pigeon";

/// `~/Library/Application Support/il.co.frost-media.pigeon` on macOS, or
/// `$PIGEON_DATA_DIR`. Created if missing.
pub fn data_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("PIGEON_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => directories::ProjectDirs::from("il.co", "frost-media", "pigeon")
            .ok_or_else(|| Error::Other("no home directory".into()))?
            .data_dir()
            .to_path_buf(),
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
