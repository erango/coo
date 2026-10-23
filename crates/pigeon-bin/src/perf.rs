//! Debug-level timings for the paths the product promises are instant:
//! process start → first list rendered, and mailbox switch → list
//! rendered. `RUST_LOG=pigeon_app=debug` shows them.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static STARTED: Mutex<Option<Instant>> = Mutex::new(None);
static SHOW_SENT: Mutex<Option<Instant>> = Mutex::new(None);
static LIST_ARRIVED: Mutex<Option<Instant>> = Mutex::new(None);
static FIRST_RENDER_LOGGED: AtomicBool = AtomicBool::new(false);

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

pub fn mark_start() {
    *STARTED.lock().unwrap() = Some(Instant::now());
}

/// A ShowMailbox command just went to the engine.
pub fn mark_show_sent() {
    *SHOW_SENT.lock().unwrap() = Some(Instant::now());
}

/// A List delta reached the UI.
pub fn mark_list_arrived() {
    let now = Instant::now();
    if let Some(t) = SHOW_SENT.lock().unwrap().take() {
        tracing::debug!(ms = ms(t), "mailbox switch: ShowMailbox -> List delta");
    }
    *LIST_ARRIVED.lock().unwrap() = Some(now);
}

/// The list just rendered (called from an effect after the render).
pub fn mark_list_rendered(rows: usize) {
    if let Some(t) = LIST_ARRIVED.lock().unwrap().take() {
        tracing::debug!(ms = ms(t), rows, "list: delta -> rendered");
    }
    if rows > 0
        && !FIRST_RENDER_LOGGED.swap(true, Ordering::Relaxed)
        && let Some(t) = *STARTED.lock().unwrap()
    {
        tracing::debug!(ms = ms(t), rows, "startup: process start -> first list rendered");
    }
}
