//! Sound themes: each pairs a new-mail sound with a meeting-reminder sound
//! of the same timbre (synthesized by scripts/gen-sounds.py), chosen in
//! Settings → Notifications. Short and quiet on purpose.

pub struct Theme {
    pub id: &'static str,
    pub label: &'static str,
    mail: &'static [u8],
    meeting: &'static [u8],
}

macro_rules! theme {
    ($id:literal, $label:literal) => {
        Theme {
            id: $id,
            label: $label,
            mail: include_bytes!(concat!("../assets/sounds/", $id, "-mail.wav")),
            meeting: include_bytes!(concat!("../assets/sounds/", $id, "-meeting.wav")),
        }
    };
}

/// The first is the default.
pub const THEMES: &[Theme] = &[
    theme!("droplet", "Droplet"),
    theme!("coo", "Coo"),
    theme!("chime", "Chime"),
    theme!("knock", "Knock"),
    theme!("bowl", "Bowl"),
];
pub const NONE: &str = "none";
pub const DEFAULT: &str = "droplet";

/// macOS's own sounds, played from the system (not bundled): Mail's New
/// Mail sound for mail and the Glass alert for meeting reminders.
pub const MACOS: &str = "macos";
const MACOS_MAIL: &str = "/System/Applications/Mail.app/Contents/Resources/New Mail.aiff";
const MACOS_MEETING: &str = "/System/Library/Sounds/Glass.aiff";

fn system_file(theme: &str, kind: Kind) -> Option<&'static str> {
    (theme == MACOS).then_some(match kind {
        Kind::Mail => MACOS_MAIL,
        Kind::Meeting => MACOS_MEETING,
    })
}

/// Theme choices for Settings: (id, label), the macOS one only when this
/// Mac has its sound file.
pub fn choices() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<_> = THEMES.iter().map(|t| (t.id, t.label)).collect();
    if std::path::Path::new(MACOS_MAIL).exists() {
        v.push((MACOS, "macOS Mail"));
    }
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mail,
    Meeting,
}

fn wav(theme: &str, kind: Kind) -> Option<&'static [u8]> {
    THEMES.iter().find(|t| t.id == theme).map(|t| match kind {
        Kind::Mail => t.mail,
        Kind::Meeting => t.meeting,
    })
}

/// Alerts of one kind closer together than this play once: accounts sync
/// side by side, so a burst of new mail arrives as several batches.
pub const ALERT_GAP: std::time::Duration = std::time::Duration::from_secs(4);

/// Whether an alert at `now` should sound, given the last one of its kind.
pub fn due(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= ALERT_GAP)
}

/// Whether a new-mail or reminder alert of `kind` should carry its sound:
/// at most once per `ALERT_GAP` for each kind, and never while the screen
/// is locked or every screen is off (asleep, or a closed lid with nothing
/// else attached). Focus and Do Not Disturb are macOS's call: the sound
/// rides on the notification (`notification_sound`).
pub fn take_alert(kind: Kind) -> bool {
    if imp::screens_off() {
        return false;
    }
    static LAST: std::sync::Mutex<[Option<std::time::Instant>; 2]> = std::sync::Mutex::new([None, None]);
    let now = std::time::Instant::now();
    let slot = kind as usize;
    let Ok(mut last) = LAST.lock() else { return false };
    if !due(last[slot], now) {
        return false;
    }
    last[slot] = Some(now);
    true
}

/// The file name a notification plays `kind` of `theme` by, for
/// UNNotificationSound. Every sound is copied into ~/Library/Sounds (as
/// "Coo …"): macOS 26 turns down sounds named from an app's bundle and
/// plays its default one instead, but takes the user's alert sounds folder.
/// None: silent.
pub fn notification_sound(theme: &str, kind: Kind) -> Option<String> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/Sounds");
    sound_in(&dir, theme, kind)
}

/// `notification_sound`, copying into `dir`.
fn sound_in(dir: &std::path::Path, theme: &str, kind: Kind) -> Option<String> {
    let (name, bytes): (String, std::borrow::Cow<'static, [u8]>) = match wav(theme, kind) {
        Some(b) => (format!("Coo {}", bundled_name(theme, kind)), b.into()),
        None => {
            let src = system_file(theme, kind)?;
            let name = format!("Coo {}", std::path::Path::new(src).file_name()?.to_str()?);
            if dir.join(&name).exists() {
                return Some(name);
            }
            (name, std::fs::read(src).ok()?.into())
        }
    };
    let dest = dir.join(&name);
    // Rewritten when a release changes the sound.
    if std::fs::metadata(&dest).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        let written = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&dest, &bytes));
        if let Err(e) = written {
            tracing::warn!(error = %e, "could not copy a sound for notifications");
            return wav(theme, kind).is_some().then(|| bundled_name(theme, kind));
        }
    }
    Some(name)
}

/// `<theme>-<kind>.wav`, as in assets/sounds.
fn bundled_name(theme: &str, kind: Kind) -> String {
    let kind = match kind {
        Kind::Mail => "mail",
        Kind::Meeting => "meeting",
    };
    format!("{theme}-{kind}.wav")
}

/// About where macOS's alert volume sits, so previews sound like alerts.
const PREVIEW_VOLUME: f32 = 0.75;

/// Play `kind` from `theme` now (Settings previews; alerts without a
/// Coo.app bundle). Unknown themes and "none" are silent.
pub fn play(theme: &str, kind: Kind) {
    if let Some(bytes) = wav(theme, kind) {
        imp::play(bytes, PREVIEW_VOLUME);
    } else if let Some(path) = system_file(theme, kind) {
        imp::play_file(path, PREVIEW_VOLUME);
    }
}

#[cfg(target_os = "macos")]
mod imp {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayIsAsleep(display: u32) -> u32;
        fn CGSessionCopyCurrentDictionary() -> *mut objc2::runtime::AnyObject;
    }

    /// The login session's screen is locked (or the session info is gone,
    /// e.g. at the login window).
    fn screen_locked() -> bool {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2_foundation::NSString;
        // SAFETY: CGSessionCopyCurrentDictionary returns a +1 CFDictionary
        // (toll-free bridged to NSDictionary) or NULL; objectForKey: and
        // boolValue with their documented signatures.
        unsafe {
            let Some(dict) = Retained::from_raw(CGSessionCopyCurrentDictionary()) else { return false };
            let key = NSString::from_str("CGSSessionScreenIsLocked");
            let v: *mut AnyObject = msg_send![&*dict, objectForKey: &*key];
            !v.is_null() && {
                let locked: bool = msg_send![v, boolValue];
                locked
            }
        }
    }

    /// Nobody's looking: the screen is locked, or no display is awake (all
    /// asleep, or none online with the lid closed).
    pub fn screens_off() -> bool {
        if screen_locked() {
            return true;
        }
        let mut ids = [0u32; 16];
        let mut count = 0u32;
        // SAFETY: CoreGraphics fills at most `max` ids and sets `count`.
        let err = unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
        if err != 0 {
            return false;
        }
        // SAFETY: plain query on a display id CoreGraphics just returned.
        ids[..count as usize].iter().all(|&d| unsafe { CGDisplayIsAsleep(d) } != 0)
    }

    use std::cell::RefCell;

    use objc2::AllocAnyThread;
    use objc2::rc::Retained;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    thread_local! {
        // NSSound stops when released: keep the last one alive.
        static CURRENT: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
    }

    pub fn play(bytes: &[u8], volume: f32) {
        let data = NSData::with_bytes(bytes);
        let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else { return };
        start(sound, volume);
    }

    /// A sound file on this Mac (missing or unreadable: silent).
    pub fn play_file(path: &str, volume: f32) {
        let path = objc2_foundation::NSString::from_str(path);
        let Some(sound) = NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &path, true) else { return };
        start(sound, volume);
    }

    fn start(sound: Retained<NSSound>, volume: f32) {
        sound.setVolume(volume);
        sound.play();
        CURRENT.with(|c| *c.borrow_mut() = Some(sound));
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn play(_bytes: &[u8], _volume: f32) {}
    pub fn play_file(_path: &str, _volume: f32) {}
    pub fn screens_off() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_alerts_sounds_once() {
        let t0 = std::time::Instant::now();
        assert!(due(None, t0));
        assert!(!due(Some(t0), t0 + std::time::Duration::from_millis(1500)), "the next account's batch");
        assert!(due(Some(t0), t0 + ALERT_GAP));
    }

    #[test]
    fn every_theme_has_both_small_mono_wavs() {
        assert_eq!(THEMES[0].id, DEFAULT);
        for t in THEMES {
            for (k, w) in [("mail", t.mail), ("meeting", t.meeting)] {
                assert_eq!(&w[0..4], b"RIFF", "{} {k}", t.id);
                assert_eq!(u16::from_le_bytes([w[22], w[23]]), 1, "{} {k} mono", t.id);
                assert!(w.len() < 300_000, "{} {k} too big", t.id);
            }
            assert!(t.meeting.len() > t.mail.len(), "{}: reminder should be the longer of the pair", t.id);
        }
        assert!(wav(NONE, Kind::Mail).is_none());
        assert_eq!(system_file(MACOS, Kind::Mail), Some(MACOS_MAIL));
        assert_eq!(system_file(DEFAULT, Kind::Mail), None);
    }

    #[test]
    fn notification_sounds_are_copied_beside_the_user_alert_sounds() {
        let dir = std::env::temp_dir().join(format!("coo-sounds-{}", std::process::id()));
        for t in THEMES {
            for kind in [Kind::Mail, Kind::Meeting] {
                let name = sound_in(&dir, t.id, kind).unwrap();
                assert_eq!(name, format!("Coo {}", bundled_name(t.id, kind)));
                assert_eq!(std::fs::read(dir.join(&name)).unwrap(), wav(t.id, kind).unwrap(), "{name}");
            }
        }
        assert_eq!(sound_in(&dir, NONE, Kind::Mail), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
