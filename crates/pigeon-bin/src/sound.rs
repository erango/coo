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

/// A new-mail or reminder alert: like `play`, but at most once per
/// `ALERT_GAP` for each kind. (Previews in Settings use `play`.)
pub fn alert(theme: &str, kind: Kind, volume: u8) {
    static LAST: std::sync::Mutex<[Option<std::time::Instant>; 2]> = std::sync::Mutex::new([None, None]);
    let now = std::time::Instant::now();
    let slot = kind as usize;
    let Ok(mut last) = LAST.lock() else { return };
    if !due(last[slot], now) {
        return;
    }
    last[slot] = Some(now);
    drop(last);
    play(theme, kind, volume);
}

/// Play `kind` from `theme` at `volume` (0–100). Unknown themes and "none"
/// are silent.
pub fn play(theme: &str, kind: Kind, volume: u8) {
    if let Some(bytes) = wav(theme, kind) {
        imp::play(bytes, f32::from(volume.min(100)) / 100.0);
    }
}

#[cfg(target_os = "macos")]
mod imp {
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
        sound.setVolume(volume);
        sound.play();
        CURRENT.with(|c| *c.borrow_mut() = Some(sound));
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn play(_bytes: &[u8], _volume: f32) {}
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
                assert!(w.len() < 200_000, "{} {k} too big", t.id);
            }
            assert!(t.meeting.len() > t.mail.len(), "{}: reminder should be the longer of the pair", t.id);
        }
        assert!(wav(NONE, Kind::Mail).is_none());
    }
}
