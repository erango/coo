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
