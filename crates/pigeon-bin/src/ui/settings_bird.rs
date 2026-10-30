//! The pigeon in Settings. It perches on the Look and Sounds groups and
//! reacts to what you change there: a new theme twirls its scarf (which
//! already wears the accent), a sound makes it bob along (Bowl: eyes shut,
//! swaying; the macOS set: a head tilt), and text size puffs it up or tucks
//! it in. On General › Help it waits by About Coo for a few taps.
//!
//! Changes anywhere in Settings call `cue`; every `Perch` on screen plays
//! cues newer than itself, so opening a page never replays an old one.
//! Motion is app.css (`bird--twirl` and friends) and stops under Reduce
//! Motion.

use std::time::Duration;

use dioxus::prelude::*;

use super::bird::{Bird, Pose};

/// A reaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// A new theme: the scarf twirls into its new color.
    Twirl,
    /// A sound: two bobs, in time.
    Bob,
    /// Bowl: eyes shut, a slow sway.
    Sway,
    /// The macOS sounds: a curious head tilt.
    Tilt,
    /// Bigger text: puffs up.
    Puff,
    /// Smaller text: tucks in.
    Tuck,
}

impl Cue {
    fn name(self) -> &'static str {
        match self {
            Cue::Twirl => "twirl",
            Cue::Bob => "bob",
            Cue::Sway => "sway",
            Cue::Tilt => "tilt",
            Cue::Puff => "puff",
            Cue::Tuck => "tuck",
        }
    }

    /// How long it runs (app.css matches).
    fn ms(self) -> u64 {
        match self {
            Cue::Twirl => 700,
            Cue::Bob => 900,
            Cue::Sway => 1600,
            Cue::Tilt => 850,
            Cue::Puff | Cue::Tuck => 650,
        }
    }
}

/// The latest cue, numbered so a repeat plays again.
static CUE: GlobalSignal<(u64, Option<Cue>)> = Signal::global(|| (0, None));

/// Have the Settings pigeon react.
pub fn cue(c: Cue) {
    let n = CUE.peek().0 + 1;
    *CUE.write() = (n, Some(c));
}

/// How the pigeon takes a sound set's sound (none for silence).
pub fn sound_cue(theme: &str) -> Option<Cue> {
    match theme {
        crate::sound::NONE => None,
        "bowl" => Some(Cue::Sway),
        crate::sound::MACOS => Some(Cue::Tilt),
        _ => Some(Cue::Bob),
    }
}

/// How the pigeon takes a text size change from `before` to `after`.
pub fn size_cue(before: f32, after: f32) -> Option<Cue> {
    if after > before + 1e-4 {
        Some(Cue::Puff)
    } else if after < before - 1e-4 {
        Some(Cue::Tuck)
    } else {
        None
    }
}

/// The pigeon perched on a group's title row.
#[component]
pub fn Perch() -> Element {
    // Only cues from after this perch appeared.
    let born = use_hook(|| CUE.peek().0);
    let (n, c) = *CUE.read();
    let mut done = use_signal(|| 0u64);
    let ms = c.map(Cue::ms).unwrap_or(0);
    use_effect(use_reactive!(|n, ms| {
        if n > born {
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                if *done.peek() < n {
                    done.set(n);
                }
            });
        }
    }));
    let active = c.filter(|_| n > born && *done.read() < n);
    let pose = if active == Some(Cue::Sway) { Pose::Sleeping } else { Pose::Awake };
    rsx! {
        span { class: "settings-perch", key: "{n}", "aria-hidden": "true",
            Bird { pose, class: active.map(|c| format!("bird--{}", c.name())) }
        }
    }
}

/// Taps that earn a backflip, and the two after it a letter.
const FLIP_AT: u32 = 5;
const LETTER_AT: u32 = 7;
/// Taps reset after this long without one.
const IDLE_MS: u64 = 5000;

/// What one more tap does, from `taps` already made.
fn after_tap(taps: u32) -> (u32, &'static str, u64) {
    match taps + 1 {
        FLIP_AT => (FLIP_AT, "flip", 1100),
        LETTER_AT => (LETTER_AT, "letter", 2600),
        n => (n, "squash", 240),
    }
}

/// The pigeon by About Coo. Each tap squashes it a little and fills a dot;
/// the fifth sends it into a backflip with a "Coo!", the seventh has it drop
/// you a little letter. A pause starts the count over.
#[component]
pub fn AboutBird() -> Element {
    let mut taps = use_signal(|| 0u32);
    let mut anim = use_signal(|| None::<&'static str>);
    let mut epoch = use_signal(|| 0u64);
    let tap = move |_| {
        let (n, what, ms) = after_tap(*taps.peek());
        let e = *epoch.peek() + 1;
        epoch.set(e);
        taps.set(n);
        anim.set(Some(what));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            if *epoch.peek() == e {
                anim.set(None);
                if n >= LETTER_AT {
                    taps.set(0);
                }
            }
            tokio::time::sleep(Duration::from_millis(IDLE_MS)).await;
            if *epoch.peek() == e {
                taps.set(0);
            }
        });
    };
    let a = *anim.read();
    let filled = (*taps.read()).min(FLIP_AT);
    rsx! {
        div { class: "about-bird",
            if a == Some("flip") {
                span { class: "about-bird__coo", key: "{epoch}", "Coo!" }
            }
            button { class: "about-bird__btn", r#type: "button", "aria-label": "The Coo pigeon", title: "Coo",
                onclick: tap,
                span { class: "about-bird__fig", key: "{epoch}",
                    Bird { pose: Pose::Awake, class: a.filter(|w| *w != "letter").map(|w| format!("bird--{w}")) }
                }
            }
            if a == Some("letter") {
                svg { class: "about-bird__letter", key: "{epoch}", view_box: "0 0 16 12", "aria-hidden": "true",
                    rect { x: "0.8", y: "0.8", width: "14.4", height: "10.4", rx: "1.6", fill: "var(--bg-0)", stroke: "var(--accent)", stroke_width: "1.3" }
                    path { d: "M1.2 1.6L8 6.6L14.8 1.6", fill: "none", stroke: "var(--accent)", stroke_width: "1.2", stroke_linejoin: "round" }
                }
            }
            div { class: "about-bird__dots", "aria-hidden": "true",
                for i in 0..FLIP_AT {
                    span { key: "{i}", "data-on": (i < filled).then_some("true") }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_and_sizes_pick_a_reaction() {
        assert_eq!(sound_cue("bowl"), Some(Cue::Sway));
        assert_eq!(sound_cue(crate::sound::MACOS), Some(Cue::Tilt));
        assert_eq!(sound_cue("chime"), Some(Cue::Bob));
        assert_eq!(sound_cue(crate::sound::NONE), None);
        assert_eq!(size_cue(1.0, 1.1), Some(Cue::Puff));
        assert_eq!(size_cue(1.1, 1.0), Some(Cue::Tuck));
        assert_eq!(size_cue(1.0, 1.0), None);
    }

    #[test]
    fn the_fifth_tap_flips_and_the_seventh_brings_a_letter() {
        let mut t = 0;
        let mut seen = Vec::new();
        for _ in 0..7 {
            let (n, what, _) = after_tap(t);
            t = n;
            seen.push(what);
        }
        assert_eq!(seen, ["squash", "squash", "squash", "squash", "flip", "squash", "letter"]);
    }
}
