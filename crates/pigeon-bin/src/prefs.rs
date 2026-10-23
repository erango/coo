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
    /// "system" | "light" | "dark".
    pub theme_mode: String,
    /// Color theme family id (`themes::FAMILIES`, or "pigeon").
    pub palette: String,
    /// Recent Move/Label targets, newest first, as `account|folder_id`.
    pub recent_move_targets: Vec<String>,
    /// Desktop notification for new inbox mail.
    pub notify_new_mail: bool,
    /// Desktop reminder before events start.
    pub notify_events: bool,
    /// How long before an event the reminder fires.
    pub event_reminder_minutes: u32,
    /// Account ids that never notify.
    pub notify_accounts_muted: Vec<String>,
    /// Load remote images in every message without asking.
    pub remote_images_always: bool,
    /// Senders (lowercased addresses) whose remote images always load.
    pub remote_images_senders: Vec<String>,
    /// Send crash reports (takes effect next launch).
    pub crash_reports: bool,
    /// First day of the week, 0 = Sunday … 6 = Saturday; None = follow macOS.
    pub week_start: Option<u8>,
    /// Undo-send window in seconds (0 = send immediately).
    pub undo_send_seconds: u32,
    /// Plain-text signature per sending address (lowercased email).
    pub signatures: std::collections::BTreeMap<String, String>,
    /// Meeting reminders break through Focus / Do Not Disturb.
    pub reminders_time_sensitive: bool,
    /// Sound theme id (`sound::THEMES`: new mail + meeting reminder) or "none".
    pub sound_theme: String,
    /// Sound volume, 0–100.
    pub sound_volume: u8,
    /// Where new messages open: "window" (their own window) or "docked"
    /// (in the reading pane).
    pub compose_mode: String,
    /// Message list width in px (0 = the design default).
    pub list_width: u32,
    /// Sidebar width in px (0 = the design default).
    pub sidebar_width: u32,
    /// Conversation order in the reader: newest message on top.
    pub thread_newest_first: bool,
    /// Where replies open: "inline" (below the conversation) or "window".
    pub reply_mode: String,
    /// Closing the main window keeps Coo running in the menu bar (false:
    /// closing quits).
    pub close_to_menu_bar: bool,
    /// The one-time "Coo is still running" hint was shown.
    pub menu_bar_hint_shown: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            account_order: Vec::new(),
            font_scale: 1.0,
            theme_mode: "system".into(),
            palette: crate::themes::DEFAULT.into(),
            recent_move_targets: Vec::new(),
            notify_new_mail: true,
            notify_events: true,
            event_reminder_minutes: 10,
            notify_accounts_muted: Vec::new(),
            remote_images_always: false,
            remote_images_senders: Vec::new(),
            crash_reports: true,
            week_start: None,
            undo_send_seconds: 10,
            signatures: Default::default(),
            list_width: 0,
            compose_mode: "window".into(),
            sidebar_width: 0,
            thread_newest_first: true,
            reply_mode: "inline".into(),
            close_to_menu_bar: true,
            menu_bar_hint_shown: false,
            reminders_time_sensitive: true,
            sound_theme: crate::sound::DEFAULT.into(),
            sound_volume: 60,
        }
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
