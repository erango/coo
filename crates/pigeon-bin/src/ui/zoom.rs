//! Text size (⌘+ / ⌘- / ⌘0): CSS `zoom` on the root, persisted in prefs.

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use pigeon_engine::service::Toast;

use crate::prefs;
use crate::state::AppState;

/// The steps ⌘+ / ⌘- walk through.
pub const LADDER: [f32; 7] = [0.8, 0.9, 1.0, 1.1, 1.2, 1.35, 1.5];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Bigger,
    Smaller,
    Reset,
}

/// Next scale from `current`, snapping arbitrary saved values onto the ladder.
pub fn next(current: f32, step: Step) -> f32 {
    const EPS: f32 = 0.001;
    match step {
        Step::Reset => 1.0,
        Step::Bigger => LADDER.iter().copied().find(|&v| v > current + EPS).unwrap_or(LADDER[LADDER.len() - 1]),
        Step::Smaller => LADDER.iter().rev().copied().find(|&v| v < current - EPS).unwrap_or(LADDER[0]),
    }
}

/// Root style for the first paint (`main.rs` head).
pub fn head_css(scale: f32) -> String {
    format!(":root{{zoom:{scale};--ui-zoom:{scale}}}")
}

/// Change the text size, apply it live, save it, and say so.
pub fn change(s: AppState, step: Step) {
    let scale = next(prefs::get().font_scale, step);
    let scale = prefs::update(|p| p.font_scale = scale).font_scale;
    document::eval(&format!(
        "const r = document.documentElement.style; r.zoom = '{scale}'; r.setProperty('--ui-zoom', '{scale}'); \
         window.pigeonFitFrames && window.pigeonFitFrames();"
    ));
    // Local toasts use ids far above the engine's counter.
    static SEQ: AtomicU64 = AtomicU64::new(1 << 62);
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut toasts = s.toasts;
    toasts.write().retain(|t| !t.message.starts_with("Text size"));
    s.push_toast(Toast { id, message: format!("Text size {}%", (scale * 100.0).round()), error: false, undo: None });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_and_snaps_the_ladder() {
        assert_eq!(next(1.0, Step::Bigger), 1.1);
        assert_eq!(next(1.0, Step::Smaller), 0.9);
        assert_eq!(next(1.2, Step::Bigger), 1.35);
        assert_eq!(next(1.5, Step::Bigger), 1.5, "stays at the top");
        assert_eq!(next(0.8, Step::Smaller), 0.8, "stays at the bottom");
        assert_eq!(next(1.05, Step::Bigger), 1.1, "snaps up from off-ladder values");
        assert_eq!(next(1.05, Step::Smaller), 1.0, "snaps down from off-ladder values");
        assert_eq!(next(1.35, Step::Reset), 1.0);
    }

    #[test]
    fn head_css_sets_root_zoom() {
        assert_eq!(head_css(1.35), ":root{zoom:1.35;--ui-zoom:1.35}");
    }
}
