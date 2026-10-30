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
    /// Account id new messages come from; empty: the top account in the
    /// sidebar (`state::default_from_account`).
    pub default_from_account: String,
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
    /// Nicknames shown instead of an account's address, by account id.
    pub account_nicknames: std::collections::BTreeMap<String, String>,
    /// Unread inbox count on the Dock icon.
    pub dock_badge: bool,
    /// The mailbox open when Coo last ran (`state::mailbox_key`); empty for
    /// the unified inbox.
    pub last_mailbox: String,
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
    /// Video call new events start with: "" none, "meet" or "zoom".
    pub default_video_call: String,
    /// ...but only when the event has no location when it's saved.
    pub default_video_only_without_location: bool,
    /// Calendar new events go to, as `account_id|calendar_id`; empty (or a
    /// calendar that's gone or read-only): automatic
    /// (`event_editor::new_event_calendar`).
    pub default_calendar: String,
    /// After archiving, deleting or moving the open message: "next" opens
    /// the next one in the list, "close" just closes it.
    pub after_remove: String,
    /// Where a message's attachments show: "top" (under its header, before
    /// the text) or "bottom" (after the text).
    pub attachments_position: String,
    /// Keyboard shortcut set: "gmail" (single keys) or "outlook" (⌘ keys).
    pub shortcut_style: String,
    /// The user's own keys for actions with none, as `style:Action` → key
    /// (shortcuts.rs).
    pub custom_shortcuts: std::collections::BTreeMap<String, String>,
    /// Drafts: "ask" (when closing a changed message) or "auto" (saved to
    /// Drafts as you write).
    pub draft_saving: String,
    /// Seconds a conversation stays open before it's marked read (0: as
    /// soon as it opens).
    pub read_delay_secs: u32,
    /// The buttons a message row shows under the pointer (ui/row_actions.rs).
    pub row_actions: Vec<String>,
    /// Undo-send window in seconds (0 = send immediately).
    pub undo_send_seconds: u32,
    /// Signature HTML per sending address (lowercased email); the key
    /// `DEFAULT_SIGNATURE` holds the one shared by every other address.
    pub signatures: std::collections::BTreeMap<String, String>,
    /// Per address: "default", "own" or "none". Missing: "own" when the
    /// address has its own signature, else "default".
    pub signature_modes: std::collections::BTreeMap<String, String>,
    /// New-mail alert: "banner" (always) or "background" (only when Coo isn't
    /// the active app; in front, just the sound).
    pub mail_alert: String,
    /// Meeting reminders break through Focus / Do Not Disturb.
    pub reminders_time_sensitive: bool,
    /// Sound theme id (`sound::THEMES`: new mail + meeting reminder) or "none".
    pub sound_theme: String,
    /// Play the theme's new-mail sound.
    pub mail_sound: bool,
    /// Play the theme's meeting-reminder sound.
    pub meeting_sound: bool,
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
    /// Show sender photos from Gravatar (sends a hash of each address).
    pub photos_gravatar: bool,
    /// Show sender photos from Google accounts' contacts (where allowed).
    pub photos_google: bool,
    /// Show sender photos from Microsoft 365 (Outlook contacts, coworkers),
    /// where allowed.
    pub photos_microsoft: bool,
    /// Message list look: "cards", "bands" or "classic" (`state::ListStyle`).
    pub list_style: String,
    /// A sender avatar column in the message list.
    pub list_avatars: bool,
    /// Accounts whose Folders/Labels group is open in the sidebar (closed
    /// by default: Gmail labels pile up).
    pub folders_open: Vec<String>,
    /// Folders pinned to show outside that group, as `account|folder_id`.
    pub pinned_folders: Vec<String>,
    /// Accounts open in the sidebar (closed by default: one quiet line
    /// each, with its unread count).
    pub accounts_open: Vec<String>,
    /// Calendars unchecked in the calendar sidebar, as `account|calendar_id`:
    /// their events don't show in the calendar views.
    pub hidden_calendars: Vec<String>,
    /// Closing the main window keeps Coo running in the menu bar (false:
    /// closing quits).
    pub close_to_menu_bar: bool,
    /// Relaunching after a silent update: start hidden in the menu bar.
    pub relaunch_hidden: bool,
    /// The one-time "Coo is still running" hint was shown.
    pub menu_bar_hint_shown: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            account_order: Vec::new(),
            default_from_account: String::new(),
            font_scale: 1.0,
            theme_mode: "system".into(),
            palette: crate::themes::DEFAULT.into(),
            recent_move_targets: Vec::new(),
            notify_new_mail: true,
            notify_events: true,
            account_nicknames: Default::default(),
            dock_badge: true,
            last_mailbox: String::new(),
            event_reminder_minutes: 10,
            notify_accounts_muted: Vec::new(),
            remote_images_always: false,
            remote_images_senders: Vec::new(),
            crash_reports: true,
            week_start: None,
            default_video_call: String::new(),
            default_video_only_without_location: true,
            default_calendar: String::new(),
            undo_send_seconds: 10,
            after_remove: "next".into(),
            attachments_position: "top".into(),
            shortcut_style: "gmail".into(),
            custom_shortcuts: Default::default(),
            draft_saving: "ask".into(),
            read_delay_secs: 0,
            row_actions: crate::ui::row_actions::DEFAULT.iter().map(|s| s.to_string()).collect(),
            signatures: Default::default(),
            signature_modes: Default::default(),
            list_width: 0,
            compose_mode: "window".into(),
            sidebar_width: 0,
            thread_newest_first: true,
            reply_mode: "inline".into(),
            photos_gravatar: false,
            photos_google: true,
            photos_microsoft: true,
            list_style: "cards".into(),
            list_avatars: true,
            folders_open: Vec::new(),
            pinned_folders: Vec::new(),
            accounts_open: Vec::new(),
            hidden_calendars: Vec::new(),
            close_to_menu_bar: true,
            relaunch_hidden: false,
            menu_bar_hint_shown: false,
            mail_alert: "banner".into(),
            reminders_time_sensitive: true,
            sound_theme: crate::sound::DEFAULT.into(),
            mail_sound: true,
            meeting_sound: true,
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
    let mut prefs = match std::fs::read(&p) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "prefs.json unreadable; using defaults");
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    };
    // "Sound only" went away when macOS took over playing alert sounds (it
    // only sounds with a banner); the closest is no banner while in front.
    if prefs.mail_alert == "sound" {
        prefs.mail_alert = "background".into();
    }
    prefs
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

/// `Prefs::signatures` key of the signature every address uses by default.
pub const DEFAULT_SIGNATURE: &str = "*";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigMode {
    Default,
    Own,
    None,
}

impl SigMode {
    pub fn key(self) -> &'static str {
        match self {
            SigMode::Default => "default",
            SigMode::Own => "own",
            SigMode::None => "none",
        }
    }
}

impl Prefs {
    /// How `email` (lowercased) picks its signature.
    pub fn signature_mode(&self, email: &str) -> SigMode {
        match self.signature_modes.get(email).map(String::as_str) {
            Some("none") => SigMode::None,
            Some("own") => SigMode::Own,
            Some("default") => SigMode::Default,
            _ if self.signatures.contains_key(email) => SigMode::Own,
            _ => SigMode::Default,
        }
    }

    /// The signature HTML that goes under mail from `email` (lowercased).
    pub fn signature(&self, email: &str) -> String {
        let key = match self.signature_mode(email) {
            SigMode::None => return String::new(),
            SigMode::Own => email,
            SigMode::Default => DEFAULT_SIGNATURE,
        };
        self.signatures.get(key).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;

    #[test]
    fn addresses_use_own_default_or_none() {
        let mut p = Prefs::default();
        p.signatures.insert(DEFAULT_SIGNATURE.into(), "D".into());
        p.signatures.insert("a@x.io".into(), "A".into());
        assert_eq!(p.signature("a@x.io"), "A", "an existing own signature keeps working");
        assert_eq!(p.signature("b@x.io"), "D", "everyone else gets the default");
        p.signature_modes.insert("a@x.io".into(), "default".into());
        assert_eq!(p.signature("a@x.io"), "D", "own text is kept but not used");
        p.signature_modes.insert("b@x.io".into(), "none".into());
        assert_eq!(p.signature("b@x.io"), "");
        p.signature_modes.insert("c@x.io".into(), "own".into());
        assert_eq!(p.signature("c@x.io"), "", "own but not written yet");
    }
}
