//! App settings that survive restarts: `prefs.json` in the data dir
//! (`PIGEON_DATA_DIR` respected). UI-only preferences live here; anything
//! the engine needs belongs in the store.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

pub const FONT_SCALE_MIN: f32 = 0.8;
pub const FONT_SCALE_MAX: f32 = 1.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Account ids in the user's chosen order; unknown/new accounts go last.
    pub account_order: Vec<String>,
    /// Multiplier on the base font size (⌘+ / ⌘- / ⌘0).
    pub font_scale: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { account_order: Vec::new(), font_scale: 1.0 }
    }
}

fn path() -> Option<PathBuf> {
    pigeon_core::paths::data_dir().ok().map(|d| d.join("prefs.json"))
}

fn cell() -> &'static Mutex<Prefs> {
    static PREFS: OnceLock<Mutex<Prefs>> = OnceLock::new();
    PREFS.get_or_init(|| Mutex::new(load()))
}

fn load() -> Prefs {
    let Some(p) = path() else { return Prefs::default() };
    match std::fs::read(&p) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "prefs.json unreadable; using defaults");
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    }
}

/// Current settings (loaded once, on first use).
pub fn get() -> Prefs {
    cell().lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Change settings and write them to disk (atomically, best effort).
#[allow(dead_code)] // first callers: account reordering, font size
pub fn update(f: impl FnOnce(&mut Prefs)) -> Prefs {
    let mut guard = cell().lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard);
    guard.font_scale = guard.font_scale.clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
    let snapshot = guard.clone();
    drop(guard);
    if let Some(p) = path() {
        let tmp = p.with_extension("json.tmp");
        let res = serde_json::to_vec_pretty(&snapshot)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(&tmp, bytes))
            .and_then(|_| std::fs::rename(&tmp, &p));
        if let Err(e) = res {
            tracing::warn!(error = %e, "could not save prefs.json");
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"account_order": ["a"]}"#).unwrap();
        assert_eq!(p.font_scale, 1.0);
        assert_eq!(p.account_order, ["a"]);
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Prefs::default());
    }
}
