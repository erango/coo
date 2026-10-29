//! Shortcuts while a message has keyboard focus. A click into a message
//! puts focus in its sandboxed frame, which runs no script, so the bridge's
//! keydown never hears R, A, F or E there. This app-wide key monitor sees
//! every key first and hands it to each window's bridge
//! (`__pigeonNativeKey`), which acts on it only when its window has focus
//! and that focus is in a read-only message. The key still goes on to the
//! web view as usual.
//!
//! It also turns ⌘ + scroll (mouse wheel or trackpad) into text size steps,
//! over message content too, and keeps those scrolls from scrolling.

use std::sync::OnceLock;

use dioxus::prelude::*;
use tokio::sync::broadcast;

fn channel() -> &'static broadcast::Sender<String> {
    static TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    TX.get_or_init(|| broadcast::channel(32).0)
}

/// A key as the bridge's `keyName` expects it: a KeyboardEvent-like object.
fn event_json(chars: &str, key_code: u16, meta: bool, shift: bool, alt: bool, ctrl: bool) -> Option<String> {
    let special = match key_code {
        36 | 76 => Some("Enter"),
        53 => Some("Escape"),
        51 => Some("Backspace"),
        117 => Some("Delete"),
        126 => Some("ArrowUp"),
        125 => Some("ArrowDown"),
        _ => None,
    };
    let key = special.map(str::to_string).or_else(|| (chars.chars().count() == 1).then(|| chars.to_string()))?;
    let code = code_for(key_code).unwrap_or_default();
    Some(
        serde_json::json!({
            "key": key, "code": code, "metaKey": meta, "shiftKey": shift, "altKey": alt, "ctrlKey": ctrl,
        })
        .to_string(),
    )
}

/// The physical key's `KeyboardEvent.code` (US layout positions), for the
/// keys shortcuts go by.
fn code_for(key_code: u16) -> Option<String> {
    const LETTERS: [(u16, char); 26] = [
        (0, 'A'),
        (11, 'B'),
        (8, 'C'),
        (2, 'D'),
        (14, 'E'),
        (3, 'F'),
        (5, 'G'),
        (4, 'H'),
        (34, 'I'),
        (38, 'J'),
        (40, 'K'),
        (37, 'L'),
        (46, 'M'),
        (45, 'N'),
        (31, 'O'),
        (35, 'P'),
        (12, 'Q'),
        (15, 'R'),
        (1, 'S'),
        (17, 'T'),
        (32, 'U'),
        (9, 'V'),
        (13, 'W'),
        (7, 'X'),
        (16, 'Y'),
        (6, 'Z'),
    ];
    if let Some((_, c)) = LETTERS.iter().find(|(k, _)| *k == key_code) {
        return Some(format!("Key{c}"));
    }
    Some(
        match key_code {
            30 => "BracketRight",
            33 => "BracketLeft",
            44 => "Slash",
            _ => return None,
        }
        .to_string(),
    )
}

/// Start the monitor (once, on the main thread).
#[cfg(target_os = "macos")]
pub fn start() {
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

    struct Monitor(#[allow(dead_code)] Retained<AnyObject>);
    // SAFETY: only stored to keep the monitor alive; never used again.
    unsafe impl Send for Monitor {}
    unsafe impl Sync for Monitor {}
    static MONITOR: OnceLock<Option<Monitor>> = OnceLock::new();

    MONITOR.get_or_init(|| {
        let block = block2::RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands the monitor a valid event for this call.
            let e = unsafe { event.as_ref() };
            let flags = e.modifierFlags();
            if e.r#type() == objc2_app_kit::NSEventType::ScrollWheel {
                if !flags.contains(NSEventModifierFlags::Command) {
                    return event.as_ptr();
                }
                // Wheel up (whatever the scroll direction setting) is bigger.
                let mut dy = e.scrollingDeltaY();
                if e.isDirectionInvertedFromDevice() {
                    dy = -dy;
                }
                if let Some(step) = wheel_step(dy, e.hasPreciseScrollingDeltas()) {
                    let _ = crate::ui::zoom::wheel_tx().send(step);
                }
                return std::ptr::null_mut();
            }
            let chars = e.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
            if let Some(json) = event_json(
                &chars,
                e.keyCode(),
                flags.contains(NSEventModifierFlags::Command),
                flags.contains(NSEventModifierFlags::Shift),
                flags.contains(NSEventModifierFlags::Option),
                flags.contains(NSEventModifierFlags::Control),
            ) {
                let _ = channel().send(json);
            }
            event.as_ptr()
        });
        // SAFETY: the handler returns the event it was given (valid).
        unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown | NSEventMask::ScrollWheel,
                &block,
            )
        }
        .map(Monitor)
    });
}

#[cfg(not(target_os = "macos"))]
pub fn start() {}

/// A text size step from ⌘ + scroll: each wheel notch is one; a trackpad's
/// fine deltas add up to one per ~40 pt of travel.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn wheel_step(dy: f64, precise: bool) -> Option<crate::ui::zoom::Step> {
    use crate::ui::zoom::Step;
    use std::sync::Mutex;
    static TRAVEL: Mutex<f64> = Mutex::new(0.0);
    let mut travel = TRAVEL.lock().unwrap_or_else(|p| p.into_inner());
    if !precise {
        *travel = 0.0;
        return (dy != 0.0).then_some(if dy > 0.0 { Step::Bigger } else { Step::Smaller });
    }
    // A change of direction starts over.
    if *travel * dy < 0.0 {
        *travel = 0.0;
    }
    *travel += dy;
    if travel.abs() < 40.0 {
        return None;
    }
    let step = if *travel > 0.0 { Step::Bigger } else { Step::Smaller };
    *travel = 0.0;
    Some(step)
}

/// In a window's root component: pass monitored keys to this window's bridge.
pub fn use_native_keys() {
    use_hook(|| {
        spawn(async {
            let mut rx = channel().subscribe();
            loop {
                match rx.recv().await {
                    Ok(json) => {
                        document::eval(&format!("window.__pigeonNativeKey && window.__pigeonNativeKey({json})"));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_as_the_bridge_reads_them() {
        let v: serde_json::Value =
            serde_json::from_str(&event_json("r", 15, false, false, false, false).unwrap()).unwrap();
        assert_eq!(v["key"], "r");
        assert_eq!(v["code"], "KeyR");
        // A Hebrew layout: the character is ר, the physical key still R.
        let v: serde_json::Value =
            serde_json::from_str(&event_json("ר", 15, false, false, false, false).unwrap()).unwrap();
        assert_eq!(v["code"], "KeyR");
        let v: serde_json::Value =
            serde_json::from_str(&event_json("", 51, false, false, false, false).unwrap()).unwrap();
        assert_eq!(v["key"], "Backspace");
        assert!(event_json("", 122, false, false, false, false).is_none(), "F1 isn't a shortcut key");
    }

    #[test]
    fn scroll_steps() {
        use crate::ui::zoom::Step;
        assert_eq!(wheel_step(3.0, false), Some(Step::Bigger));
        assert_eq!(wheel_step(-1.0, false), Some(Step::Smaller));
        assert_eq!(wheel_step(25.0, true), None);
        assert_eq!(wheel_step(25.0, true), Some(Step::Bigger));
        assert_eq!(wheel_step(-30.0, true), None, "turning back starts over");
    }
}
